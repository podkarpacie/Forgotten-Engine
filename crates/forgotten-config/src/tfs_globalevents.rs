//! Bounded, non-executing parser for the operator-owned TFS `globalevents/globalevents.xml`
//! registry. It retains only a unique registration `name`, an optional timer `interval`, an
//! optional event `type`, and a validated `script` path per entry. It never reads or executes Lua;
//! global timer scheduling and server-lifecycle event delivery stay a deferred runtime boundary.

use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

const GLOBALEVENTS_REGISTRY_RELATIVE_PATH: &str = "globalevents/globalevents.xml";
const MAX_GLOBALEVENTS_REGISTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_GLOBALEVENTS_REGISTRY_DEPTH: usize = 32;
const MAX_GLOBALEVENT_ENTRIES: usize = 65_536;
const MAX_GLOBALEVENT_NAME_BYTES: usize = 64;
const MAX_GLOBALEVENT_TYPE_BYTES: usize = 64;
const MAX_GLOBALEVENT_SCRIPT_PATH_BYTES: usize = 512;

/// One declared global event. `interval` marks a periodic event (e.g. `record`/`save`); a
/// lifecycle `event_type` (e.g. `startup`/`shutdown`) has no interval and is delivered once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsGlobalEventEntry {
    pub name: String,
    pub interval: Option<u32>,
    pub event_type: Option<String>,
    pub script: PathBuf,
}

/// A bounded global-event catalog keyed by unique registration name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TfsGlobalEventRegistry {
    entries: BTreeMap<String, TfsGlobalEventEntry>,
}

impl TfsGlobalEventRegistry {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&TfsGlobalEventEntry> {
        self.entries.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TfsGlobalEventEntry> {
        self.entries.values()
    }

    fn insert(&mut self, entry: TfsGlobalEventEntry) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_GLOBALEVENT_ENTRIES {
            return Err(invalid(
                "TFS globalevent registry exceeds the configured entry limit",
            ));
        }
        if self.entries.insert(entry.name.clone(), entry).is_some() {
            return Err(invalid("duplicate TFS globalevent name"));
        }
        Ok(())
    }
}

/// Loads the optional TFS globalevent registry. A missing file intentionally yields an empty
/// catalog so worlds without global events keep the existing no-op behavior.
pub fn load_tfs_globalevent_registry(
    config: &EngineConfig,
) -> Result<TfsGlobalEventRegistry, ConfigError> {
    let path = config
        .content_directory
        .join(GLOBALEVENTS_REGISTRY_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(TfsGlobalEventRegistry::default());
    }
    parse_tfs_globalevents_xml(&fs::read(path).map_err(ConfigError::Io)?)
}

pub fn parse_tfs_globalevents_xml(bytes: &[u8]) -> Result<TfsGlobalEventRegistry, ConfigError> {
    if bytes.len() > MAX_GLOBALEVENTS_REGISTRY_BYTES {
        return Err(invalid(
            "TFS globalevent registry exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut registry = TfsGlobalEventRegistry::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_GLOBALEVENTS_REGISTRY_DEPTH {
                    return Err(invalid(
                        "TFS globalevent registry nesting exceeds the configured limit",
                    ));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"globalevents" {
                        return Err(invalid(
                            "TFS globalevent registry has an invalid root element",
                        ));
                    }
                    root_seen = true;
                } else {
                    return Err(invalid("TFS globalevent entries must be empty elements"));
                }
            }
            Event::Empty(event) => {
                if !root_seen || depth + 1 != 2 || event.name().as_ref() != b"globalevent" {
                    return Err(invalid("TFS globalevent entry is malformed"));
                }
                registry.insert(parse_globalevent_entry(&event)?)?;
            }
            Event::End(event) => {
                if event.name().as_ref() != b"globalevents" {
                    return Err(invalid("TFS globalevent registry closing tag is invalid"));
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("TFS globalevent registry has unbalanced tags"))?;
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            Event::Text(text) if text.as_ref().iter().all(u8::is_ascii_whitespace) => {}
            _ => return Err(invalid("unsupported TFS globalevent registry XML node")),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid(
            "TFS globalevent registry is missing a complete root",
        ));
    }
    Ok(registry)
}

