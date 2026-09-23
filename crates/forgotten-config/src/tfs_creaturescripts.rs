//! Bounded, non-executing parser for the operator-owned TFS
//! `creaturescripts/creaturescripts.xml` registry. It retains only an event `type`, a unique
//! registration `name`, and a validated `script` path per entry. It never reads or executes Lua;
//! per-creature event registration and callback semantics stay a deferred runtime boundary.

use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

const CREATURESCRIPTS_REGISTRY_RELATIVE_PATH: &str = "creaturescripts/creaturescripts.xml";
const MAX_CREATURESCRIPTS_REGISTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_CREATURESCRIPTS_REGISTRY_DEPTH: usize = 32;
const MAX_CREATURESCRIPT_ENTRIES: usize = 65_536;
const MAX_CREATURESCRIPT_EVENT_TYPE_BYTES: usize = 64;
const MAX_CREATURESCRIPT_NAME_BYTES: usize = 64;
const MAX_CREATURESCRIPT_SCRIPT_PATH_BYTES: usize = 512;

/// One declared creature event. `event_type` is the legacy event label (`login`, `death`, ...);
/// `name` is the registration handle the runtime later uses to bind the event to a creature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsCreatureScriptEntry {
    pub event_type: String,
    pub name: String,
    pub script: PathBuf,
}

/// A bounded creature-script catalog keyed by unique registration name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TfsCreatureScriptRegistry {
    entries: BTreeMap<String, TfsCreatureScriptEntry>,
}

impl TfsCreatureScriptRegistry {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&TfsCreatureScriptEntry> {
        self.entries.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TfsCreatureScriptEntry> {
        self.entries.values()
    }

    fn insert(&mut self, entry: TfsCreatureScriptEntry) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_CREATURESCRIPT_ENTRIES {
            return Err(invalid(
                "TFS creaturescript registry exceeds the configured entry limit",
            ));
        }
        if self.entries.insert(entry.name.clone(), entry).is_some() {
            return Err(invalid("duplicate TFS creaturescript name"));
        }
        Ok(())
    }

    /// Position of one entry in name-sorted iteration order, or `None` when absent.
    /// Feeds [`creature_callback_name`]; deterministic for a given registry.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.entries.keys().position(|key| key == name)
    }
}

/// Deterministic dispatcher name for one creature entry: `creature:{index}` with the
/// entry position in name-sorted registry order. Positional (never the raw operator
/// name) so arbitrary names can never collide or breach callback bounds; both the
/// CLI verb and the host router derive it from the same registry, so the names
/// always agree.
pub fn creature_callback_name(index: usize) -> String {
    format!("creature:{index}")
}

/// All entries of one event type in registry (name-sorted) order with indices.
/// Login-style events run every match; the live router unions their effects.
pub fn resolve_creature_entries<'a>(
    registry: &'a TfsCreatureScriptRegistry,
    event_type: &str,
) -> Vec<(usize, &'a TfsCreatureScriptEntry)> {
    registry
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.event_type == event_type)
        .collect()
}

/// One entry by registration name with its dispatcher callback plus script path,
/// for the CLI verb and single-name host lookups. `None` for unknown names.
pub fn resolve_creature_callback(
    registry: &TfsCreatureScriptRegistry,
    name: &str,
) -> Option<(String, PathBuf)> {
    let index = registry.index_of(name)?;
    let entry = registry.get(name)?;
    Some((creature_callback_name(index), entry.script.clone()))
}

/// Loads the optional TFS creaturescript registry. A missing file intentionally yields an empty
/// catalog so worlds without creature events keep the existing no-op behavior.
pub fn load_tfs_creaturescript_registry(
    config: &EngineConfig,
) -> Result<TfsCreatureScriptRegistry, ConfigError> {
    let path = config
        .content_directory
        .join(CREATURESCRIPTS_REGISTRY_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(TfsCreatureScriptRegistry::default());
    }
    parse_tfs_creaturescripts_xml(&fs::read(path).map_err(ConfigError::Io)?)
}

pub fn parse_tfs_creaturescripts_xml(
    bytes: &[u8],
) -> Result<TfsCreatureScriptRegistry, ConfigError> {
    if bytes.len() > MAX_CREATURESCRIPTS_REGISTRY_BYTES {
        return Err(invalid(
            "TFS creaturescript registry exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut registry = TfsCreatureScriptRegistry::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_CREATURESCRIPTS_REGISTRY_DEPTH {
                    return Err(invalid(
                        "TFS creaturescript registry nesting exceeds the configured limit",
                    ));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"creaturescripts" {
                        return Err(invalid(
                            "TFS creaturescript registry has an invalid root element",
                        ));
                    }
                    root_seen = true;
                } else {
                    return Err(invalid("TFS creaturescript entries must be empty elements"));
                }
            }
            Event::Empty(event) => {
                if !root_seen || depth + 1 != 2 || event.name().as_ref() != b"event" {
                    return Err(invalid("TFS creaturescript entry is malformed"));
                }
                registry.insert(parse_creaturescript_entry(&event)?)?;
            }
            Event::End(event) => {
                if event.name().as_ref() != b"creaturescripts" {
                    return Err(invalid(
                        "TFS creaturescript registry closing tag is invalid",
                    ));
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("TFS creaturescript registry has unbalanced tags"))?;
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            Event::Text(text) if text.as_ref().iter().all(u8::is_ascii_whitespace) => {}
            _ => return Err(invalid("unsupported TFS creaturescript registry XML node")),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid(
            "TFS creaturescript registry is missing a complete root",
        ));
    }
    Ok(registry)
}

