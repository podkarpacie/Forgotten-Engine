use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;

const CHANNEL_CATALOG_RELATIVE_PATH: &str = "chatchannels/chatchannels.xml";
const MAX_CHANNEL_CATALOG_BYTES: usize = 256 * 1024;
const MAX_CHANNEL_CATALOG_DEPTH: usize = 2;
const MAX_PUBLIC_CHANNELS: usize = u8::MAX as usize;
const MAX_CHANNEL_NAME_BYTES: usize = 64;

/// One validated public entry from an operator-supplied standard TFS `chatchannels.xml` file.
/// Script references and all membership policy remain intentionally outside this data adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyPublicChannelDefinition {
    pub id: u16,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyPublicChannelCatalog {
    entries: BTreeMap<u16, LegacyPublicChannelDefinition>,
}

impl LegacyPublicChannelCatalog {
    pub fn get(&self, id: u16) -> Option<&LegacyPublicChannelDefinition> {
        self.entries.get(&id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &LegacyPublicChannelDefinition> {
        self.entries.values()
    }

    fn insert(&mut self, definition: LegacyPublicChannelDefinition) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_PUBLIC_CHANNELS && !self.entries.contains_key(&definition.id) {
            return Err(invalid(
                "public channel count exceeds the classic channel-list limit",
            ));
        }
        if self.entries.insert(definition.id, definition).is_some() {
            return Err(invalid("duplicate public channel ID"));
        }
        Ok(())
    }
}

/// Loads the standard operator-owned TFS channel-definition path when it exists. Missing channel
/// data keeps the native channel list empty. FE exposes only validated `public` entries and never
/// loads referenced scripts or constructs membership state through this adapter.
pub fn load_tfs_public_channel_catalog(
    config: &EngineConfig,
) -> Result<Option<LegacyPublicChannelCatalog>, ConfigError> {
    let path = config.content_directory.join(CHANNEL_CATALOG_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    parse_tfs_public_channels_xml(&fs::read(path).map_err(ConfigError::Io)?).map(Some)
}

pub fn parse_tfs_public_channels_xml(
    bytes: &[u8],
) -> Result<LegacyPublicChannelCatalog, ConfigError> {
    if bytes.len() > MAX_CHANNEL_CATALOG_BYTES {
        return Err(invalid(
            "TFS channel catalog exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut catalog = LegacyPublicChannelCatalog::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_CHANNEL_CATALOG_DEPTH {
                    return Err(invalid(
                        "TFS channel XML nesting exceeds the configured limit",
                    ));
                }
                if depth != 1 || root_seen || event.name().as_ref() != b"channels" {
                    return Err(invalid("TFS channel catalog has an unexpected XML element"));
                }
                root_seen = true;
            }
            Event::Empty(event) => {
                if depth != 1 || event.name().as_ref() != b"channel" {
                    return Err(invalid(
                        "TFS channel entries must be empty channel elements",
                    ));
                }
                if let Some(definition) = parse_public_channel(&event)? {
                    catalog.insert(definition)?;
                }
            }
            Event::End(event) => {
                if depth != 1 || event.name().as_ref() != b"channels" {
                    return Err(invalid("TFS channel catalog has an invalid closing tag"));
                }
                depth -= 1;
            }
            Event::Text(text) if !text.as_ref().iter().all(u8::is_ascii_whitespace) => {
                return Err(invalid("TFS channel catalog cannot contain text nodes"));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return Err(invalid("unsupported TFS channel XML node")),
        }
        buffer.clear();
    }
    if depth != 0 || !root_seen {
        return Err(invalid(
            "TFS channel catalog is missing a complete channels root",
        ));
    }
    Ok(catalog)
}

fn parse_public_channel(
    event: &BytesStart<'_>,
) -> Result<Option<LegacyPublicChannelDefinition>, ConfigError> {
    let mut id = None;
    let mut name = None;
    let mut public = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid channel attribute: {error}")))?;
        let value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| invalid(format!("invalid channel attribute value: {error}")))?;
        match attribute.key.as_ref() {
            b"id" => {
                if id
                    .replace(
                        value
                            .parse::<u16>()
                            .map_err(|_| invalid("channel ID must be an unsigned integer"))?,
                    )
                    .is_some()
                {
                    return Err(invalid("duplicate channel ID attribute"));
                }
            }
            b"name" => {
                if value.is_empty() || value.len() > MAX_CHANNEL_NAME_BYTES {
                    return Err(invalid(
                        "channel name is empty or exceeds the configured limit",
                    ));
                }
                if name.replace(value.into_owned()).is_some() {
                    return Err(invalid("duplicate channel name attribute"));
                }
            }
            b"public" => {
                let value = match value.as_ref() {
                    "1" | "true" => true,
                    "0" | "false" => false,
                    _ => return Err(invalid("channel public attribute must be true or false")),
                };
                if public.replace(value).is_some() {
                    return Err(invalid("duplicate channel public attribute"));
                }
            }
            b"script" => {}
            _ => return Err(invalid("unsupported TFS channel attribute")),
        }
    }
    let id = id.ok_or_else(|| invalid("channel is missing an ID"))?;
    if id == 0 {
        return Err(invalid("channel ID must be nonzero"));
    }
    let name = name.ok_or_else(|| invalid("channel is missing a name"))?;
    Ok(public
        .unwrap_or(false)
        .then_some(LegacyPublicChannelDefinition { id, name }))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid TFS channel XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_only_validated_public_tfs_channels_without_script_execution() {
        let catalog = parse_tfs_public_channels_xml(
            br#"<channels><channel id="1" name="World Chat" public="true" script="world.lua"/><channel id="2" name="Staff" public="false"/></channels>"#,
        )
        .unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(
            catalog.iter().collect::<Vec<_>>(),
            vec![&LegacyPublicChannelDefinition {
                id: 1,
                name: "World Chat".into(),
            }]
        );
    }

    #[test]
    fn rejects_invalid_public_channel_metadata_and_duplicate_ids() {
        assert!(parse_tfs_public_channels_xml(
            br#"<channels><channel id="1" name="World" public="true"/><channel id="1" name="Trade" public="true"/></channels>"#,
        )
        .is_err());
        assert!(parse_tfs_public_channels_xml(
            br#"<channels><channel id="0" name="World" public="true"/></channels>"#,
        )
        .is_err());
        assert!(parse_tfs_public_channels_xml(
            br#"<channels><channel id="1" name="World" public="yes"/></channels>"#,
        )
        .is_err());
    }
}
