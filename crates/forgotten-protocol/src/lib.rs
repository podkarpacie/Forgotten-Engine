//! Bounded packet framing for versioned Forgotten Engine compatibility profiles.

pub const MAX_FRAME_SIZE: usize = 8 * 1024;
pub const FE_1_2_RELEASE: &str = "1.2.0";
pub const FE_8_0_RELEASE: &str = "8.0.0";
pub const FE_7_4_RELEASE: &str = "7.4.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityProfile {
    pub id: &'static str,
    pub fe_release: &'static str,
    pub compatibility_reference: &'static str,
    pub tibia_protocol: &'static str,
    pub complete_protocol_emulation: bool,
}

pub const FE_1_2_PROFILE: CompatibilityProfile = CompatibilityProfile {
    id: "fe-1.2",
    fe_release: FE_1_2_RELEASE,
    compatibility_reference: "TFS 1.2",
    tibia_protocol: "10.98",
    complete_protocol_emulation: false,
};

pub const FE_8_0_PROFILE: CompatibilityProfile = CompatibilityProfile {
    id: "fe-8.0",
    fe_release: FE_8_0_RELEASE,
    compatibility_reference: "Tibia 8.0 protocol",
    tibia_protocol: "8.0",
    complete_protocol_emulation: false,
};

pub const FE_7_4_PROFILE: CompatibilityProfile = CompatibilityProfile {
    id: "fe-7.4",
    fe_release: FE_7_4_RELEASE,
    compatibility_reference: "Tibia 7.4 protocol",
    tibia_protocol: "7.4",
    complete_protocol_emulation: false,
};

pub const COMPATIBILITY_PROFILES: [CompatibilityProfile; 3] =
    [FE_7_4_PROFILE, FE_8_0_PROFILE, FE_1_2_PROFILE];

pub fn profile_by_id(id: &str) -> Option<CompatibilityProfile> {
    COMPATIBILITY_PROFILES
        .iter()
        .copied()
        .find(|profile| profile.id == id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame(pub Vec<u8>);

pub fn encode(frame: &Frame) -> Result<Vec<u8>, ProtocolError> {
    if frame.0.is_empty() || frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    let length =
        u16::try_from(frame.0.len()).map_err(|_| ProtocolError::InvalidLength(frame.0.len()))?;
    let mut bytes = length.to_le_bytes().to_vec();
    bytes.extend_from_slice(&frame.0);
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Frame, ProtocolError> {
    if bytes.len() < 3 {
        return Err(ProtocolError::Truncated);
    }
    let declared = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
    if declared == 0 || declared > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(declared));
    }
    if bytes.len() != declared + 2 {
        return Err(ProtocolError::LengthMismatch {
            declared,
            actual: bytes.len() - 2,
        });
    }
    Ok(Frame(bytes[2..].to_vec()))
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidLength(usize),
    LengthMismatch { declared: usize, actual: usize },
    Truncated,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip() {
        let payload = Frame(vec![0x10, 0x20, 0x30]);
        assert_eq!(decode(&encode(&payload).unwrap()), Ok(payload));
    }

    #[test]
    fn rejects_malformed_length() {
        assert!(matches!(
            decode(&[0, 0, 1]),
            Err(ProtocolError::InvalidLength(0))
        ));
    }

    #[test]
    fn exposes_an_explicit_and_limited_tfs_1_2_profile() {
        assert_eq!(FE_1_2_PROFILE.fe_release, "1.2.0");
        assert_eq!(FE_1_2_PROFILE.compatibility_reference, "TFS 1.2");
        assert_eq!(FE_1_2_PROFILE.tibia_protocol, "10.98");
        assert!(!profile_by_id("fe-1.2").unwrap().complete_protocol_emulation);
    }

    #[test]
    fn exposes_a_separate_direct_tibia_8_profile() {
        assert_eq!(FE_8_0_PROFILE.fe_release, "8.0.0");
        assert_eq!(FE_8_0_PROFILE.compatibility_reference, "Tibia 8.0 protocol");
        assert_eq!(FE_8_0_PROFILE.tibia_protocol, "8.0");
        assert_eq!(profile_by_id("fe-8.0"), Some(FE_8_0_PROFILE));
        assert_eq!(profile_by_id("unknown"), None);
    }

    #[test]
    fn exposes_a_separate_direct_tibia_7_4_profile() {
        assert_eq!(FE_7_4_PROFILE.fe_release, "7.4.0");
        assert_eq!(FE_7_4_PROFILE.compatibility_reference, "Tibia 7.4 protocol");
        assert_eq!(FE_7_4_PROFILE.tibia_protocol, "7.4");
        assert_eq!(profile_by_id("fe-7.4"), Some(FE_7_4_PROFILE));
    }

    #[test]
    fn rejects_truncated_frame_payloads() {
        assert!(matches!(decode(&[1, 0]), Err(ProtocolError::Truncated)));
    }
}
