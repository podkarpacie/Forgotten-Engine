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
                    multiplier_milli: multiplier.saturating_mul(1000),
                });
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    stages.sort_by_key(|stage| stage.min_level);
    if !root_seen
        || stages.is_empty()
        || stages
            .windows(2)
            .any(|pair| pair[0].max_level >= pair[1].min_level)
    {
        return Err(ConfigError::InvalidContent(
            "stages.xml is malformed, empty, or overlapping".into(),
        ));
    }
    Ok(ExperienceStages(stages))
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
}
