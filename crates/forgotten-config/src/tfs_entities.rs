use crate::legacy_xml::{LegacySpawnKind, LegacyWorldCompanionData};
use crate::{ConfigError, EngineConfig};
use forgotten_core::{FeTfsStaticEntity, FeTfsStaticSpawnCollection};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_ENTITY_CATALOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTITY_DEFINITION_BYTES: usize = 2 * 1024 * 1024;
const MAX_ENTITY_XML_DEPTH: usize = 32;
const MAX_ENTITY_DEFINITIONS: usize = 200_000;
const STATIC_TFS_ENTITY_ID_START: u32 = 0x4000_0001;
const DEFAULT_STATIC_ENTITY_SPEED: u16 = 220;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TfsEntityKind {
    Monster,
    Npc,
}

impl TfsEntityKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Monster => "monster",
            Self::Npc => "npc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsEntityDefinition {
    pub kind: TfsEntityKind,
    pub name: String,
    pub definition_path: PathBuf,
    pub script_path: Option<PathBuf>,
    pub script_present: bool,
    /// Render-only metadata from XML. Definition scripts remain unexecuted.
    pub appearance: Option<TfsEntityAppearance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsEntityAppearance {
    pub look_type: u8,
    pub head: u8,
    pub body: u8,
    pub legs: u8,
    pub feet: u8,
    pub addons: u8,
    /// Monster definitions may provide this value; NPC definitions generally do not.
    pub speed: u16,
    /// Static entity display initializes at full health.
    pub max_health: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsEntityCatalog {
    pub monsters: Vec<TfsEntityDefinition>,
    pub npcs: Vec<TfsEntityDefinition>,
    pub missing_definitions: Vec<PathBuf>,
    pub missing_scripts: Vec<PathBuf>,
    pub unsafe_references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsSpawnResolution {
    pub spawn_creature_count: usize,
    pub resolved_creature_count: usize,
    pub unresolved_monsters: Vec<String>,
    pub unresolved_npcs: Vec<String>,
}

impl TfsEntityCatalog {
    pub fn entity_count(&self) -> usize {
        self.monsters.len() + self.npcs.len()
    }

    pub fn contains(&self, kind: TfsEntityKind, name: &str) -> bool {
        self.find(kind, name).is_some()
    }

    pub fn find(&self, kind: TfsEntityKind, name: &str) -> Option<&TfsEntityDefinition> {
        let normalized = normalize_name(name);
        let entities = match kind {
            TfsEntityKind::Monster => &self.monsters,
            TfsEntityKind::Npc => &self.npcs,
        };
        entities
            .iter()
            .find(|entity| normalize_name(&entity.name) == normalized)
    }
}

pub(crate) fn resolve_tfs_spawns(
    companions: &LegacyWorldCompanionData,
    catalog: &TfsEntityCatalog,
) -> TfsSpawnResolution {
    let mut spawn_creature_count = 0usize;
    let mut resolved_creature_count = 0usize;
    let mut unresolved_monsters = BTreeSet::new();
    let mut unresolved_npcs = BTreeSet::new();
    for spawn_area in &companions.spawns {
        for creature in &spawn_area.creatures {
            spawn_creature_count += 1;
            let (kind, unresolved) = match creature.kind {
                LegacySpawnKind::Monster => (TfsEntityKind::Monster, &mut unresolved_monsters),
                LegacySpawnKind::Npc => (TfsEntityKind::Npc, &mut unresolved_npcs),
            };
            if catalog.contains(kind, &creature.name) {
                resolved_creature_count += 1;
            } else {
                unresolved.insert(creature.name.clone());
            }
        }
    }
    TfsSpawnResolution {
        spawn_creature_count,
        resolved_creature_count,
        unresolved_monsters: unresolved_monsters.into_iter().collect(),
        unresolved_npcs: unresolved_npcs.into_iter().collect(),
    }
}

pub fn materialize_tfs_static_spawns(
    companions: &LegacyWorldCompanionData,
    catalog: &TfsEntityCatalog,
) -> Result<FeTfsStaticSpawnCollection, ConfigError> {
    let mut entities = Vec::new();
    let mut respawn_intervals_seconds = BTreeMap::new();
    let mut next_id = STATIC_TFS_ENTITY_ID_START;
    for spawn_area in &companions.spawns {
        for creature in &spawn_area.creatures {
            let kind = match creature.kind {
                LegacySpawnKind::Monster => TfsEntityKind::Monster,
                LegacySpawnKind::Npc => TfsEntityKind::Npc,
            };
            let Some(definition) = catalog.find(kind, &creature.name) else {
                continue;
            };
            let Some(appearance) = &definition.appearance else {
                continue;
            };
            // A zero look type has no operator-provided client appearance and is not rendered.
            if appearance.look_type == 0 {
                continue;
            }
            if creature.spawn_interval_seconds > 0 {
                respawn_intervals_seconds.insert(next_id, creature.spawn_interval_seconds);
            }
            entities.push(FeTfsStaticEntity {
                id: next_id,
                name: definition.name.clone(),
                position: creature.position,
                look_type: appearance.look_type,
                head: appearance.head,
                body: appearance.body,
                legs: appearance.legs,
                feet: appearance.feet,
                addons: appearance.addons,
                speed: if appearance.speed == 0 {
                    DEFAULT_STATIC_ENTITY_SPEED
                } else {
                    appearance.speed
                },
                health_percent: u8::from(appearance.max_health > 0) * 100,
                direction: creature.direction % 4,
            });
            next_id = next_id
                .checked_add(1)
                .ok_or_else(|| invalid("static TFS spawn identifier range is exhausted"))?;
        }
    }
    FeTfsStaticSpawnCollection::with_respawn_intervals(entities, respawn_intervals_seconds)
        .map_err(|error| invalid(format!("invalid static TFS spawn collection: {error}")))
}

pub(crate) fn load_tfs_entity_catalog(
    config: &EngineConfig,
) -> Result<TfsEntityCatalog, ConfigError> {
    load_tfs_entity_catalog_from_data(&config.content_directory)
}

fn load_tfs_entity_catalog_from_data(
    data_directory: &Path,
) -> Result<TfsEntityCatalog, ConfigError> {
    let monster_directory = data_directory.join("monster");
    let npc_directory = data_directory.join("npc");
    let mut catalog = TfsEntityCatalog {
        monsters: Vec::new(),
        npcs: Vec::new(),
        missing_definitions: Vec::new(),
        missing_scripts: Vec::new(),
        unsafe_references: Vec::new(),
    };

    load_monsters(&monster_directory, &mut catalog)?;
    load_npcs(&npc_directory, &mut catalog)?;
    Ok(catalog)
}

fn load_monsters(directory: &Path, catalog: &mut TfsEntityCatalog) -> Result<(), ConfigError> {
    let registry_path = directory.join("monsters.xml");
    if !registry_path.is_file() {
        return Ok(());
    }
    for reference in parse_registry_references(&registry_path, b"monsters", b"monster")? {
        let Some(relative) = safe_relative_path(&reference.file) else {
            catalog.unsafe_references.push(reference.file);
            continue;
        };
        let definition_path = directory.join(relative);
        if !definition_path.is_file() {
            catalog.missing_definitions.push(definition_path);
            continue;
        }
        let definition =
            parse_entity_definition(&definition_path, TfsEntityKind::Monster, b"monster", None)?;
        add_entity(&mut catalog.monsters, definition)?;
    }
    Ok(())
}

fn load_npcs(directory: &Path, catalog: &mut TfsEntityCatalog) -> Result<(), ConfigError> {
    let registry_path = directory.join("npcs.xml");
    if registry_path.is_file() {
        for reference in parse_registry_references(&registry_path, b"npcs", b"npc")? {
            let Some(relative) = safe_relative_path(&reference.file) else {
                catalog.unsafe_references.push(reference.file);
                continue;
            };
            let definition_path = directory.join(relative);
            if !definition_path.is_file() {
                catalog.missing_definitions.push(definition_path);
                continue;
            }
            let definition = parse_entity_definition(
                &definition_path,
                TfsEntityKind::Npc,
                b"npc",
                Some(directory),
            )?;
            if !definition.script_present {
                if let Some(script_path) = &definition.script_path {
                    catalog.missing_scripts.push(script_path.clone());
                }
            }
            add_entity(&mut catalog.npcs, definition)?;
        }
        return Ok(());
    }

    if !directory.is_dir() {
        return Ok(());
    }
    let mut direct_definitions = fs::read_dir(directory)
        .map_err(ConfigError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ConfigError::Io)?;
    direct_definitions.sort_by_key(|entry| entry.file_name());
    for entry in direct_definitions {
        let path = entry.path();
        if path == registry_path
            || !path.is_file()
            || path.extension().and_then(|value| value.to_str()) != Some("xml")
        {
            continue;
        }
        let definition =
            parse_entity_definition(&path, TfsEntityKind::Npc, b"npc", Some(directory))?;
        if !definition.script_present {
            if let Some(script_path) = &definition.script_path {
                catalog.missing_scripts.push(script_path.clone());
            }
        }
        add_entity(&mut catalog.npcs, definition)?;
    }
    Ok(())
}

fn add_entity(
    entities: &mut Vec<TfsEntityDefinition>,
    definition: TfsEntityDefinition,
) -> Result<(), ConfigError> {
    if entities.len() >= MAX_ENTITY_DEFINITIONS {
        return Err(invalid(
            "TFS entity catalog exceeds the configured definition limit",
        ));
    }
    entities.push(definition);
    Ok(())
}

struct RegistryReference {
    file: String,
}

fn parse_registry_references(
    path: &Path,
    root: &[u8],
    entry: &[u8],
) -> Result<Vec<RegistryReference>, ConfigError> {
    let bytes = fs::read(path).map_err(ConfigError::Io)?;
    ensure_size(&bytes, MAX_ENTITY_CATALOG_BYTES, "TFS entity registry")?;
    let mut reader = Reader::from_reader(bytes.as_slice());
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut references = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                ensure_depth(depth)?;
                parse_registry_event(&event, depth, root, entry, &mut root_seen, &mut references)?;
            }
            Event::Empty(event) => {
                parse_registry_event(
                    &event,
                    depth + 1,
                    root,
                    entry,
                    &mut root_seen,
                    &mut references,
                )?;
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed TFS entity registry XML depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid(
            "TFS entity registry is malformed or has the wrong root",
        ));
    }
    Ok(references)
}

