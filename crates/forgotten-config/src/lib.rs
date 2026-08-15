//! Bounded, typed configuration and original content-skeleton contracts for Forgotten Engine.
//!
//! The loader recognizes a deliberately limited `config.lua` assignment subset. It does not
//! execute Lua; a sandboxed scripting runtime belongs to a later milestone.

use forgotten_core::{Position, WorldMap, WorldMapTile};
use forgotten_protocol::{profile_by_id, CompatibilityProfile, NativeOtClientProfile};
use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE_NAME: &str = "config.lua";
pub const CONTENT_MANIFEST_NAME: &str = "fe-content.manifest";
pub const EMPTY_WORLD_MANIFEST_NAME: &str = "fe-empty-world.manifest";
pub const FE_MAP_EXTENSION: &str = "femap";
pub const FE_MAP_FORMAT: &str = "fe-map-v1";
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
    pub game_session_enabled: bool,
    pub game_session_port: u16,
    pub advertised_game_session_host: String,
    pub advertised_game_session_port: u16,
    pub otclient_v8_native_enabled: bool,
    pub otclient_v8_login_port: u16,
    pub otclient_v8_game_port: u16,
    pub advertised_otclient_v8_host: String,
    pub advertised_otclient_v8_game_port: u16,
    pub otclient_v8_protocol_version: u16,
    pub otclient_v8_numeric_account_ids: bool,
    pub otclient_v8_login_packet_encryption: bool,
    pub otclient_v8_protocol_checksum: bool,
    pub otclient_v8_challenge_on_login: bool,
    pub otclient_v8_native_empty_world_enabled: bool,
    pub otclient_v8_empty_world_ground_thing_id: u16,
    pub otclient_v8_player_look_type: u16,
    pub otclient_v8_player_speed: u16,
    pub otclient_v8_server_beat: u16,
}

