use forgotten_config::{ensure_content_skeleton, load, validate_content, write_template};
use forgotten_host::{
    start, start_game_session, start_status, GameSessionHostConfig, HostConfig, LegacyLoginConfig,
    StatusHostConfig,
};
use forgotten_persistence::{create_backup, EngineDatabase};
use forgotten_protocol::{
    profile_by_id, CompatibilityProfile, LegacyRsaPrivateKey, OtClientEndpoint,
    COMPATIBILITY_PROFILES,
};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    match command {
        "init" => init(
            required_path(&arguments, 1)?,
            selected_profile(&arguments, 2)?,
        ),
        "validate" => validate(required_path(&arguments, 1)?),
        "run" => run_host(required_path(&arguments, 1)?),
        "status" => status(required_path(&arguments, 1)?),
        "generate-key" => generate_key(required_path(&arguments, 1)?),
        "backup" => backup(required_path(&arguments, 1)?),
        "command" => command_line(&arguments),
        "compatibility" => compatibility(),
        "version" | "--version" | "-V" => version(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(format!("unknown command `{unknown}`; run `forgotten-engine help`").into()),
    }
}

fn required_path(
    arguments: &[String],
    index: usize,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .map(PathBuf::from)
        .ok_or_else(|| "a Forgotten Engine world directory is required".into())
}

fn selected_profile(
    arguments: &[String],
    index: usize,
) -> Result<CompatibilityProfile, Box<dyn std::error::Error>> {
    let selector = match arguments.get(index).map(String::as_str) {
        None => "fe-1.2",
        Some("--profile") => arguments
            .get(index + 1)
            .map(String::as_str)
            .ok_or("a compatibility profile is required after --profile")?,
        Some(value) => value,
    };
    profile_by_id(selector).ok_or_else(|| {
        format!("unknown compatibility profile `{selector}`; use fe-7.4, fe-8.0, or fe-1.2").into()
    })
}

fn init(
    directory: PathBuf,
    profile: CompatibilityProfile,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&directory)?;
    write_template(&directory, profile)?;
    ensure_content_skeleton(&directory)?;
    let config = load(&directory)?;
    let database = EngineDatabase::open(&config.database_path)?;
    database.record_event("info", "Forgotten Engine world initialized")?;
    println!(
        "Forgotten Engine world initialized\n> config.lua profile={} protocol={}\n> content={}\n> database={} schema={}",
        config.profile.id,
        config.profile.tibia_protocol,
        config.content_directory.display(),
        database.path().display(),
        database.schema_version()?
    );
    Ok(())
}

fn validate(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!(">> Loading config");
    let config = load(&directory)?;
    println!(">> Reconciling original content skeleton");
    ensure_content_skeleton(&directory)?;
    println!(">> Validating data content");
    let content = validate_content(&directory)?;
    println!(">> Opening database");
    let database = EngineDatabase::open(&config.database_path)?;
    if database.schema_version()? < 1 {
        return Err("database schema is not migrated".into());
    }
    println!(
        "> Validation complete: profile={} protocol={} game-port={} status-port={} data={} database={}",
        config.profile.id,
        config.profile.tibia_protocol,
        config.game_protocol_port,
        config.status_protocol_port,
        content.data_directory.display(),
        database.path().display()
    );
    Ok(())
}

