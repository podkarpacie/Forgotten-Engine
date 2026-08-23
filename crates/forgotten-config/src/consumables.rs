use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::fs;

const CONSUMABLE_CATALOG_RELATIVE_PATH: &str = "XML/forgotten-engine-consumables.xml";

/// Loads the optional operator consumable catalog from
/// `data/XML/forgotten-engine-consumables.xml`. A missing file yields no consumable effects.
pub fn load_consumable_catalog(
    config: &EngineConfig,
) -> Result<Option<ConsumableCatalog>, ConfigError> {
    let path = config
        .content_directory
        .join(CONSUMABLE_CATALOG_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    parse_consumables_xml(&fs::read(path).map_err(ConfigError::Io)?).map(Some)
}

const MAX_CONSUMABLE_CATALOG_BYTES: usize = 64 * 1024;
const MAX_CONSUMABLE_CATALOG_DEPTH: usize = 4;
const MAX_CONSUMABLE_AMOUNT: u16 = 500;

/// One bounded operator-declared consumable effect keyed by authoritative server item ID.
/// Health and mana restore instantly on UseItem from owned inventory; regeneration food,
/// condition cures, and charge semantics remain outside this first adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumableEffect {
    pub health: u16,
    pub mana: u16,
}

/// Validated consumable catalog. It is immutable runtime input; nothing here executes scripts or
/// claims TFS action.lua compatibility.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsumableCatalog {
    effects: BTreeMap<u16, ConsumableEffect>,
}

impl ConsumableCatalog {
    pub fn get(&self, server_id: u16) -> Option<ConsumableEffect> {
        self.effects.get(&server_id).copied()
    }

    pub fn len(&self) -> usize {
        self.effects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, ConsumableEffect)> + '_ {
        self.effects.iter().map(|(&id, &effect)| (id, effect))
    }

    fn insert(&mut self, server_id: u16, effect: ConsumableEffect) -> Result<(), ConfigError> {
        if self.effects.insert(server_id, effect).is_some() {
            return Err(invalid("duplicate consumable item declaration"));
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid consumable XML: {error}"))
}

pub fn parse_consumables_xml(bytes: &[u8]) -> Result<ConsumableCatalog, ConfigError> {
    if bytes.len() > MAX_CONSUMABLE_CATALOG_BYTES {
        return Err(invalid(
            "consumable catalog exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut catalog = ConsumableCatalog::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_CONSUMABLE_CATALOG_DEPTH {
                    return Err(invalid(
                        "consumable XML nesting exceeds the configured limit",
                    ));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"fe-consumables" {
                        return Err(invalid("consumable root element is invalid"));
                    }
                    root_seen = true;
                } else {
                    return Err(invalid("consumable entries must be empty XML elements"));
                }
            }
            Event::Empty(event) => {
                if depth != 1 || event.name().as_ref() != b"fe-consumable" {
                    return Err(invalid("unexpected consumable XML element"));
                }
                let (server_id, effect) = parse_consumable(&event)?;
                catalog.insert(server_id, effect)?;
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(invalid("unexpected closing XML element"));
                }
                if depth == 1 && event.name().as_ref() != b"fe-consumables" {
                    return Err(invalid("consumable root closing tag is invalid"));
                }
                depth -= 1;
            }
            Event::Text(text) if !text.as_ref().iter().all(u8::is_ascii_whitespace) => {
                return Err(invalid("consumable catalog cannot contain text nodes"));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return Err(invalid("unsupported consumable XML node")),
        }
        buffer.clear();
    }
    if depth != 0 || !root_seen {
        return Err(invalid("consumable catalog is missing a complete root"));
    }
    Ok(catalog)
}

fn optional_u16(event: &BytesStart<'_>, key: &[u8]) -> Result<Option<u16>, ConfigError> {
    event
        .attributes()
        .with_checks(false)
        .find_map(|attribute| {
            let attribute = attribute.ok()?;
            (attribute.key.as_ref() == key).then_some(attribute)
        })
        .map(|attribute| {
            let value = attribute
                .unescape_value()
                .map_err(|error| invalid(format!("invalid consumable attribute: {error}")))?;
            value
                .parse::<u16>()
                .map_err(|_| invalid("consumable amount must fit an unsigned 16-bit integer"))
        })
        .transpose()
}

fn parse_consumable(event: &BytesStart<'_>) -> Result<(u16, ConsumableEffect), ConfigError> {
    let mut known = [false; 2];
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid consumable attribute: {error}")))?;
        match attribute.key.as_ref() {
            b"id" | b"health" | b"mana" => {}
            _ => return Err(invalid("unsupported consumable attribute")),
        }
    }
    let id_attr = event
        .attributes()
        .with_checks(false)
        .find_map(|attribute| {
            let attribute = attribute.ok()?;
            (attribute.key.as_ref() == b"id").then_some(attribute)
        })
        .ok_or_else(|| invalid("consumable entry is missing its id attribute"))?;
    let value = id_attr
        .unescape_value()
        .map_err(|error| invalid(format!("invalid consumable id: {error}")))?;
    let server_id = value
        .parse::<u16>()
        .map_err(|_| invalid("consumable id must fit an unsigned 16-bit integer"))?;
    if server_id == 0 {
        return Err(invalid("consumable id must be nonzero"));
    }
    let health = optional_u16(event, b"health")?.unwrap_or(0);
    let mana = optional_u16(event, b"mana")?.unwrap_or(0);
    known[0] = health > 0 || mana > 0;
    if !known[0] {
        return Err(invalid(
            "consumable must declare a positive health or mana restore",
        ));
    }
    if health > MAX_CONSUMABLE_AMOUNT || mana > MAX_CONSUMABLE_AMOUNT {
        return Err(invalid(
            "consumable restore amounts exceed the configured bound",
        ));
    }
    Ok((server_id, ConsumableEffect { health, mana }))
}