impl EngineConfig {
    pub fn game_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.game_protocol_port)
    }

    pub fn status_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.status_protocol_port)
    }

    pub fn game_session_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.game_session_port)
    }

    pub fn otclient_v8_login_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.otclient_v8_login_port)
    }

    pub fn otclient_v8_game_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.otclient_v8_game_port)
    }

    pub fn otclient_v8_native_profile(&self) -> NativeOtClientProfile {
        NativeOtClientProfile {
            protocol_version: self.otclient_v8_protocol_version,
            numeric_account_ids: self.otclient_v8_numeric_account_ids,
            login_packet_encryption: self.otclient_v8_login_packet_encryption,
            protocol_checksum: self.otclient_v8_protocol_checksum,
            challenge_on_login: self.otclient_v8_challenge_on_login,
            max_padding_bytes: 128,
        }
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

    let bind_ip: IpAddr =
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

    let game_session_port = optional_u16(&values, "gameSessionPort", 7173)?;
    let advertised_game_session_host =
        optional_string_owned(&values, "advertisedGameSessionHost", bind_ip.to_string())?;
    let advertised_game_session_port =
        optional_u16(&values, "advertisedGameSessionPort", game_session_port)?;
    let otclient_v8_login_port = optional_u16(&values, "otclientV8LoginPort", 7174)?;
    let otclient_v8_game_port = optional_u16(&values, "otclientV8GamePort", 7175)?;
    let advertised_otclient_v8_host =
        optional_string_owned(&values, "advertisedOtClientV8Host", bind_ip.to_string())?;
    let advertised_otclient_v8_game_port = optional_u16(
        &values,
        "advertisedOtClientV8GamePort",
        otclient_v8_game_port,
    )?;
    let otclient_v8_native_enabled = optional_boolean(&values, "otclientV8NativeEnabled", false)?;
    let otclient_v8_protocol_version = optional_u16(&values, "otclientV8ProtocolVersion", 0)?;
    let otclient_v8_numeric_account_ids =
        optional_boolean(&values, "otclientV8NumericAccountIds", true)?;
    let otclient_v8_login_packet_encryption =
        optional_boolean(&values, "otclientV8LoginPacketEncryption", false)?;
    let otclient_v8_protocol_checksum =
        optional_boolean(&values, "otclientV8ProtocolChecksum", false)?;
    let otclient_v8_challenge_on_login =
        optional_boolean(&values, "otclientV8ChallengeOnLogin", false)?;
    let otclient_v8_native_empty_world_enabled =
        optional_boolean(&values, "otclientV8NativeEmptyWorldEnabled", false)?;
    let otclient_v8_empty_world_ground_thing_id =
        optional_u16(&values, "otclientV8EmptyWorldGroundThingId", 0)?;
    let otclient_v8_player_look_type = optional_u16(&values, "otclientV8PlayerLookType", 0)?;
    let otclient_v8_player_speed = optional_u16(&values, "otclientV8PlayerSpeed", 220)?;
    let otclient_v8_server_beat = optional_u16(&values, "otclientV8ServerBeat", 50)?;
    let native_profile = NativeOtClientProfile {
        protocol_version: otclient_v8_protocol_version,
        numeric_account_ids: otclient_v8_numeric_account_ids,
        login_packet_encryption: otclient_v8_login_packet_encryption,
        protocol_checksum: otclient_v8_protocol_checksum,
        challenge_on_login: otclient_v8_challenge_on_login,
        max_padding_bytes: 128,
    };
    if otclient_v8_native_enabled && !native_profile.supports_current_native_foundation() {
        return Err(ConfigError::InvalidValue {
            key: "otclientV8NativeEnabled",
            message: "requires a selected plain numeric-account native client profile".into(),
        });
    }
    if otclient_v8_native_empty_world_enabled
        && (!otclient_v8_native_enabled
            || !native_profile.supports_current_native_foundation()
            || otclient_v8_player_look_type > u8::MAX as u16
            || otclient_v8_player_speed == 0
            || otclient_v8_server_beat == 0)
    {
        return Err(ConfigError::InvalidValue {
            key: "otclientV8NativeEmptyWorldEnabled",
            message: "requires an enabled supported native profile plus valid optional asset IDs and nonzero speed/server-beat values".into(),
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
        game_session_enabled: optional_boolean(&values, "gameSessionEnabled", false)?,
        game_session_port,
        advertised_game_session_host,
        advertised_game_session_port,
        otclient_v8_native_enabled,
        otclient_v8_login_port,
        otclient_v8_game_port,
        advertised_otclient_v8_host,
        advertised_otclient_v8_game_port,
        otclient_v8_protocol_version,
        otclient_v8_numeric_account_ids,
        otclient_v8_login_packet_encryption,
        otclient_v8_protocol_checksum,
        otclient_v8_challenge_on_login,
        otclient_v8_native_empty_world_enabled,
        otclient_v8_empty_world_ground_thing_id,
        otclient_v8_player_look_type,
        otclient_v8_player_speed,
        otclient_v8_server_beat,
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
    let empty_world = data.join("world").join(EMPTY_WORLD_MANIFEST_NAME);
    if !empty_world.exists() {
        fs::write(
            empty_world,
            "format=fe-empty-world-v1\nworld=empty\nviewport_radius_x=8\nviewport_radius_y=6\nsource=original-forgotten-engine-content-contract\n",
        )
        .map_err(ConfigError::Io)?;
    }
    let default_map = data
        .join("world")
        .join(format!("forgotten.{FE_MAP_EXTENSION}"));
    if !default_map.exists() {
        fs::write(
            default_map,
            "# Forgotten Engine original map document\nformat=fe-map-v1\nspawn=100,100,7\n# x1,y1,x2,y2,z,groundThingId,walkable\nfill=80,80,120,120,7,0,true\n",
        )
        .map_err(ConfigError::Io)?;
    }
    Ok(())
}

pub fn world_map_path(config: &EngineConfig) -> Result<PathBuf, ConfigError> {
    if config.map_name.is_empty()
        || !config.map_name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return Err(ConfigError::InvalidContent(
            "mapName must contain only ASCII letters, digits, underscores, or hyphens".into(),
        ));
    }
    Ok(config
        .content_directory
        .join("world")
        .join(format!("{}.{}", config.map_name, FE_MAP_EXTENSION)))
}

pub fn load_world_map(config: &EngineConfig) -> Result<WorldMap, ConfigError> {
    let path = world_map_path(config)?;
    let source = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ConfigError::InvalidContent(format!(
                "missing selected map {}; expected an original {} document",
                path.display(),
                FE_MAP_EXTENSION
            ))
        } else {
            ConfigError::Io(error)
        }
    })?;
    parse_world_map(&config.map_name, &source)
}