fn run_host(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("Forgotten Engine - {}", env!("CARGO_PKG_VERSION"));
    validate(directory.clone())?;
    let config = load(&directory)?;
    let database = EngineDatabase::open(&config.database_path)?;
    database.record_event("info", "Forgotten Engine host startup requested")?;

    println!(">> Registering services");
    let rsa_private_key = if config.legacy_login_enabled || config.game_session_enabled {
        if config.profile.id != "fe-7.4" {
            return Err(
                "legacy login and game-session foundations are currently available only for the fe-7.4 profile".into(),
            );
        }
        println!(">> Loading fe-7.4 foundation private key");
        Some(Arc::new(LegacyRsaPrivateKey::load_pem(
            &config.rsa_private_key_path,
        )?))
    } else {
        None
    };
    let legacy_login = match (config.legacy_login_enabled, &rsa_private_key) {
        (true, Some(rsa_private_key)) => Some(LegacyLoginConfig {
            rsa_private_key: Arc::clone(rsa_private_key),
            server_name: config.server_name.clone(),
            message_of_the_day: format!("Welcome to {}", config.server_name),
        }),
        _ => None,
    };
    let host = start(
        HostConfig {
            bind_addr: config.game_socket_addr(),
            profile: config.profile,
            max_connections: config.max_connections(),
            session_timeout: Duration::from_secs(5),
            legacy_login,
        },
        &config.database_path,
    )?;
    let status = match start_status(
        StatusHostConfig {
            bind_addr: config.status_socket_addr(),
            profile: config.profile,
            server_name: config.server_name.clone(),
            map_name: config.map_name.clone(),
            max_players: config.max_players,
            max_connections: config.max_connections(),
            session_timeout: Duration::from_secs(5),
        },
        &config.database_path,
    ) {
        Ok(status) => status,
        Err(error) => {
            host.shutdown()?;
            return Err(Box::new(error));
        }
    };
    let game_session = if config.game_session_enabled {
        let Some(rsa_private_key) = &rsa_private_key else {
            status.shutdown()?;
            host.shutdown()?;
            return Err("gameSessionEnabled requires an FE legacy private key".into());
        };
        match start_game_session(
            GameSessionHostConfig {
                bind_addr: config.game_session_socket_addr(),
                profile: config.profile,
                rsa_private_key: Arc::clone(rsa_private_key),
                advertised_endpoint: OtClientEndpoint {
                    host: config.advertised_game_session_host.clone(),
                    port: config.advertised_game_session_port,
                },
                max_connections: config.max_connections(),
                session_timeout: Duration::from_secs(5),
            },
            &config.database_path,
        ) {
            Ok(session) => Some(session),
            Err(error) => {
                status.shutdown()?;
                host.shutdown()?;
                return Err(Box::new(error));
            }
        }
    } else {
        None
    };
    let game_shutdown = host.shutdown_signal();
    let status_shutdown = status.shutdown_signal();
    let game_session_shutdown = game_session
        .as_ref()
        .map(|session| session.shutdown_signal());
    ctrlc::set_handler({
        let game_shutdown = game_shutdown.clone();
        let status_shutdown = status_shutdown.clone();
        let game_session_shutdown = game_session_shutdown.clone();
        move || {
            game_shutdown.store(true, Ordering::SeqCst);
            status_shutdown.store(true, Ordering::SeqCst);
            if let Some(game_session_shutdown) = &game_session_shutdown {
                game_session_shutdown.store(true, Ordering::SeqCst);
            }
        }
    })?;

    println!(
        "> FE game endpoint running on {} for {} / Tibia {}",
        host.local_addr(),
        config.profile.compatibility_reference,
        config.profile.tibia_protocol
    );
    println!(
        "> TFS-style status service running on {}",
        status.local_addr()
    );
    if let Some(game_session) = &game_session {
        println!(
            "> Bounded fe-7.4 game-session foundation running on {}; official-client acceptance remains unverified.",
            game_session.local_addr()
        );
    }
    if config.legacy_login_enabled {
        println!("> Bounded 7.4 login/character-list foundation is enabled; official-client acceptance remains unverified.");
    } else {
        println!(
            "> Diagnostic probe service is enabled; legacy login remains disabled in config.lua."
        );
    }
    println!("> Server host online. Press Ctrl+C for an orderly shutdown.");

    while !game_shutdown.load(Ordering::SeqCst)
        && !status_shutdown.load(Ordering::SeqCst)
        && game_session_shutdown
            .as_ref()
            .map(|shutdown| !shutdown.load(Ordering::SeqCst))
            .unwrap_or(true)
    {
        thread::sleep(Duration::from_millis(100));
    }
    if let Some(game_session) = game_session {
        game_session.shutdown()?;
    }
    status.shutdown()?;
    host.shutdown()?;
    println!("> Server host stopped.");
    Ok(())
}

fn status(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load(&directory)?;
    let database = EngineDatabase::open(&config.database_path)?;
    println!(
        "serverName={} profile={} reference={} targetProtocol={} database={} schema={} events={}",
        config.server_name,
        config.profile.id,
        config.profile.compatibility_reference,
        config.profile.tibia_protocol,
        database.path().display(),
        database.schema_version()?,
        database.event_count()?
    );
    Ok(())
}

