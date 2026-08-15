use crate::legacy_xml::{LegacySpawnKind, LegacyWorldCompanionData};
use crate::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_ENTITY_CATALOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTITY_DEFINITION_BYTES: usize = 2 * 1024 * 1024;
const MAX_ENTITY_XML_DEPTH: usize = 32;
const MAX_ENTITY_DEFINITIONS: usize = 200_000;

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
        let normalized = normalize_name(name);
        let entities = match kind {
            TfsEntityKind::Monster => &self.monsters,
            TfsEntityKind::Npc => &self.npcs,
        };
        entities
            .iter()
            .any(|entity| normalize_name(&entity.name) == normalized)
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
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                ensure_depth(depth)?;
                if event.name().as_ref() == root && depth == 1 {
                    return entity_from_root_event(&event, definition_path, kind, npc_directory);
                }
            }
            Event::Empty(event) => {
                if event.name().as_ref() == root && depth + 1 == 1 {
                    return entity_from_root_event(&event, definition_path, kind, npc_directory);
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
    Err(invalid(format!(
        "TFS {} definition {} has no {} root",
        kind.label(),
        definition_path.display(),
        String::from_utf8_lossy(root)
    )))
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
    })
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
            r#"<monster name="Rat"/>"#,
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
            }],
            npcs: vec![TfsEntityDefinition {
                kind: TfsEntityKind::Npc,
                name: "Alice".into(),
                definition_path: PathBuf::from("Alice.xml"),
                script_path: None,
                script_present: true,
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
}
