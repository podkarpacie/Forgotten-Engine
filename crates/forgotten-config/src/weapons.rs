use super::{ConfigError, EngineConfig};
use forgotten_core::{
    CombatAttackTiming, CombatDamageType, PlayerCombatEvent, PlayerSkill, MAX_COMBAT_EVENT_DAMAGE,
    MAX_DISTANCE_WEAPON_RANGE,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;

const MAX_WEAPON_CATALOG_BYTES: usize = 256 * 1024;
const MAX_WEAPON_CATALOG_DEPTH: usize = 4;
const MAX_WEAPON_CATALOG_ENTRIES: usize = 10_000;
const WEAPON_CATALOG_RELATIVE_PATH: &str = "weapons/forgotten-engine-weapons.xml";

/// An operator-owned, scriptless combat declaration. It is intentionally not a parser for the
/// broad historical TFS weapon-script surface: it supplies only typed physical adjacent-melee
/// inputs that FE can execute and test without running Lua.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclarativeWeaponDefinition {
    pub item_id: u16,
    pub physical_damage: u16,
    pub timing: CombatAttackTiming,
    /// Plan v49 slice 9: when declared (1..=5), the weapon is a distance weapon whose use
    /// against a creature fires a ranged shot instead of adjacent melee.
    pub distance_range: Option<u8>,
    /// Client-visible projectile effect id for the 0x85 missile record. Required together with
    /// `distance_range`; absent for melee declarations.
    pub shot_effect: Option<u8>,
}

impl DeclarativeWeaponDefinition {
    pub fn adjacent_melee_event(
        self,
        attacker_id: u64,
        target_id: u64,
    ) -> Result<PlayerCombatEvent, ConfigError> {
        if self.distance_range.is_some() || self.shot_effect.is_some() {
            return Err(invalid(
                "distance weapon declarations cannot fire adjacent-melee events",
            ));
        }
        PlayerCombatEvent::adjacent_melee(
            attacker_id,
            target_id,
            CombatDamageType::Physical,
            self.physical_damage,
            self.timing,
        )
        .map_err(|_| invalid("validated declarative weapon could not create a combat event"))
    }

