//! Bounded, non-executing parser for the operator-owned TFS `talkactions/talkactions.xml`
//! registry. It retains only the `words` trigger, optional `separator`, and a validated
//! `script` path per entry. It never reads or executes Lua, and deferring routing, parameter
//! splitting, authorization (`access`/`groups`), and legacy TFS matching semantics stays an
//! explicit runtime boundary.

use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

const TALKACTIONS_REGISTRY_RELATIVE_PATH: &str = "talkactions/talkactions.xml";
const MAX_TALKACTIONS_REGISTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_TALKACTIONS_REGISTRY_DEPTH: usize = 32;
const MAX_TALKACTION_ENTRIES: usize = 65_536;
const MAX_TALKACTION_WORDS_BYTES: usize = 64;
const MAX_TALKACTION_SEPARATOR_BYTES: usize = 16;
const MAX_TALKACTION_SCRIPT_PATH_BYTES: usize = 512;
const DEFAULT_TALKACTION_SEPARATOR: &str = " ";

/// One declared talkaction trigger. `script` is a safe relative path into the operator content
/// tree; matching, authorization, and parameter semantics are not represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsTalkActionEntry {
    pub words: String,
    pub separator: String,
    pub script: PathBuf,
}

/// A bounded exact-`words` talkaction catalog. Duplicate words are rejected; matching is
/// case-sensitive here and any case-insensitive policy belongs to the routing layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TfsTalkActionRegistry {
    entries: BTreeMap<String, TfsTalkActionEntry>,
}

impl TfsTalkActionRegistry {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, words: &str) -> Option<&TfsTalkActionEntry> {
        self.entries.get(words)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TfsTalkActionEntry> {
        self.entries.values()
    }

    fn insert(&mut self, entry: TfsTalkActionEntry) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_TALKACTION_ENTRIES {
            return Err(invalid(
                "TFS talkaction registry exceeds the configured entry limit",
            ));
        }
        if self.entries.insert(entry.words.clone(), entry).is_some() {
            return Err(invalid("duplicate TFS talkaction words"));
        }
        Ok(())
    }
}

/// Loads the optional TFS talkaction registry. A missing file intentionally yields an empty
/// catalog so worlds without talkactions keep the existing no-op behavior.
pub fn load_tfs_talkaction_registry(
    config: &EngineConfig,
) -> Result<TfsTalkActionRegistry, ConfigError> {
    let path = config
        .content_directory
        .join(TALKACTIONS_REGISTRY_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(TfsTalkActionRegistry::default());
    }
    parse_tfs_talkactions_xml(&fs::read(path).map_err(ConfigError::Io)?)
}

