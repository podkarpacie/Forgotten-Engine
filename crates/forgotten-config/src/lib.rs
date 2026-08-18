//! Bounded, typed configuration and original content-skeleton contracts for Forgotten Engine.
//!
//! The loader recognizes a deliberately limited `config.lua` assignment subset. It does not
//! execute Lua; a sandboxed scripting runtime belongs to a later milestone.

mod items;
mod legacy_xml;
mod otbm;
mod spells;
mod stages;
mod tfs_entities;
mod tfs_registry;
mod vocations;
mod weapons;

use forgotten_core::{
    OtbmMapHeader, Position, WorldMap, WorldMapItem, WorldMapSource, WorldMapTile, WorldMapTown,
};
use forgotten_protocol::{
    profile_by_id, CompatibilityProfile, NativeOtClientFoundation, NativeOtClientProfile,
};
use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

pub use items::{LegacyItemCatalog, LegacyItemDefinition};
pub use legacy_xml::{
    LegacyHouse, LegacySpawnArea, LegacySpawnCreature, LegacySpawnKind, LegacyWorldCompanionData,
};
pub use spells::{
    load_declarative_spell_catalog, parse_declarative_spells_xml, DeclarativeSpellCatalog,
    DeclarativeSpellDefinition,
};
pub use stages::{parse_tfs_stages_xml, ExperienceStage, ExperienceStages};
pub use tfs_entities::{
    materialize_tfs_static_spawns, TfsEntityAppearance, TfsEntityCatalog, TfsEntityDefinition,
    TfsEntityKind, TfsSpawnResolution,
};
pub use tfs_registry::{TfsContentInventory, TfsRegistryCategory, TfsRegistryInventory};
pub use vocations::{
    load_tfs_vocation_registry, parse_tfs_vocations_xml, TfsVocationDefinition,
    TfsVocationRegistry, VocationMultiplier, VocationRegeneration,
};
pub use weapons::{
    load_declarative_weapon_catalog, parse_declarative_weapons_xml, DeclarativeWeaponCatalog,
    DeclarativeWeaponDefinition,
};

pub const CONFIG_FILE_NAME: &str = "config.lua";
pub const CONTENT_MANIFEST_NAME: &str = "fe-content.manifest";
pub const EMPTY_WORLD_MANIFEST_NAME: &str = "fe-empty-world.manifest";
pub const FE_MAP_EXTENSION: &str = "femap";
pub const OTBM_MAP_EXTENSION: &str = "otbm";
pub const FE_MAP_FORMAT: &str = "fe-map-v1";
pub const FE_MAP_INTERCHANGE_FORMAT: &str = "fe-map-v2";
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
    pub experience_rate: u32,
    pub skill_rate: u32,
    pub magic_rate: u32,
    pub static_creature_target_attack_damage: u16,
    pub experience_stages: Option<ExperienceStages>,
    pub death_loss_percent: i32,
    pub server_name: String,
    pub map_name: String,
    pub map_format: WorldMapFormat,
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
    pub otclient_v8_outfit_first_look_type: u16,
    pub otclient_v8_outfit_last_look_type: u16,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMapFormat {
    Auto,
    FeMap,
    Otbm,
}