fn parse_creaturescript_entry(
    event: &BytesStart<'_>,
) -> Result<TfsCreatureScriptEntry, ConfigError> {
    let mut event_type = None;
    let mut name = None;
    let mut script = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| invalid(format!("invalid TFS creaturescript attribute: {error}")))?;
        let value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| {
                invalid(format!(
                    "invalid TFS creaturescript attribute value: {error}"
                ))
            })?
            .into_owned();
        match attribute.key.as_ref() {
            b"type" => {
                if event_type.is_some() {
                    return Err(invalid("duplicate TFS creaturescript type attribute"));
                }
                event_type = Some(value);
            }
            b"name" => {
                if name.is_some() {
                    return Err(invalid("duplicate TFS creaturescript name attribute"));
                }
                name = Some(value);
            }
            b"script" => {
                if script.is_some() {
                    return Err(invalid("duplicate TFS creaturescript script attribute"));
                }
                script = Some(value);
            }
            _ => {}
        }
    }
    let event_type =
        event_type.ok_or_else(|| invalid("TFS creaturescript is missing its type attribute"))?;
    if event_type.is_empty()
        || event_type.len() > MAX_CREATURESCRIPT_EVENT_TYPE_BYTES
        || event_type.trim() != event_type
        || event_type.chars().any(char::is_control)
    {
        return Err(invalid(
            "TFS creaturescript type is outside the configured bounds",
        ));
    }
    let name = name.ok_or_else(|| invalid("TFS creaturescript is missing its name attribute"))?;
    if name.is_empty()
        || name.len() > MAX_CREATURESCRIPT_NAME_BYTES
        || name.trim() != name
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            "TFS creaturescript name is outside the configured bounds",
        ));
    }
    let script =
        script.ok_or_else(|| invalid("TFS creaturescript is missing its script attribute"))?;
    let script = validate_script_path(&script)?;
    Ok(TfsCreatureScriptEntry {
        event_type,
        name,
        script,
    })
}

fn validate_script_path(raw: &str) -> Result<PathBuf, ConfigError> {
    if raw.is_empty() || raw.len() > MAX_CREATURESCRIPT_SCRIPT_PATH_BYTES {
        return Err(invalid(
            "TFS creaturescript script path is outside the configured bounds",
        ));
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid("TFS creaturescript script path is unsafe"));
    }
    Ok(path.to_path_buf())
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid TFS creaturescript registry XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_creaturescript_entries() {
        let registry = parse_tfs_creaturescripts_xml(
            br#"<creaturescripts>
                <event type="login" name="PlayerLogin" script="login.lua"/>
                <event type="death" name="PlayerDeath" script="death.lua"/>
            </creaturescripts>"#,
        )
        .unwrap();
        assert_eq!(registry.len(), 2);
        let login = registry.get("PlayerLogin").unwrap();
        assert_eq!(login.event_type, "login");
        assert_eq!(login.script, PathBuf::from("login.lua"));
        assert_eq!(registry.get("PlayerDeath").unwrap().event_type, "death");
        assert_eq!(registry.get("missing"), None);
    }

    #[test]
    fn rejects_duplicate_missing_and_unsafe_creaturescripts() {
        assert!(parse_tfs_creaturescripts_xml(
            br#"<creaturescripts><event type="login" name="A" script="a.lua"/><event type="logout" name="A" script="b.lua"/></creaturescripts>"#,
        )
        .is_err());
        assert!(parse_tfs_creaturescripts_xml(
            br#"<creaturescripts><event type="login" script="a.lua"/></creaturescripts>"#,
        )
        .is_err());
        assert!(parse_tfs_creaturescripts_xml(
            br#"<creaturescripts><event type="  login" name="A" script="a.lua"/></creaturescripts>"#,
        )
        .is_err());
        assert!(parse_tfs_creaturescripts_xml(
            br#"<creaturescripts><event type="StepIn" fromid="9" toid="2" script="a.lua"/></movements>"#,
        )
        .is_err());
    }

    #[test]
    fn resolution_filters_by_type_and_names_callbacks_positionally() {
        let registry = parse_tfs_creaturescripts_xml(
            br#"<creaturescripts>
                <event type="login" name="SecondLogin" script="second.lua"/>
                <event type="death" name="PlayerDeath" script="death.lua"/>
                <event type="login" name="FirstLogin" script="first.lua"/>
            </creaturescripts>"#,
        )
        .unwrap();
        // Name-sorted order: FirstLogin (0), PlayerDeath (1), SecondLogin (2).
        assert_eq!(registry.index_of("FirstLogin"), Some(0));
        assert_eq!(registry.index_of("Nope"), None);
        let logins = resolve_creature_entries(&registry, "login");
        assert_eq!(logins.len(), 2);
        assert_eq!(logins[0].1.name, "FirstLogin");
        assert_eq!(logins[1].1.name, "SecondLogin");
        assert_eq!(
            resolve_creature_callback(&registry, "SecondLogin"),
            Some(("creature:2".to_owned(), PathBuf::from("second.lua")))
        );
        assert_eq!(resolve_creature_callback(&registry, "Nope"), None);
        assert_eq!(resolve_creature_entries(&registry, "advance").len(), 0);
        assert_eq!(creature_callback_name(5), "creature:5");
    }
}
