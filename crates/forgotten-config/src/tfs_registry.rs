use crate::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_REGISTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_REGISTRY_DEPTH: usize = 32;
const MAX_REGISTRY_ENTRIES: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TfsRegistryCategory {
    Actions,
    CreatureScripts,
    Events,
    GlobalEvents,
    Movements,
    Spells,
    TalkActions,
    Weapons,
    Monsters,
    Npcs,
}

impl TfsRegistryCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Actions => "actions",
            Self::CreatureScripts => "creaturescripts",
            Self::Events => "events",
            Self::GlobalEvents => "globalevents",
            Self::Movements => "movements",
            Self::Spells => "spells",
            Self::TalkActions => "talkactions",
            Self::Weapons => "weapons",
            Self::Monsters => "monsters",
            Self::Npcs => "npcs",
        }
    }

    pub fn runtime_status(self) -> &'static str {
        match self {
            Self::Monsters | Self::Npcs => "deferred creature runtime",
            Self::Weapons => "deferred weapon runtime",
            _ => "deferred Lua event runtime",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsRegistryInventory {
    pub category: TfsRegistryCategory,
    pub registry_path: PathBuf,
    pub present: bool,
    pub entry_count: usize,
    pub reference_count: usize,
    pub missing_references: Vec<PathBuf>,
    pub unsafe_references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsContentInventory {
    pub registries: Vec<TfsRegistryInventory>,
}

/// One explicitly selected legacy XML `script` reference. Resolving this value does not read or
/// execute the script. The caller must pass its `script_root` and `relative_path` to an execution
/// boundary that independently enforces canonical-root and source-resource limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsRegistryScriptReference {
    pub category: TfsRegistryCategory,
    pub registry_path: PathBuf,
    pub script_root: PathBuf,
    pub relative_path: PathBuf,
}

impl TfsContentInventory {
    pub fn present_registry_count(&self) -> usize {
        self.registries
            .iter()
            .filter(|registry| registry.present)
            .count()
    }

    pub fn entry_count(&self) -> usize {
        self.registries
            .iter()
            .map(|registry| registry.entry_count)
            .sum()
    }

    pub fn reference_count(&self) -> usize {
        self.registries
            .iter()
            .map(|registry| registry.reference_count)
            .sum()
    }

    pub fn missing_reference_count(&self) -> usize {
        self.registries
            .iter()
            .map(|registry| registry.missing_references.len())
            .sum()
    }

    pub fn unsafe_reference_count(&self) -> usize {
        self.registries
            .iter()
            .map(|registry| registry.unsafe_references.len())
            .sum()
    }
}

struct RegistrySpec {
    category: TfsRegistryCategory,
    relative_path: &'static str,
    root: &'static [u8],
    entries: &'static [&'static [u8]],
    reference_attribute: Option<&'static [u8]>,
}