fn parse_world_map(identifier: &str, source: &str) -> Result<WorldMap, ConfigError> {
    let mut format_seen = false;
    let mut spawn = None;
    let mut declarations = Vec::new();
    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(ConfigError::InvalidContent(format!(
                "map line {} must use key=value syntax",
                index + 1
            )))?;
        match key.trim() {
            "format" if value.trim() == FE_MAP_FORMAT => format_seen = true,
            "format" => {
                return Err(ConfigError::InvalidContent(format!(
                    "map line {} must declare format={FE_MAP_FORMAT}",
                    index + 1
                )))
            }
            "spawn" => {
                if spawn.is_some() {
                    return Err(ConfigError::InvalidContent(
                        "map declares spawn more than once".into(),
                    ));
                }
                spawn = Some(parse_position(value.trim(), index + 1)?);
            }
            "tile" | "fill" => declarations.push((key.trim(), value.trim(), index + 1)),
            other => {
                return Err(ConfigError::InvalidContent(format!(
                    "map line {} has unsupported key `{other}`",
                    index + 1
                )))
            }
        }
    }
    if !format_seen {
        return Err(ConfigError::InvalidContent(format!(
            "map must declare format={FE_MAP_FORMAT}"
        )));
    }
    let spawn =
        spawn.ok_or_else(|| ConfigError::InvalidContent("map must declare spawn=x,y,z".into()))?;
    let mut map = WorldMap::new(identifier, spawn);
    for (kind, value, line) in declarations {
        match kind {
            "tile" => {
                let fields = split_map_fields(value, 5, line)?;
                let position = parse_position_fields(&fields[0..3], line)?;
                let tile = parse_tile_fields(&fields[3..5], line)?;
                map.set_tile(position, tile).map_err(|error| {
                    ConfigError::InvalidContent(format!("map line {line}: {error}"))
                })?;
            }
            "fill" => {
                let fields = split_map_fields(value, 7, line)?;
                let x_start = parse_map_u16(fields[0], line, "x1")?;
                let y_start = parse_map_u16(fields[1], line, "y1")?;
                let x_end = parse_map_u16(fields[2], line, "x2")?;
                let y_end = parse_map_u16(fields[3], line, "y2")?;
                let z = parse_map_u8(fields[4], line, "z")?;
                if x_start > x_end || y_start > y_end {
                    return Err(ConfigError::InvalidContent(format!(
                        "map line {line}: fill bounds must be ascending"
                    )));
                }
                let tile = parse_tile_fields(&fields[5..7], line)?;
                for x in x_start..=x_end {
                    for y in y_start..=y_end {
                        map.set_tile(Position { x, y, z }, tile).map_err(|error| {
                            ConfigError::InvalidContent(format!("map line {line}: {error}"))
                        })?;
                    }
                }
            }
            _ => unreachable!("map declarations were validated"),
        }
    }
    map.validate()
        .map_err(|error| ConfigError::InvalidContent(format!("invalid map: {error}")))?;
    Ok(map)
}

fn split_map_fields(value: &str, expected: usize, line: usize) -> Result<Vec<&str>, ConfigError> {
    let fields = value.split(',').map(str::trim).collect::<Vec<_>>();
    if fields.len() != expected || fields.iter().any(|field| field.is_empty()) {
        return Err(ConfigError::InvalidContent(format!(
            "map line {line}: expected {expected} comma-separated values"
        )));
    }
    Ok(fields)
}

fn parse_position(value: &str, line: usize) -> Result<Position, ConfigError> {
    let fields = split_map_fields(value, 3, line)?;
    parse_position_fields(&fields, line)
}

fn parse_position_fields(fields: &[&str], line: usize) -> Result<Position, ConfigError> {
    Ok(Position {
        x: parse_map_u16(fields[0], line, "x")?,
        y: parse_map_u16(fields[1], line, "y")?,
        z: parse_map_u8(fields[2], line, "z")?,
    })
}

fn parse_tile_fields(fields: &[&str], line: usize) -> Result<WorldMapTile, ConfigError> {
    let walkable = match fields[1] {
        "true" => true,
        "false" => false,
        _ => {
            return Err(ConfigError::InvalidContent(format!(
                "map line {line}: walkable must be true or false"
            )))
        }
    };
    Ok(WorldMapTile {
        ground_thing_id: parse_map_u16(fields[0], line, "groundThingId")?,
        walkable,
    })
}

fn parse_map_u16(value: &str, line: usize, field: &str) -> Result<u16, ConfigError> {
    value
        .parse::<u16>()
        .map_err(|_| ConfigError::InvalidContent(format!("map line {line}: {field} must be a u16")))
}

