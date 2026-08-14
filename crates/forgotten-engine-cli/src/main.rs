use forgotten_core::{LifecycleCommand, ServerStatus};
use forgotten_persistence::{create_backup, EngineDatabase};
use forgotten_protocol::{profile_by_id, CompatibilityProfile, COMPATIBILITY_PROFILES};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
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
        "run" => run_local(required_path(&arguments, 1)?),
        "status" => status(required_path(&arguments, 1)?),
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
        format!("unknown compatibility profile `{selector}`; use fe-1.2 or fe-8.0").into()
    })
}

fn config_template(profile: CompatibilityProfile) -> String {
    format!(
        "[forgotten_engine]\nname = \"My Forgotten Engine World\"\nip = \"127.0.0.1\"\nport = 7172\ncompatibility_profile = \"{}\"\nengine_version = \"{}\"\ncompatibility_reference = \"{}\"\nprotocol = \"{}\"\n\n[database]\ndriver = \"sqlite\"\npath = \"data/world.db\"\n\n[world]\nmap = \"content/maps/world.otbm\"\npvp = true\n\n[experience]\nrate = 1\n\n[logging]\nlevel = \"info\"\n",
        profile.id, profile.fe_release, profile.compatibility_reference, profile.tibia_protocol
    )
}

fn init(
    directory: PathBuf,
    profile: CompatibilityProfile,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(directory.join("content/maps"))?;
    fs::create_dir_all(directory.join("backups"))?;
    fs::create_dir_all(directory.join("data"))?;
    let config = directory.join("forgotten-engine.toml");
    if config.exists() {
        return Err(format!("{} already exists; refusing to overwrite", config.display()).into());
    }
    fs::write(&config, config_template(profile))?;
    let database = EngineDatabase::open(database_path(&directory))?;
    database.record_event("info", "Forgotten Engine world initialized")?;
    println!(
        "initialized {} with {} / {} / Tibia {} and SQLite schema {}",
        directory.display(),
        profile.id,
        profile.compatibility_reference,
        profile.tibia_protocol,
        database.schema_version()?
    );
    Ok(())
}

fn profile_from_config(contents: &str) -> Result<CompatibilityProfile, Box<dyn std::error::Error>> {
    let selector = config_value(contents, "compatibility_profile")
        .ok_or("missing compatibility_profile in forgotten-engine.toml")?;
    profile_by_id(selector)
        .ok_or_else(|| format!("unsupported compatibility_profile `{selector}`").into())
}

fn validate(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = directory.join("forgotten-engine.toml");
    let contents =
        fs::read_to_string(&config).map_err(|_| format!("missing {}", config.display()))?;
    let profile = profile_from_config(&contents)?;
    let mut failures = Vec::new();
    for (key, expected) in [
        ("engine_version", profile.fe_release),
        ("compatibility_reference", profile.compatibility_reference),
        ("protocol", profile.tibia_protocol),
    ] {
        if !has_config_value(&contents, key, expected) {
            failures.push(format!("{key} must be `{expected}` for {}", profile.id));
        }
    }
    if !has_config_value(&contents, "driver", "sqlite") {
        failures.push("this compatibility foundation requires embedded SQLite".to_owned());
    }
    if !directory.join("content/maps").exists() {
        failures.push("content/maps directory is missing".to_owned());
    }
    let database = EngineDatabase::open(database_path(&directory))?;
    if database.schema_version()? < 1 {
        failures.push("database schema is not migrated".to_owned());
    }
    if failures.is_empty() {
        println!(
            "diagnostics: database=ok config=ok map-directory=ok profile={} reference={} protocol={}",
            profile.id, profile.compatibility_reference, profile.tibia_protocol
        );
        Ok(())
    } else {
        Err(format!("validation failed: {}", failures.join("; ")).into())
    }
}

fn run_local(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    validate(directory.clone())?;
    let profile = read_profile(&directory)?;
    let database = EngineDatabase::open(database_path(&directory))?;
    let status = ServerStatus::Offline
        .apply(LifecycleCommand::Start)?
        .apply(LifecycleCommand::Ready)?;
    database.record_event("info", "Forgotten Engine local runtime started")?;
    println!(
        "status={} profile={} reference={} protocol={} events={}",
        status.as_str(),
        profile.id,
        profile.compatibility_reference,
        profile.tibia_protocol,
        database.event_count()?
    );
    println!("network listener is intentionally not enabled in this compatibility foundation; use this command to verify local configuration and persistence.");
    Ok(())
}

fn status(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let profile = read_profile(&directory)?;
    let database = EngineDatabase::open(database_path(&directory))?;
    println!(
        "database={} schema={} events={} profile={} reference={} target_protocol={}",
        database.path().display(),
        database.schema_version()?,
        database.event_count()?,
        profile.id,
        profile.compatibility_reference,
        profile.tibia_protocol
    );
    Ok(())
}

fn read_profile(directory: &Path) -> Result<CompatibilityProfile, Box<dyn std::error::Error>> {
    let config = directory.join("forgotten-engine.toml");
    let contents =
        fs::read_to_string(&config).map_err(|_| format!("missing {}", config.display()))?;
    profile_from_config(&contents)
}

fn backup(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let artifact = create_backup(database_path(&directory), directory.join("backups"))?;
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
            let database = EngineDatabase::open(database_path(&directory))?;
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
            "FE {}\t{}\tTibia {}\tcomplete-emulation={}",
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

fn config_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key} = ");
    contents.lines().find_map(|line| {
        let value = line.trim().strip_prefix(&prefix)?;
        value.strip_prefix('"')?.strip_suffix('"')
    })
}

fn has_config_value(contents: &str, key: &str, expected: &str) -> bool {
    config_value(contents, key) == Some(expected)
}

fn database_path(directory: &Path) -> PathBuf {
    directory.join("data/world.db")
}

fn print_help() {
    println!("Forgotten Engine\n\nCompatibility profiles:\n  fe-1.2  — TFS 1.2 / Tibia 10.98\n  fe-8.0  — Tibia 8.0\n\nCommands:\n  init <directory> [--profile fe-1.2|fe-8.0]\n  validate <directory>\n  run <directory>\n  status <directory>\n  backup <directory>\n  command <directory> broadcast <message>\n  compatibility\n  version");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_tibia_8_profile_by_direct_selector() {
        let arguments = vec!["init".to_owned(), "world".to_owned(), "fe-8.0".to_owned()];
        let profile = selected_profile(&arguments, 2).unwrap();
        assert_eq!(profile.id, "fe-8.0");
        assert_eq!(profile.tibia_protocol, "8.0");
    }

    #[test]
    fn configuration_validation_is_profile_specific() {
        let config = config_template(forgotten_protocol::FE_8_0_PROFILE);
        let profile = profile_from_config(&config).unwrap();
        assert_eq!(profile, forgotten_protocol::FE_8_0_PROFILE);
        assert!(has_config_value(&config, "protocol", "8.0"));
        assert!(!has_config_value(&config, "protocol", "10.98"));
    }
}
