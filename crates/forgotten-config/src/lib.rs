//! Bounded, typed configuration and original content-skeleton contracts for Forgotten Engine.
//!
//! The loader recognizes a deliberately limited `config.lua` assignment subset. It does not
//! execute Lua; a sandboxed scripting runtime belongs to a later milestone.

use forgotten_protocol::{profile_by_id, CompatibilityProfile};
use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE_NAME: &str = "config.lua";
pub const CONTENT_MANIFEST_NAME: &str = "fe-content.manifest";
pub const REQUIRED_CONTENT_DIRECTORIES: [&str; 15] = [
    "actions",
    "creaturescripts",
    "events",
    "globalevents",
    "items",
    "lib",
    "migrations",
    "monster",
    "movements",
    "npc",
    "spells",
    "talkactions",
    "weapons",
    "world",
    "XML",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub bind_ip: IpAddr,
    pub game_protocol_port: u16,
    pub status_protocol_port: u16,
    pub max_players: u32,
    pub server_name: String,
    pub map_name: String,
    pub world_type: WorldType,
    pub mysql: MysqlConfig,
    pub profile: CompatibilityProfile,
    pub content_directory: PathBuf,
    pub database_path: PathBuf,
    pub legacy_login_enabled: bool,
    pub rsa_private_key_path: PathBuf,
}

impl EngineConfig {
    pub fn game_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.game_protocol_port)
    }

    pub fn status_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.status_protocol_port)
    }

    pub fn max_connections(&self) -> usize {
        match self.max_players {
            0 => 128,
            value => value as usize,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MysqlConfig {
    pub host: String,
    pub user: String,
    pub database: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldType {
    Pvp,
    NoPvp,
    PvpEnforced,
}

impl WorldType {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "pvp" => Ok(Self::Pvp),
            "no-pvp" => Ok(Self::NoPvp),
            "pvp-enforced" => Ok(Self::PvpEnforced),
            other => Err(ConfigError::InvalidValue {
                key: "worldType",
                message: format!("unsupported world type `{other}`"),
            }),
        }
    }
}

pub fn load(world_directory: impl AsRef<Path>) -> Result<EngineConfig, ConfigError> {
    let world_directory = world_directory.as_ref();
    let path = world_directory.join(CONFIG_FILE_NAME);
    let contents = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ConfigError::Missing(path.clone())
        } else {
            ConfigError::Io(error)
        }
    })?;
    let values = parse_assignments(&contents)?;

    let bind_ip =
        required_string(&values, "ip")?
            .parse()
            .map_err(|_| ConfigError::InvalidValue {
                key: "ip",
                message: "must be an IPv4 or IPv6 address".into(),
            })?;
    let game_protocol_port = required_u16(&values, "gameProtocolPort")?;
    let status_protocol_port = required_u16(&values, "statusProtocolPort")?;
    let max_players = required_u32(&values, "maxPlayers")?;
    let profile_id = required_string(&values, "feProfile")?;
    let profile = profile_by_id(profile_id).ok_or_else(|| ConfigError::InvalidValue {
        key: "feProfile",
        message: format!("unknown Forgotten Engine profile `{profile_id}`"),
    })?;
    let protocol = required_string(&values, "tibiaProtocol")?;
    if protocol != profile.tibia_protocol {
        return Err(ConfigError::InvalidValue {
            key: "tibiaProtocol",
            message: format!(
                "must be `{}` for profile `{}`",
                profile.tibia_protocol, profile.id
            ),
        });
    }

    Ok(EngineConfig {
        bind_ip,
        game_protocol_port,
        status_protocol_port,
        max_players,
        server_name: required_string(&values, "serverName")?.to_owned(),
        map_name: required_string(&values, "mapName")?.to_owned(),
        world_type: WorldType::parse(required_string(&values, "worldType")?)?,
        mysql: MysqlConfig {
            host: required_string(&values, "mysqlHost")?.to_owned(),
            user: required_string(&values, "mysqlUser")?.to_owned(),
            database: required_string(&values, "mysqlDatabase")?.to_owned(),
        },
        profile,
        content_directory: world_directory.join("data"),
        database_path: world_directory.join("data/forgotten-engine.db"),
        legacy_login_enabled: optional_boolean(&values, "legacyLoginEnabled", false)?,
        rsa_private_key_path: world_directory.join(optional_string(
            &values,
            "rsaPrivateKey",
            "key.pem",
        )?),
    })
}

