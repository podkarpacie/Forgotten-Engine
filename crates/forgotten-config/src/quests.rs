use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::fs;

const QUEST_CATALOG_RELATIVE_PATH: &str = "XML/forgotten-engine-quests.xml";
const MAX_QUEST_CATALOG_BYTES: usize = 128 * 1024;
const MAX_QUEST_NAME_BYTES: usize = 64;
const MAX_QUESTS: usize = 256;
const MAX_QUEST_MISSIONS: usize = 16;

/// One bounded operator-declared quest identity plus optional mission lines and item rewards
/// granted into the starter backpack on the completion transition (plan v49 slice 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestDefinition {
    pub quest_id: u16,
    pub name: String,
    pub missions: Vec<(String, String)>,
    pub rewards: Vec<(u16, u16)>,
}

/// Validated quest catalog keyed by numeric quest ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestCatalog {
    quests: BTreeMap<u16, QuestDefinition>,
}

impl QuestCatalog {
    pub fn get(&self, quest_id: u16) -> Option<&QuestDefinition> {
        self.quests.get(&quest_id)
    }

    /// Returns the catalog entries sorted by quest ID.
    pub fn iter(&self) -> impl Iterator<Item = (u16, &str)> + '_ {
        self.quests
            .iter()
            .map(|(&id, quest)| (id, quest.name.as_str()))
    }

    pub fn len(&self) -> usize {
        self.quests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quests.is_empty()
    }

    fn insert(&mut self, definition: QuestDefinition) -> Result<(), ConfigError> {
        if self
            .quests
            .insert(definition.quest_id, definition)
            .is_some()
        {
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
    let mut active_quest: Option<QuestDefinition> = None;
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > 3 {
                    return Err(invalid("quest XML nesting exceeds the configured limit"));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"fe-quests" {
                        return Err(invalid("quest root element is invalid"));
                    }
                    root_seen = true;
                } else if depth == 2 && event.name().as_ref() == b"fe-quest" {
                    if catalog.len() >= MAX_QUESTS {
                        return Err(invalid("quest catalog exceeds the supported bound"));
                    }
                    let (quest_id, name) = parse_quest(&event)?;
                    active_quest = Some(QuestDefinition {
                        quest_id,
                        name,
                        missions: Vec::new(),
                        rewards: Vec::new(),
                    });
                } else if !(depth == 3
                    && (event.name().as_ref() == b"fe-mission"
                        || event.name().as_ref() == b"fe-reward"))
                {
                    return Err(invalid("unexpected quest XML element"));
                }
            }
            Event::Empty(event) => {
                if depth == 2 && event.name().as_ref() == b"fe-mission" && active_quest.is_some() {
                    let Some(quest) = active_quest.as_mut() else {
                        return Err(invalid("mission outside a quest element"));
                    };
                    if quest.missions.len() >= MAX_QUEST_MISSIONS {
                        return Err(invalid("quest missions exceed the supported bound"));
                    }
                    quest.missions.push(parse_mission(&event)?);
                } else if depth == 2
                    && event.name().as_ref() == b"fe-reward"
                    && active_quest.is_some()
                {
                    let Some(quest) = active_quest.as_mut() else {
                        return Err(invalid("reward outside a quest element"));
                    };
                    quest.rewards.push(parse_reward(&event)?);
                } else if depth == 1 && event.name().as_ref() == b"fe-quest" {
                    let (quest_id, name) = parse_quest(&event)?;
                    catalog.insert(QuestDefinition {
                        quest_id,
                        name,
                        missions: Vec::new(),
                        rewards: Vec::new(),
                    })?;
                } else {
                    return Err(invalid("unexpected quest XML element"));
                }
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(invalid("unexpected closing XML element"));
                }
                if depth == 2 && event.name().as_ref() == b"fe-quest" {
                    let Some(definition) = active_quest.take() else {
                        return Err(invalid("quest closing tag without an opening element"));
                    };
                    catalog.insert(definition)?;
                } else if depth == 1 && event.name().as_ref() != b"fe-quests" {
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

/// One bounded quest reward: a nonzero item id and count within the classic stack bound.
fn parse_reward(event: &BytesStart<'_>) -> Result<(u16, u16), ConfigError> {
    let item_id = optional_u16_attr(event, b"itemid")?.unwrap_or(0);
    let count = optional_u16_attr(event, b"count")?.unwrap_or(1);
    if item_id == 0 || count == 0 {
        return Err(invalid("quest rewards need a nonzero itemid and count"));
    }
    Ok((item_id, count))
}

fn optional_u16_attr(event: &BytesStart<'_>, key: &[u8]) -> Result<Option<u16>, ConfigError> {
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
                .map_err(|error| invalid(format!("invalid quest reward value: {error}")))?;
            value
                .trim()
                .parse::<u16>()
                .map_err(|_| invalid("quest reward values must fit an unsigned 16-bit integer"))
        })
        .transpose()
}

fn parse_mission(event: &BytesStart<'_>) -> Result<(String, String), ConfigError> {
    let name = event
        .attributes()
        .with_checks(false)
        .find_map(|attribute| {
            let attribute = attribute.ok()?;
            (attribute.key.as_ref() == b"name").then_some(attribute)
        })
        .ok_or_else(|| invalid("quest mission is missing its name attribute"))?;
    let description = event
        .attributes()
        .with_checks(false)
        .find_map(|attribute| {
            let attribute = attribute.ok()?;
            (attribute.key.as_ref() == b"description").then_some(attribute)
        })
        .ok_or_else(|| invalid("quest mission is missing its description attribute"))?;
    let name_value = name
        .unescape_value()
        .map_err(|error| invalid(format!("invalid mission name: {error}")))?;
    let description_value = description
        .unescape_value()
        .map_err(|error| invalid(format!("invalid mission description: {error}")))?;
    let name_value = name_value.trim();
    let description_value = description_value.trim();
    if name_value.is_empty()
        || name_value.len() > 64
        || description_value.is_empty()
        || description_value.len() > 255
    {
        return Err(invalid(
            "mission name and description must stay within bounded lengths",
        ));
    }
    Ok((name_value.to_owned(), description_value.to_owned()))
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