fn parse_registry_event(
    event: &BytesStart<'_>,
    depth: usize,
    root: &[u8],
    entry: &[u8],
    root_seen: &mut bool,
    references: &mut Vec<RegistryReference>,
) -> Result<(), ConfigError> {
    let name = event.name();
    if depth == 1 && name.as_ref() == root {
        *root_seen = true;
        return Ok(());
    }
    if depth != 2 || name.as_ref() != entry {
        return Ok(());
    }
    let file = required_attribute_string(event, b"file")?;
    references.push(RegistryReference { file });
    Ok(())
}

fn parse_entity_definition(
    definition_path: &Path,
    kind: TfsEntityKind,
    root: &[u8],
    npc_directory: Option<&Path>,
) -> Result<TfsEntityDefinition, ConfigError> {
    let bytes = fs::read(definition_path).map_err(ConfigError::Io)?;
    ensure_size(&bytes, MAX_ENTITY_DEFINITION_BYTES, "TFS entity definition")?;
    let mut reader = Reader::from_reader(bytes.as_slice());
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut definition = None;
    let mut root_speed = 0u16;
    let mut look = None;
    let mut max_health = None;
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                ensure_depth(depth)?;
                if event.name().as_ref() == root && depth == 1 {
                    root_speed = optional_attribute_u16(&event, b"speed")?.unwrap_or_default();
                    definition = Some(entity_from_root_event(
                        &event,
                        definition_path,
                        kind,
                        npc_directory,
                    )?);
                } else if definition.is_some() && depth == 2 {
                    parse_appearance_event(&event, &mut look, &mut max_health)?;
                }
            }
            Event::Empty(event) => {
                if event.name().as_ref() == root && depth + 1 == 1 {
                    root_speed = optional_attribute_u16(&event, b"speed")?.unwrap_or_default();
                    definition = Some(entity_from_root_event(
                        &event,
                        definition_path,
                        kind,
                        npc_directory,
                    )?);
                } else if definition.is_some() && depth + 1 == 2 {
                    parse_appearance_event(&event, &mut look, &mut max_health)?;
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed TFS entity definition XML depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    let mut definition = definition.ok_or_else(|| {
        invalid(format!(
            "TFS {} definition {} has no {} root",
            kind.label(),
            definition_path.display(),
            String::from_utf8_lossy(root)
        ))
    })?;
    definition.appearance = look.map(|look| TfsEntityAppearance {
        look_type: look.look_type,
        head: look.head,
        body: look.body,
        legs: look.legs,
        feet: look.feet,
        addons: look.addons,
        speed: root_speed,
        max_health: max_health.unwrap_or(1),
    });
    Ok(definition)
}

fn entity_from_root_event(
    event: &BytesStart<'_>,
    definition_path: &Path,
    kind: TfsEntityKind,
    npc_directory: Option<&Path>,
) -> Result<TfsEntityDefinition, ConfigError> {
    let name = required_attribute_string(event, b"name")?;
    let (script_path, script_present) = if let Some(directory) = npc_directory {
        let script = optional_attribute_string(event, b"script")?;
        resolve_npc_script(directory, script.as_deref())
    } else {
        (None, true)
    };
    Ok(TfsEntityDefinition {
        kind,
        name,
        definition_path: definition_path.to_path_buf(),
        script_path,
        script_present,
        appearance: None,
    })
}

#[derive(Debug, Clone, Copy)]
struct EntityLook {
    look_type: u8,
    head: u8,
    body: u8,
    legs: u8,
    feet: u8,
    addons: u8,
}

fn parse_appearance_event(
    event: &BytesStart<'_>,
    look: &mut Option<EntityLook>,
    max_health: &mut Option<u16>,
) -> Result<(), ConfigError> {
    match event.name().as_ref() {
        b"look" => {
            let Some(look_type) = optional_attribute_u8(event, b"type")? else {
                return Ok(());
            };
            *look = Some(EntityLook {
                look_type,
                head: optional_attribute_u8(event, b"head")?.unwrap_or_default(),
                body: optional_attribute_u8(event, b"body")?.unwrap_or_default(),
                legs: optional_attribute_u8(event, b"legs")?.unwrap_or_default(),
                feet: optional_attribute_u8(event, b"feet")?.unwrap_or_default(),
                addons: optional_attribute_u8(event, b"addons")?.unwrap_or_default(),
            });
        }
        b"health" => *max_health = optional_attribute_u16(event, b"max")?,
        _ => {}
    }
    Ok(())
}

fn resolve_npc_script(directory: &Path, script: Option<&str>) -> (Option<PathBuf>, bool) {
    let Some(script) = script else {
        return (None, true);
    };
    let Some(relative) = safe_relative_path(script) else {
        return (None, false);
    };
    let direct = directory.join(&relative);
    if direct.is_file() {
        return (Some(direct), true);
    }
    let conventional = directory.join("scripts").join(relative);
    let present = conventional.is_file();
    (Some(conventional), present)
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        None
    } else {
        Some(path.to_path_buf())
    }
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn ensure_size(bytes: &[u8], limit: usize, label: &str) -> Result<(), ConfigError> {
    if bytes.len() > limit {
        Err(invalid(format!(
            "{label} exceeds the configured size limit"
        )))
    } else {
        Ok(())
    }
}

fn ensure_depth(depth: usize) -> Result<(), ConfigError> {
    if depth > MAX_ENTITY_XML_DEPTH {
        Err(invalid(
            "TFS entity XML nesting exceeds the configured limit",
        ))
    } else {
        Ok(())
    }
}

fn required_attribute_string(event: &BytesStart<'_>, name: &[u8]) -> Result<String, ConfigError> {
    optional_attribute_string(event, name)?.ok_or_else(|| {
        invalid(format!(
            "TFS entity XML is missing required {} attribute",
            String::from_utf8_lossy(name)
        ))
    })
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

fn optional_attribute_u8(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<u8>, ConfigError> {
    optional_attribute_string(event, name)?
        .map(|value| {
            value.parse::<u8>().map_err(|_| {
                invalid(format!(
                    "TFS entity XML has invalid {} value",
                    String::from_utf8_lossy(name)
                ))
            })
        })
        .transpose()
}

fn optional_attribute_u16(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<u16>, ConfigError> {
    optional_attribute_string(event, name)?
        .map(|value| {
            value.parse::<u16>().map_err(|_| {
                invalid(format!(
                    "TFS entity XML has invalid {} value",
                    String::from_utf8_lossy(name)
                ))
            })
        })
        .transpose()
}

fn xml_error(error: impl std::fmt::Display) -> ConfigError {
    invalid(format!("TFS entity XML parse error: {error}"))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_xml::{LegacySpawnArea, LegacySpawnCreature};
    use forgotten_core::Position;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_data_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forgotten-engine-{name}-{nonce}/data"))
    }

    #[test]
    fn loads_monster_registry_and_direct_npc_definitions_without_executing_scripts() {
        let data = temporary_data_directory("tfs-entities");
        fs::create_dir_all(data.join("monster/monsters")).unwrap();
        fs::create_dir_all(data.join("npc/scripts")).unwrap();
        fs::write(
            data.join("monster/monsters.xml"),
            r#"<monsters><monster name="Rat" file="monsters/rat.xml"/></monsters>"#,
        )
        .unwrap();
        fs::write(
            data.join("monster/monsters/rat.xml"),
            r#"<monster name="Rat" speed="134"><health now="20" max="20"/><look type="21"/></monster>"#,
        )
        .unwrap();
        fs::write(
            data.join("npc/Alice.xml"),
            r#"<npc name="Alice" script="bless.lua"/>"#,
        )
        .unwrap();
        fs::write(data.join("npc/scripts/bless.lua"), "-- private script").unwrap();

        let catalog = load_tfs_entity_catalog_from_data(&data).unwrap();
        assert!(catalog.contains(TfsEntityKind::Monster, "rat"));
        assert!(catalog.contains(TfsEntityKind::Npc, "ALICE"));
        assert_eq!(
            catalog.monsters[0].appearance,
            Some(TfsEntityAppearance {
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                max_health: 20,
            })
        );
        assert_eq!(catalog.missing_definitions.len(), 0);
        assert_eq!(catalog.missing_scripts.len(), 0);
        let _ = fs::remove_dir_all(data.parent().unwrap());
    }

    #[test]
    fn records_missing_and_unsafe_entity_references() {
        let data = temporary_data_directory("tfs-entity-errors");
        fs::create_dir_all(data.join("monster")).unwrap();
        fs::write(
            data.join("monster/monsters.xml"),
            r#"<monsters><monster name="Missing" file="missing.xml"/><monster name="Unsafe" file="../unsafe.xml"/></monsters>"#,
        )
        .unwrap();

        let catalog = load_tfs_entity_catalog_from_data(&data).unwrap();
        assert_eq!(catalog.missing_definitions.len(), 1);
        assert_eq!(catalog.unsafe_references, vec!["../unsafe.xml"]);
        let _ = fs::remove_dir_all(data.parent().unwrap());
    }

    #[test]
    fn resolves_spawn_entities_case_insensitively_and_reports_missing_names() {
        let catalog = TfsEntityCatalog {
            monsters: vec![TfsEntityDefinition {
                kind: TfsEntityKind::Monster,
                name: "Rat".into(),
                definition_path: PathBuf::from("rat.xml"),
                script_path: None,
                script_present: true,
                appearance: None,
            }],
            npcs: vec![TfsEntityDefinition {
                kind: TfsEntityKind::Npc,
                name: "Alice".into(),
                definition_path: PathBuf::from("Alice.xml"),
                script_path: None,
                script_present: true,
                appearance: None,
            }],
            missing_definitions: Vec::new(),
            missing_scripts: Vec::new(),
            unsafe_references: Vec::new(),
        };
        let companions = LegacyWorldCompanionData {
            spawn_file: None,
            house_file: None,
            houses: Vec::new(),
            spawns: vec![LegacySpawnArea {
                center: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                radius: 1,
                creatures: vec![
                    LegacySpawnCreature {
                        kind: LegacySpawnKind::Monster,
                        name: "rat".into(),
                        position: Position {
                            x: 100,
                            y: 100,
                            z: 7,
                        },
                        spawn_interval_seconds: 60,
                        direction: 0,
                        chance: 100,
                    },
                    LegacySpawnCreature {
                        kind: LegacySpawnKind::Npc,
                        name: "Missing NPC".into(),
                        position: Position {
                            x: 101,
                            y: 100,
                            z: 7,
                        },
                        spawn_interval_seconds: 60,
                        direction: 0,
                        chance: 100,
                    },
                ],
            }],
        };

        let resolution = resolve_tfs_spawns(&companions, &catalog);
        assert_eq!(resolution.spawn_creature_count, 2);
        assert_eq!(resolution.resolved_creature_count, 1);
        assert!(resolution.unresolved_monsters.is_empty());
        assert_eq!(resolution.unresolved_npcs, vec!["Missing NPC"]);
    }

    #[test]
    fn materializes_only_resolved_renderable_spawns_in_stable_order() {
        let position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let catalog = TfsEntityCatalog {
            monsters: vec![TfsEntityDefinition {
                kind: TfsEntityKind::Monster,
                name: "Rat".into(),
                definition_path: PathBuf::from("rat.xml"),
                script_path: None,
                script_present: true,
                appearance: Some(TfsEntityAppearance {
                    look_type: 21,
                    head: 1,
                    body: 2,
                    legs: 3,
                    feet: 4,
                    addons: 0,
                    speed: 134,
                    max_health: 20,
                }),
            }],
            npcs: Vec::new(),
            missing_definitions: Vec::new(),
            missing_scripts: Vec::new(),
            unsafe_references: Vec::new(),
        };
        let companions = LegacyWorldCompanionData {
            spawn_file: None,
            house_file: None,
            houses: Vec::new(),
            spawns: vec![LegacySpawnArea {
                center: position,
                radius: 0,
                creatures: vec![
                    LegacySpawnCreature {
                        kind: LegacySpawnKind::Monster,
                        name: "RAT".into(),
                        position,
                        spawn_interval_seconds: 60,
                        direction: 6,
                        chance: 100,
                    },
                    LegacySpawnCreature {
                        kind: LegacySpawnKind::Monster,
                        name: "Missing".into(),
                        position,
                        spawn_interval_seconds: 60,
                        direction: 0,
                        chance: 100,
                    },
                ],
            }],
        };

        let spawns = materialize_tfs_static_spawns(&companions, &catalog).unwrap();
        assert_eq!(spawns.entities.len(), 1);
        assert_eq!(spawns.entities[0].id, STATIC_TFS_ENTITY_ID_START);
        assert_eq!(spawns.entities[0].name, "Rat");
        assert_eq!(spawns.entities[0].position, position);
        assert_eq!(spawns.entities[0].speed, 134);
        assert_eq!(spawns.entities[0].health_percent, 100);
        assert_eq!(spawns.entities[0].direction, 2);
        assert_eq!(
            spawns.respawn_interval_seconds(STATIC_TFS_ENTITY_ID_START),
            60
        );
    }
}