pub fn write_template(
    world_directory: impl AsRef<Path>,
    profile: CompatibilityProfile,
) -> Result<(), ConfigError> {
    let world_directory = world_directory.as_ref();
    let path = world_directory.join(CONFIG_FILE_NAME);
    if path.exists() {
        return Err(ConfigError::AlreadyExists(path));
    }
    fs::write(path, template(profile)).map_err(ConfigError::Io)
}

pub fn ensure_content_skeleton(world_directory: impl AsRef<Path>) -> Result<(), ConfigError> {
    let data = world_directory.as_ref().join("data");
    for directory in REQUIRED_CONTENT_DIRECTORIES {
        fs::create_dir_all(data.join(directory)).map_err(ConfigError::Io)?;
    }
    let manifest = data.join(CONTENT_MANIFEST_NAME);
    if !manifest.exists() {
        fs::write(
            manifest,
            "format=fe-content-v1\nsource=original-forgotten-engine-content-contract\nstatus=empty-skeleton\n",
        )
        .map_err(ConfigError::Io)?;
    }
    Ok(())
}

pub fn validate_content(world_directory: impl AsRef<Path>) -> Result<ContentReport, ConfigError> {
    let data = world_directory.as_ref().join("data");
    let missing_directories = REQUIRED_CONTENT_DIRECTORIES
        .iter()
        .filter(|directory| !data.join(directory).is_dir())
        .map(|directory| (*directory).to_owned())
        .collect::<Vec<_>>();
    let manifest = data.join(CONTENT_MANIFEST_NAME);
    if !missing_directories.is_empty() {
        return Err(ConfigError::InvalidContent(format!(
            "missing data directories: {}",
            missing_directories.join(", ")
        )));
    }
    let manifest_contents = fs::read_to_string(&manifest)
        .map_err(|_| ConfigError::InvalidContent(format!("missing {}", manifest.display())))?;
    if !manifest_contents
        .lines()
        .any(|line| line == "format=fe-content-v1")
    {
        return Err(ConfigError::InvalidContent(format!(
            "{} is not an FE content manifest",
            manifest.display()
        )));
    }
    Ok(ContentReport {
        data_directory: data,
        missing_directories,
        manifest,
    })
}

pub fn template(profile: CompatibilityProfile) -> String {
    format!(
        "-- Forgotten Engine configuration\n-- TFS-style layout; parsed as a bounded assignment subset during P0.\n\n-- Connection Config\nip = \"127.0.0.1\"\ngameProtocolPort = 7172\nstatusProtocolPort = 7171\nmaxPlayers = 0\nserverName = \"Forgotten Engine\"\n\n-- Legacy login foundation\n-- Set true only after providing an original 1024-bit RSA private key.\nlegacyLoginEnabled = false\nrsaPrivateKey = \"key.pem\"\n\n-- Map\nmapName = \"forgotten\"\nworldType = \"pvp\"\n\n-- MySQL compatibility contract (SQLite remains the current storage backend)\nmysqlHost = \"127.0.0.1\"\nmysqlUser = \"forgottenengine\"\nmysqlDatabase = \"forgottenengine\"\n\n-- Forgotten Engine profile\nfeProfile = \"{}\"\ntibiaProtocol = \"{}\"\n",
        profile.id, profile.tibia_protocol
    )
}

fn parse_assignments(contents: &str) -> Result<BTreeMap<String, Literal>, ConfigError> {
    let mut values = BTreeMap::new();
    for (index, source_line) in contents.lines().enumerate() {
        let line = source_line.split("--").next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let (key, raw_value) = line.split_once('=').ok_or(ConfigError::Syntax {
            line: index + 1,
            message: "expected key = value".into(),
        })?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(ConfigError::Syntax {
                line: index + 1,
                message: "invalid key".into(),
            });
        }
        let literal = Literal::parse(raw_value.trim()).map_err(|message| ConfigError::Syntax {
            line: index + 1,
            message,
        })?;
        values.insert(key.to_owned(), literal);
    }
    Ok(values)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Literal {
    String(String),
    Integer(i64),
    Boolean(bool),
}

impl Literal {
    fn parse(value: &str) -> Result<Self, String> {
        if let Some(value) = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
        {
            return Ok(Self::String(value.to_owned()));
        }
        if value == "true" {
            return Ok(Self::Boolean(true));
        }
        if value == "false" {
            return Ok(Self::Boolean(false));
        }
        value
            .parse::<i64>()
            .map(Self::Integer)
            .map_err(|_| "expected quoted string, integer, or boolean literal".into())
    }
}

