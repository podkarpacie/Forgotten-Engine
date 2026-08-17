use super::{ConfigError, EngineConfig};
use forgotten_core::{
    CoreError, PlayerProgressionRules, PlayerSkill, ProgressionMultiplier, VocationId,
    VocationLevelUpGains,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;
use std::fs;

const MAX_VOCATIONS_XML_BYTES: usize = 4 * 1024 * 1024;
const MAX_VOCATIONS_XML_DEPTH: usize = 8;
const MAX_VOCATIONS: usize = 4_096;
const MAX_VOCATION_NAME_BYTES: usize = 128;
const MAX_REGENERATION_SECONDS: u16 = 60 * 60;
const MAX_REGENERATION_AMOUNT: u16 = 65_535;
const MAX_MULTIPLIER_MILLI: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VocationRegeneration {
    pub interval_seconds: u16,
    pub amount: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VocationMultiplier {
    milli: u32,
}

impl VocationMultiplier {
    pub const fn milli(self) -> u32 {
        self.milli
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TfsVocationDefinition {
    pub id: VocationId,
    pub client_id: u16,
    pub name: String,
    pub description: String,
    pub from_vocation: VocationId,
    pub gain_capacity: u16,
    pub gain_health: u16,
    pub gain_mana: u16,
    pub health_regeneration: VocationRegeneration,
    pub mana_regeneration: VocationRegeneration,
    pub soul_regeneration: VocationRegeneration,
    pub magic_level_multiplier: VocationMultiplier,
    pub skill_multipliers: [VocationMultiplier; 7],
}

impl TfsVocationDefinition {
    /// Converts already-validated operator-owned vocation multipliers into the core's deterministic
    /// fixed-point progression inputs. Gameplay gain events and profile-parity claims remain
    /// outside this configuration adapter.
    pub fn progression_rules(&self) -> Result<PlayerProgressionRules, CoreError> {
        let mut skill_multipliers = [ProgressionMultiplier::new(1_000)?; 7];
        for skill in PlayerSkill::ALL {
            skill_multipliers[skill.code() as usize] =
                ProgressionMultiplier::new(self.skill_multipliers[skill.code() as usize].milli())?;
        }
        Ok(PlayerProgressionRules {
            magic_level_multiplier: ProgressionMultiplier::new(
                self.magic_level_multiplier.milli(),
            )?,
            skill_multipliers,
        })
    }

    /// Converts the already bounded `gainhp`, `gainmana`, and `gaincap` metadata into explicit
    /// core inputs. The caller selects the definition for the persisted player vocation; this
    /// adapter neither changes player state nor claims profile-specific client delivery.
    pub const fn level_up_gains(&self) -> VocationLevelUpGains {
        VocationLevelUpGains::new(self.gain_health, self.gain_mana, self.gain_capacity)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TfsVocationRegistry {
    entries: BTreeMap<VocationId, TfsVocationDefinition>,
}

impl TfsVocationRegistry {
    pub fn get(&self, id: VocationId) -> Option<&TfsVocationDefinition> {
        self.entries.get(&id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&VocationId, &TfsVocationDefinition)> {
        self.entries.iter()
    }

    fn insert(&mut self, definition: TfsVocationDefinition) -> Result<(), ConfigError> {
        if self.entries.len() >= MAX_VOCATIONS && !self.entries.contains_key(&definition.id) {
            return Err(invalid("vocation count exceeds configured limit"));
        }
        if self.entries.insert(definition.id, definition).is_some() {
            return Err(invalid("duplicate vocation ID"));
        }
        Ok(())
    }
}

pub fn load_tfs_vocation_registry(
    config: &EngineConfig,
) -> Result<Option<TfsVocationRegistry>, ConfigError> {
    let path = config.content_directory.join("XML").join("vocations.xml");
    if !path.is_file() {
        return Ok(None);
    }
    parse_tfs_vocations_xml(&fs::read(path).map_err(ConfigError::Io)?).map(Some)
}

pub fn parse_tfs_vocations_xml(bytes: &[u8]) -> Result<TfsVocationRegistry, ConfigError> {
    if bytes.len() > MAX_VOCATIONS_XML_BYTES {
        return Err(invalid("vocations.xml exceeds the configured size limit"));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut current = None;
    let mut registry = TfsVocationRegistry::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_VOCATIONS_XML_DEPTH {
                    return Err(invalid(
                        "vocations.xml exceeds the configured nesting depth",
                    ));
                }
                match event.name().as_ref() {
                    b"vocations" if depth == 1 => root_seen = true,
                    b"vocation" if depth == 2 && current.is_none() => {
                        current = Some(parse_vocation(&event)?);
                    }
                    b"skill" if depth == 3 => add_skill_multiplier(&mut current, &event)?,
                    _ => {}
                }
            }
            Event::Empty(event) => match event.name().as_ref() {
                b"vocation" if depth == 1 => {
                    let definition = finish_vocation(parse_vocation(&event)?)?;
                    registry.insert(definition)?;
                }
                b"skill" if depth == 2 => add_skill_multiplier(&mut current, &event)?,
                _ => {}
            },
            Event::End(event) => {
                if event.name().as_ref() == b"vocation" {
                    let definition = current
                        .take()
                        .ok_or_else(|| invalid("vocation closing tag without an open vocation"))?;
                    registry.insert(finish_vocation(definition)?)?;
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed vocations.xml depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 || current.is_some() || registry.is_empty() {
        return Err(invalid(
            "vocations.xml is malformed, empty, or does not have a vocations root",
        ));
    }
    Ok(registry)
}

#[derive(Debug)]
struct PendingVocation {
    definition: TfsVocationDefinition,
    seen_skills: [bool; 7],
}

fn parse_vocation(event: &BytesStart<'_>) -> Result<PendingVocation, ConfigError> {
    let id = VocationId::new(attribute_u16(event, b"id")?);
    let name = attribute_string(event, b"name")?;
    if name.trim().is_empty() || name.len() > MAX_VOCATION_NAME_BYTES {
        return Err(invalid(
            "vocation name is empty or exceeds the configured limit",
        ));
    }
    let health_regeneration = VocationRegeneration {
        interval_seconds: bounded_u16(
            optional_attribute_u16(event, b"gainhpticks")?.unwrap_or_default(),
            "health regeneration interval",
            1,
            MAX_REGENERATION_SECONDS,
        )?,
        amount: bounded_u16(
            optional_attribute_u16(event, b"gainhpamount")?.unwrap_or_default(),
            "health regeneration amount",
            0,
            MAX_REGENERATION_AMOUNT,
        )?,
    };
    let mana_regeneration = VocationRegeneration {
        interval_seconds: bounded_u16(
            optional_attribute_u16(event, b"gainmanaticks")?.unwrap_or_default(),
            "mana regeneration interval",
            1,
            MAX_REGENERATION_SECONDS,
        )?,
        amount: bounded_u16(
            optional_attribute_u16(event, b"gainmanaamount")?.unwrap_or_default(),
            "mana regeneration amount",
            0,
            MAX_REGENERATION_AMOUNT,
        )?,
    };
    let soul_regeneration = VocationRegeneration {
        interval_seconds: bounded_u16(
            optional_attribute_u16(event, b"gainsoulticks")?.unwrap_or_default(),
            "soul regeneration interval",
            1,
            MAX_REGENERATION_SECONDS,
        )?,
        amount: 1,
    };
    Ok(PendingVocation {
        definition: TfsVocationDefinition {
            id,
            client_id: optional_attribute_u16(event, b"clientid")?.unwrap_or_default(),
            name,
            description: optional_attribute_string(event, b"description")?.unwrap_or_default(),
            from_vocation: VocationId::new(
                optional_attribute_u16(event, b"fromvoc")?.unwrap_or(id.value()),
            ),
            gain_capacity: optional_attribute_u16(event, b"gaincap")?.unwrap_or_default(),
            gain_health: optional_attribute_u16(event, b"gainhp")?.unwrap_or_default(),
            gain_mana: optional_attribute_u16(event, b"gainmana")?.unwrap_or_default(),
            health_regeneration,
            mana_regeneration,
            soul_regeneration,
            magic_level_multiplier: parse_multiplier(&attribute_string(event, b"manamultiplier")?)?,
            skill_multipliers: [VocationMultiplier { milli: 1_000 }; 7],
        },
        seen_skills: [false; 7],
    })
}

fn add_skill_multiplier(
    current: &mut Option<PendingVocation>,
    event: &BytesStart<'_>,
) -> Result<(), ConfigError> {
    let current = current
        .as_mut()
        .ok_or_else(|| invalid("skill is outside a vocation"))?;
    let skill_id = attribute_u8(event, b"id")?;
    let skill = PlayerSkill::from_code(skill_id)
        .ok_or_else(|| invalid("vocation skill ID is outside the classic seven-skill range"))?;
    let index = usize::from(skill.code());
    if current.seen_skills[index] {
        return Err(invalid("duplicate vocation skill multiplier"));
    }
    current.definition.skill_multipliers[index] =
        parse_multiplier(&attribute_string(event, b"multiplier")?)?;
    current.seen_skills[index] = true;
    Ok(())
}

fn finish_vocation(pending: PendingVocation) -> Result<TfsVocationDefinition, ConfigError> {
    if pending.seen_skills.iter().any(|seen| !seen) {
        return Err(invalid(
            "every vocation must declare all seven classic skill multipliers",
        ));
    }
    Ok(pending.definition)
}

fn parse_multiplier(value: &str) -> Result<VocationMultiplier, ConfigError> {
    let value = value.trim();
    let (whole, fraction) = value
        .split_once('.')
        .map_or((value, ""), |(whole, fraction)| (whole, fraction));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 3
    {
        return Err(invalid(
            "vocation multiplier must be a non-negative decimal with up to three fractional digits",
        ));
    }
    let whole = whole
        .parse::<u32>()
        .map_err(|_| invalid("vocation multiplier is outside the supported range"))?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u32>()
            .map_err(|_| invalid("vocation multiplier is outside the supported range"))?
            * 10_u32.pow((3 - fraction.len()) as u32)
    };
    let milli = whole
        .checked_mul(1_000)
        .and_then(|whole| whole.checked_add(fraction))
        .filter(|value| *value > 0 && *value <= MAX_MULTIPLIER_MILLI)
        .ok_or_else(|| {
            invalid("vocation multiplier must be greater than zero and within the configured limit")
        })?;
    Ok(VocationMultiplier { milli })
}

fn bounded_u16(value: u16, label: &str, minimum: u16, maximum: u16) -> Result<u16, ConfigError> {
    if !(minimum..=maximum).contains(&value) {
        return Err(invalid(format!("{label} is outside the configured range")));
    }
    Ok(value)
}

fn attribute_string(event: &BytesStart<'_>, name: &[u8]) -> Result<String, ConfigError> {
    event
        .try_get_attribute(name)
        .map_err(|error| invalid(format!("invalid vocations.xml attribute: {error}")))?
        .ok_or_else(|| {
            invalid(format!(
                "missing vocations.xml attribute `{}`",
                String::from_utf8_lossy(name)
            ))
        })?
        .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
        .map_err(xml_error)
        .map(|value| value.into_owned())
}

fn optional_attribute_string(
    event: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, ConfigError> {
    event
        .try_get_attribute(name)
        .map_err(|error| invalid(format!("invalid vocations.xml attribute: {error}")))?
        .map(|attribute| {
            attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
                .map_err(xml_error)
                .map(|value| value.into_owned())
        })
        .transpose()
}

fn attribute_u16(event: &BytesStart<'_>, name: &[u8]) -> Result<u16, ConfigError> {
    attribute_string(event, name)?.parse::<u16>().map_err(|_| {
        invalid(format!(
            "vocations.xml attribute `{}` must be u16",
            String::from_utf8_lossy(name)
        ))
    })
}

fn attribute_u8(event: &BytesStart<'_>, name: &[u8]) -> Result<u8, ConfigError> {
    attribute_string(event, name)?.parse::<u8>().map_err(|_| {
        invalid(format!(
            "vocations.xml attribute `{}` must be u8",
            String::from_utf8_lossy(name)
        ))
    })
}

fn optional_attribute_u16(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<u16>, ConfigError> {
    optional_attribute_string(event, name)?
        .map(|value| {
            value.parse::<u16>().map_err(|_| {
                invalid(format!(
                    "vocations.xml attribute `{}` must be u16",
                    String::from_utf8_lossy(name)
                ))
            })
        })
        .transpose()
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid vocations.xml: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOCATIONS: &[u8] = br#"<?xml version="1.0"?><vocations>
      <vocation id="4" clientid="1" name="Knight" description="a knight" gaincap="25" gainhp="15" gainmana="5" gainhpticks="3" gainhpamount="5" gainmanaticks="6" gainmanaamount="5" manamultiplier="3.0" gainsoulticks="120" fromvoc="4">
        <skill id="0" multiplier="1.1"/><skill id="1" multiplier="1.1"/><skill id="2" multiplier="1.1"/><skill id="3" multiplier="1.1"/><skill id="4" multiplier="1.4"/><skill id="5" multiplier="1.1"/><skill id="6" multiplier="1.1"/>
      </vocation>
    </vocations>"#;

    #[test]
    fn parses_bounded_tfs_vocation_regeneration_and_skill_metadata() {
        let registry = parse_tfs_vocations_xml(VOCATIONS).unwrap();
        let knight = registry.get(VocationId::new(4)).unwrap();
        assert_eq!(knight.name, "Knight");
        assert_eq!(knight.gain_capacity, 25);
        assert_eq!(knight.gain_health, 15);
        assert_eq!(knight.gain_mana, 5);
        assert_eq!(
            knight.level_up_gains(),
            VocationLevelUpGains::new(15, 5, 25)
        );
        assert_eq!(
            knight.health_regeneration,
            VocationRegeneration {
                interval_seconds: 3,
                amount: 5
            }
        );
        assert_eq!(
            knight.mana_regeneration,
            VocationRegeneration {
                interval_seconds: 6,
                amount: 5
            }
        );
        assert_eq!(
            knight.skill_multipliers[PlayerSkill::Distance.code() as usize].milli(),
            1_400
        );
        let rules = knight.progression_rules().unwrap();
        assert_eq!(rules.magic_level_multiplier.milli(), 3_000);
        assert_eq!(
            rules.skill_multipliers[PlayerSkill::Distance.code() as usize].milli(),
            1_400
        );
    }

    #[test]
    fn rejects_missing_or_duplicate_skill_metadata() {
        let source = std::str::from_utf8(VOCATIONS).unwrap();
        let missing = source.replace("<skill id=\"6\" multiplier=\"1.1\"/>", "");
        assert!(parse_tfs_vocations_xml(missing.as_bytes()).is_err());
        let duplicate = source.replace(
            "<skill id=\"6\" multiplier=\"1.1\"/>",
            "<skill id=\"6\" multiplier=\"1.1\"/><skill id=\"6\" multiplier=\"1.1\"/>",
        );
        assert!(parse_tfs_vocations_xml(duplicate.as_bytes()).is_err());
    }
}
