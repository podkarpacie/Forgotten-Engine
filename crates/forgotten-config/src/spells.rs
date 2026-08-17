use super::{ConfigError, EngineConfig};
use forgotten_core::{
    CombatAttackTiming, PlayerSpellCastEvent, MAX_COMBAT_INTERVAL_TICKS, MAX_SPELL_MANA_COST,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;

const MAX_SPELL_CATALOG_BYTES: usize = 256 * 1024;
const MAX_SPELL_CATALOG_DEPTH: usize = 4;
const MAX_SPELL_CATALOG_ENTRIES: usize = 10_000;
const SPELL_CATALOG_RELATIVE_PATH: &str = "spells/forgotten-engine-spells.xml";

/// An operator-owned, scriptless spell declaration. It creates only a typed resource-and-timing
/// event; target resolution, formula execution, effects, words, Lua, and client delivery remain
/// outside this adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclarativeSpellDefinition {
    pub spell_id: u16,
    pub mana_cost: u16,
    pub timing: CombatAttackTiming,
}

impl DeclarativeSpellDefinition {
    pub fn cast_event(self, caster_id: u64) -> Result<PlayerSpellCastEvent, ConfigError> {
        PlayerSpellCastEvent::new(caster_id, self.spell_id, self.mana_cost, self.timing)
            .map_err(|_| invalid("validated declarative spell could not create a cast event"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarativeSpellCatalog {
    entries: BTreeMap<u16, DeclarativeSpellDefinition>,
}

impl DeclarativeSpellCatalog {
    pub fn get(&self, spell_id: u16) -> Option<DeclarativeSpellDefinition> {
        self.entries.get(&spell_id).copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, DeclarativeSpellDefinition)> + '_ {
        self.entries
            .iter()
            .map(|(spell_id, definition)| (*spell_id, *definition))
    }

    fn insert(&mut self, definition: DeclarativeSpellDefinition) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_SPELL_CATALOG_ENTRIES
            && !self.entries.contains_key(&definition.spell_id)
        {
            return Err(invalid(
                "declarative spell count exceeds the configured limit",
            ));
        }
        if self
            .entries
            .insert(definition.spell_id, definition)
            .is_some()
        {
            return Err(invalid("duplicate declarative spell ID"));
        }
        Ok(())
    }
}

pub fn load_declarative_spell_catalog(
    config: &EngineConfig,
) -> Result<Option<DeclarativeSpellCatalog>, ConfigError> {
    let path = config.content_directory.join(SPELL_CATALOG_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    parse_declarative_spells_xml(&fs::read(path).map_err(ConfigError::Io)?).map(Some)
}

pub fn parse_declarative_spells_xml(bytes: &[u8]) -> Result<DeclarativeSpellCatalog, ConfigError> {
    if bytes.len() > MAX_SPELL_CATALOG_BYTES {
        return Err(invalid(
            "declarative spell catalog exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut catalog = DeclarativeSpellCatalog::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_SPELL_CATALOG_DEPTH {
                    return Err(invalid(
                        "declarative spell XML nesting exceeds the configured limit",
                    ));
                }
                if depth == 1 {
                    require_root(&event, &mut root_seen)?;
                } else {
                    return Err(invalid(
                        "declarative spell entries must be empty XML elements",
                    ));
                }
            }
            Event::Empty(event) => {
                if depth != 1 || event.name().as_ref() != b"fe-spell" {
                    return Err(invalid("unexpected declarative spell XML element"));
                }
                catalog.insert(parse_spell(&event)?)?;
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(invalid("unexpected closing XML element"));
                }
                if depth == 1 && event.name().as_ref() != b"fe-spells" {
                    return Err(invalid("declarative spell root closing tag is invalid"));
                }
                depth -= 1;
            }
            Event::Text(text) if !text.as_ref().iter().all(u8::is_ascii_whitespace) => {
                return Err(invalid(
                    "declarative spell catalog cannot contain text nodes",
                ));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return Err(invalid("unsupported declarative spell XML node")),
        }
        buffer.clear();
    }
    if depth != 0 || !root_seen {
        return Err(invalid(
            "declarative spell catalog is missing a complete fe-spells root",
        ));
    }
    Ok(catalog)
}

fn require_root(event: &BytesStart<'_>, root_seen: &mut bool) -> Result<(), ConfigError> {
    if *root_seen || event.name().as_ref() != b"fe-spells" || event.attributes().next().is_some() {
        return Err(invalid(
            "declarative spell root must be one attribute-free fe-spells element",
        ));
    }
    *root_seen = true;
    Ok(())
}

fn parse_spell(event: &BytesStart<'_>) -> Result<DeclarativeSpellDefinition, ConfigError> {
    let spell_id = required_u16(event, b"id")?;
    let mana_cost = required_u16(event, b"manacost")?;
    if spell_id == 0 || mana_cost == 0 || mana_cost > MAX_SPELL_MANA_COST {
        return Err(invalid(
            "declarative spell ID or mana cost is outside the configured range",
        ));
    }
    let interval_ticks = required_u16(event, b"intervalticks")?;
    if interval_ticks == 0 || interval_ticks > MAX_COMBAT_INTERVAL_TICKS {
        return Err(invalid(
            "declarative spell interval is outside the configured range",
        ));
    }
    let timing = CombatAttackTiming::new(interval_ticks)
        .map_err(|_| invalid("declarative spell interval is outside the configured range"))?;
    let mut known = [false; 3];
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid spell attribute: {error}")))?;
        match attribute.key.as_ref() {
            b"id" => known[0] = true,
            b"manacost" => known[1] = true,
            b"intervalticks" => known[2] = true,
            _ => return Err(invalid("unsupported declarative spell attribute")),
        }
    }
    if known.iter().any(|known| !known) {
        return Err(invalid("declarative spell is missing a required attribute"));
    }
    Ok(DeclarativeSpellDefinition {
        spell_id,
        mana_cost,
        timing,
    })
}

fn required_u16(event: &BytesStart<'_>, key: &[u8]) -> Result<u16, ConfigError> {
    let value = event
        .attributes()
        .with_checks(false)
        .find_map(|attribute| {
            let attribute = attribute.ok()?;
            (attribute.key.as_ref() == key).then_some(attribute)
        })
        .ok_or_else(|| invalid("declarative spell is missing a required attribute"))?;
    let value = value
        .normalized_value(XmlVersion::Explicit1_0)
        .map_err(|error| invalid(format!("invalid spell attribute value: {error}")))?;
    value
        .parse::<u16>()
        .map_err(|_| invalid("declarative spell attribute must be an unsigned integer"))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid declarative spell XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_scriptless_spell_catalog() {
        let catalog = parse_declarative_spells_xml(
            br#"<?xml version="1.0"?><fe-spells><fe-spell id="100" manacost="20" intervalticks="2"/><fe-spell id="200" manacost="35" intervalticks="3"/></fe-spells>"#,
        )
        .unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(
            catalog.get(100),
            Some(DeclarativeSpellDefinition {
                spell_id: 100,
                mana_cost: 20,
                timing: CombatAttackTiming::new(2).unwrap(),
            })
        );
        let event = catalog.get(200).unwrap().cast_event(7).unwrap();
        assert_eq!(event.caster_id, 7);
        assert_eq!(event.spell_id, 200);
        assert_eq!(event.mana_cost, 35);
    }

    #[test]
    fn rejects_scripted_duplicate_and_invalid_spell_declarations() {
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="100" manacost="20" intervalticks="2" script="spell.lua"/></fe-spells>"#,
        )
        .is_err());
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="100" manacost="20" intervalticks="2"/><fe-spell id="100" manacost="20" intervalticks="2"/></fe-spells>"#,
        )
        .is_err());
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="0" manacost="20" intervalticks="2"/></fe-spells>"#,
        )
        .is_err());
        assert!(parse_declarative_spells_xml(
            br#"<spells><fe-spell id="100" manacost="20" intervalticks="2"/></spells>"#,
        )
        .is_err());
    }
}