fn required_string<'a>(
    values: &'a BTreeMap<String, Literal>,
    key: &'static str,
) -> Result<&'a str, ConfigError> {
    match values.get(key) {
        Some(Literal::String(value)) => Ok(value),
        Some(_) => Err(ConfigError::InvalidValue {
            key,
            message: "must be a quoted string".into(),
        }),
        None => Err(ConfigError::MissingValue(key)),
    }
}

fn optional_string<'a>(
    values: &'a BTreeMap<String, Literal>,
    key: &'static str,
    default: &'static str,
) -> Result<&'a str, ConfigError> {
    match values.get(key) {
        Some(Literal::String(value)) => Ok(value),
        Some(_) => Err(ConfigError::InvalidValue {
            key,
            message: "must be a quoted string".into(),
        }),
        None => Ok(default),
    }
}

fn optional_boolean(
    values: &BTreeMap<String, Literal>,
    key: &'static str,
    default: bool,
) -> Result<bool, ConfigError> {
    match values.get(key) {
        Some(Literal::Boolean(value)) => Ok(*value),
        Some(_) => Err(ConfigError::InvalidValue {
            key,
            message: "must be true or false".into(),
        }),
        None => Ok(default),
    }
}

fn required_u16(values: &BTreeMap<String, Literal>, key: &'static str) -> Result<u16, ConfigError> {
    let value = required_u32(values, key)?;
    u16::try_from(value).map_err(|_| ConfigError::InvalidValue {
        key,
        message: "must be between 0 and 65535".into(),
    })
}

fn required_u32(values: &BTreeMap<String, Literal>, key: &'static str) -> Result<u32, ConfigError> {
    match values.get(key) {
        Some(Literal::Integer(value)) => {
            u32::try_from(*value).map_err(|_| ConfigError::InvalidValue {
                key,
                message: "must be a non-negative integer".into(),
            })
        }
        Some(_) => Err(ConfigError::InvalidValue {
            key,
            message: "must be an integer".into(),
        }),
        None => Err(ConfigError::MissingValue(key)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentReport {
    pub data_directory: PathBuf,
    pub missing_directories: Vec<String>,
    pub manifest: PathBuf,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Missing(PathBuf),
    AlreadyExists(PathBuf),
    MissingValue(&'static str),
    InvalidValue { key: &'static str, message: String },
    Syntax { line: usize, message: String },
    InvalidContent(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Missing(path) => write!(formatter, "cannot open {}", path.display()),
            Self::AlreadyExists(path) => write!(formatter, "{} already exists", path.display()),
            Self::MissingValue(key) => write!(formatter, "missing required config value `{key}`"),
            Self::InvalidValue { key, message } => write!(formatter, "invalid `{key}`: {message}"),
            Self::Syntax { line, message } => {
                write!(formatter, "config syntax error at line {line}: {message}")
            }
            Self::InvalidContent(message) => {
                write!(formatter, "content validation failed: {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;
    use forgotten_protocol::FE_7_4_PROFILE;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_world(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forgotten-engine-config-{name}-{nonce}"))
    }

    #[test]
    fn creates_and_loads_a_tfs_style_config_contract() {
        let world = temporary_world("load");
        fs::create_dir_all(&world).unwrap();
        write_template(&world, FE_7_4_PROFILE).unwrap();
        let config = load(&world).unwrap();
        assert_eq!(config.profile, FE_7_4_PROFILE);
        assert_eq!(config.game_protocol_port, 7172);
        assert_eq!(config.world_type, WorldType::Pvp);
        assert!(!config.legacy_login_enabled);
        assert_eq!(config.rsa_private_key_path, world.join("key.pem"));
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn loads_explicit_legacy_login_settings() {
        let world = temporary_world("legacy-login");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}legacyLoginEnabled = true\nrsaPrivateKey = \"keys/legacy.pem\"\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();
        let config = load(&world).unwrap();
        assert!(config.legacy_login_enabled);
        assert_eq!(config.rsa_private_key_path, world.join("keys/legacy.pem"));
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn refuses_profile_protocol_mismatch() {
        let world = temporary_world("mismatch");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            template(FE_7_4_PROFILE).replace("tibiaProtocol = \"7.4\"", "tibiaProtocol = \"8.0\""),
        )
        .unwrap();
        assert!(matches!(
            load(&world),
            Err(ConfigError::InvalidValue {
                key: "tibiaProtocol",
                ..
            })
        ));
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn creates_and_validates_an_original_content_skeleton() {
        let world = temporary_world("content");
        ensure_content_skeleton(&world).unwrap();
        let report = validate_content(&world).unwrap();
        assert!(report.missing_directories.is_empty());
        let _ = fs::remove_dir_all(world);
    }
}
