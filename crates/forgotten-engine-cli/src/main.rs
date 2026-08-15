use forgotten_config::{
    apply_legacy_item_metadata, ensure_content_skeleton, load, load_legacy_item_catalog,
    load_tfs_content_inventory, load_world_companions, load_world_map, validate_content,
    world_map_path, write_template,
};
use forgotten_core::WorldMapSource;
use forgotten_host::{
    start, start_game_session, start_native_otclient_game, start_native_otclient_login,
    start_status, GameSessionHostConfig, HostConfig, LegacyLoginConfig,
    NativeOtClientEmptyWorldConfig, NativeOtClientHostConfig, StatusHostConfig,
};
use forgotten_persistence::{create_backup, EngineDatabase};
use forgotten_protocol::{
    profile_by_id, CompatibilityProfile, LegacyRsaPrivateKey, OtClientEndpoint,
    COMPATIBILITY_PROFILES,
};
use std::env;
use std::fs;
use std::net::{IpAddr, SocketAddr};
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
        "tfs-audit" => audit_tfs_conversion(required_path(&arguments, 1)?),
        "run" => run_host(required_path(&arguments, 1)?),
        "status" => status(required_path(&arguments, 1)?),
        "generate-key" => generate_key(required_path(&arguments, 1)?),
        "backup" => backup(required_path(&arguments, 1)?),
        "command" => command_line(&arguments),
        "account" => account_command(&arguments),
        "player" => player_command(&arguments),
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
    let raw_world_map = load_world_map(&config)?;
    let item_catalog = load_legacy_item_catalog(&config, &raw_world_map)?;
    let world_map = match &item_catalog {
        Some(catalog) => apply_legacy_item_metadata(&raw_world_map, catalog)?,
        None => raw_world_map,
    };
    let companions = load_world_companions(&config, &world_map)?;
    println!(">> Opening database");
    let database = EngineDatabase::open(&config.database_path)?;
    if database.schema_version()? < 1 {
        return Err("database schema is not migrated".into());
    }
    println!(
        "> Validation complete: profile={} protocol={} game-port={} status-port={} map={} tiles={} spawn={},{},{} items={} spawns={} houses={} data={} database={}",
        config.profile.id,
        config.profile.tibia_protocol,
        config.game_protocol_port,
        config.status_protocol_port,
        config.map_name,
        world_map.tile_count(),
        world_map.spawn().x,
        world_map.spawn().y,
        world_map.spawn().z,
        item_catalog.as_ref().map_or(0, |catalog| catalog.len()),
        companions.spawns.len(),
        companions.houses.len(),
        content.data_directory.display(),
        database.path().display()
    );
    Ok(())
}

