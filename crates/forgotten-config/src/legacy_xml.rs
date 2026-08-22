use crate::{ConfigError, EngineConfig};
use forgotten_core::{Position, WorldMap, WorldMapSource};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_LEGACY_XML_BYTES: usize = 16 * 1024 * 1024;
const MAX_LEGACY_XML_DEPTH: usize = 32;
const MAX_LEGACY_SPAWNS: usize = 65_536;
const MAX_LEGACY_HOUSES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyWorldCompanionData {
    pub spawn_file: Option<PathBuf>,
    pub house_file: Option<PathBuf>,
    pub spawns: Vec<LegacySpawnArea>,
    pub houses: Vec<LegacyHouse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySpawnArea {
    pub center: Position,
    pub radius: u16,
    pub creatures: Vec<LegacySpawnCreature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySpawnCreature {
    pub kind: LegacySpawnKind,
    pub name: String,
    pub position: Position,
    pub spawn_interval_seconds: u32,
    pub direction: u8,
    pub chance: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySpawnKind {
    Monster,
    Npc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyHouse {
    pub id: u32,
    pub name: String,
    pub entry: Position,
    pub rent: u32,
    pub town_id: u32,
    pub size: u32,
    pub guildhall: bool,
}

pub(crate) fn load_legacy_world_companions(
    config: &EngineConfig,
    world_map: &WorldMap,
) -> Result<LegacyWorldCompanionData, ConfigError> {
    let WorldMapSource::Otbm(header) = world_map.source() else {
        return Ok(LegacyWorldCompanionData {
            spawn_file: None,
            house_file: None,
            spawns: Vec::new(),
            houses: Vec::new(),
        });
    };
    let directory = config.content_directory.join("world");
    let spawn_name = header
        .spawn_file
        .clone()
        .unwrap_or_else(|| format!("{}-spawn.xml", config.map_name));
    let house_name = header
        .house_file
        .clone()
        .unwrap_or_else(|| format!("{}-house.xml", config.map_name));
    let spawn_path = resolve_companion_path(&directory, &spawn_name)?;
    let house_path = resolve_companion_path(&directory, &house_name)?;
    let spawns = if spawn_path.is_file() {
        parse_spawns_xml(&fs::read(&spawn_path).map_err(ConfigError::Io)?)?
    } else {
        Vec::new()
    };
    let houses = if house_path.is_file() {
        parse_houses_xml(&fs::read(&house_path).map_err(ConfigError::Io)?)?
    } else {
        Vec::new()
    };
    validate_legacy_houses_against_map(&houses, world_map)?;
    Ok(LegacyWorldCompanionData {
        spawn_file: spawn_path.is_file().then_some(spawn_path),
        house_file: house_path.is_file().then_some(house_path),
        spawns,
        houses,
    })
}

fn resolve_companion_path(directory: &Path, file_name: &str) -> Result<PathBuf, ConfigError> {
    let candidate = Path::new(file_name);
    if candidate.as_os_str().is_empty()
        || candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid(
            "legacy map companion path must be a safe relative path",
        ));
    }
    Ok(directory.join(candidate))
}

fn parse_spawns_xml(bytes: &[u8]) -> Result<Vec<LegacySpawnArea>, ConfigError> {
    ensure_size(bytes)?;
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut current_spawn = None;
    let mut spawns = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                ensure_depth(depth)?;
                match event.name().as_ref() {
                    b"spawns" if depth == 1 => root_seen = true,
                    b"spawn" if depth == 2 => {
                        if current_spawn.is_some() {
                            return Err(invalid("nested spawn elements are not supported"));
                        }
                        current_spawn = Some(LegacySpawnArea {
                            center: Position {
                                x: attribute_u16(&event, b"centerx")?,
                                y: attribute_u16(&event, b"centery")?,
                                z: attribute_u8(&event, b"centerz")?,
                            },
                            radius: optional_attribute_u16(&event, b"radius")?.unwrap_or_default(),
                            creatures: Vec::new(),
                        });
                    }
                    b"monster" | b"npc" if current_spawn.is_some() => {
                        add_spawn_creature(&mut current_spawn, &event)?;
                    }
                    _ => {}
                }
            }
            Event::Empty(event) => match event.name().as_ref() {
                b"monster" | b"npc" if current_spawn.is_some() => {
                    add_spawn_creature(&mut current_spawn, &event)?;
                }
                b"spawn" if depth == 1 => {
                    let spawn = LegacySpawnArea {
                        center: Position {
                            x: attribute_u16(&event, b"centerx")?,
                            y: attribute_u16(&event, b"centery")?,
                            z: attribute_u8(&event, b"centerz")?,
                        },
                        radius: optional_attribute_u16(&event, b"radius")?.unwrap_or_default(),
                        creatures: Vec::new(),
                    };
                    add_spawn(&mut spawns, spawn)?;
                }
                _ => {}
            },
            Event::End(event) => {
                if event.name().as_ref() == b"spawn" {
                    if let Some(spawn) = current_spawn.take() {
                        add_spawn(&mut spawns, spawn)?;
                    }
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed XML depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 || current_spawn.is_some() {
        return Err(invalid(
            "spawns XML is malformed or does not have a spawns root",
        ));
    }
    Ok(spawns)
}

fn add_spawn_creature(
    current_spawn: &mut Option<LegacySpawnArea>,
    event: &BytesStart<'_>,
) -> Result<(), ConfigError> {
    let spawn = current_spawn
        .as_mut()
        .ok_or_else(|| invalid("spawn creature is outside a spawn element"))?;
    let kind = match event.name().as_ref() {
        b"monster" => LegacySpawnKind::Monster,
        b"npc" => LegacySpawnKind::Npc,
        _ => return Err(invalid("unsupported spawn creature element")),
    };
    let x = optional_attribute_u16(event, b"x")?.unwrap_or_default();
    let y = optional_attribute_u16(event, b"y")?.unwrap_or_default();
    let position = Position {
        x: spawn
            .center
            .x
            .checked_add(x)
            .ok_or_else(|| invalid("spawn creature x coordinate overflow"))?,
        y: spawn
            .center
            .y
            .checked_add(y)
            .ok_or_else(|| invalid("spawn creature y coordinate overflow"))?,
        z: spawn.center.z,
    };
    spawn.creatures.push(LegacySpawnCreature {
        kind,
        name: attribute_string(event, b"name")?,
        position,
        spawn_interval_seconds: optional_attribute_u32(event, b"spawntime")?.unwrap_or_default(),
        direction: optional_attribute_u8(event, b"direction")?.unwrap_or_default(),
        chance: optional_attribute_u16(event, b"chance")?.unwrap_or(100),
    });
    Ok(())
}

fn add_spawn(spawns: &mut Vec<LegacySpawnArea>, spawn: LegacySpawnArea) -> Result<(), ConfigError> {
    if spawns.len() >= MAX_LEGACY_SPAWNS {
        return Err(invalid("spawn count exceeds the configured limit"));
    }
    spawns.push(spawn);
    Ok(())
}

fn parse_houses_xml(bytes: &[u8]) -> Result<Vec<LegacyHouse>, ConfigError> {
    ensure_size(bytes)?;
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut houses = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                ensure_depth(depth)?;
                if event.name().as_ref() == b"houses" && depth == 1 {
                    root_seen = true;
                } else if event.name().as_ref() == b"house" {
                    if houses.len() >= MAX_LEGACY_HOUSES {
                        return Err(invalid("house count exceeds the configured limit"));
                    }
                    houses.push(LegacyHouse {
                        id: attribute_u32(&event, b"houseid")?,
                        name: attribute_string(&event, b"name")?,
                        entry: Position {
                            x: attribute_u16(&event, b"entryx")?,
                            y: attribute_u16(&event, b"entryy")?,
                            z: attribute_u8(&event, b"entryz")?,
                        },
                        rent: optional_attribute_u32(&event, b"rent")?.unwrap_or_default(),
                        town_id: optional_attribute_u32(&event, b"townid")?.unwrap_or_default(),
                        size: optional_attribute_u32(&event, b"size")?.unwrap_or_default(),
                        guildhall: optional_attribute_bool(&event, b"guildhall")?.unwrap_or(false),
                    });
                }
            }
            Event::Empty(event) if event.name().as_ref() == b"house" => {
                if houses.len() >= MAX_LEGACY_HOUSES {
                    return Err(invalid("house count exceeds the configured limit"));
                }
                houses.push(LegacyHouse {
                    id: attribute_u32(&event, b"houseid")?,
                    name: attribute_string(&event, b"name")?,
                    entry: Position {
                        x: attribute_u16(&event, b"entryx")?,
                        y: attribute_u16(&event, b"entryy")?,
                        z: attribute_u8(&event, b"entryz")?,
                    },
                    rent: optional_attribute_u32(&event, b"rent")?.unwrap_or_default(),
                    town_id: optional_attribute_u32(&event, b"townid")?.unwrap_or_default(),
                    size: optional_attribute_u32(&event, b"size")?.unwrap_or_default(),
                    guildhall: optional_attribute_bool(&event, b"guildhall")?.unwrap_or(false),
                });
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed XML depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid(
            "houses XML is malformed or does not have a houses root",
        ));
    }
    Ok(houses)
}

/// Ensures a parsed legacy house catalog refers only to actual imported OTBM house identities and
/// walkable entries. Ownership, access-list interpretation, rent, doors, and all runtime effects
/// remain separate boundaries.
fn validate_legacy_houses_against_map(
    houses: &[LegacyHouse],
    world_map: &WorldMap,
) -> Result<(), ConfigError> {
    let map_house_ids = world_map
        .house_tile_entries()
        .map(|(_, house_id)| house_id)
        .collect::<BTreeSet<_>>();
    let mut seen_house_ids = BTreeSet::new();
    for house in houses {
        if house.id == 0 {
            return Err(invalid("legacy house ID must be nonzero"));
        }
        if !seen_house_ids.insert(house.id) {
            return Err(invalid("legacy house IDs must be unique"));
        }
        if !map_house_ids.contains(&house.id) {
            return Err(invalid(
                "legacy house ID does not have a matching imported map house tile",
            ));
        }
        if !world_map.is_walkable(house.entry) {
            return Err(invalid(
                "legacy house entry must be a walkable imported map tile",
            ));
        }
    }
    Ok(())
}

fn attribute_string(event: &BytesStart<'_>, name: &[u8]) -> Result<String, ConfigError> {
    event
        .try_get_attribute(name)
        .map_err(xml_error)?
        .ok_or_else(|| {
            invalid(format!(
                "missing XML attribute `{}`",
                String::from_utf8_lossy(name)
            ))
        })?
        .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
        .map_err(xml_error)
        .map(|value| value.into_owned())
}

fn optional_attribute_string(
    event: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, ConfigError> {
    event
        .try_get_attribute(name)
        .map_err(xml_error)?
        .map(|attribute| {
            attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
                .map_err(xml_error)
                .map(|value| value.into_owned())
        })
        .transpose()
}

fn attribute_u8(event: &BytesStart<'_>, name: &[u8]) -> Result<u8, ConfigError> {
    attribute_string(event, name)?.parse::<u8>().map_err(|_| {
        invalid(format!(
            "XML attribute `{}` must be a u8",
            String::from_utf8_lossy(name)
        ))
    })
}

fn attribute_u16(event: &BytesStart<'_>, name: &[u8]) -> Result<u16, ConfigError> {
    attribute_string(event, name)?.parse::<u16>().map_err(|_| {
        invalid(format!(
            "XML attribute `{}` must be a u16",
            String::from_utf8_lossy(name)
        ))
    })
}

fn attribute_u32(event: &BytesStart<'_>, name: &[u8]) -> Result<u32, ConfigError> {
    attribute_string(event, name)?.parse::<u32>().map_err(|_| {
        invalid(format!(
            "XML attribute `{}` must be a u32",
            String::from_utf8_lossy(name)
        ))
    })
}

fn optional_attribute_u8(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<u8>, ConfigError> {
    optional_attribute_string(event, name)?.map_or(Ok(None), |value| {
        value.parse::<u8>().map(Some).map_err(|_| {
            invalid(format!(
                "XML attribute `{}` must be a u8",
                String::from_utf8_lossy(name)
            ))
        })
    })
}

fn optional_attribute_u16(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<u16>, ConfigError> {
    optional_attribute_string(event, name)?.map_or(Ok(None), |value| {
        value.parse::<u16>().map(Some).map_err(|_| {
            invalid(format!(
                "XML attribute `{}` must be a u16",
                String::from_utf8_lossy(name)
            ))
        })
    })
}

fn optional_attribute_u32(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<u32>, ConfigError> {
    optional_attribute_string(event, name)?.map_or(Ok(None), |value| {
        value.parse::<u32>().map(Some).map_err(|_| {
            invalid(format!(
                "XML attribute `{}` must be a u32",
                String::from_utf8_lossy(name)
            ))
        })
    })
}

fn optional_attribute_bool(
    event: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<bool>, ConfigError> {
    optional_attribute_string(event, name)?.map_or(Ok(None), |value| match value.as_str() {
        "true" | "1" => Ok(Some(true)),
        "false" | "0" => Ok(Some(false)),
        _ => Err(invalid(format!(
            "XML attribute `{}` must be true, false, 1, or 0",
            String::from_utf8_lossy(name)
        ))),
    })
}

fn ensure_size(bytes: &[u8]) -> Result<(), ConfigError> {
    if bytes.len() > MAX_LEGACY_XML_BYTES {
        Err(invalid(
            "legacy XML file exceeds the configured 16 MiB limit",
        ))
    } else {
        Ok(())
    }
}

fn ensure_depth(depth: usize) -> Result<(), ConfigError> {
    if depth > MAX_LEGACY_XML_DEPTH {
        Err(invalid("legacy XML nesting exceeds the configured limit"))
    } else {
        Ok(())
    }
}

fn xml_error(error: impl std::fmt::Display) -> ConfigError {
    invalid(format!("legacy XML parse error: {error}"))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgotten_core::WorldMapTile;

    #[test]
    fn parses_hand_authored_spawn_and_house_fixtures() {
        let spawns = parse_spawns_xml(
            br#"<spawns><spawn centerx="100" centery="100" centerz="7" radius="3"><monster name="Rat" x="1" y="2" spawntime="60" direction="2" chance="75"/><npc name="Guide" x="0" y="0" spawntime="0"/></spawn></spawns>"#,
        )
        .unwrap();
        assert_eq!(spawns.len(), 1);
        assert_eq!(
            spawns[0].creatures[0].position,
            Position {
                x: 101,
                y: 102,
                z: 7
            }
        );
        assert_eq!(spawns[0].creatures[0].chance, 75);
        assert_eq!(spawns[0].creatures[1].kind, LegacySpawnKind::Npc);

        let houses = parse_houses_xml(
            br#"<houses><house houseid="42" name="Beach House" entryx="101" entryy="102" entryz="7" rent="150" townid="1" size="12" guildhall="false"/></houses>"#,
        )
        .unwrap();
        assert_eq!(houses[0].id, 42);
        assert_eq!(
            houses[0].entry,
            Position {
                x: 101,
                y: 102,
                z: 7
            }
        );
        assert!(!houses[0].guildhall);
    }

    #[test]
    fn rejects_unsafe_companion_paths_and_malformed_xml() {
        assert!(resolve_companion_path(Path::new("data/world"), "../secret.xml").is_err());
        assert!(parse_spawns_xml(b"<spawns><spawn>").is_err());
    }

    #[test]
    fn validates_legacy_house_catalog_against_imported_house_tiles_and_entries() {
        let entry = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("houses", entry);
        map.set_tile(
            entry,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        map.set_house_tile(entry, 42).unwrap();
        let house = LegacyHouse {
            id: 42,
            name: "Beach House".into(),
            entry,
            rent: 150,
            town_id: 1,
            size: 12,
            guildhall: false,
        };
        validate_legacy_houses_against_map(std::slice::from_ref(&house), &map).unwrap();
        assert!(validate_legacy_houses_against_map(&[house.clone(), house.clone()], &map).is_err());

        let mut zero_id = house.clone();
        zero_id.id = 0;
        assert!(validate_legacy_houses_against_map(&[zero_id], &map).is_err());
        let mut missing_tile = house.clone();
        missing_tile.id = 43;
        assert!(validate_legacy_houses_against_map(&[missing_tile], &map).is_err());

        map.set_tile(
            entry,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: false,
            },
        )
        .unwrap();
        assert!(validate_legacy_houses_against_map(&[house], &map).is_err());
    }
}