impl WorldMapFormat {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "auto" => Ok(Self::Auto),
            "femap" => Ok(Self::FeMap),
            "otbm" => Ok(Self::Otbm),
            other => Err(ConfigError::InvalidValue {
                key: "mapFormat",
                message: format!("unsupported map format `{other}`; expected auto, femap, or otbm"),
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

    let bind_ip: IpAddr = optional_string(&values, "ip", "127.0.0.1")?
        .parse()
        .map_err(|_| ConfigError::InvalidValue {
            key: "ip",
            message: "must be an IPv4 or IPv6 address".into(),
        })?;
    let game_protocol_port = optional_u16(&values, "gameProtocolPort", 7172)?;
    let status_protocol_port = optional_u16(&values, "statusProtocolPort", 7171)?;
    let max_players = optional_u32(&values, "maxPlayers", 0)?;
    let experience_rate = optional_u32(&values, "rateExp", 5)?;
    let skill_rate = optional_u32(&values, "rateSkill", 1)?;
    let magic_rate = optional_u32(&values, "rateMagic", 1)?;
    let static_creature_target_attack_damage =
        optional_u16(&values, "staticCreatureTargetAttackDamage", 0)?;
    if static_creature_target_attack_damage > 100 {
        return Err(ConfigError::InvalidValue {
            key: "staticCreatureTargetAttackDamage",
            message: "must be between 0 and 100".into(),
        });
    }
    let content_directory = world_directory.join("data");
    let experience_stages = load_optional_experience_stages(&content_directory)?;
    let death_loss_percent = optional_i32(&values, "deathLosePercent", -1)?;
    if !(-1..=100).contains(&death_loss_percent) {
        return Err(ConfigError::InvalidValue {
            key: "deathLosePercent",
            message: "must be between -1 and 100".into(),
        });
    }
    let profile_id = optional_string(&values, "feProfile", "fe-7.4")?;
    let profile = profile_by_id(profile_id).ok_or_else(|| ConfigError::InvalidValue {
        key: "feProfile",
        message: format!("unknown Forgotten Engine profile `{profile_id}`"),
    })?;
    let protocol = optional_string(&values, "tibiaProtocol", profile.tibia_protocol)?;
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
    let configured_otclient_v8_outfit_first_look_type =
        optional_u16(&values, "otclientV8OutfitFirstLookType", 0)?;
    let configured_otclient_v8_outfit_last_look_type =
        optional_u16(&values, "otclientV8OutfitLastLookType", 0)?;
    let otclient_v8_outfit_first_look_type = if otclient_v8_player_look_type != 0
        && configured_otclient_v8_outfit_first_look_type == 0
    {
        otclient_v8_player_look_type
    } else {
        configured_otclient_v8_outfit_first_look_type
    };
    let otclient_v8_outfit_last_look_type =
        if otclient_v8_player_look_type != 0 && configured_otclient_v8_outfit_last_look_type == 0 {
            otclient_v8_player_look_type
        } else {
            configured_otclient_v8_outfit_last_look_type
        };
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
        let message = match native_profile.foundation() {
            NativeOtClientFoundation::Classic800RequiresRsaXtea => {
                "protocol 800 requires parser-backed RSA/XTEA native transport, which is not implemented"
                    .into()
            }
            NativeOtClientFoundation::PlainClassic740 => {
                "selected native profile is not runnable".into()
            }
            NativeOtClientFoundation::Unsupported => {
                "requires the selected plain numeric-account native client profile".into()
            }
        };
        return Err(ConfigError::InvalidValue {
            key: "otclientV8NativeEnabled",
            message,
        });
    }
    if otclient_v8_native_empty_world_enabled
        && (!otclient_v8_native_enabled
            || !native_profile.supports_current_native_foundation()
            || otclient_v8_player_look_type > u8::MAX as u16
            || otclient_v8_outfit_first_look_type > u8::MAX as u16
            || otclient_v8_outfit_last_look_type > u8::MAX as u16
            || (otclient_v8_player_look_type == 0
                && (otclient_v8_outfit_first_look_type != 0
                    || otclient_v8_outfit_last_look_type != 0))
            || (otclient_v8_player_look_type != 0
                && (otclient_v8_outfit_first_look_type == 0
                    || otclient_v8_outfit_first_look_type > otclient_v8_player_look_type
                    || otclient_v8_player_look_type > otclient_v8_outfit_last_look_type))
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
        experience_rate,
        skill_rate,
        magic_rate,
        static_creature_target_attack_damage,
        experience_stages,
        death_loss_percent,
        server_name: optional_string(&values, "serverName", "Forgotten Engine")?.to_owned(),
        map_name: optional_string(&values, "mapName", "forgotten")?.to_owned(),
        map_format: WorldMapFormat::parse(optional_string(&values, "mapFormat", "auto")?)?,
        world_type: WorldType::parse(optional_string(&values, "worldType", "pvp")?)?,
        mysql: MysqlConfig {
            host: optional_string(&values, "mysqlHost", "127.0.0.1")?.to_owned(),
            user: optional_string(&values, "mysqlUser", "forgottenserver")?.to_owned(),
            database: optional_string(&values, "mysqlDatabase", "forgottenserver")?.to_owned(),
        },
        profile,
        content_directory,
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
        otclient_v8_outfit_first_look_type,
        otclient_v8_outfit_last_look_type,
        otclient_v8_player_speed,
        otclient_v8_server_beat,
    })
}

fn load_optional_experience_stages(
    content_directory: &Path,
) -> Result<Option<ExperienceStages>, ConfigError> {
    let path = content_directory.join("XML/stages.xml");
    match fs::read(&path) {
        Ok(bytes) => parse_tfs_stages_xml(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::Io(error)),
    }
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
    let world_directory = config.content_directory.join("world");
    let femap = world_directory.join(format!("{}.{}", config.map_name, FE_MAP_EXTENSION));
    let otbm = world_directory.join(format!("{}.{}", config.map_name, OTBM_MAP_EXTENSION));
    match config.map_format {
        WorldMapFormat::FeMap => Ok(femap),
        WorldMapFormat::Otbm => Ok(otbm),
        WorldMapFormat::Auto if otbm.is_file() => Ok(otbm),
        WorldMapFormat::Auto => Ok(femap),
    }
}