pub fn parse_tfs_talkactions_xml(bytes: &[u8]) -> Result<TfsTalkActionRegistry, ConfigError> {
    if bytes.len() > MAX_TALKACTIONS_REGISTRY_BYTES {
        return Err(invalid(
            "TFS talkaction registry exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut registry = TfsTalkActionRegistry::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_TALKACTIONS_REGISTRY_DEPTH {
                    return Err(invalid(
                        "TFS talkaction registry nesting exceeds the configured limit",
                    ));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"talkactions" {
                        return Err(invalid(
                            "TFS talkaction registry has an invalid root element",
                        ));
                    }
                    root_seen = true;
                } else {
                    return Err(invalid("TFS talkaction entries must be empty elements"));
                }
            }
            Event::Empty(event) => {
                if !root_seen || depth + 1 != 2 || event.name().as_ref() != b"talkaction" {
                    return Err(invalid("TFS talkaction entry is malformed"));
                }
                registry.insert(parse_talkaction_entry(&event)?)?;
            }
            Event::End(event) => {
                if event.name().as_ref() != b"talkactions" {
                    return Err(invalid("TFS talkaction registry closing tag is invalid"));
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("TFS talkaction registry has unbalanced tags"))?;
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            Event::Text(text) if text.as_ref().iter().all(u8::is_ascii_whitespace) => {}
            _ => return Err(invalid("unsupported TFS talkaction registry XML node")),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(invalid(
            "TFS talkaction registry is missing a complete root",
        ));
    }
    Ok(registry)
}

fn parse_talkaction_entry(event: &BytesStart<'_>) -> Result<TfsTalkActionEntry, ConfigError> {
    let mut words = None;
    let mut separator = None;
    let mut script = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute
            .map_err(|error| invalid(format!("invalid TFS talkaction attribute: {error}")))?;
        let value = attribute
            .normalized_value(XmlVersion::Explicit1_0)
            .map_err(|error| invalid(format!("invalid TFS talkaction attribute value: {error}")))?
            .into_owned();
        match attribute.key.as_ref() {
            b"words" => {
                if words.replace(value).is_some() {
                    return Err(invalid("duplicate TFS talkaction words attribute"));
                }
            }
            b"script" => {
                if script.replace(value).is_some() {
                    return Err(invalid("duplicate TFS talkaction script attribute"));
                }
            }
            b"separator" => {
                if separator.replace(value).is_some() {
                    return Err(invalid("duplicate TFS talkaction separator attribute"));
                }
            }
            // `access`, `groups`, `log`, `hidden`, and `caseSensitive` are authorization or
            // matching metadata deferred to the routing boundary; ignored here, not executed.
            _ => {}
        }
    }
    let words = words.ok_or_else(|| invalid("TFS talkaction is missing its words attribute"))?;
    let script = script.ok_or_else(|| invalid("TFS talkaction is missing its script attribute"))?;
    if words.is_empty()
        || words.len() > MAX_TALKACTION_WORDS_BYTES
        || words.trim() != words
        || words.chars().any(char::is_control)
    {
        return Err(invalid(
            "TFS talkaction words are outside the configured bounds",
        ));
    }
    let separator = separator.unwrap_or_else(|| DEFAULT_TALKACTION_SEPARATOR.to_owned());
    if separator.len() > MAX_TALKACTION_SEPARATOR_BYTES || separator.chars().any(char::is_control) {
        return Err(invalid(
            "TFS talkaction separator is outside the configured bounds",
        ));
    }
    let script = validate_script_path(&script)?;
    Ok(TfsTalkActionEntry {
        words,
        separator,
        script,
    })
}

fn validate_script_path(raw: &str) -> Result<PathBuf, ConfigError> {
    if raw.is_empty() || raw.len() > MAX_TALKACTION_SCRIPT_PATH_BYTES {
        return Err(invalid(
            "TFS talkaction script path is outside the configured bounds",
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
        return Err(invalid("TFS talkaction script path is unsafe"));
    }
    Ok(path.to_path_buf())
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid TFS talkaction registry XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_talkaction_entries() {
        let registry = parse_tfs_talkactions_xml(
            br#"<talkactions>
                <talkaction words="/!save" script="save.lua"/>
                <talkaction words="/goto" separator=" " script="magic/goto.lua"/>
            </talkactions>"#,
        )
        .unwrap();
        assert_eq!(registry.len(), 2);
        let save = registry.get("/!save").unwrap();
        assert_eq!(save.script, PathBuf::from("save.lua"));
        assert_eq!(save.separator, " ");
        let goto = registry.get("/goto").unwrap();
        assert_eq!(goto.script, PathBuf::from("magic/goto.lua"));
        assert_eq!(registry.get("/missing"), None);
    }

    #[test]
    fn rejects_duplicate_unbounded_and_unsafe_talkactions() {
        assert!(parse_tfs_talkactions_xml(
            br#"<talkactions><talkaction words="/x" script="a.lua"/><talkaction words="/x" script="b.lua"/></talkactions>"#,
        )
        .is_err());
        assert!(parse_tfs_talkactions_xml(
            br#"<talkactions><talkaction words="  /x" script="a.lua"/></talkactions>"#,
        )
        .is_err());
        assert!(parse_tfs_talkactions_xml(
            br#"<talkactions><talkaction words="/x" script="../a.lua"/></talkactions>"#,
        )
        .is_err());
        assert!(parse_tfs_talkactions_xml(
            br#"<talkactions><talkaction script="a.lua"/></talkactions>"#,
        )
        .is_err());
    }
}