fn audit_tfs_conversion(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!(">> Loading TFS-style configuration without executing Lua");
    let config = load(&directory)?;
    let map_path = world_map_path(&config)?;
    println!(">> Inspecting selected world data");
    let raw_world_map = load_world_map(&config)?;
    let item_catalog = load_legacy_item_catalog(&config, &raw_world_map)?;
    let world_map = match &item_catalog {
        Some(catalog) => apply_legacy_item_metadata(&raw_world_map, catalog)?,
        None => raw_world_map.clone(),
    };
    let companions = load_world_companions(&config, &world_map)?;
    let registry_inventory = load_tfs_content_inventory(&config)?;
    let map_kind = match raw_world_map.source() {
        WorldMapSource::Otbm(_) => "OTBM",
        WorldMapSource::FeMapV1 => "FE-native",
    };

    println!(
        "TFS conversion readiness\n> config={} (FE profile={} protocol={})\n> map={} format={} tiles={} spawn={},{},{}\n> item-mappings={} spawns={} houses={} towns={} waypoints={}\n> registries={} entries={} references={} missing-references={} unsafe-references={}",
        directory.join("config.lua").display(),
        config.profile.id,
        config.profile.tibia_protocol,
        map_path.display(),
        map_kind,
        world_map.tile_count(),
        world_map.spawn().x,
        world_map.spawn().y,
        world_map.spawn().z,
        item_catalog.as_ref().map_or(0, |catalog| catalog.len()),
        companions.spawns.len(),
        companions.houses.len(),
        world_map.towns().count(),
        world_map.waypoints().count(),
        registry_inventory.present_registry_count(),
        registry_inventory.entry_count(),
        registry_inventory.reference_count(),
        registry_inventory.missing_reference_count(),
        registry_inventory.unsafe_reference_count(),
    );
    if matches!(raw_world_map.source(), WorldMapSource::Otbm(_)) {
        println!("> OTBM world data is importable by the current FE map pipeline.");
    } else {
        println!("> FE-native map selected; use mapFormat = \"otbm\" or auto with an .otbm file to audit legacy map data.");
    }
    for registry in registry_inventory
        .registries
        .iter()
        .filter(|registry| registry.present)
    {
        println!(
            "> registry={} entries={} references={} missing={} unsafe={} status={}",
            registry.category.label(),
            registry.entry_count,
            registry.reference_count,
            registry.missing_references.len(),
            registry.unsafe_references.len(),
            registry.category.runtime_status(),
        );
        if !registry.missing_references.is_empty() {
            let paths = registry
                .missing_references
                .iter()
                .take(3)
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            println!(">   missing references (up to 3): {paths}");
        }
        if !registry.unsafe_references.is_empty() {
            println!(
                ">   unsafe relative references (up to 3): {}",
                registry
                    .unsafe_references
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if registry_inventory.present_registry_count() == 0 {
        println!("> No standard TFS XML registries were found beneath data/.");
    } else {
        println!(
            "> Registry entries were parsed for conversion inventory only. Referenced Lua scripts and creature definitions remain local and are not executed by this FE milestone."
        );
    }
    if config.otclient_v8_native_enabled {
        println!("> Native OTCv8 is enabled through the explicitly configured profile.");
    } else {
        println!("> Native OTCv8 is disabled. Configure it explicitly only after choosing a matching lawful client asset set.");
    }
    Ok(())
}

fn run_host(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("Forgotten Engine - {}", env!("CARGO_PKG_VERSION"));
    validate(directory.clone())?;
    let config = load(&directory)?;
    let raw_world_map = load_world_map(&config)?;
    let item_catalog = load_legacy_item_catalog(&config, &raw_world_map)?;
    let world_map = Arc::new(match &item_catalog {
        Some(catalog) => apply_legacy_item_metadata(&raw_world_map, catalog)?,
        None => raw_world_map,
    });
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
    let native_config = if config.otclient_v8_native_enabled {
        let advertised_ip: IpAddr = config.advertised_otclient_v8_host.parse().map_err(|_| {
            "advertisedOtClientV8Host must be an IPv4 or IPv6 address for the native client path"
        })?;
        let empty_world = if config.otclient_v8_native_empty_world_enabled {
            Some(NativeOtClientEmptyWorldConfig {
                ground_thing_id: config.otclient_v8_empty_world_ground_thing_id,
                player_look_type: config
                    .otclient_v8_player_look_type
                    .try_into()
                    .map_err(|_| "otclientV8PlayerLookType must fit the selected native profile")?,
                player_speed: config.otclient_v8_player_speed,
                server_beat: config.otclient_v8_server_beat,
            })
        } else {
            None
        };
        Some(NativeOtClientHostConfig {
            bind_addr: config.otclient_v8_login_socket_addr(),
            client_profile: config.otclient_v8_native_profile(),
            server_name: config.server_name.clone(),
            advertised_game_addr: SocketAddr::new(
                advertised_ip,
                config.advertised_otclient_v8_game_port,
            ),
            max_connections: config.max_connections(),
            session_timeout: Duration::from_secs(5),
            empty_world,
            world_map: Some(Arc::clone(&world_map)),
        })
    } else {
        None
    };
    let native_login = if let Some(native_config) = &native_config {
        match start_native_otclient_login(native_config.clone(), &config.database_path) {
            Ok(listener) => Some(listener),
            Err(error) => {
                if let Some(game_session) = game_session {
                    game_session.shutdown()?;
                }
                status.shutdown()?;
                host.shutdown()?;
                return Err(Box::new(error));
            }
        }
    } else {
        None
    };
    let native_game = if let Some(native_config) = native_config {
        let mut native_game_config = native_config;
        native_game_config.bind_addr = config.otclient_v8_game_socket_addr();
        match start_native_otclient_game(native_game_config, &config.database_path) {
            Ok(listener) => Some(listener),
            Err(error) => {
                if let Some(native_login) = native_login {
                    native_login.shutdown()?;
                }
                if let Some(game_session) = game_session {
                    game_session.shutdown()?;
                }
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
    let native_login_shutdown = native_login
        .as_ref()
        .map(|listener| listener.shutdown_signal());
    let native_game_shutdown = native_game
        .as_ref()
        .map(|listener| listener.shutdown_signal());
    ctrlc::set_handler({
        let game_shutdown = game_shutdown.clone();
        let status_shutdown = status_shutdown.clone();
        let game_session_shutdown = game_session_shutdown.clone();
        let native_login_shutdown = native_login_shutdown.clone();
        let native_game_shutdown = native_game_shutdown.clone();
        move || {
            game_shutdown.store(true, Ordering::SeqCst);
            status_shutdown.store(true, Ordering::SeqCst);
            if let Some(game_session_shutdown) = &game_session_shutdown {
                game_session_shutdown.store(true, Ordering::SeqCst);
            }
            if let Some(native_login_shutdown) = &native_login_shutdown {
                native_login_shutdown.store(true, Ordering::SeqCst);
            }
            if let Some(native_game_shutdown) = &native_game_shutdown {
                native_game_shutdown.store(true, Ordering::SeqCst);
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
    if let (Some(native_login), Some(native_game)) = (&native_login, &native_game) {
        println!(
            "> Native OTClientV8 profile={} login={} game={} empty-world={}",
            config.otclient_v8_protocol_version,
            native_login.local_addr(),
            native_game.local_addr(),
            config.otclient_v8_native_empty_world_enabled,
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
        && native_login_shutdown
            .as_ref()
            .map(|shutdown| !shutdown.load(Ordering::SeqCst))
            .unwrap_or(true)
        && native_game_shutdown
            .as_ref()
            .map(|shutdown| !shutdown.load(Ordering::SeqCst))
            .unwrap_or(true)
    {
        thread::sleep(Duration::from_millis(100));
    }
    if let Some(game_session) = game_session {
        game_session.shutdown()?;
    }
    if let Some(native_game) = native_game {
        native_game.shutdown()?;
    }
    if let Some(native_login) = native_login {
        native_login.shutdown()?;
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

fn account_command(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let action = arguments
        .get(1)
        .map(String::as_str)
        .ok_or("an account action is required")?;
    match action {
        "create" => {
            let directory = required_path(arguments, 2)?;
            let account_name = arguments
                .get(3)
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("an account name is required")?;
            let password = arguments
                .get(4)
                .map(String::as_str)
                .filter(|value| !value.is_empty())
                .ok_or("an account password is required")?;
            if arguments.len() != 5 {
                return Err("usage: account create <directory> <account-name> <password>".into());
            }
            let config = load(&directory)?;
            let database = EngineDatabase::open(&config.database_path)?;
            let account_id =
                database.create_account_with_password(account_name.trim(), password)?;
            println!(
                "created local account name={} native-account-id={account_id}",
                account_name.trim()
            );
            Ok(())
        }
        unsupported => Err(format!("unsupported account action `{unsupported}`").into()),
    }
}

fn player_command(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let action = arguments
        .get(1)
        .map(String::as_str)
        .ok_or("a player action is required")?;
    match action {
        "create" => {
            let directory = required_path(arguments, 2)?;
            let account_id: u32 = arguments
                .get(3)
                .ok_or("a numeric native account ID is required")?
                .parse()
                .map_err(|_| "account ID must be an unsigned 32-bit integer")?;
            let player_name = arguments
                .get(4)
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("a character name is required")?;
            if arguments.len() != 5 {
                return Err(
                    "usage: player create <directory> <account-id> <character-name>".into(),
                );
            }
            let config = load(&directory)?;
            let database = EngineDatabase::open(&config.database_path)?;
            let player = database.create_player_for_account(account_id, player_name)?;
            println!(
                "created character name={} player-id={} account-id={} position={},{},{} level={}",
                player.name,
                player.id,
                account_id,
                player.position.x,
                player.position.y,
                player.position.z,
                player.level
            );
            Ok(())
        }
        unsupported => Err(format!("unsupported player action `{unsupported}`").into()),
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
    println!(
        "Forgotten Engine build {}\nCompatibility profiles:",
        env!("CARGO_PKG_VERSION")
    );
    for profile in COMPATIBILITY_PROFILES {
        println!(
            "  FE {} — {} / Tibia {}",
            profile.fe_release, profile.compatibility_reference, profile.tibia_protocol
        );
    }
    Ok(())
}

fn print_help() {
    println!("Forgotten Engine\n\nCompatibility profiles:\n  fe-7.4  — Tibia 7.4 (experimental native OTCv8 empty-world fixture)\n  fe-8.0  — Tibia 8.0 (protocol foundation)\n  fe-1.2  — TFS 1.2 / Tibia 10.98 (protocol foundation)\n\nCommands:\n  init <directory> [--profile fe-7.4|fe-8.0|fe-1.2]\n  validate <directory>\n  tfs-audit <directory>\n  run <directory>\n  status <directory>\n  generate-key <directory>\n  backup <directory>\n  account create <directory> <account-name> <password>\n  player create <directory> <account-id> <character-name>\n  command <directory> broadcast <message>\n  compatibility\n  version");
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

    #[test]
    fn audits_a_tfs_style_world_without_fe_only_config_assignments() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-tfs-audit-{nonce}"));
        ensure_content_skeleton(&directory).unwrap();
        fs::write(
            directory.join("config.lua"),
            r#"worldType = "pvp"
ip = "127.0.0.1"
gameProtocolPort = 7172
statusProtocolPort = 7171
maxPlayers = 0
serverName = "Private TFS"
mapName = "forgotten"
mysqlHost = "127.0.0.1"
mysqlUser = "forgottenserver"
mysqlDatabase = "forgottenserver"
experienceStages = {
  { minlevel = 1, multiplier = 7 }
}
"#,
        )
        .unwrap();

        fs::create_dir_all(directory.join("data/actions/scripts")).unwrap();
        fs::write(
            directory.join("data/actions/actions.xml"),
            r#"<actions><action itemid="100" script="scripts/rope.lua"/></actions>"#,
        )
        .unwrap();
        fs::write(
            directory.join("data/actions/scripts/rope.lua"),
            "-- private TFS action; inventory only",
        )
        .unwrap();

        audit_tfs_conversion(directory.clone()).unwrap();
        assert!(!directory.join("data/forgotten-engine.db").exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn provisions_a_native_test_account_and_character_without_sql_console_access() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-provision-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();

        account_command(&[
            "account".into(),
            "create".into(),
            directory.display().to_string(),
            "test-account".into(),
            "test-password".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Knight".into(),
        ])
        .unwrap();

        let config = load(&directory).unwrap();
        let database = EngineDatabase::open(&config.database_path).unwrap();
        let account = database
            .authenticate_account_id(1, "test-password")
            .unwrap()
            .unwrap();
        assert_eq!(account.name, "test-account");
        assert_eq!(account.characters[0].name, "Knight");
        let _ = fs::remove_dir_all(directory);
    }
}