pub fn load_world_map(config: &EngineConfig) -> Result<WorldMap, ConfigError> {
    let path = world_map_path(config)?;
    let bytes = fs::read(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ConfigError::InvalidContent(format!(
                "missing selected map {}; expected a {} or {} document selected by mapFormat",
                path.display(),
                FE_MAP_EXTENSION,
                OTBM_MAP_EXTENSION
            ))
        } else {
            ConfigError::Io(error)
        }
    })?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(OTBM_MAP_EXTENSION) => otbm::parse_otbm_world_map(&config.map_name, &bytes),
        Some(FE_MAP_EXTENSION) => {
            let source = String::from_utf8(bytes)
                .map_err(|_| ConfigError::InvalidContent("FE map document is not UTF-8".into()))?;
            parse_world_map(&config.map_name, &source)
        }
        _ => Err(ConfigError::InvalidContent(format!(
            "selected map {} has an unsupported extension",
            path.display()
        ))),
    }
}

pub fn load_world_companions(
    config: &EngineConfig,
    world_map: &WorldMap,
) -> Result<LegacyWorldCompanionData, ConfigError> {
    legacy_xml::load_legacy_world_companions(config, world_map)
}

pub fn load_tfs_content_inventory(
    config: &EngineConfig,
) -> Result<TfsContentInventory, ConfigError> {
    tfs_registry::load_tfs_content_inventory(config)
}

pub fn load_tfs_entity_catalog(config: &EngineConfig) -> Result<TfsEntityCatalog, ConfigError> {
    tfs_entities::load_tfs_entity_catalog(config)
}

pub fn resolve_tfs_spawn_references(
    companions: &LegacyWorldCompanionData,
    catalog: &TfsEntityCatalog,
) -> TfsSpawnResolution {
    tfs_entities::resolve_tfs_spawns(companions, catalog)
}

pub fn load_legacy_item_catalog(
    config: &EngineConfig,
    world_map: &WorldMap,
) -> Result<Option<LegacyItemCatalog>, ConfigError> {
    if !matches!(world_map.source(), WorldMapSource::Otbm(_)) {
        return Ok(None);
    }
    let item_directory = config.content_directory.join("items");
    let otb_path = item_directory.join("items.otb");
    let xml_path = item_directory.join("items.xml");
    let otb = fs::read(&otb_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ConfigError::InvalidContent(format!(
                "selected OTBM world requires operator-supplied {}",
                otb_path.display()
            ))
        } else {
            ConfigError::Io(error)
        }
    })?;
    let mut catalog = items::parse_items_otb(&otb)?;
    if xml_path.is_file() {
        items::apply_items_xml(&mut catalog, &fs::read(&xml_path).map_err(ConfigError::Io)?)?;
    }
    Ok(Some(catalog))
}

pub fn apply_legacy_item_metadata(
    world_map: &WorldMap,
    catalog: &LegacyItemCatalog,
) -> Result<WorldMap, ConfigError> {
    let mut normalized = world_map.clone();
    for (position, tile) in world_map.tiles() {
        let items = world_map.tile_items(position).unwrap_or_default();
        let ground = if let Some(item) = items.first() {
            catalog.definition(item.server_id).ok_or_else(|| {
                ConfigError::InvalidContent(format!(
                    "items.otb has no definition for map ground item {} at {},{},{}",
                    item.server_id, position.x, position.y, position.z
                ))
            })?
        } else if tile.ground_thing_id == 0 {
            continue;
        } else {
            catalog.definition(tile.ground_thing_id).ok_or_else(|| {
                ConfigError::InvalidContent(format!(
                    "items.otb has no definition for map ground item {} at {},{},{}",
                    tile.ground_thing_id, position.x, position.y, position.z
                ))
            })?
        };
        let blocks_movement = items.iter().try_fold(false, |blocked, item| {
            catalog
                .definition(item.server_id)
                .map(|definition| blocked || definition.blocks_movement())
                .ok_or_else(|| {
                    ConfigError::InvalidContent(format!(
                        "items.otb has no definition for map item {} at {},{},{}",
                        item.server_id, position.x, position.y, position.z
                    ))
                })
        })?;
        let mapped_items = items
            .iter()
            .cloned()
            .map(|mut item| {
                item.client_thing_id = Some(
                    catalog
                        .definition(item.server_id)
                        .expect("item definitions were validated above")
                        .client_id,
                );
                item
            })
            .collect();
        normalized
            .set_tile_items(position, mapped_items)
            .map_err(|error| {
                ConfigError::InvalidContent(format!("normalized map items: {error}"))
            })?;
        normalized
            .set_tile(
                position,
                WorldMapTile {
                    ground_thing_id: ground.client_id,
                    walkable: tile.walkable && !blocks_movement,
                },
            )
            .map_err(|error| {
                ConfigError::InvalidContent(format!("normalized map tile: {error}"))
            })?;
    }
    normalized
        .validate()
        .map_err(|error| ConfigError::InvalidContent(format!("normalized map: {error}")))?;
    Ok(normalized)
}