fn parse_globalevent_entry(event: &BytesStart<'_>) -> Result<TfsGlobalEventEntry, ConfigError> {
    let mut name = None;
    let mut interval = None;
    let mut event_type = None;
    let mut script = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| invalid(format!("invalid TFS globalevent attribute: {error}")))?;
        let value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| invalid(format!("invalid TFS globalevent attribute value: {error}")))?
            .into_owned();
        match attribute.key.as_ref() {
            b"name" => {
                if name.is_some() {
                    return Err(invalid("duplicate TFS globalevent name attribute"));
                }
                name = Some(value);
            }
            b"interval" => {
                if interval.is_some() {
                    return Err(invalid("duplicate TFS globalevent interval attribute"));
                }
                interval = Some(value);
            }
            b"type" => {
                if event_type.is_some() {
                    return Err(invalid("duplicate TFS globalevent type attribute"));
                }
                event_type = Some(value);
            }
            b"script" => {
                if script.is_some() {
                    return Err(invalid("duplicate TFS globalevent script attribute"));
                }
                script = Some(value);
            }
            _ => {}
        }
    }
    let name = name.ok_or_else(|| invalid("TFS globalevent is missing its name attribute"))?;
    if name.is_empty()
        || name.len() > MAX_GLOBALEVENT_NAME_BYTES
        || name.trim() != name
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            "TFS globalevent name is outside the configured bounds",
        ));
    }
    let interval = match interval {
        Some(value) => Some(
            value
                .parse::<u32>()
                .map_err(|_| invalid("TFS globalevent interval is not a valid integer"))?,
        ),
        None => None,
    };
    let event_type = match event_type {
        Some(value)
            if value.is_empty()
                || value.len() > MAX_GLOBALEVENT_TYPE_BYTES
                || value.trim() != value
                || value.chars().any(char::is_control) =>
        {
            return Err(invalid(
                "TFS globalevent type is outside the configured bounds",
            ));
        }
        Some(value) => Some(value),
        None => None,
    };
    let script =
        script.ok_or_else(|| invalid("TFS globalevent is missing its script attribute"))?;
    let script = validate_script_path(&script)?;
    Ok(TfsGlobalEventEntry {
        name,
        interval,
        event_type,
        script,
    })
}

fn validate_script_path(raw: &str) -> Result<PathBuf, ConfigError> {
    if raw.is_empty() || raw.len() > MAX_GLOBALEVENT_SCRIPT_PATH_BYTES {
        return Err(invalid(
            "TFS globalevent script path is outside the configured bounds",
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
        return Err(invalid("TFS globalevent script path is unsafe"));
    }
    Ok(path.to_path_buf())
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid TFS globalevent registry XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_globalevent_entries() {
        let registry = parse_tfs_globalevents_xml(
            br#"<globalevents>
                <globalevent name="startup" type="startup" script="startup.lua"/>
                <globalevent name="record" interval="5" script="record.lua"/>
            </globalevents>"#,
        )
        .unwrap();
        assert_eq!(registry.len(), 2);
        let startup = registry.get("startup").unwrap();
        assert_eq!(startup.event_type.as_deref(), Some("startup"));
        assert_eq!(startup.interval, None);
        assert_eq!(startup.script, PathBuf::from("startup.lua"));
        let record = registry.get("record").unwrap();
        assert_eq!(record.interval, Some(5));
        assert_eq!(record.event_type, None);
        assert_eq!(registry.get("missing"), None);
    }

    #[test]
    fn rejects_duplicate_missing_and_unsafe_globalevents() {
        assert!(parse_tfs_globalevents_xml(
            br#"<globalevents><globalevent name="A" script="a.lua"/><globalevent name="A" script="b.lua"/></globalevents>"#,
        )
        .is_err());
        assert!(parse_tfs_globalevents_xml(
            br#"<globalevents><globalevent script="a.lua"/></globalevents>"#,
        )
        .is_err());
        assert!(parse_tfs_globalevents_xml(
            br#"<globalevents><globalevent name="A" interval="abc" script="a.lua"/></globalevents>"#,
        )
        .is_err());
        assert!(parse_tfs_globalevents_xml(
            br#"<globalevents><globalevent name="A" script="../a.lua"/></globalevents>"#,
        )
        .is_err());
    }
}