const REGISTRY_SPECS: &[RegistrySpec] = &[
    RegistrySpec {
        category: TfsRegistryCategory::Actions,
        relative_path: "actions/actions.xml",
        root: b"actions",
        entries: &[b"action"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::CreatureScripts,
        relative_path: "creaturescripts/creaturescripts.xml",
        root: b"creaturescripts",
        entries: &[b"event"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::Events,
        relative_path: "events/events.xml",
        root: b"events",
        entries: &[b"event"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::GlobalEvents,
        relative_path: "globalevents/globalevents.xml",
        root: b"globalevents",
        entries: &[b"globalevent"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::Movements,
        relative_path: "movements/movements.xml",
        root: b"movements",
        entries: &[b"moveevent"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::Spells,
        relative_path: "spells/spells.xml",
        root: b"spells",
        entries: &[b"instant", b"rune", b"conjure"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::TalkActions,
        relative_path: "talkactions/talkactions.xml",
        root: b"talkactions",
        entries: &[b"talkaction"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::Weapons,
        relative_path: "weapons/weapons.xml",
        root: b"weapons",
        entries: &[b"weapon"],
        reference_attribute: Some(b"script"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::Monsters,
        relative_path: "monster/monsters.xml",
        root: b"monsters",
        entries: &[b"monster"],
        reference_attribute: Some(b"file"),
    },
    RegistrySpec {
        category: TfsRegistryCategory::Npcs,
        relative_path: "npc/npcs.xml",
        root: b"npcs",
        entries: &[b"npc"],
        reference_attribute: Some(b"file"),
    },
];

pub(crate) fn load_tfs_content_inventory(
    config: &EngineConfig,
) -> Result<TfsContentInventory, ConfigError> {
    inventory_tfs_content_directory(&config.content_directory)
}

/// Resolves one caller-named `script` attribute already declared in an operator-owned TFS XML
/// registry. This is an explicit selection helper, not discovery: it never scans `data/scripts`,
/// executes Lua, accepts a `file` attribute, or claims legacy callback compatibility.
pub fn resolve_tfs_registry_script_reference(
    config: &EngineConfig,
    category: TfsRegistryCategory,
    relative_path: &Path,
) -> Result<TfsRegistryScriptReference, ConfigError> {
    resolve_tfs_registry_script_reference_in_directory(
        &config.content_directory,
        category,
        relative_path,
    )
}

fn resolve_tfs_registry_script_reference_in_directory(
    content_directory: &Path,
    category: TfsRegistryCategory,
    relative_path: &Path,
) -> Result<TfsRegistryScriptReference, ConfigError> {
    if !relative_path.is_relative()
        || !relative_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(invalid(
            "TFS callback path must contain only normal relative components",
        ));
    }
    let spec = REGISTRY_SPECS
        .iter()
        .find(|spec| spec.category == category)
        .ok_or_else(|| invalid("unknown TFS registry category"))?;
    if spec.reference_attribute != Some(b"script") {
        return Err(invalid(format!(
            "TFS {} registry has no Lua script reference attribute",
            category.label()
        )));
    }
    let registry_path = content_directory.join(spec.relative_path);
    if !registry_path.is_file() {
        return Err(invalid(format!(
            "TFS {} registry is not a regular file",
            category.label()
        )));
    }
    let bytes = fs::read(&registry_path).map_err(ConfigError::Io)?;
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(invalid(format!(
            "TFS {} registry exceeds the configured 16 MiB limit",
            category.label()
        )));
    }
    let script_root = registry_path
        .parent()
        .ok_or_else(|| invalid("TFS registry has no parent directory"))?
        .to_path_buf();
    if !registry_declares_script_reference(spec, &bytes, relative_path)? {
        return Err(invalid(format!(
            "TFS {} registry does not declare script `{}`",
            category.label(),
            relative_path.display()
        )));
    }
    Ok(TfsRegistryScriptReference {
        category,
        registry_path,
        script_root,
        relative_path: relative_path.to_path_buf(),
    })
}

fn inventory_tfs_content_directory(
    content_directory: &Path,
) -> Result<TfsContentInventory, ConfigError> {
    let mut registries = Vec::with_capacity(REGISTRY_SPECS.len());
    for spec in REGISTRY_SPECS {
        registries.push(inventory_registry(content_directory, spec)?);
    }
    Ok(TfsContentInventory { registries })
}

fn inventory_registry(
    content_directory: &Path,
    spec: &RegistrySpec,
) -> Result<TfsRegistryInventory, ConfigError> {
    let registry_path = content_directory.join(spec.relative_path);
    if !registry_path.is_file() {
        return Ok(TfsRegistryInventory {
            category: spec.category,
            registry_path,
            present: false,
            entry_count: 0,
            reference_count: 0,
            missing_references: Vec::new(),
            unsafe_references: Vec::new(),
        });
    }
    let bytes = fs::read(&registry_path).map_err(ConfigError::Io)?;
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(invalid(format!(
            "TFS {} registry exceeds the configured 16 MiB limit",
            spec.category.label()
        )));
    }
    let base_directory = registry_path
        .parent()
        .ok_or_else(|| invalid("TFS registry has no parent directory"))?;
    parse_registry(spec, &registry_path, base_directory, &bytes)
}

fn parse_registry(
    spec: &RegistrySpec,
    registry_path: &Path,
    base_directory: &Path,
    bytes: &[u8],
) -> Result<TfsRegistryInventory, ConfigError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut entry_count = 0usize;
    let mut reference_count = 0usize;
    let mut missing_references = Vec::new();
    let mut unsafe_references = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                ensure_depth(depth)?;
                parse_registry_element(
                    spec,
                    &event,
                    depth,
                    base_directory,
                    &mut root_seen,
                    &mut entry_count,
                    &mut reference_count,
                    &mut missing_references,
                    &mut unsafe_references,
                )?;
            }
            Event::Empty(event) => {
                parse_registry_element(
                    spec,
                    &event,
                    depth + 1,
                    base_directory,
                    &mut root_seen,
                    &mut entry_count,
                    &mut reference_count,
                    &mut missing_references,
                    &mut unsafe_references,
                )?;
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed TFS registry XML depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid(format!(
            "TFS {} registry is malformed or has the wrong root element",
            spec.category.label()
        )));
    }
    Ok(TfsRegistryInventory {
        category: spec.category,
        registry_path: registry_path.to_path_buf(),
        present: true,
        entry_count,
        reference_count,
        missing_references,
        unsafe_references,
    })
}

fn registry_declares_script_reference(
    spec: &RegistrySpec,
    bytes: &[u8],
    expected_relative_path: &Path,
) -> Result<bool, ConfigError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut entry_count = 0usize;
    let mut declared = false;
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                ensure_depth(depth)?;
                inspect_registry_script_reference(
                    spec,
                    &event,
                    depth,
                    expected_relative_path,
                    &mut root_seen,
                    &mut entry_count,
                    &mut declared,
                )?;
            }
            Event::Empty(event) => {
                inspect_registry_script_reference(
                    spec,
                    &event,
                    depth + 1,
                    expected_relative_path,
                    &mut root_seen,
                    &mut entry_count,
                    &mut declared,
                )?;
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed TFS registry XML depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid(format!(
            "TFS {} registry is malformed or has the wrong root element",
            spec.category.label()
        )));
    }
    Ok(declared)
}