fn parse_world_map(identifier: &str, source: &str) -> Result<WorldMap, ConfigError> {
    if source
        .lines()
        .any(|line| line.trim() == format!("format={FE_MAP_INTERCHANGE_FORMAT}"))
    {
        return parse_world_map_interchange(identifier, source);
    }
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

pub fn export_world_map_to_femap(map: &WorldMap) -> Result<String, ConfigError> {
    map.validate().map_err(|error| {
        ConfigError::InvalidContent(format!("cannot export invalid map: {error}"))
    })?;
    let mut output = format!(
        "# Forgotten Engine original map interchange document\nformat={FE_MAP_INTERCHANGE_FORMAT}\n"
    );
    match map.source() {
        WorldMapSource::FeMapV1 => output.push_str("source=femap\n"),
        WorldMapSource::Otbm(header) => output.push_str(&format!(
            "source=otbm,{},{},{},{},{}\n",
            header.version,
            header.width,
            header.height,
            header.item_major_version,
            header.item_minor_version
        )),
    }
    let spawn = map.spawn();
    output.push_str(&format!("spawn={},{},{}\n", spawn.x, spawn.y, spawn.z));
    for (position, tile) in map.tiles() {
        output.push_str(&format!(
            "tile={},{},{},{},{}\n",
            position.x, position.y, position.z, tile.ground_thing_id, tile.walkable
        ));
    }
    for (position, items) in map.tile_item_entries() {
        for item in items {
            if !item.children.is_empty()
                || item.text.is_some()
                || item.description.is_some()
                || item.teleport_destination.is_some()
                || item.duration.is_some()
                || item.charges.is_some()
            {
                return Err(ConfigError::InvalidContent(format!(
                    "cannot export rich item {} at {},{},{} to fe-map-v2 yet",
                    item.server_id, position.x, position.y, position.z
                )));
            }
            output.push_str(&format!(
                "item={},{},{},{},{},{},{}\n",
                position.x,
                position.y,
                position.z,
                item.server_id,
                item.count,
                item.action_id.unwrap_or_default(),
                item.unique_id.unwrap_or_default()
            ));
        }
    }
    for (position, flags) in map.tile_flag_entries() {
        output.push_str(&format!(
            "tileflags={},{},{},{}\n",
            position.x, position.y, position.z, flags
        ));
    }
    for (position, house_id) in map.house_tile_entries() {
        output.push_str(&format!(
            "house={},{},{},{}\n",
            position.x, position.y, position.z, house_id
        ));
    }
    for town in map.towns() {
        if town.name.contains(',') || town.name.contains('\n') {
            return Err(ConfigError::InvalidContent(
                "cannot export a town name containing a comma or newline to fe-map-v2".into(),
            ));
        }
        output.push_str(&format!(
            "town={},{},{},{},{}\n",
            town.id,
            town.name,
            town.temple_position.x,
            town.temple_position.y,
            town.temple_position.z
        ));
    }
    for (name, position) in map.waypoints() {
        if name.contains(',') || name.contains('\n') {
            return Err(ConfigError::InvalidContent(
                "cannot export a waypoint name containing a comma or newline to fe-map-v2".into(),
            ));
        }
        output.push_str(&format!(
            "waypoint={},{},{},{}\n",
            name, position.x, position.y, position.z
        ));
    }
    Ok(output)
}

fn parse_world_map_interchange(identifier: &str, source: &str) -> Result<WorldMap, ConfigError> {
    let mut format_seen = false;
    let mut spawn = None;
    let mut source_metadata = None;
    let mut map = None;
    let mut pending_items = BTreeMap::<Position, Vec<WorldMapItem>>::new();
    let mut pending_flags = Vec::new();
    let mut pending_houses = Vec::new();
    let mut towns = Vec::new();
    let mut waypoints = Vec::new();
    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            ConfigError::InvalidContent(format!("map line {line_number} must use key=value syntax"))
        })?;
        match key.trim() {
            "format" if value.trim() == FE_MAP_INTERCHANGE_FORMAT => format_seen = true,
            "format" => {
                return Err(ConfigError::InvalidContent(format!(
                    "map line {line_number} must declare format={FE_MAP_INTERCHANGE_FORMAT}"
                )))
            }
            "source" => {
                if source_metadata.is_some() {
                    return Err(ConfigError::InvalidContent(
                        "map declares source more than once".into(),
                    ));
                }
                source_metadata = Some(parse_interchange_source(value.trim(), line_number)?);
            }
            "spawn" => {
                if spawn.is_some() {
                    return Err(ConfigError::InvalidContent(
                        "map declares spawn more than once".into(),
                    ));
                }
                spawn = Some(parse_position(value.trim(), line_number)?);
            }
            "tile" => {
                let spawn = spawn.ok_or_else(|| {
                    ConfigError::InvalidContent(
                        "fe-map-v2 requires spawn before tile records".into(),
                    )
                })?;
                let map_ref = map.get_or_insert_with(|| WorldMap::new(identifier, spawn));
                let fields = split_map_fields(value, 5, line_number)?;
                let position = parse_position_fields(&fields[0..3], line_number)?;
                let tile = parse_tile_fields(&fields[3..5], line_number)?;
                map_ref.set_tile(position, tile).map_err(|error| {
                    ConfigError::InvalidContent(format!("map line {line_number}: {error}"))
                })?;
            }
            "item" => {
                let fields = split_map_fields(value, 7, line_number)?;
                let position = parse_position_fields(&fields[0..3], line_number)?;
                let item = WorldMapItem {
                    server_id: parse_map_u16(fields[3], line_number, "serverId")?,
                    client_thing_id: None,
                    count: parse_map_u8(fields[4], line_number, "count")?.max(1),
                    action_id: nonzero_u16(fields[5], line_number, "actionId")?,
                    unique_id: nonzero_u16(fields[6], line_number, "uniqueId")?,
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                };
                pending_items.entry(position).or_default().push(item);
            }
            "tileflags" => {
                let fields = split_map_fields(value, 4, line_number)?;
                pending_flags.push((
                    parse_position_fields(&fields[0..3], line_number)?,
                    parse_map_u32(fields[3], line_number, "flags")?,
                ));
            }
            "house" => {
                let fields = split_map_fields(value, 4, line_number)?;
                pending_houses.push((
                    parse_position_fields(&fields[0..3], line_number)?,
                    parse_map_u32(fields[3], line_number, "houseId")?,
                ));
            }
            "town" => {
                let fields = split_map_fields(value, 5, line_number)?;
                towns.push(WorldMapTown {
                    id: parse_map_u32(fields[0], line_number, "townId")?,
                    name: fields[1].to_owned(),
                    temple_position: parse_position_fields(&fields[2..5], line_number)?,
                });
            }
            "waypoint" => {
                let fields = split_map_fields(value, 4, line_number)?;
                waypoints.push((
                    fields[0].to_owned(),
                    parse_position_fields(&fields[1..4], line_number)?,
                ));
            }
            other => {
                return Err(ConfigError::InvalidContent(format!(
                    "map line {line_number} has unsupported key `{other}`"
                )))
            }
        }
    }
    if !format_seen {
        return Err(ConfigError::InvalidContent(format!(
            "map must declare format={FE_MAP_INTERCHANGE_FORMAT}"
        )));
    }
    let spawn =
        spawn.ok_or_else(|| ConfigError::InvalidContent("map must declare spawn=x,y,z".into()))?;
    let mut map = map.unwrap_or_else(|| WorldMap::new(identifier, spawn));
    if let Some(source_metadata) = source_metadata {
        map.set_source(source_metadata);
    }
    for (position, items) in pending_items {
        map.set_tile_items(position, items).map_err(|error| {
            ConfigError::InvalidContent(format!("invalid fe-map-v2 item record: {error}"))
        })?;
    }
    for (position, flags) in pending_flags {
        map.set_tile_flags(position, flags);
    }
    for (position, house_id) in pending_houses {
        map.set_house_tile(position, house_id).map_err(|error| {
            ConfigError::InvalidContent(format!("invalid fe-map-v2 house record: {error}"))
        })?;
    }
    for town in towns {
        map.set_town(town).map_err(|error| {
            ConfigError::InvalidContent(format!("invalid fe-map-v2 town record: {error}"))
        })?;
    }
    for (name, position) in waypoints {
        map.set_waypoint(name, position).map_err(|error| {
            ConfigError::InvalidContent(format!("invalid fe-map-v2 waypoint record: {error}"))
        })?;
    }
    map.validate()
        .map_err(|error| ConfigError::InvalidContent(format!("invalid map: {error}")))?;
    Ok(map)
}

