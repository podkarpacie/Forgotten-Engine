//! Bounded, non-executing parser for the operator-owned TFS `movements/movements.xml` registry.
//! It retains only the movement `type`, an item id or range selector, an optional equip `slot`,
//! and a validated `script` path per entry. It never reads or executes Lua; leg movement
//! routing, equip slot semantics, and legacy `event`/`function` handlers stay a deferred boundary.

use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MOVEMENTS_REGISTRY_RELATIVE_PATH: &str = "movements/movements.xml";
const MAX_MOVEMENTS_REGISTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_MOVEMENTS_REGISTRY_DEPTH: usize = 32;
const MAX_MOVEMENT_ENTRIES: usize = 65_536;
const MAX_MOVEMENT_SCRIPT_PATH_BYTES: usize = 512;
const MAX_MOVEMENT_SLOT_BYTES: usize = 32;

/// The declared movement trigger kind. Matching is case-sensitive against the legacy registry;
/// any unknown `type` is rejected rather than silently deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TfsMoveEventType {
    StepIn,
    StepOut,
    Equip,
    DeEquip,
    AddItem,
    RemoveItem,
}

impl TfsMoveEventType {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "StepIn" => Some(Self::StepIn),
            "StepOut" => Some(Self::StepOut),
            "Equip" => Some(Self::Equip),
            "DeEquip" => Some(Self::DeEquip),
            "AddItem" => Some(Self::AddItem),
            "RemoveItem" => Some(Self::RemoveItem),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::StepIn => "StepIn",
            Self::StepOut => "StepOut",
            Self::Equip => "Equip",
            Self::DeEquip => "DeEquip",
            Self::AddItem => "AddItem",
            Self::RemoveItem => "RemoveItem",
        }
    }

    const fn requires_slot(self) -> bool {
        matches!(self, Self::Equip | Self::DeEquip)
    }
}

impl fmt::Display for TfsMoveEventType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// The item selector of one movement entry: either a single id or an inclusive id range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TfsMoveItemSelector {
    item_id: Option<u16>,
    range: Option<(u16, u16)>,
}

/// One declared movement. `script` is a safe relative path into the operator content tree;
/// movement routing and slot compatibility are not represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsMoveEventEntry {
    pub movement_type: TfsMoveEventType,
    pub item_id: Option<u16>,
    pub item_range: Option<(u16, u16)>,
    pub slot: Option<String>,
    pub script: PathBuf,
}

/// A bounded movement catalog kept in document order so overlapping ranges can later resolve
/// first-match precedence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TfsMoveEventRegistry {
    entries: Vec<TfsMoveEventEntry>,
}

impl TfsMoveEventRegistry {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TfsMoveEventEntry> {
        self.entries.iter()
    }

    fn insert(&mut self, entry: TfsMoveEventEntry) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_MOVEMENT_ENTRIES {
            return Err(invalid(
                "TFS movement registry exceeds the configured entry limit",
            ));
        }
        self.entries.push(entry);
        Ok(())
    }
}

/// Loads the optional TFS movement registry. A missing file intentionally yields an empty catalog.
pub fn load_tfs_movement_registry(
    config: &EngineConfig,
) -> Result<TfsMoveEventRegistry, ConfigError> {
    let path = config
        .content_directory
        .join(MOVEMENTS_REGISTRY_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(TfsMoveEventRegistry::default());
    }
    parse_tfs_movements_xml(&fs::read(path).map_err(ConfigError::Io)?)
}

pub fn parse_tfs_movements_xml(bytes: &[u8]) -> Result<TfsMoveEventRegistry, ConfigError> {
    if bytes.len() > MAX_MOVEMENTS_REGISTRY_BYTES {
        return Err(invalid(
            "TFS movement registry exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut registry = TfsMoveEventRegistry::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_MOVEMENTS_REGISTRY_DEPTH {
                    return Err(invalid(
                        "TFS movement registry nesting exceeds the configured limit",
                    ));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"movements" {
                        return Err(invalid("TFS movement registry has an invalid root element"));
                    }
                    root_seen = true;
                } else {
                    return Err(invalid("TFS movement entries must be empty elements"));
                }
            }
            Event::Empty(event) => {
                if !root_seen || depth + 1 != 2 || event.name().as_ref() != b"moveevent" {
                    return Err(invalid("TFS movement entry is malformed"));
                }
                registry.insert(parse_movement_entry(&event)?)?;
            }
            Event::End(event) => {
                if event.name().as_ref() != b"movements" {
                    return Err(invalid("TFS movement registry closing tag is invalid"));
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("TFS movement registry has unbalanced tags"))?;
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            Event::Text(text) if text.as_ref().iter().all(u8::is_ascii_whitespace) => {}
            _ => return Err(invalid("unsupported TFS movement registry XML node")),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid("TFS movement registry is missing a complete root"));
    }
    Ok(registry)
}

