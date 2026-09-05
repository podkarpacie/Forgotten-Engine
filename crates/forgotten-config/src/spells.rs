use super::{ConfigError, EngineConfig};
use forgotten_core::{
    CombatAttackTiming, PlayerSpellCastEvent, MAX_COMBAT_INTERVAL_TICKS, MAX_SPELL_MANA_COST,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;

const MAX_SPELL_CATALOG_BYTES: usize = 256 * 1024;
/// Bounded Say-keyword length for exact spell invocation routing.
const MAX_SPELL_WORDS_BYTES: usize = 32;
/// Bounded fixed spell damage; shares the combat-event ceiling scale.
pub const MAX_SPELL_DAMAGE: u32 = 500;
/// Bounded haste self-modifier ceiling: no operator configuration can exceed 2x speed.
pub const MAX_SPELL_SPEED_PERCENT: u16 = 100;
const MAX_SPELL_CATALOG_DEPTH: usize = 4;
const MAX_SPELL_CATALOG_ENTRIES: usize = 10_000;
const SPELL_CATALOG_RELATIVE_PATH: &str = "spells/forgotten-engine-spells.xml";

/// An operator-owned, scriptless spell declaration. It creates only a typed resource-and-timing
/// event; target resolution, formula execution, effects, Lua, and client delivery remain
/// outside this adapter. Optional `words` route exact Say-keyword invocation, and optional
/// `damage` applies one bounded fixed hit to the caster's already-selected living adjacent target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarativeSpellDefinition {
    pub spell_id: u16,
    pub mana_cost: u16,
    pub timing: CombatAttackTiming,
    pub words: Option<String>,
    pub damage: Option<u16>,
    /// Timed self speed modifier (haste) in additive percent, 1..=MAX_SPELL_SPEED_PERCENT.
    pub speed_percent: Option<u16>,
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
        self.entries.get(&spell_id).cloned()
    }

    /// Resolves one exact Say-keyword invocation to its spell. Keywords are matched after ASCII
    /// whitespace trimming and ASCII lowercasing on both sides; an empty catalog word list never
    /// matches.
    pub fn by_words(&self, message: &str) -> Option<DeclarativeSpellDefinition> {
        let trimmed = message.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut normalized = trimmed.to_owned();
        normalized.make_ascii_lowercase();
        self.entries
            .values()
            .find(|spell| spell.words.as_deref() == Some(normalized.as_str()))
            .cloned()
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
            .map(|(spell_id, definition)| (*spell_id, definition.clone()))
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
    let mut known = [false; 6];
    let mut words: Option<String> = None;
    let mut damage: Option<u16> = None;
    let mut speed_percent: Option<u16> = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid spell attribute: {error}")))?;
        match attribute.key.as_ref() {
            b"id" => known[0] = true,
            b"manacost" => known[1] = true,
            b"intervalticks" => known[2] = true,
            b"words" => {
                let value = attribute
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
                    .map_err(|error| invalid(format!("invalid spell words: {error}")))?;
                let value = value.trim();
                if value.is_empty() || value.len() > MAX_SPELL_WORDS_BYTES {
                    return Err(invalid(
                        "declarative spell words must stay within the bounded length",
                    ));
                }
                if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
                    return Err(invalid(
                        "declarative spell words must be bounded ASCII without whitespace",
                    ));
                }
                words = Some(value.to_ascii_lowercase());
                known[3] = true;
            }
            b"damage" => {
                let value = required_u16(event, b"damage")?;
                if value == 0 || value as u32 > MAX_SPELL_DAMAGE {
                    return Err(invalid(
                        "declarative spell damage is outside the configured range",
                    ));
                }
                damage = Some(value);
                known[4] = true;
            }
            b"speed" => {
                let value = required_u16(event, b"speed")?;
                if value == 0 || value > MAX_SPELL_SPEED_PERCENT {
                    return Err(invalid(
                        "declarative spell speed percent is outside the configured range",
                    ));
                }
                speed_percent = Some(value);
                known[5] = true;
            }
            _ => return Err(invalid("unsupported declarative spell attribute")),
        }
    }
    if known.iter().take(3).any(|known| !known) {
        return Err(invalid("declarative spell is missing a required attribute"));
    }
    if damage.is_some() && speed_percent.is_some() {
        return Err(invalid(
            "declarative spell cannot combine damage and speed effects",
        ));
    }
    Ok(DeclarativeSpellDefinition {
        spell_id,
        mana_cost,
        timing,
        words,
        damage,
        speed_percent,
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
        .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
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
                words: None,
                damage: None,
                speed_percent: None,
            })
        );
        let event = catalog.get(200).unwrap().cast_event(7).unwrap();
        assert_eq!(event.caster_id, 7);
        assert_eq!(event.spell_id, 200);
        assert_eq!(event.mana_cost, 35);
    }

    #[test]
    fn parses_spell_words_and_damage_with_validation() {
        let catalog = parse_declarative_spells_xml(
            br#"<fe-spells>
                    <fe-spell id="100" manacost="20" intervalticks="2" words="exura" damage="15"/>
                    <fe-spell id="200" manacost="35" intervalticks="4"/>
                </fe-spells>"#,
        )
        .unwrap();
        let healer = catalog.get(100).unwrap();
        assert_eq!(healer.words.as_deref(), Some("exura"));
        assert_eq!(healer.damage, Some(15));
        assert_eq!(
            catalog.by_words("  EXURA ").map(|spell| spell.spell_id),
            Some(100)
        );
        assert_eq!(catalog.by_words("exura vita"), None);
        // Damage beyond the bounded ceiling is rejected.
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="300" manacost="20" intervalticks="2" damage="501"/></fe-spells>"#,
        )
        .is_err());
        // Empty or whitespace words are rejected.
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="300" manacost="20" intervalticks="2" words="  "/></fe-spells>"#,
        )
        .is_err());
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

    #[test]
    fn parses_speed_effect_with_bounds_and_rejects_damage_combination() {
        let catalog = parse_declarative_spells_xml(
            br#"<fe-spells>
                    <fe-spell id="400" manacost="30" intervalticks="4" words="hur" speed="40"/>
                </fe-spells>"#,
        )
        .unwrap();
        let haste = catalog.get(400).unwrap();
        assert_eq!(haste.speed_percent, Some(40));
        assert_eq!(haste.damage, None);
        assert_eq!(haste.words.as_deref(), Some("hur"));

        // Speed beyond the bounded ceiling is rejected.
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="401" manacost="30" intervalticks="4" speed="101"/></fe-spells>"#,
        )
        .is_err());
        // Zero speed is rejected.
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="401" manacost="30" intervalticks="4" speed="0"/></fe-spells>"#,
        )
        .is_err());
        // Combining damage and speed on one spell is rejected.
        assert!(parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="402" manacost="30" intervalticks="2" damage="15" speed="40"/></fe-spells>"#,
        )
        .is_err());
    }
}
