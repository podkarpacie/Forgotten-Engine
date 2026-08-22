use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;

const NPC_DIALOGUE_CATALOG_RELATIVE_PATH: &str = "npc/forgotten-engine-dialogue.xml";
const MAX_NPC_DIALOGUE_CATALOG_BYTES: usize = 256 * 1024;
const MAX_NPC_DIALOGUE_CATALOG_DEPTH: usize = 3;
const MAX_NPC_DIALOGUE_RESPONSES: usize = 10_000;
const MAX_NPC_DIALOGUE_NAME_BYTES: usize = 64;
const MAX_NPC_DIALOGUE_KEYWORD_BYTES: usize = 64;
const MAX_NPC_DIALOGUE_TEXT_BYTES: usize = 255;

/// One operator-owned exact static-NPC response. It has no script, focus, shop, travel, quest, or
/// mutable conversation state attached to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarativeNpcDialogueResponse {
    pub npc_name: String,
    pub keyword: String,
    pub text: String,
}

/// A bounded exact-match dialogue catalog keyed by normalized ASCII NPC name and keyword. It does
/// not discover TFS NPC scripts or infer any response from XML files outside this FE-owned format.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarativeNpcDialogueCatalog {
    entries: BTreeMap<(String, String), DeclarativeNpcDialogueResponse>,
}

impl DeclarativeNpcDialogueCatalog {
    pub fn get(&self, npc_name: &str, keyword: &str) -> Option<&DeclarativeNpcDialogueResponse> {
        self.entries.get(&(normalize(npc_name), normalize(keyword)))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn insert(&mut self, response: DeclarativeNpcDialogueResponse) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_NPC_DIALOGUE_RESPONSES {
            return Err(invalid(
                "declarative NPC dialogue response count exceeds the configured limit",
            ));
        }
        let key = (normalize(&response.npc_name), normalize(&response.keyword));
        if self.entries.insert(key, response).is_some() {
            return Err(invalid("duplicate declarative NPC name and keyword"));
        }
        Ok(())
    }
}

/// Loads an optional FE-owned static NPC dialogue catalog. Missing data intentionally leaves NPCs
/// display-only and preserves the existing no-dialogue behavior.
pub fn load_declarative_npc_dialogue_catalog(
    config: &EngineConfig,
) -> Result<Option<DeclarativeNpcDialogueCatalog>, ConfigError> {
    let path = config
        .content_directory
        .join(NPC_DIALOGUE_CATALOG_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    parse_declarative_npc_dialogue_xml(&fs::read(path).map_err(ConfigError::Io)?).map(Some)
}

pub fn parse_declarative_npc_dialogue_xml(
    bytes: &[u8],
) -> Result<DeclarativeNpcDialogueCatalog, ConfigError> {
    if bytes.len() > MAX_NPC_DIALOGUE_CATALOG_BYTES {
        return Err(invalid(
            "declarative NPC dialogue catalog exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut current_npc = None;
    let mut catalog = DeclarativeNpcDialogueCatalog::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_NPC_DIALOGUE_CATALOG_DEPTH {
                    return Err(invalid(
                        "declarative NPC dialogue XML nesting exceeds the limit",
                    ));
                }
                match depth {
                    1 => require_root(&event, &mut root_seen)?,
                    2 if event.name().as_ref() == b"npc" => {
                        current_npc = Some(required_ascii_attribute(
                            &event,
                            b"name",
                            MAX_NPC_DIALOGUE_NAME_BYTES,
                        )?);
                    }
                    _ => return Err(invalid("unexpected declarative NPC dialogue XML element")),
                }
            }
            Event::Empty(event) => {
                if depth != 2 || event.name().as_ref() != b"response" {
                    return Err(invalid(
                        "NPC dialogue responses must be empty response elements",
                    ));
                }
                let npc_name = current_npc.clone().ok_or_else(|| {
                    invalid("declarative NPC response is missing its parent NPC name")
                })?;
                let (keyword, text) = parse_response_attributes(&event)?;
                catalog.insert(DeclarativeNpcDialogueResponse {
                    npc_name,
                    keyword,
                    text,
                })?;
            }
            Event::End(event) => match depth {
                1 if event.name().as_ref() == b"fe-npc-dialogues" => depth -= 1,
                2 if event.name().as_ref() == b"npc" => {
                    current_npc = None;
                    depth -= 1;
                }
                _ => {
                    return Err(invalid(
                        "declarative NPC dialogue XML closing tag is invalid",
                    ))
                }
            },
            Event::Text(text) if !text.as_ref().iter().all(u8::is_ascii_whitespace) => {
                return Err(invalid(
                    "declarative NPC dialogue catalog cannot contain text nodes",
                ));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return Err(invalid("unsupported declarative NPC dialogue XML node")),
        }
        buffer.clear();
    }
    if depth != 0 || !root_seen || current_npc.is_some() {
        return Err(invalid(
            "declarative NPC dialogue catalog is missing a complete root",
        ));
    }
    Ok(catalog)
}

fn require_root(event: &BytesStart<'_>, root_seen: &mut bool) -> Result<(), ConfigError> {
    if *root_seen
        || event.name().as_ref() != b"fe-npc-dialogues"
        || event.attributes().next().is_some()
    {
        return Err(invalid(
            "declarative NPC dialogue root must be one attribute-free fe-npc-dialogues element",
        ));
    }
    *root_seen = true;
    Ok(())
}

fn required_ascii_attribute(
    event: &BytesStart<'_>,
    key: &[u8],
    maximum_bytes: usize,
) -> Result<String, ConfigError> {
    let value = required_attribute(event, key)?;
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.is_ascii()
        || value.trim() != value
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid(
            "declarative NPC dialogue name or keyword is outside the configured bounds",
        ));
    }
    Ok(value)
}

