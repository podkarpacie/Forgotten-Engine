use crate::ConfigError;
use forgotten_core::{CoreError, ExperienceAwardPolicy, ExperienceAwardStage};
use quick_xml::events::Event;
use quick_xml::Reader;

const MAX_STAGE_BYTES: usize = 1024 * 1024;
const MAX_STAGES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExperienceStage {
    pub min_level: u32,
    pub max_level: u32,
    pub multiplier_milli: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceStages(pub Vec<ExperienceStage>);

impl ExperienceStages {
    /// Converts parsed stage data and a configured global rate into the core's deterministic
    /// award policy. Runtime event sources and client delivery remain outside this adapter.
    pub fn award_policy(&self, experience_rate: u32) -> Result<ExperienceAwardPolicy, CoreError> {
        let stages = self
            .0
            .iter()
            .map(|stage| {
                ExperienceAwardStage::new(stage.min_level, stage.max_level, stage.multiplier_milli)
            })
            .collect::<Result<Vec<_>, _>>()?;
        ExperienceAwardPolicy::new(experience_rate, stages)
    }
}

pub fn parse_tfs_stages_xml(bytes: &[u8]) -> Result<ExperienceStages, ConfigError> {
    if bytes.len() > MAX_STAGE_BYTES {
        return Err(ConfigError::InvalidContent(
            "stages.xml exceeds size limit".into(),
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut stages = Vec::new();
    let mut root_seen = false;
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| ConfigError::InvalidContent(error.to_string()))?
        {
            Event::Start(event) if event.name().as_ref() == b"stages" => root_seen = true,
            Event::Empty(event) if event.name().as_ref() == b"stage" => {
                if stages.len() >= MAX_STAGES {
                    return Err(ConfigError::InvalidContent(
                        "stage count exceeds limit".into(),
                    ));
                }
                let mut min = None;
                let mut max = u32::MAX;
                let mut multiplier = None;
                for attribute in event.attributes().with_checks(false) {
                    let attribute = attribute
                        .map_err(|error| ConfigError::InvalidContent(error.to_string()))?;
                    let value = std::str::from_utf8(attribute.value.as_ref())
                        .map_err(|_| {
                            ConfigError::InvalidContent("stage attribute is not UTF-8".into())
                        })?
                        .parse::<u32>()
                        .map_err(|_| {
                            ConfigError::InvalidContent(
                                "stage attribute is not an unsigned integer".into(),
                            )
                        })?;
                    match attribute.key.as_ref() {
                        b"minlevel" => min = Some(value),
                        b"maxlevel" => max = value,
                        b"multiplier" => multiplier = Some(value),
                        _ => {}
                    }
                }
                let min_level =
                    min.ok_or_else(|| ConfigError::InvalidContent("stage lacks minlevel".into()))?;
                let multiplier = multiplier
                    .ok_or_else(|| ConfigError::InvalidContent("stage lacks multiplier".into()))?;
                if min_level == 0 || max < min_level || multiplier == 0 {
                    return Err(ConfigError::InvalidContent(
                        "stage range or multiplier is invalid".into(),
                    ));
                }
                stages.push(ExperienceStage {
                    min_level,
                    max_level: max,
                    multiplier_milli: stage_multiplier_milli(multiplier)?,
                });
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen {
        return Err(ConfigError::InvalidContent(
            "stages.xml is malformed or does not have a stages root".into(),
        ));
    }
    validate_experience_stages(stages, "stages.xml")
}

/// Parses only the literal table shape documented for TFS `config.lua` `experienceStages`. It
/// deliberately supports neither Lua evaluation nor expressions: an entry may contain only
/// `minlevel`, optional `maxlevel`, and `multiplier` unsigned-integer fields.
pub fn parse_tfs_config_experience_stages_table(
    table: &str,
) -> Result<ExperienceStages, ConfigError> {
    if table.len() > MAX_STAGE_BYTES {
        return Err(ConfigError::InvalidContent(
            "experienceStages table exceeds size limit".into(),
        ));
    }
    let mut reader = ConfigStageTableReader::new(table);
    reader.expect(b'{')?;
    let mut stages = Vec::new();
    loop {
        reader.skip_whitespace();
        if reader.consume(b'}') {
            break;
        }
        if stages.len() >= MAX_STAGES {
            return Err(ConfigError::InvalidContent(
                "experienceStages entry count exceeds limit".into(),
            ));
        }
        reader.expect(b'{')?;
        let mut min_level = None;
        let mut max_level = None;
        let mut multiplier = None;
        loop {
            reader.skip_whitespace();
            if reader.consume(b'}') {
                break;
            }
            let key = reader.identifier()?;
            reader.expect(b'=')?;
            let value = reader.unsigned_integer()?;
            match key {
                "minlevel" if min_level.replace(value).is_none() => {}
                "maxlevel" if max_level.replace(value).is_none() => {}
                "multiplier" if multiplier.replace(value).is_none() => {}
                "minlevel" | "maxlevel" | "multiplier" => {
                    return Err(ConfigError::InvalidContent(format!(
                        "experienceStages entry repeats `{key}`"
                    )));
                }
                _ => {
                    return Err(ConfigError::InvalidContent(format!(
                        "experienceStages entry has unsupported field `{key}`"
                    )));
                }
            }
            reader.skip_whitespace();
            if !reader.consume(b',') && !reader.next_is(b'}') {
                return Err(reader.error("expected a comma or entry terminator"));
            }
        }
        let min_level = min_level.ok_or_else(|| {
            ConfigError::InvalidContent("experienceStages entry lacks minlevel".into())
        })?;
        let multiplier = multiplier.ok_or_else(|| {
            ConfigError::InvalidContent("experienceStages entry lacks multiplier".into())
        })?;
        let max_level = max_level.unwrap_or(u32::MAX);
        if min_level == 0 || max_level < min_level || multiplier == 0 {
            return Err(ConfigError::InvalidContent(
                "experienceStages range or multiplier is invalid".into(),
            ));
        }
        stages.push(ExperienceStage {
            min_level,
            max_level,
            multiplier_milli: stage_multiplier_milli(multiplier)?,
        });
        reader.skip_whitespace();
        if !reader.consume(b',') && !reader.next_is(b'}') {
            return Err(reader.error("expected a comma or table terminator"));
        }
    }
    reader.skip_whitespace();
    if !reader.is_finished() {
        return Err(reader.error("unexpected trailing input"));
    }
    validate_experience_stages(stages, "experienceStages table")
}

fn stage_multiplier_milli(multiplier: u32) -> Result<u32, ConfigError> {
    multiplier
        .checked_mul(1_000)
        .filter(|value| *value <= forgotten_core::MAX_PROGRESSION_MULTIPLIER_MILLI)
        .ok_or_else(|| {
            ConfigError::InvalidContent(
                "stage multiplier exceeds FE fixed-point compatibility limit".into(),
            )
        })
}

fn validate_experience_stages(
    mut stages: Vec<ExperienceStage>,
    source: &str,
) -> Result<ExperienceStages, ConfigError> {
    stages.sort_by_key(|stage| stage.min_level);
    if stages.is_empty()
        || stages
            .windows(2)
            .any(|pair| pair[0].max_level >= pair[1].min_level)
    {
        return Err(ConfigError::InvalidContent(format!(
            "{source} is empty or has overlapping level ranges"
        )));
    }
    for stage in &stages {
        ExperienceAwardStage::new(stage.min_level, stage.max_level, stage.multiplier_milli)
            .map_err(|_| {
                ConfigError::InvalidContent(format!(
                    "{source} contains an unsupported stage multiplier"
                ))
            })?;
    }
    Ok(ExperienceStages(stages))
}

struct ConfigStageTableReader<'a> {
    source: &'a [u8],
    offset: usize,
}

impl<'a> ConfigStageTableReader<'a> {
    const fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            offset: 0,
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .source
            .get(self.offset)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.offset += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        self.skip_whitespace();
        if self.source.get(self.offset) == Some(&expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn next_is(&mut self, expected: u8) -> bool {
        self.skip_whitespace();
        self.source.get(self.offset) == Some(&expected)
    }

    fn expect(&mut self, expected: u8) -> Result<(), ConfigError> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected `{}`", expected as char)))
        }
    }

    fn identifier(&mut self) -> Result<&'a str, ConfigError> {
        self.skip_whitespace();
        let start = self.offset;
        while self
            .source
            .get(self.offset)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.offset += 1;
        }
        if start == self.offset {
            return Err(self.error("expected a field name"));
        }
        std::str::from_utf8(&self.source[start..self.offset])
            .map_err(|_| self.error("field name is not valid UTF-8"))
    }

    fn unsigned_integer(&mut self) -> Result<u32, ConfigError> {
        self.skip_whitespace();
        let start = self.offset;
        while self.source.get(self.offset).is_some_and(u8::is_ascii_digit) {
            self.offset += 1;
        }
        if start == self.offset {
            return Err(self.error("expected an unsigned integer literal"));
        }
        std::str::from_utf8(&self.source[start..self.offset])
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| self.error("integer literal is outside the supported range"))
    }

    fn is_finished(&self) -> bool {
        self.offset == self.source.len()
    }

    fn error(&self, message: &str) -> ConfigError {
        ConfigError::InvalidContent(format!(
            "experienceStages table at byte {}: {message}",
            self.offset
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ordered_non_overlapping_tfs_stages() {
        let stages = parse_tfs_stages_xml(
            br#"<stages><stage minlevel="9" multiplier="3"/><stage minlevel="1" maxlevel="8" multiplier="7"/></stages>"#,
        )
        .unwrap();
        assert_eq!(stages.0.len(), 2);
        assert_eq!(stages.0[0].min_level, 1);
        assert_eq!(stages.0[0].multiplier_milli, 7_000);
        assert_eq!(stages.0[1].min_level, 9);
        assert_eq!(stages.0[1].max_level, u32::MAX);
        let policy = stages.award_policy(5).unwrap();
        assert_eq!(policy.award_for(1, 10), 350);
        assert_eq!(policy.award_for(9, 10), 150);
    }

    #[test]
    fn rejects_overlapping_or_missing_stage_fields() {
        assert!(parse_tfs_stages_xml(
            br#"<stages><stage minlevel="1" maxlevel="9" multiplier="2"/><stage minlevel="9" multiplier="1"/></stages>"#,
        )
        .is_err());
        assert!(parse_tfs_stages_xml(br#"<stages><stage minlevel="1"/></stages>"#).is_err());
    }

    #[test]
    fn parses_documented_legacy_config_stage_table_without_lua_execution() {
        let stages = parse_tfs_config_experience_stages_table(
            r#"{
                { minlevel = 9, multiplier = 3 },
                { minlevel = 1, maxlevel = 8, multiplier = 7 },
            }"#,
        )
        .unwrap();
        assert_eq!(stages.0.len(), 2);
        assert_eq!(stages.0[0].min_level, 1);
        assert_eq!(stages.0[0].max_level, 8);
        assert_eq!(stages.0[0].multiplier_milli, 7_000);
        assert_eq!(stages.0[1].min_level, 9);
        assert_eq!(stages.0[1].max_level, u32::MAX);
        assert_eq!(stages.award_policy(1).unwrap().award_for(9, 10), 30);
    }

    #[test]
    fn rejects_unsafe_or_invalid_legacy_config_stage_tables() {
        for table in [
            "{ { minlevel = 1, multiplier = 0 } }",
            "{ { minlevel = 1, maxlevel = 9, multiplier = 2 }, { minlevel = 9, multiplier = 1 } }",
            "{ { minlevel = 1, multiplier = 1, script = 1 } }",
            "{ { minlevel = 1, multiplier = rateExp } }",
            "{ { minlevel = 1, multiplier = 101 } }",
            "{ { minlevel = 1, minlevel = 2, multiplier = 1 } }",
        ] {
            assert!(parse_tfs_config_experience_stages_table(table).is_err());
        }
    }
}
