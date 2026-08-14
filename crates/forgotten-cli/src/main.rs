use forgotten_core::{LifecycleCommand, ServerStatus};
use forgotten_persistence::{create_backup, EngineDatabase};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_TEMPLATE: &str = "[server]\nname = \"My Forgotten Server\"\nip = \"127.0.0.1\"\nport = 7172\nprotocol = \"8.0\"\n\n[database]\ndriver = \"sqlite\"\npath = \"data/server.db\"\n\n[world]\nmap = \"content/maps/world.otbm\"\npvp = true\n\n[experience]\nrate = 1\n\n[logging]\nlevel = \"info\"\n";

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
        "init" => init(required_path(&arguments, 1)?),
        "validate" => validate(required_path(&arguments, 1)?),
        "run" => run_local(required_path(&arguments, 1)?),
        "status" => status(required_path(&arguments, 1)?),
        "backup" => backup(required_path(&arguments, 1)?),
        "command" => command_line(&arguments),
        "compatibility" => compatibility(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(format!("unknown command `{unknown}`; run `forgotten help`").into()),
    }
}

fn required_path(
    arguments: &[String],
    index: usize,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .map(PathBuf::from)
        .ok_or_else(|| "a server directory is required".into())
}

fn init(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(directory.join("content/maps"))?;
    fs::create_dir_all(directory.join("backups"))?;
    let config = directory.join("server.toml");
    if config.exists() {
        return Err(format!("{} already exists; refusing to overwrite", config.display()).into());
    }
    fs::write(&config, CONFIG_TEMPLATE)?;
    let database = EngineDatabase::open(database_path(&directory))?;
    database.record_event("info", "server initialized")?;
    println!(
        "initialized {} with SQLite schema {}",
        directory.display(),
        database.schema_version()?
    );
    Ok(())
}

fn validate(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = directory.join("server.toml");
    let contents =
        fs::read_to_string(&config).map_err(|_| format!("missing {}", config.display()))?;
    let mut failures = Vec::new();
    if !contents.contains("protocol = \"8.0\"") {
        failures.push("protocol must be Tibia 8.0");
    }
    if !contents.contains("driver = \"sqlite\"") {
        failures.push("MVP requires embedded SQLite");
    }
    if !directory.join("content/maps").exists() {
        failures.push("content/maps directory is missing");
    }
    let database = EngineDatabase::open(database_path(&directory))?;
    if database.schema_version()? < 1 {
        failures.push("database schema is not migrated");
    }
    if failures.is_empty() {
        println!(
            "diagnostics: database=ok config=ok map-directory=ok protocol={}",
            forgotten_protocol::TARGET_PROTOCOL
        );
        Ok(())
    } else {
        Err(format!("validation failed: {}", failures.join("; ")).into())
    }
}

fn run_local(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    validate(directory.clone())?;
    let database = EngineDatabase::open(database_path(&directory))?;
    let status = ServerStatus::Offline
        .apply(LifecycleCommand::Start)?
        .apply(LifecycleCommand::Ready)?;
    database.record_event("info", "local engine runtime started")?;
    println!(
        "status={} protocol={} events={}",
        status.as_str(),
        forgotten_protocol::TARGET_PROTOCOL,
        database.event_count()?
    );
    println!("network listener is intentionally not enabled in 0.1; use this command to verify local configuration and persistence.");
    Ok(())
}

fn status(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let database = EngineDatabase::open(database_path(&directory))?;
    println!(
        "database={} schema={} events={} target_protocol={}",
        database.path().display(),
        database.schema_version()?,
        database.event_count()?,
        forgotten_protocol::TARGET_PROTOCOL
    );
    Ok(())
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
            println!("recorded broadcast command");
            Ok(())
        }
        unsupported => Err(format!("unsupported command action `{unsupported}`").into()),
    }
}

fn compatibility() -> Result<(), Box<dyn std::error::Error>> {
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

fn database_path(directory: &Path) -> PathBuf {
    directory.join("data/server.db")
}

fn print_help() {
    println!("Forgotten Engine 0.1\n\nCommands:\n  init <directory>\n  validate <directory>\n  run <directory>\n  status <directory>\n  backup <directory>\n  command <directory> broadcast <message>\n  compatibility");
}