fn parse_map_u8(value: &str, line: usize, field: &str) -> Result<u8, ConfigError> {
    value
        .parse::<u8>()
        .map_err(|_| ConfigError::InvalidContent(format!("map line {line}: {field} must be a u8")))
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
    let empty_world_manifest = data.join("world").join(EMPTY_WORLD_MANIFEST_NAME);
    let empty_world_contents = fs::read_to_string(&empty_world_manifest).map_err(|_| {
        ConfigError::InvalidContent(format!("missing {}", empty_world_manifest.display()))
    })?;
    for required_line in ["format=fe-empty-world-v1", "world=empty"] {
        if !empty_world_contents
            .lines()
            .any(|line| line == required_line)
        {
            return Err(ConfigError::InvalidContent(format!(
                "{} is not an FE empty-world manifest",
                empty_world_manifest.display()
            )));
        }
    }
    Ok(ContentReport {
        data_directory: data,
        missing_directories,
        manifest,
        empty_world_manifest,
    })
}

pub fn template(profile: CompatibilityProfile) -> String {
    format!(
        "-- Forgotten Engine configuration\n-- TFS-style layout; parsed as a bounded assignment subset during P0.\n\n-- Connection Config\nip = \"127.0.0.1\"\ngameProtocolPort = 7172\nstatusProtocolPort = 7171\nmaxPlayers = 0\nserverName = \"Forgotten Engine\"\n\n-- Legacy login foundation\n-- Set true only after providing an original 1024-bit RSA private key.\nlegacyLoginEnabled = false\nrsaPrivateKey = \"key.pem\"\n\n-- Legacy game-session foundation\n-- Separate opt-in port until official session compatibility is proven.\ngameSessionEnabled = false\ngameSessionPort = 7173\n-- Public endpoint advertised to a custom OTClient module; may be a proxy/domain/IP-changing endpoint.\nadvertisedGameSessionHost = \"127.0.0.1\"\nadvertisedGameSessionPort = 7173\n\n-- Native stock OTClientV8 foundation\n-- Select the compatible protocol and feature switches for this world; do not assume FE release versions.\notclientV8NativeEnabled = false\notclientV8LoginPort = 7174\notclientV8GamePort = 7175\notclientV8ProtocolVersion = 0\notclientV8NumericAccountIds = true\notclientV8LoginPacketEncryption = false\notclientV8ProtocolChecksum = false\notclientV8ChallengeOnLogin = false\n-- Address returned in the native legacy character list.\nadvertisedOtClientV8Host = \"127.0.0.1\"\nadvertisedOtClientV8GamePort = 7175\n\n-- Native empty-world fixture. Nonzero IDs must exist in operator-owned matching OTCv8 data; zero selects an asset-free fallback.\notclientV8NativeEmptyWorldEnabled = false\notclientV8EmptyWorldGroundThingId = 0\notclientV8PlayerLookType = 0\notclientV8PlayerSpeed = 220\notclientV8ServerBeat = 50\n\n-- Map\nmapName = \"forgotten\"\nworldType = \"pvp\"\n\n-- MySQL compatibility contract (SQLite remains the current storage backend)\nmysqlHost = \"127.0.0.1\"\nmysqlUser = \"forgottenengine\"\nmysqlDatabase = \"forgottenengine\"\n\n-- Forgotten Engine profile\nfeProfile = \"{}\"\ntibiaProtocol = \"{}\"\n",
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

fn optional_string_owned(
    values: &BTreeMap<String, Literal>,
    key: &'static str,
    default: String,
) -> Result<String, ConfigError> {
    match values.get(key) {
        Some(Literal::String(value)) => Ok(value.clone()),
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

fn optional_u16(
    values: &BTreeMap<String, Literal>,
    key: &'static str,
    default: u16,
) -> Result<u16, ConfigError> {
    match values.get(key) {
        Some(Literal::Integer(value)) => {
            u16::try_from(*value).map_err(|_| ConfigError::InvalidValue {
                key,
                message: "must be between 0 and 65535".into(),
            })
        }
        Some(_) => Err(ConfigError::InvalidValue {
            key,
            message: "must be an integer".into(),
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
    pub empty_world_manifest: PathBuf,
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
        assert!(!config.game_session_enabled);
        assert_eq!(config.game_session_port, 7173);
        assert_eq!(config.advertised_game_session_host, "127.0.0.1");
        assert_eq!(config.advertised_game_session_port, 7173);
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn loads_explicit_legacy_login_settings() {
        let world = temporary_world("legacy-login");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}legacyLoginEnabled = true\nrsaPrivateKey = \"keys/legacy.pem\"\ngameSessionEnabled = true\ngameSessionPort = 7183\nadvertisedGameSessionHost = \"fe.example.test\"\nadvertisedGameSessionPort = 443\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();
        let config = load(&world).unwrap();
        assert!(config.legacy_login_enabled);
        assert_eq!(config.rsa_private_key_path, world.join("keys/legacy.pem"));
        assert!(config.game_session_enabled);
        assert_eq!(config.game_session_port, 7183);
        assert_eq!(config.advertised_game_session_host, "fe.example.test");
        assert_eq!(config.advertised_game_session_port, 443);
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn loads_a_profile_driven_native_otclient_endpoint() {
        let world = temporary_world("native-otclient");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}otclientV8NativeEnabled = true\notclientV8LoginPort = 7264\notclientV8GamePort = 7265\nadvertisedOtClientV8Host = \"203.0.113.24\"\nadvertisedOtClientV8GamePort = 7265\notclientV8ProtocolVersion = 740\notclientV8NumericAccountIds = true\notclientV8LoginPacketEncryption = false\notclientV8ProtocolChecksum = false\notclientV8ChallengeOnLogin = false\notclientV8NativeEmptyWorldEnabled = true\notclientV8EmptyWorldGroundThingId = 102\notclientV8PlayerLookType = 128\notclientV8PlayerSpeed = 220\notclientV8ServerBeat = 50\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();

        let config = load(&world).unwrap();
        assert!(config.otclient_v8_native_enabled);
        assert_eq!(config.otclient_v8_login_socket_addr().port(), 7264);
        assert_eq!(config.otclient_v8_game_socket_addr().port(), 7265);
        assert_eq!(config.advertised_otclient_v8_host, "203.0.113.24");
        assert_eq!(config.advertised_otclient_v8_game_port, 7265);
        assert!(config.otclient_v8_native_empty_world_enabled);
        assert_eq!(config.otclient_v8_empty_world_ground_thing_id, 102);
        assert_eq!(config.otclient_v8_player_look_type, 128);
        assert_eq!(config.otclient_v8_player_speed, 220);
        assert_eq!(config.otclient_v8_server_beat, 50);
        assert!(config
            .otclient_v8_native_profile()
            .supports_current_native_foundation());
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn rejects_an_incomplete_enabled_native_otclient_profile() {
        let world = temporary_world("incomplete-native-otclient");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}otclientV8NativeEnabled = true\notclientV8ProtocolVersion = 740\notclientV8LoginPacketEncryption = true\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();

        assert!(matches!(
            load(&world),
            Err(ConfigError::InvalidValue {
                key: "otclientV8NativeEnabled",
                ..
            })
        ));
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn accepts_an_asset_free_empty_world_fixture_and_rejects_zero_timing() {
        let world = temporary_world("asset-free-native-world");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}otclientV8NativeEnabled = true\notclientV8ProtocolVersion = 740\notclientV8NativeEmptyWorldEnabled = true\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();

        let config = load(&world).unwrap();
        assert!(config.otclient_v8_native_empty_world_enabled);
        assert_eq!(config.otclient_v8_empty_world_ground_thing_id, 0);
        assert_eq!(config.otclient_v8_player_look_type, 0);

        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}otclientV8NativeEnabled = true\notclientV8ProtocolVersion = 740\notclientV8NativeEmptyWorldEnabled = true\notclientV8PlayerSpeed = 0\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();

        assert!(matches!(
            load(&world),
            Err(ConfigError::InvalidValue {
                key: "otclientV8NativeEmptyWorldEnabled",
                ..
            })
        ));
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
        assert!(report.empty_world_manifest.is_file());
        fs::remove_file(&report.empty_world_manifest).unwrap();
        ensure_content_skeleton(&world).unwrap();
        assert!(validate_content(&world)
            .unwrap()
            .empty_world_manifest
            .is_file());
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn parses_the_generated_original_map_document_and_rejects_an_unwalkable_spawn() {
        let world = temporary_world("map-document");
        ensure_content_skeleton(&world).unwrap();
        let source = fs::read_to_string(world.join("data/world/forgotten.femap")).unwrap();
        let map = parse_world_map("forgotten", &source).unwrap();
        assert_eq!(map.identifier(), "forgotten");
        assert_eq!(
            map.spawn(),
            Position {
                x: 100,
                y: 100,
                z: 7
            }
        );
        assert!(map.is_walkable(map.spawn()));
        assert!(map.tile_count() > 1_000);
        assert!(matches!(
            parse_world_map(
                "invalid-spawn",
                "format=fe-map-v1\nspawn=100,100,7\ntile=100,100,7,0,false\n"
            ),
            Err(ConfigError::InvalidContent(_))
        ));
        let _ = fs::remove_dir_all(world);
    }
}