fn parse_interchange_source(value: &str, line: usize) -> Result<WorldMapSource, ConfigError> {
    if value == "femap" {
        return Ok(WorldMapSource::FeMapV1);
    }
    let fields = split_map_fields(value, 6, line)?;
    if fields[0] != "otbm" {
        return Err(ConfigError::InvalidContent(format!(
            "map line {line}: source must be femap or otbm,version,width,height,itemMajor,itemMinor"
        )));
    }
    Ok(WorldMapSource::Otbm(OtbmMapHeader {
        version: parse_map_u32(fields[1], line, "otbmVersion")?,
        width: parse_map_u16(fields[2], line, "width")?,
        height: parse_map_u16(fields[3], line, "height")?,
        item_major_version: parse_map_u32(fields[4], line, "itemMajor")?,
        item_minor_version: parse_map_u32(fields[5], line, "itemMinor")?,
        description: None,
        spawn_file: None,
        house_file: None,
    }))
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

fn parse_map_u32(value: &str, line: usize, field: &str) -> Result<u32, ConfigError> {
    value
        .parse::<u32>()
        .map_err(|_| ConfigError::InvalidContent(format!("map line {line}: {field} must be a u32")))
}

fn nonzero_u16(value: &str, line: usize, field: &str) -> Result<Option<u16>, ConfigError> {
    let value = parse_map_u16(value, line, field)?;
    Ok((value != 0).then_some(value))
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
        "-- Forgotten Engine configuration\n-- TFS-style layout; parsed as a bounded assignment subset during P0.\n\n-- Connection Config\nip = \"127.0.0.1\"\ngameProtocolPort = 7172\nstatusProtocolPort = 7171\nmaxPlayers = 0\nserverName = \"Forgotten Engine\"\n\n-- Legacy login foundation\n-- Set true only after providing an original 1024-bit RSA private key.\nlegacyLoginEnabled = false\nrsaPrivateKey = \"key.pem\"\n\n-- Legacy game-session foundation\n-- Separate opt-in port until official session compatibility is proven.\ngameSessionEnabled = false\ngameSessionPort = 7173\n-- Public endpoint advertised to a custom OTClient module; may be a proxy/domain/IP-changing endpoint.\nadvertisedGameSessionHost = \"127.0.0.1\"\nadvertisedGameSessionPort = 7173\n\n-- Native stock OTClientV8 foundation\n-- Select the compatible protocol and feature switches for this world; do not assume FE release versions.\notclientV8NativeEnabled = false\notclientV8LoginPort = 7174\notclientV8GamePort = 7175\notclientV8ProtocolVersion = 0\notclientV8NumericAccountIds = true\notclientV8LoginPacketEncryption = false\notclientV8ProtocolChecksum = false\notclientV8ChallengeOnLogin = false\n-- Address returned in the native legacy character list.\nadvertisedOtClientV8Host = \"127.0.0.1\"\nadvertisedOtClientV8GamePort = 7175\n\n-- Native empty-world fixture. Nonzero IDs must exist in operator-owned matching OTCv8 data; zero selects an asset-free fallback.\notclientV8NativeEmptyWorldEnabled = false\notclientV8EmptyWorldGroundThingId = 0\notclientV8PlayerLookType = 0\n-- For classic 740 only: inclusive current-outfit chooser range. Defaults to PlayerLookType.\notclientV8OutfitFirstLookType = 0\notclientV8OutfitLastLookType = 0\notclientV8PlayerSpeed = 220\notclientV8ServerBeat = 50\n\n-- Map\nmapName = \"forgotten\"\nworldType = \"pvp\"\n\n-- MySQL compatibility contract (SQLite remains the current storage backend)\nmysqlHost = \"127.0.0.1\"\nmysqlUser = \"forgottenengine\"\nmysqlDatabase = \"forgottenengine\"\n\n-- Forgotten Engine profile\nfeProfile = \"{}\"\ntibiaProtocol = \"{}\"\n",
        profile.id, profile.tibia_protocol
    )
}

