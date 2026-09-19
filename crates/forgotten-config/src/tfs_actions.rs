//! Bounded, non-executing parser for the operator-owned TFS `actions/actions.xml` registry.
//! It retains only the item/action/unique key that selects an action plus a validated `script`
//! path per entry. It never reads or executes Lua, and routing, event attribution (`fromid` range
//! precedence), and legacy `event`/`allowfaruse` semantics stay an explicit runtime boundary.

use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::fs;
use std::path::{Component, Path, PathBuf};

const ACTIONS_REGISTRY_RELATIVE_PATH: &str = "actions/actions.xml";
const MAX_ACTIONS_REGISTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_ACTIONS_REGISTRY_DEPTH: usize = 32;
const MAX_ACTION_ENTRIES: usize = 65_536;
const MAX_ACTION_SCRIPT_PATH_BYTES: usize = 512;

/// The declaring selector of one action entry. An action is keyed by exactly one of an item id or
/// range, an action id or range, or a unique id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TfsActionKey {
    ItemId(u16),
    ItemIdRange { from: u16, to: u16 },
    ActionId(u16),
    ActionIdRange { from: u16, to: u16 },
    UniqueId(u16),
}

/// One declared action. `script` is a safe relative path into the operator content tree; matching
/// precedence, ranges, use flags, and authorization are not represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsActionEntry {
    pub key: TfsActionKey,
    pub script: PathBuf,
}

/// A bounded action catalog. Entries are kept in document order because legacy ranges may overlap
/// and the runtime resolves first-match precedence; duplicate single ids are rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TfsActionRegistry {
    entries: Vec<TfsActionEntry>,
}

impl TfsActionRegistry {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TfsActionEntry> {
        self.entries.iter()
    }

    fn insert(&mut self, entry: TfsActionEntry) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_ACTION_ENTRIES {
            return Err(invalid(
                "TFS action registry exceeds the configured entry limit",
            ));
        }
        self.entries.push(entry);
        Ok(())
    }
}

/// Loads the optional TFS action registry. A missing file intentionally yields an empty catalog so
/// worlds without actions keep the existing no-op behavior.
pub fn load_tfs_action_registry(config: &EngineConfig) -> Result<TfsActionRegistry, ConfigError> {
    let path = config
        .content_directory
        .join(ACTIONS_REGISTRY_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(TfsActionRegistry::default());
    }
    parse_tfs_actions_xml(&fs::read(path).map_err(ConfigError::Io)?)
}

pub fn parse_tfs_actions_xml(bytes: &[u8]) -> Result<TfsActionRegistry, ConfigError> {
    if bytes.len() > MAX_ACTIONS_REGISTRY_BYTES {
        return Err(invalid(
            "TFS action registry exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut registry = TfsActionRegistry::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_ACTIONS_REGISTRY_DEPTH {
                    return Err(invalid(
                        "TFS action registry nesting exceeds the configured limit",
                    ));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"actions" {
                        return Err(invalid("TFS action registry has an invalid root element"));
                    }
                    root_seen = true;
                } else {
                    return Err(invalid("TFS action entries must be empty elements"));
                }
            }
            Event::Empty(event) => {
                if !root_seen || depth + 1 != 2 || event.name().as_ref() != b"action" {
                    return Err(invalid("TFS action entry is malformed"));
                }
                registry.insert(parse_action_entry(&event)?)?;
            }
            Event::End(event) => {
                if event.name().as_ref() != b"actions" {
                    return Err(invalid("TFS action registry closing tag is invalid"));
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("TFS action registry has unbalanced tags"))?;
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            Event::Text(text) if text.as_ref().iter().all(u8::is_ascii_whitespace) => {}
            _ => return Err(invalid("unsupported TFS action registry XML node")),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid("TFS action registry is missing a complete root"));
    }
    Ok(registry)
}

fn parse_action_entry(event: &BytesStart<'_>) -> Result<TfsActionEntry, ConfigError> {
    let mut item_id = None;
    let mut item_from = None;
    let mut item_to = None;
    let mut action_id = None;
    let mut action_from = None;
    let mut action_to = None;
    let mut unique_id = None;
    let mut script = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid TFS action attribute: {error}")))?;
        let value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| invalid(format!("invalid TFS action attribute value: {error}")))?
            .into_owned();
        match attribute.key.as_ref() {
            b"itemid" => set_unique_u16(&mut item_id, &value, "duplicate TFS action itemid")?,
            b"fromid" => set_unique_u16(&mut item_from, &value, "duplicate TFS action fromid")?,
            b"toid" => set_unique_u16(&mut item_to, &value, "duplicate TFS action toid")?,
            b"actionid" => set_unique_u16(&mut action_id, &value, "duplicate TFS action actionid")?,
            b"fromactionid" => set_unique_u16(
                &mut action_from,
                &value,
                "duplicate TFS action fromactionid",
            )?,
            b"toactionid" => {
                set_unique_u16(&mut action_to, &value, "duplicate TFS action toactionid")?
            }
            b"uniqueid" => set_unique_u16(&mut unique_id, &value, "duplicate TFS action uniqueid")?,
            b"script" => {
                if script.is_some() {
                    return Err(invalid("duplicate TFS action script attribute"));
                }
                script = Some(value);
            }
            // `event`, `allowfaruse`, `blockwalls`, `checkfloor`, `checklineofsight` are action
            // flags or legacy function names deferred to the routing boundary.
            _ => {}
        }
    }
    let key = resolve_action_key(
        item_id,
        item_from,
        item_to,
        action_id,
        action_from,
        action_to,
        unique_id,
    )?;
    let script = script.ok_or_else(|| invalid("TFS action is missing its script attribute"))?;
    let script = validate_script_path(&script)?;
    Ok(TfsActionEntry { key, script })
}