#[allow(clippy::too_many_arguments)]
fn inspect_registry_script_reference(
    spec: &RegistrySpec,
    event: &BytesStart<'_>,
    depth: usize,
    expected_relative_path: &Path,
    root_seen: &mut bool,
    entry_count: &mut usize,
    declared: &mut bool,
) -> Result<(), ConfigError> {
    let name = event.name();
    if depth == 1 && name.as_ref() == spec.root {
        *root_seen = true;
        return Ok(());
    }
    if depth != 2 || !spec.entries.iter().any(|entry| name.as_ref() == *entry) {
        return Ok(());
    }
    if !*root_seen {
        return Err(invalid(
            "TFS registry entry appears before the root element",
        ));
    }
    if *entry_count >= MAX_REGISTRY_ENTRIES {
        return Err(invalid(format!(
            "TFS {} registry entry count exceeds the configured limit",
            spec.category.label()
        )));
    }
    *entry_count += 1;
    let Some(reference) = optional_attribute_string(event, b"script")? else {
        return Ok(());
    };
    if safe_relative_path(&reference).as_deref() == Some(expected_relative_path) {
        *declared = true;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_registry_element(
    spec: &RegistrySpec,
    event: &BytesStart<'_>,
    depth: usize,
    base_directory: &Path,
    root_seen: &mut bool,
    entry_count: &mut usize,
    reference_count: &mut usize,
    missing_references: &mut Vec<PathBuf>,
    unsafe_references: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let name = event.name();
    if depth == 1 && name.as_ref() == spec.root {
        *root_seen = true;
        return Ok(());
    }
    if depth != 2 || !spec.entries.iter().any(|entry| name.as_ref() == *entry) {
        return Ok(());
    }
    if !*root_seen {
        return Err(invalid(
            "TFS registry entry appears before the root element",
        ));
    }
    if *entry_count >= MAX_REGISTRY_ENTRIES {
        return Err(invalid(format!(
            "TFS {} registry entry count exceeds the configured limit",
            spec.category.label()
        )));
    }
    *entry_count += 1;
    let Some(attribute) = spec.reference_attribute else {
        return Ok(());
    };
    let Some(reference) = optional_attribute_string(event, attribute)? else {
        return Ok(());
    };
    *reference_count += 1;
    match safe_relative_path(&reference) {
        Some(path) => {
            let resolved = base_directory.join(path);
            if !resolved.is_file() {
                missing_references.push(resolved);
            }
        }
        None => unsafe_references.push(reference),
    }
    Ok(())
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

fn ensure_depth(depth: usize) -> Result<(), ConfigError> {
    if depth > MAX_REGISTRY_DEPTH {
        Err(invalid(
            "TFS registry XML nesting exceeds the configured limit",
        ))
    } else {
        Ok(())
    }
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
    invalid(format!("TFS registry XML parse error: {error}"))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_content_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forgotten-engine-{name}-{nonce}/data"))
    }

    #[test]
    fn inventories_registry_references_without_executing_scripts() {
        let data = temporary_content_directory("tfs-registry");
        fs::create_dir_all(data.join("actions/scripts")).unwrap();
        fs::create_dir_all(data.join("monster/monsters")).unwrap();
        fs::create_dir_all(data.join("weapons/scripts")).unwrap();
        fs::write(
            data.join("actions/actions.xml"),
            r#"<actions><action itemid="100" script="scripts/rope.lua"/><action itemid="101" script="../escape.lua"/></actions>"#,
        )
        .unwrap();
        fs::write(data.join("actions/scripts/rope.lua"), "-- private script").unwrap();
        fs::write(
            data.join("monster/monsters.xml"),
            r#"<monsters><monster name="Rat" file="monsters/rat.xml"/></monsters>"#,
        )
        .unwrap();
        fs::write(
            data.join("weapons/weapons.xml"),
            r#"<weapons><weapon id="2376" script="scripts/sword.lua"/></weapons>"#,
        )
        .unwrap();
        fs::write(
            data.join("weapons/scripts/sword.lua"),
            "-- private weapon script",
        )
        .unwrap();

        let inventory = inventory_tfs_content_directory(&data).unwrap();
        assert_eq!(inventory.present_registry_count(), 3);
        assert_eq!(inventory.entry_count(), 4);
        assert_eq!(inventory.reference_count(), 4);
        assert_eq!(inventory.missing_reference_count(), 1);
        assert_eq!(inventory.unsafe_reference_count(), 1);
        assert!(inventory.registries.iter().any(|registry| {
            registry.category == TfsRegistryCategory::Weapons
                && registry.entry_count == 1
                && registry.category.runtime_status() == "deferred weapon runtime"
        }));
        let _ = fs::remove_dir_all(data.parent().unwrap());
    }

    #[test]
    fn resolves_only_an_explicit_safe_tfs_script_declaration() {
        let data = temporary_content_directory("tfs-registry-callback-reference");
        fs::create_dir_all(data.join("actions/scripts")).unwrap();
        fs::create_dir_all(data.join("monster/monsters")).unwrap();
        fs::write(
            data.join("actions/actions.xml"),
            r#"<actions><action itemid="100" script="scripts/safe.lua"/><action itemid="101" script="../unsafe.lua"/></actions>"#,
        )
        .unwrap();
        fs::write(
            data.join("actions/scripts/safe.lua"),
            "return function() end",
        )
        .unwrap();
        fs::write(
            data.join("monster/monsters.xml"),
            r#"<monsters><monster name="Rat" file="monsters/rat.xml"/></monsters>"#,
        )
        .unwrap();

        let reference = resolve_tfs_registry_script_reference_in_directory(
            &data,
            TfsRegistryCategory::Actions,
            Path::new("scripts/safe.lua"),
        )
        .unwrap();
        assert_eq!(reference.category, TfsRegistryCategory::Actions);
        assert_eq!(reference.script_root, data.join("actions"));
        assert_eq!(reference.relative_path, PathBuf::from("scripts/safe.lua"));
        assert_eq!(reference.registry_path, data.join("actions/actions.xml"));
        assert!(resolve_tfs_registry_script_reference_in_directory(
            &data,
            TfsRegistryCategory::Actions,
            Path::new("scripts/missing.lua"),
        )
        .is_err());
        assert!(resolve_tfs_registry_script_reference_in_directory(
            &data,
            TfsRegistryCategory::Actions,
            Path::new("../unsafe.lua"),
        )
        .is_err());
        assert!(resolve_tfs_registry_script_reference_in_directory(
            &data,
            TfsRegistryCategory::Monsters,
            Path::new("monsters/rat.xml"),
        )
        .is_err());
        let _ = fs::remove_dir_all(data.parent().unwrap());
    }

    #[test]
    fn rejects_registry_with_wrong_root() {
        let data = temporary_content_directory("wrong-registry-root");
        fs::create_dir_all(data.join("actions")).unwrap();
        fs::write(data.join("actions/actions.xml"), "<broken/>").unwrap();
        assert!(inventory_tfs_content_directory(&data).is_err());
        let _ = fs::remove_dir_all(data.parent().unwrap());
    }

    #[test]
    fn weapon_registry_is_reported_as_a_distinct_deferred_runtime() {
        assert_eq!(
            TfsRegistryCategory::Weapons.runtime_status(),
            "deferred weapon runtime"
        );
        assert_eq!(
            TfsRegistryCategory::Spells.runtime_status(),
            "deferred Lua event runtime"
        );
    }
}