fn generate_key(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load(&directory)?;
    if config.profile.id != "fe-7.4" {
        return Err("generate-key is currently available only for the fe-7.4 profile".into());
    }
    if config.rsa_private_key_path.exists() {
        return Err(format!(
            "refusing to overwrite existing private key {}",
            config.rsa_private_key_path.display()
        )
        .into());
    }
    if let Some(parent) = config.rsa_private_key_path.parent() {
        fs::create_dir_all(parent)?;
    }
    LegacyRsaPrivateKey::generate()?.write_pem(&config.rsa_private_key_path)?;
    println!(
        "generated original FE 1024-bit legacy-login private key at {}; set legacyLoginEnabled = true only when using the bounded 7.4 login foundation",
        config.rsa_private_key_path.display()
    );
    Ok(())
}

fn backup(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load(&directory)?;
    let artifact = create_backup(&config.database_path, directory.join("backups"))?;
    println!(
        "backup={} manifest={}",
        artifact.database_copy.display(),
        artifact.manifest_path.display()
    );
    Ok(())
}

fn command_line(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let directory = required_path(arguments, 1)?;
    let action = arguments
        .get(2)
        .map(String::as_str)
        .ok_or("a command action is required")?;
    match action {
        "broadcast" => {
            let message = arguments.get(3..).unwrap_or_default().join(" ");
            if message.trim().is_empty() {
                return Err("broadcast message is required".into());
            }
            let config = load(&directory)?;
            let database = EngineDatabase::open(&config.database_path)?;
            database.record_event("command", &format!("broadcast: {message}"))?;
            println!("recorded Forgotten Engine broadcast command");
            Ok(())
        }
        unsupported => Err(format!("unsupported command action `{unsupported}`").into()),
    }
}

fn compatibility() -> Result<(), Box<dyn std::error::Error>> {
    for profile in COMPATIBILITY_PROFILES {
        println!(
            "FE {}\t{}\tTibia {}\tofficial-client={}",
            profile.fe_release,
            profile.compatibility_reference,
            profile.tibia_protocol,
            profile.complete_protocol_emulation,
        );
    }
    for entry in forgotten_scripting::compatibility_matrix() {
        println!(
            "{}\t{}\t{}",
            entry.api,
            entry.capability.as_str(),
            entry.note
        );
    }
    Ok(())
}

fn version() -> Result<(), Box<dyn std::error::Error>> {
    println!("Forgotten Engine compatibility profiles:");
    for profile in COMPATIBILITY_PROFILES {
        println!(
            "  FE {} — {} / Tibia {}",
            profile.fe_release, profile.compatibility_reference, profile.tibia_protocol
        );
    }
    Ok(())
}

fn print_help() {
    println!("Forgotten Engine\n\nCompatibility profiles:\n  fe-7.4  — Tibia 7.4 (official client service not yet implemented)\n  fe-8.0  — Tibia 8.0 (official client service not yet implemented)\n  fe-1.2  — TFS 1.2 / Tibia 10.98 (official client service not yet implemented)\n\nCommands:\n  init <directory> [--profile fe-7.4|fe-8.0|fe-1.2]\n  validate <directory>\n  run <directory>\n  status <directory>\n  generate-key <directory>\n  backup <directory>\n  command <directory> broadcast <message>\n  compatibility\n  version");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn selects_tibia_7_4_profile_by_direct_selector() {
        let arguments = vec!["init".to_owned(), "world".to_owned(), "fe-7.4".to_owned()];
        let profile = selected_profile(&arguments, 2).unwrap();
        assert_eq!(profile.id, "fe-7.4");
        assert_eq!(profile.tibia_protocol, "7.4");
    }

    #[test]
    fn rejects_unknown_profile_by_direct_selector() {
        let arguments = vec!["init".to_owned(), "world".to_owned(), "unknown".to_owned()];
        assert!(selected_profile(&arguments, 2).is_err());
    }

    #[test]
    fn generates_an_original_legacy_key_for_a_7_4_world() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-key-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        generate_key(directory.clone()).unwrap();
        let config = load(&directory).unwrap();
        assert!(config.rsa_private_key_path.exists());
        assert!(LegacyRsaPrivateKey::load_pem(&config.rsa_private_key_path).is_ok());
        let _ = fs::remove_dir_all(directory);
    }
}