fn resolve_action_key(
    item_id: Option<u16>,
    item_from: Option<u16>,
    item_to: Option<u16>,
    action_id: Option<u16>,
    action_from: Option<u16>,
    action_to: Option<u16>,
    unique_id: Option<u16>,
) -> Result<TfsActionKey, ConfigError> {
    let item_single = item_id.is_some();
    let item_range = item_from.is_some() || item_to.is_some();
    let action_single = action_id.is_some();
    let action_range = action_from.is_some() || action_to.is_some();
    let unique = unique_id.is_some();
    let forms = item_single as u8
        + item_range as u8
        + action_single as u8
        + action_range as u8
        + unique as u8;
    if forms != 1 {
        return Err(invalid(
            "TFS action must key on exactly one of item id, action id, or unique id",
        ));
    }
    if item_single {
        return Ok(TfsActionKey::ItemId(item_id.unwrap()));
    }
    if item_range {
        let (from, to) = (item_from.unwrap_or(0), item_to.unwrap_or(u16::MAX));
        if from > to {
            return Err(invalid("TFS action item range is reversed"));
        }
        return Ok(TfsActionKey::ItemIdRange { from, to });
    }
    if action_single {
        return Ok(TfsActionKey::ActionId(action_id.unwrap()));
    }
    if action_range {
        let (from, to) = (action_from.unwrap_or(0), action_to.unwrap_or(u16::MAX));
        if from > to {
            return Err(invalid("TFS action action-id range is reversed"));
        }
        return Ok(TfsActionKey::ActionIdRange { from, to });
    }
    Ok(TfsActionKey::UniqueId(unique_id.ok_or_else(|| {
        invalid("TFS action is missing its unique id")
    })?))
}

fn set_unique_u16(
    slot: &mut Option<u16>,
    value: &str,
    duplicate_message: &str,
) -> Result<(), ConfigError> {
    let parsed = value
        .parse::<u16>()
        .map_err(|_| invalid("TFS action id attribute is not a valid integer"))?;
    if slot.replace(parsed).is_some() {
        return Err(invalid(duplicate_message));
    }
    Ok(())
}

fn validate_script_path(raw: &str) -> Result<PathBuf, ConfigError> {
    if raw.is_empty() || raw.len() > MAX_ACTION_SCRIPT_PATH_BYTES {
        return Err(invalid(
            "TFS action script path is outside the configured bounds",
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
        return Err(invalid("TFS action script path is unsafe"));
    }
    Ok(path.to_path_buf())
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid TFS action registry XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_action_entries() {
        let registry = parse_tfs_actions_xml(
            br#"<actions>
                <action itemid="2160" script="rope.lua"/>
                <action fromid="100" toid="200" script="range.lua"/>
                <action actionid="1000" script="lever.lua"/>
                <action uniqueid="7" script="door.lua"/>
            </actions>"#,
        )
        .unwrap();
        assert_eq!(registry.len(), 4);
        let entries: Vec<&TfsActionEntry> = registry.iter().collect();
        assert_eq!(entries[0].key, TfsActionKey::ItemId(2160));
        assert_eq!(entries[0].script, PathBuf::from("rope.lua"));
        assert_eq!(
            entries[1].key,
            TfsActionKey::ItemIdRange { from: 100, to: 200 }
        );
        assert_eq!(entries[2].key, TfsActionKey::ActionId(1000));
        assert_eq!(entries[3].key, TfsActionKey::UniqueId(7));
    }

    #[test]
    fn rejects_ambiguous_missing_and_unsafe_actions() {
        assert!(parse_tfs_actions_xml(
            br#"<actions><action itemid="1" actionid="2" script="a.lua"/></actions>"#,
        )
        .is_err());
        assert!(parse_tfs_actions_xml(br#"<actions><action script="a.lua"/></actions>"#).is_err());
        assert!(parse_tfs_actions_xml(
            br#"<actions><action itemid="1" script="../a.lua"/></actions>"#,
        )
        .is_err());
        assert!(parse_tfs_actions_xml(
            br#"<actions><action fromid="200" toid="100" script="a.lua"/></actions>"#,
        )
        .is_err());
        assert!(parse_tfs_actions_xml(
            br#"<actions><action itemid="notanumber" script="a.lua"/></actions>"#,
        )
        .is_err());
    }
}