    pub fn distance_shot_event(
        self,
        attacker_id: u64,
        target_id: u64,
    ) -> Result<PlayerCombatEvent, ConfigError> {
        let Some(distance_range) = self.distance_range else {
            return Err(invalid(
                "only declared distance weapons can fire distance-shot events",
            ));
        };
        PlayerCombatEvent::distance_shot(
            attacker_id,
            target_id,
            CombatDamageType::Physical,
            self.physical_damage,
            self.timing,
            distance_range,
        )
        .map_err(|_| invalid("validated declarative weapon could not create a combat event"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarativeWeaponCatalog {
    entries: BTreeMap<u16, DeclarativeWeaponDefinition>,
    adjacent_melee_skills: BTreeMap<u16, PlayerSkill>,
}

impl DeclarativeWeaponCatalog {
    pub fn get(&self, item_id: u16) -> Option<DeclarativeWeaponDefinition> {
        self.entries.get(&item_id).copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, DeclarativeWeaponDefinition)> + '_ {
        self.entries
            .iter()
            .map(|(item_id, definition)| (*item_id, *definition))
    }

    /// Attaches only prevalidated sword, club, and axe classifications for declarations that
    /// already exist in this catalog. The source map is bounded `items.xml` metadata; missing or
    /// unsupported classifications remain ineligible for a skill-try award.
    pub fn with_adjacent_melee_skills(
        mut self,
        skills_by_server_id: Option<&BTreeMap<u16, PlayerSkill>>,
    ) -> Self {
        self.adjacent_melee_skills = skills_by_server_id
            .map(|skills| {
                self.entries
                    .keys()
                    .filter_map(|item_id| {
                        skills.get(item_id).copied().map(|skill| (*item_id, skill))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self
    }

    /// Returns a skill classification only for an already declared adjacent-melee weapon.
    pub fn adjacent_melee_skill(&self, item_id: u16) -> Option<PlayerSkill> {
        self.adjacent_melee_skills.get(&item_id).copied()
    }

    fn insert(&mut self, definition: DeclarativeWeaponDefinition) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_WEAPON_CATALOG_ENTRIES
            && !self.entries.contains_key(&definition.item_id)
        {
            return Err(invalid(
                "declarative weapon count exceeds the configured limit",
            ));
        }
        if self
            .entries
            .insert(definition.item_id, definition)
            .is_some()
        {
            return Err(invalid("duplicate declarative weapon item ID"));
        }
        Ok(())
    }
}

pub fn load_declarative_weapon_catalog(
    config: &EngineConfig,
) -> Result<Option<DeclarativeWeaponCatalog>, ConfigError> {
    let path = config.content_directory.join(WEAPON_CATALOG_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    parse_declarative_weapons_xml(&fs::read(path).map_err(ConfigError::Io)?).map(Some)
}

pub fn parse_declarative_weapons_xml(
    bytes: &[u8],
) -> Result<DeclarativeWeaponCatalog, ConfigError> {
    if bytes.len() > MAX_WEAPON_CATALOG_BYTES {
        return Err(invalid(
            "declarative weapon catalog exceeds the configured size limit",
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut catalog = DeclarativeWeaponCatalog::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                validate_depth(depth)?;
                if depth == 1 {
                    require_root(&event, &mut root_seen)?;
                } else {
                    return Err(invalid(
                        "declarative weapon entries must be empty XML elements",
                    ));
                }
            }
            Event::Empty(event) => {
                if depth != 1 || event.name().as_ref() != b"weapon" {
                    return Err(invalid("unexpected declarative weapon XML element"));
                }
                catalog.insert(parse_weapon(&event)?)?;
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(invalid("unexpected closing XML element"));
                }
                if depth == 1 && event.name().as_ref() != b"fe-weapons" {
                    return Err(invalid("declarative weapon root closing tag is invalid"));
                }
                depth -= 1;
            }
            Event::Text(text) if !text.as_ref().iter().all(u8::is_ascii_whitespace) => {
                return Err(invalid(
                    "declarative weapon catalog cannot contain text nodes",
                ));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return Err(invalid("unsupported declarative weapon XML node")),
        }
        buffer.clear();
    }
    if depth != 0 || !root_seen {
        return Err(invalid(
            "declarative weapon catalog is missing a complete fe-weapons root",
        ));
    }
    Ok(catalog)
}

fn require_root(event: &BytesStart<'_>, root_seen: &mut bool) -> Result<(), ConfigError> {
    if *root_seen || event.name().as_ref() != b"fe-weapons" || event.attributes().next().is_some() {
        return Err(invalid(
            "declarative weapon root must be one attribute-free fe-weapons element",
        ));
    }
    *root_seen = true;
    Ok(())
}

fn validate_depth(depth: usize) -> Result<(), ConfigError> {
    if depth > MAX_WEAPON_CATALOG_DEPTH {
        return Err(invalid(
            "declarative weapon XML nesting exceeds the configured limit",
        ));
    }
    Ok(())
}

fn parse_weapon(event: &BytesStart<'_>) -> Result<DeclarativeWeaponDefinition, ConfigError> {
    let item_id = required_u16(event, b"itemid")?;
    let physical_damage = required_u16(event, b"damage")?;
    if physical_damage == 0 || physical_damage > MAX_COMBAT_EVENT_DAMAGE {
        return Err(invalid(
            "declarative weapon damage is outside the configured range",
        ));
    }
    let timing = CombatAttackTiming::new(required_u16(event, b"intervalticks")?)
        .map_err(|_| invalid("declarative weapon interval is outside the configured range"))?;
    let mut known = [false; 3];
    let mut distance_range: Option<u8> = None;
    let mut shot_effect: Option<u8> = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid weapon attribute: {error}")))?;
        match attribute.key.as_ref() {
            b"itemid" => known[0] = true,
            b"damage" => known[1] = true,
            b"intervalticks" => known[2] = true,
            b"distance" => {
                let raw = attribute
                    .unescape_value()
                    .map_err(|error| invalid(format!("invalid weapon attribute: {error}")))?;
                let parsed: u16 = raw
                    .trim()
                    .parse()
                    .map_err(|_| invalid("declarative weapon distance must be numeric"))?;
                if parsed == 0 || parsed > u16::from(MAX_DISTANCE_WEAPON_RANGE) {
                    return Err(invalid(
                        "declarative weapon distance must stay between 1 and 5",
                    ));
                }
                distance_range = Some(parsed as u8);
            }
            b"shoteffect" => {
                let raw = attribute
                    .unescape_value()
                    .map_err(|error| invalid(format!("invalid weapon attribute: {error}")))?;
                let parsed: u8 = raw
                    .trim()
                    .parse()
                    .map_err(|_| invalid("declarative weapon shotEffect must be numeric"))?;
                if parsed == 0 {
                    return Err(invalid("declarative weapon shotEffect must be nonzero"));
                }
                shot_effect = Some(parsed);
            }
            _ => return Err(invalid("unsupported declarative weapon attribute")),
        }
    }
    if known.iter().any(|known| !known) {
        return Err(invalid(
            "declarative weapon is missing a required attribute",
        ));
    }
    // Distance declarations require a projectile effect; melee declarations must not carry
    // distance-only attributes.
    match (distance_range.is_some(), shot_effect.is_some()) {
        (true, true) => {}
        (false, false) => {}
        _ => {
            return Err(invalid(
                "declarative distance weapons require both distance and shotEffect attributes",
            ))
        }
    }
    Ok(DeclarativeWeaponDefinition {
        item_id,
        physical_damage,
        timing,
        distance_range,
        shot_effect,
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
        .ok_or_else(|| invalid("declarative weapon is missing a required attribute"))?;
    let value = value
        .normalized_value(XmlVersion::Explicit1_0)
        .map_err(|error| invalid(format!("invalid weapon attribute value: {error}")))?;
    value
        .parse::<u16>()
        .map_err(|_| invalid("declarative weapon attribute must be an unsigned integer"))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid declarative weapon XML: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_scriptless_physical_weapon_catalog() {
        let catalog = parse_declarative_weapons_xml(
            br#"<?xml version="1.0"?><fe-weapons><weapon itemid="2376" damage="12" intervalticks="2"/><weapon itemid="2400" damage="24" intervalticks="3"/></fe-weapons>"#,
        )
        .unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(
            catalog.get(2376),
            Some(DeclarativeWeaponDefinition {
                item_id: 2376,
                physical_damage: 12,
                timing: CombatAttackTiming::new(2).unwrap(),
                distance_range: None,
                shot_effect: None,
            })
        );
        let event = catalog
            .get(2400)
            .unwrap()
            .adjacent_melee_event(7, 8)
            .unwrap();
        assert_eq!(event.damage_type, CombatDamageType::Physical);
        assert_eq!(event.requested_damage, 24);
    }

    #[test]
    fn parses_distance_declarations_and_rejects_incomplete_pairs() {
        let catalog = parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2455" damage="18" intervalticks="2" distance="5" shoteffect="29"/></fe-weapons>"#,
        )
        .unwrap();
        let definition = catalog.get(2455).unwrap();
        assert_eq!(definition.distance_range, Some(5));
        assert_eq!(definition.shot_effect, Some(29));
        let event = definition.distance_shot_event(7, 8).unwrap();
        assert!(matches!(
            event.delivery,
            forgotten_core::CombatDelivery::RangedDistance { max_range: 5 }
        ));

        // Distance without a shot effect (and the inverse) is rejected.
        assert!(parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2455" damage="18" intervalticks="2" distance="5"/></fe-weapons>"#,
        )
        .is_err());
        assert!(parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2455" damage="18" intervalticks="2" shoteffect="29"/></fe-weapons>"#,
        )
        .is_err());
        // Out-of-bounded ranges are rejected.
        assert!(parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2455" damage="18" intervalticks="2" distance="6" shoteffect="29"/></fe-weapons>"#,
        )
        .is_err());
    }

    #[test]
    fn attaches_skill_classifications_only_to_declared_adjacent_weapons() {
        let catalog = parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2376" damage="12" intervalticks="2"/></fe-weapons>"#,
        )
        .unwrap()
        .with_adjacent_melee_skills(Some(&BTreeMap::from([
            (2376, PlayerSkill::Sword),
            (2400, PlayerSkill::Axe),
        ])));
        assert_eq!(catalog.adjacent_melee_skill(2376), Some(PlayerSkill::Sword));
        assert_eq!(catalog.adjacent_melee_skill(2400), None);
    }

    #[test]
    fn rejects_scripted_duplicate_and_out_of_range_weapon_declarations() {
        assert!(parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2376" damage="12" intervalticks="2" script="sword.lua"/></fe-weapons>"#,
        )
        .is_err());
        assert!(parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2376" damage="12" intervalticks="2"/><weapon itemid="2376" damage="12" intervalticks="2"/></fe-weapons>"#,
        )
        .is_err());
        assert!(parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2376" damage="0" intervalticks="2"/></fe-weapons>"#,
        )
        .is_err());
        assert!(parse_declarative_weapons_xml(
            br#"<weapons><weapon itemid="2376" damage="12" intervalticks="2"/></weapons>"#,
        )
        .is_err());
    }
}
