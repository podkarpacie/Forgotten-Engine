use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::fs;

const QUEST_CATALOG_RELATIVE_PATH: &str = "XML/forgotten-engine-quests.xml";
const MAX_QUEST_CATALOG_BYTES: usize = 128 * 1024;
const MAX_QUEST_NAME_BYTES: usize = 64;
const MAX_QUESTS: usize = 256;

/// One bounded operator-declared quest identity: a stable numeric ID clients understand plus a
/// display name. Mission lines, storage flags, and reward logic remain outside this adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestDefinition {
    pub quest_id: u16,
    pub name: String,
}

/// Validated quest catalog keyed by numeric quest ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestCatalog {
    quests: BTreeMap<u16, String>,
}

impl QuestCatalog {
    pub fn get(&self, quest_id: u16) -> Option<&str> {
        self.quests.get(&quest_id).map(|name| name.as_str())
    }

    /// Returns the catalog entries sorted by quest ID.
    pub fn iter(&self) -> impl Iterator<Item = (u16, &str)> + '_ {
        self.quests.iter().map(|(&id, name)| (id, name.as_str()))
    }

    pub fn len(&self) -> usize {
        self.quests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quests.is_empty()
    }

    fn insert(&mut self, quest_id: u16, name: String) -> Result<(), ConfigError> {
        if self.quests.insert(quest_id, name).is_some() {
            return Err(invalid("duplicate quest declaration"));
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid quest XML: {error}"))
}

/// Loads the optional operator quest catalog from
/// `data/XML/forgotten-engine-quests.xml`. A missing file yields an empty catalog.
pub fn load_quest_catalog(config: &EngineConfig) -> Result<QuestCatalog, ConfigError> {
    let path = config.content_directory.join(QUEST_CATALOG_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(QuestCatalog::default());
    }
    parse_quests_xml(&fs::read(path).map_err(ConfigError::Io)?)
}

pub fn parse_quests_xml(bytes: &[u8]) -> Result<QuestCatalog, ConfigError> {
    if bytes.len() > MAX_QUEST_CATALOG_BYTES {
        return Err(invalid("quest catalog exceeds the configured size limit"));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut catalog = QuestCatalog::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > 2 {
                    return Err(invalid("quest XML nesting exceeds the configured limit"));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"fe-quests" {
                        return Err(invalid("quest root element is invalid"));
                    }
                    root_seen = true;
                } else {
                    return Err(invalid("unexpected quest XML element"));
                }
            }
            Event::Empty(event) => {
                if depth != 1 || event.name().as_ref() != b"fe-quest" {
                    return Err(invalid("unexpected quest XML element"));
                }
                if catalog.len() >= MAX_QUESTS {
                    return Err(invalid("quest catalog exceeds the supported bound"));
                }
                let (quest_id, name) = parse_quest(&event)?;
                catalog.insert(quest_id, name)?;
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(invalid("unexpected closing XML element"));
                }
                if depth == 1 && event.name().as_ref() != b"fe-quests" {
                    return Err(invalid("quest root closing tag is invalid"));
                }
                depth -= 1;
            }
            Event::Text(text) if !text.as_ref().iter().all(u8::is_ascii_whitespace) => {
                return Err(invalid("quest catalog cannot contain text nodes"));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return Err(invalid("unsupported quest XML node")),
        }
        buffer.clear();
    }
    if depth != 0 || !root_seen {
        return Err(invalid("quest catalog is missing a complete root"));
    }
    Ok(catalog)
}

fn parse_quest(event: &BytesStart<'_>) -> Result<(u16, String), ConfigError> {
    let mut known = [false; 2];
    let mut quest_id = None;
    let mut name: Option<String> = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid quest attribute: {error}")))?;
        match attribute.key.as_ref() {
            b"id" => {
                let value = attribute
                    .unescape_value()
                    .map_err(|error| invalid(format!("invalid quest id: {error}")))?;
                quest_id = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| invalid("quest id must fit an unsigned 16-bit integer"))?,
                );
                known[0] = true;
            }
            b"name" => {
                let value = attribute
                    .unescape_value()
                    .map_err(|error| invalid(format!("invalid quest name: {error}")))?;
                let trimmed = value.trim();
                if trimmed.is_empty() || trimmed.len() > MAX_QUEST_NAME_BYTES {
                    return Err(invalid("quest name must stay within the bounded length"));
                }
                name = Some(trimmed.to_owned());
                known[1] = true;
            }
            _ => return Err(invalid("unsupported quest attribute")),
        }
    }
    match (known[0] && known[1], quest_id, name) {
        (true, Some(id), Some(name)) if id != 0 => Ok((id, name)),
        _ => Err(invalid(
            "quest entries need a nonzero id and a bounded name",
        )),
    }
}