fn required_attribute(event: &BytesStart<'_>, key: &[u8]) -> Result<String, ConfigError> {
    let mut value = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| invalid(format!("invalid NPC dialogue attribute: {error}")))?;
        let attribute_value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| invalid(format!("invalid NPC dialogue attribute value: {error}")))?;
        if attribute.key.as_ref() == key {
            if value.replace(attribute_value.into_owned()).is_some() {
                return Err(invalid("duplicate declarative NPC dialogue attribute"));
            }
        } else {
            return Err(invalid("unsupported declarative NPC dialogue attribute"));
        }
    }
    value.ok_or_else(|| invalid("declarative NPC dialogue attribute is missing"))
}

fn parse_response_attributes(event: &BytesStart<'_>) -> Result<(String, String), ConfigError> {
    let mut keyword = None;
    let mut text = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| invalid(format!("invalid NPC dialogue attribute: {error}")))?;
        let value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| invalid(format!("invalid NPC dialogue attribute value: {error}")))?
            .into_owned();
        match attribute.key.as_ref() {
            b"keyword" => {
                if keyword.replace(value).is_some() {
                    return Err(invalid("duplicate declarative NPC dialogue keyword"));
                }
            }
            b"text" => {
                if text.replace(value).is_some() {
                    return Err(invalid("duplicate declarative NPC dialogue text"));
                }
            }
            _ => return Err(invalid("unsupported declarative NPC dialogue attribute")),
        }
    }
    let keyword = keyword.ok_or_else(|| invalid("declarative NPC response is missing keyword"))?;
    let text = text.ok_or_else(|| invalid("declarative NPC response is missing text"))?;
    if keyword.is_empty()
        || keyword.len() > MAX_NPC_DIALOGUE_KEYWORD_BYTES
        || !keyword.is_ascii()
        || keyword.trim() != keyword
        || keyword.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid(
            "declarative NPC dialogue name or keyword is outside the configured bounds",
        ));
    }
    if text.is_empty()
        || text.len() > MAX_NPC_DIALOGUE_TEXT_BYTES
        || text.trim() != text
        || text.chars().any(char::is_control)
    {
        return Err(invalid(
            "declarative NPC dialogue text is outside the configured bounds",
        ));
    }
    Ok((keyword, text))
}

fn normalize(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid declarative NPC dialogue XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_exact_npc_dialogue_responses() {
        let catalog = parse_declarative_npc_dialogue_xml(
            br#"<fe-npc-dialogues><npc name="Guide"><response keyword="hi" text="Welcome."/><response keyword="trade" text="I do not trade yet."/></npc></fe-npc-dialogues>"#,
        )
        .unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(
            catalog.get("guide", "HI"),
            Some(&DeclarativeNpcDialogueResponse {
                npc_name: "Guide".into(),
                keyword: "hi".into(),
                text: "Welcome.".into(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_and_unbounded_npc_dialogue_entries() {
        assert!(parse_declarative_npc_dialogue_xml(
            br#"<fe-npc-dialogues><npc name="Guide"><response keyword="hi" text="One."/><response keyword="HI" text="Two."/></npc></fe-npc-dialogues>"#,
        )
        .is_err());
        assert!(parse_declarative_npc_dialogue_xml(
            br#"<fe-npc-dialogues><npc name="Guide"><response keyword="hi" text="  Welcome."/></npc></fe-npc-dialogues>"#,
        )
        .is_err());
    }
}