fn parse_assignments(contents: &str) -> Result<BTreeMap<String, Literal>, ConfigError> {
    let mut values = BTreeMap::new();
    for (index, source_line) in contents.lines().enumerate() {
        let line = strip_lua_line_comment(source_line).trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            continue;
        }
        if !is_recognized_config_key(key) {
            continue;
        }
        let literal = Literal::parse(raw_value.trim()).map_err(|message| ConfigError::Syntax {
            line: index + 1,
            message,
        })?;
        values.insert(key.to_owned(), literal);
    }
    Ok(values)
}

fn strip_lua_line_comment(source: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut characters = source.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == '-'
            && characters
                .peek()
                .is_some_and(|(_, next_character)| *next_character == '-')
        {
            return &source[..index];
        }
    }
    source
}

fn is_recognized_config_key(key: &str) -> bool {
    matches!(
        key,
        "ip" | "gameProtocolPort"
            | "statusProtocolPort"
            | "maxPlayers"
            | "rateExp"
            | "rateSkill"
            | "rateMagic"
            | "staticCreatureTargetAttackDamage"
            | "deathLosePercent"
            | "serverName"
            | "mapName"
            | "mapFormat"
            | "worldType"
            | "mysqlHost"
            | "mysqlUser"
            | "mysqlDatabase"
            | "feProfile"
            | "tibiaProtocol"
            | "legacyLoginEnabled"
            | "rsaPrivateKey"
            | "gameSessionEnabled"
            | "gameSessionPort"
            | "advertisedGameSessionHost"
            | "advertisedGameSessionPort"
            | "otclientV8NativeEnabled"
            | "otclientV8LoginPort"
            | "otclientV8GamePort"
            | "advertisedOtClientV8Host"
            | "advertisedOtClientV8GamePort"
            | "otclientV8ProtocolVersion"
            | "otclientV8NumericAccountIds"
            | "otclientV8LoginPacketEncryption"
            | "otclientV8ProtocolChecksum"
            | "otclientV8ChallengeOnLogin"
            | "otclientV8NativeEmptyWorldEnabled"
            | "otclientV8EmptyWorldGroundThingId"
            | "otclientV8PlayerLookType"
            | "otclientV8OutfitFirstLookType"
            | "otclientV8OutfitLastLookType"
            | "otclientV8PlayerSpeed"
            | "otclientV8ServerBeat"
    )
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

fn optional_u32(
    values: &BTreeMap<String, Literal>,
    key: &'static str,
    default: u32,
) -> Result<u32, ConfigError> {
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
        None => Ok(default),
    }
}