fn parse_movement_entry(event: &BytesStart<'_>) -> Result<TfsMoveEventEntry, ConfigError> {
    let mut movement_type = None;
    let mut item_id = None;
    let mut item_from = None;
    let mut item_to = None;
    let mut slot = None;
    let mut script = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| invalid(format!("invalid TFS movement attribute: {error}")))?;
        let value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| invalid(format!("invalid TFS movement attribute value: {error}")))?
            .into_owned();
        match attribute.key.as_ref() {
            b"type" => {
                if movement_type.is_some() {
                    return Err(invalid("duplicate TFS movement type attribute"));
                }
                movement_type = Some(value);
            }
            b"itemid" => set_unique_u16(&mut item_id, &value, "duplicate TFS movement itemid")?,
            b"fromid" => set_unique_u16(&mut item_from, &value, "duplicate TFS movement fromid")?,
            b"toid" => set_unique_u16(&mut item_to, &value, "duplicate TFS movement toid")?,
            b"slot" => {
                if slot.is_some() {
                    return Err(invalid("duplicate TFS movement slot attribute"));
                }
                slot = Some(value);
            }
            b"script" => {
                if script.is_some() {
                    return Err(invalid("duplicate TFS movement script attribute"));
                }
                script = Some(value);
            }
            // `event` and `function` are legacy handler names; runtime dispatch is deferred.
            _ => {}
        }
    }
    let movement_type =
        movement_type.ok_or_else(|| invalid("TFS movement is missing its type attribute"))?;
    let movement_type = TfsMoveEventType::parse(&movement_type)
        .ok_or_else(|| invalid("TFS movement has an unsupported type"))?;
    let TfsMoveItemSelector { item_id, range } =
        resolve_item_selector(item_id, item_from, item_to)?;
    let slot = match slot {
        Some(slot)
            if slot.is_empty()
                || slot.len() > MAX_MOVEMENT_SLOT_BYTES
                || slot.chars().any(char::is_control) =>
        {
            return Err(invalid(
                "TFS movement slot is outside the configured bounds",
            ));
        }
        Some(slot) => Some(slot),
        None if movement_type.requires_slot() => {
            return Err(invalid(
                "TFS equip movement is missing its required slot attribute",
            ));
        }
        None => None,
    };
    let script = script.ok_or_else(|| invalid("TFS movement is missing its script attribute"))?;
    let script = validate_script_path(&script)?;
    Ok(TfsMoveEventEntry {
        movement_type,
        item_id,
        item_range: range,
        slot,
        script,
    })
}

fn resolve_item_selector(
    item_id: Option<u16>,
    item_from: Option<u16>,
    item_to: Option<u16>,
) -> Result<TfsMoveItemSelector, ConfigError> {
    let single = item_id.is_some();
    let range = item_from.is_some() || item_to.is_some();
    if single && range {
        return Err(invalid("TFS movement item selector is ambiguous"));
    }
    if single {
        return Ok(TfsMoveItemSelector {
            item_id,
            range: None,
        });
    }
    if range {
        let (from, to) = (item_from.unwrap_or(0), item_to.unwrap_or(u16::MAX));
        if from > to {
            return Err(invalid("TFS movement item range is reversed"));
        }
        return Ok(TfsMoveItemSelector {
            item_id: None,
            range: Some((from, to)),
        });
    }
    Err(invalid("TFS movement is missing its item selector"))
}

fn set_unique_u16(
    slot: &mut Option<u16>,
    value: &str,
    duplicate_message: &str,
) -> Result<(), ConfigError> {
    let parsed = value
        .parse::<u16>()
        .map_err(|_| invalid("TFS movement id attribute is not a valid integer"))?;
    if slot.replace(parsed).is_some() {
        return Err(invalid(duplicate_message));
    }
    Ok(())
}

fn validate_script_path(raw: &str) -> Result<PathBuf, ConfigError> {
    if raw.is_empty() || raw.len() > MAX_MOVEMENT_SCRIPT_PATH_BYTES {
        return Err(invalid(
            "TFS movement script path is outside the configured bounds",
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
        return Err(invalid("TFS movement script path is unsafe"));
    }
    Ok(path.to_path_buf())
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid TFS movement registry XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_movement_entries() {
        let registry = parse_tfs_movements_xml(
            br#"<movements>
                <moveevent type="StepIn" itemid="2160" script="hole.lua"/>
                <moveevent type="StepOut" fromid="100" toid="120" script="pit.lua"/>
                <moveevent type="Equip" itemid="2500" slot="ring" script="ring.lua"/>
            </movements>"#,
        )
        .unwrap();
        assert_eq!(registry.len(), 3);
        let entries: Vec<&TfsMoveEventEntry> = registry.iter().collect();
        assert_eq!(entries[0].movement_type, TfsMoveEventType::StepIn);
        assert_eq!(entries[0].item_id, Some(2160));
        assert_eq!(entries[1].movement_type, TfsMoveEventType::StepOut);
        assert_eq!(entries[1].item_range, Some((100, 120)));
        assert_eq!(entries[2].movement_type, TfsMoveEventType::Equip);
        assert_eq!(entries[2].slot.as_deref(), Some("ring"));
    }

    #[test]
    fn rejects_missing_type_item_and_slot_movements() {
        assert!(parse_tfs_movements_xml(
            br#"<movements><moveevent itemid="1" script="a.lua"/></movements>"#,
        )
        .is_err());
        assert!(parse_tfs_movements_xml(
            br#"<movements><moveevent type="StepIn" script="a.lua"/></movements>"#,
        )
        .is_err());
        assert!(parse_tfs_movements_xml(
            br#"<movements><moveevent type="Equip" itemid="1" script="a.lua"/></movements>"#,
        )
        .is_err());
        assert!(parse_tfs_movements_xml(
            br#"<movements><moveevent type="Teleport" itemid="1" script="a.lua"/></movements>"#,
        )
        .is_err());
        assert!(parse_tfs_movements_xml(
            br#"<movements><moveevent type="StepIn" fromid="9" toid="2" script="a.lua"/></movements>"#,
        )
        .is_err());
    }
}