fn optional_i32(
    values: &BTreeMap<String, Literal>,
    key: &'static str,
    default: i32,
) -> Result<i32, ConfigError> {
    match values.get(key) {
        Some(Literal::Integer(value)) => {
            i32::try_from(*value).map_err(|_| ConfigError::InvalidValue {
                key,
                message: "must fit a signed 32-bit integer".into(),
            })
        }
        Some(_) => Err(ConfigError::InvalidValue {
            key,
            message: "must be an integer".into(),
        }),
        None => Ok(default),
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
    use forgotten_protocol::{FE_7_4_PROFILE, FE_8_0_PROFILE};
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
                "{}otclientV8NativeEnabled = true\notclientV8LoginPort = 7264\notclientV8GamePort = 7265\nadvertisedOtClientV8Host = \"203.0.113.24\"\nadvertisedOtClientV8GamePort = 7265\notclientV8ProtocolVersion = 740\notclientV8NumericAccountIds = true\notclientV8LoginPacketEncryption = false\notclientV8ProtocolChecksum = false\notclientV8ChallengeOnLogin = false\notclientV8NativeEmptyWorldEnabled = true\notclientV8EmptyWorldGroundThingId = 102\notclientV8PlayerLookType = 128\notclientV8OutfitFirstLookType = 128\notclientV8OutfitLastLookType = 131\notclientV8PlayerSpeed = 220\notclientV8ServerBeat = 50\n",
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
        assert_eq!(config.otclient_v8_outfit_first_look_type, 128);
        assert_eq!(config.otclient_v8_outfit_last_look_type, 131);
        assert_eq!(config.otclient_v8_player_speed, 220);
        assert_eq!(config.otclient_v8_server_beat, 50);
        assert!(config
            .otclient_v8_native_profile()
            .supports_current_native_foundation());
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn rejects_a_native_outfit_range_that_excludes_the_current_look_type() {
        let world = temporary_world("invalid-native-outfit-range");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}otclientV8NativeEnabled = true\notclientV8ProtocolVersion = 740\notclientV8NativeEmptyWorldEnabled = true\notclientV8PlayerLookType = 128\notclientV8OutfitFirstLookType = 129\notclientV8OutfitLastLookType = 131\n",
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
    fn accepts_a_tfs_style_config_without_fe_only_assignments() {
        let world = temporary_world("tfs-config");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            r#"-- A private TFS-style configuration with settings FE does not execute.
worldType = "pvp"
ip = "127.0.0.1"
gameProtocolPort = 7172
statusProtocolPort = 7171
maxPlayers = 0
rateExp = 5
rateSkill = 1
rateMagic = 1
deathLosePercent = -1
serverName = "Private Forgotten"
mapName = "myworld"
mapAuthor = "Operator"
mysqlHost = "127.0.0.1"
mysqlUser = "forgottenserver"
mysqlPass = "private"
mysqlDatabase = "forgottenserver"
timeToDecreaseFrags = 24 * 60 * 60
experienceStages = {
    { minlevel = 1, maxlevel = 8, multiplier = 7 },
    { minlevel = 9, multiplier = 6 }
}
"#,
        )
        .unwrap();

        let config = load(&world).unwrap();
        assert_eq!(config.profile, FE_7_4_PROFILE);
        assert_eq!(config.map_format, WorldMapFormat::Auto);
        assert_eq!(config.server_name, "Private Forgotten");
        assert_eq!(config.map_name, "myworld");
        assert_eq!(config.game_protocol_port, 7172);
        assert_eq!(config.experience_rate, 5);
        assert_eq!(config.skill_rate, 1);
        assert_eq!(config.magic_rate, 1);
        assert_eq!(config.static_creature_target_attack_damage, 0);
        assert_eq!(config.death_loss_percent, -1);
        assert!(!config.otclient_v8_native_enabled);
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn loads_a_bounded_opt_in_static_target_attack_damage() {
        let world = temporary_world("static-target-attack-damage");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}staticCreatureTargetAttackDamage = 2\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();
        assert_eq!(
            load(&world).unwrap().static_creature_target_attack_damage,
            2
        );

        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}staticCreatureTargetAttackDamage = 101\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();
        assert!(matches!(
            load(&world),
            Err(ConfigError::InvalidValue {
                key: "staticCreatureTargetAttackDamage",
                ..
            })
        ));
        let _ = fs::remove_dir_all(world);
    }

    #[test]
    fn loads_optional_tfs_style_experience_stages() {
        let world = temporary_world("experience-stages");
        fs::create_dir_all(world.join("data/XML")).unwrap();
        fs::write(world.join(CONFIG_FILE_NAME), template(FE_7_4_PROFILE)).unwrap();
        fs::write(
            world.join("data/XML/stages.xml"),
            r#"<stages><stage minlevel="1" maxlevel="8" multiplier="7"/><stage minlevel="9" multiplier="3"/></stages>"#,
        )
        .unwrap();

        let config = load(&world).unwrap();
        let stages = config.experience_stages.as_ref().unwrap();
        assert_eq!(stages.0.len(), 2);
        let policy = stages.award_policy(config.experience_rate).unwrap();
        assert_eq!(policy.award_for(1, 10), 350);
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
    fn reports_the_unimplemented_encrypted_transport_for_enabled_protocol_800() {
        let world = temporary_world("native-800-transport-boundary");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}otclientV8NativeEnabled = true\notclientV8ProtocolVersion = 800\notclientV8LoginPacketEncryption = true\n",
                template(FE_8_0_PROFILE)
            ),
        )
        .unwrap();

        assert!(matches!(
            load(&world),
            Err(ConfigError::InvalidValue { key: "otclientV8NativeEnabled", message })
                if message.contains("RSA/XTEA")
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

    #[test]
    fn round_trips_legacy_map_records_through_the_fe_interchange_document() {
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("legacy-export", spawn);
        map.set_tile(
            spawn,
            WorldMapTile {
                ground_thing_id: 4526,
                walkable: true,
            },
        )
        .unwrap();
        map.set_source(WorldMapSource::Otbm(OtbmMapHeader {
            version: 2,
            width: 512,
            height: 512,
            item_major_version: 57,
            item_minor_version: 1098,
            description: None,
            spawn_file: None,
            house_file: None,
        }));
        map.set_tile_items(
            spawn,
            vec![WorldMapItem {
                server_id: 4526,
                client_thing_id: Some(102),
                count: 1,
                action_id: Some(7),
                unique_id: Some(9),
                text: None,
                description: None,
                teleport_destination: None,
                duration: None,
                charges: None,
                children: Vec::new(),
            }],
        )
        .unwrap();
        map.set_tile_flags(spawn, 1);
        map.set_house_tile(spawn, 42).unwrap();
        map.set_town(WorldMapTown {
            id: 1,
            name: "Thais".into(),
            temple_position: spawn,
        })
        .unwrap();
        map.set_waypoint("temple", spawn).unwrap();

        let document = export_world_map_to_femap(&map).unwrap();
        let parsed = parse_world_map("legacy-export", &document).unwrap();
        assert_eq!(parsed.tile(spawn), map.tile(spawn));
        assert_eq!(parsed.tile_items(spawn).unwrap()[0].action_id, Some(7));
        assert_eq!(parsed.tile_flags(spawn), 1);
        assert_eq!(parsed.house_tile_id(spawn), Some(42));
        assert_eq!(parsed.towns().next().unwrap().name, "Thais");
        assert_eq!(parsed.waypoint("temple"), Some(spawn));
        assert!(matches!(parsed.source(), WorldMapSource::Otbm(_)));
    }

    #[test]
    fn preserves_double_hyphens_inside_quoted_tfs_configuration_values() {
        let world = temporary_world("quoted-lua-comment-marker");
        fs::create_dir_all(&world).unwrap();
        fs::write(
            world.join(CONFIG_FILE_NAME),
            format!(
                "{}serverName = \"Private -- World\" -- trailing operator note\nrsaPrivateKey = \"keys--private/legacy.pem\"\n",
                template(FE_7_4_PROFILE)
            ),
        )
        .unwrap();

        let config = load(&world).unwrap();
        assert_eq!(config.server_name, "Private -- World");
        assert_eq!(
            config.rsa_private_key_path,
            world.join("keys--private/legacy.pem")
        );
        let _ = fs::remove_dir_all(world);
    }
}
