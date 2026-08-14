//! Bounded packet framing for the FE 1.2.0 Tibia 10.98 compatibility foundation.

pub const MAX_FRAME_SIZE: usize = 8 * 1024;
pub const TARGET_PROTOCOL: &str = "10.98";
pub const FE_RELEASE: &str = "1.2.0";
pub const TFS_REFERENCE: &str = "1.2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityProfile {
    pub fe_release: &'static str,
    pub tfs_reference: &'static str,
    pub tibia_protocol: &'static str,
    pub complete_protocol_emulation: bool,
}

pub const FE_1_2_PROFILE: CompatibilityProfile = CompatibilityProfile {
    fe_release: FE_RELEASE,
    tfs_reference: TFS_REFERENCE,
    tibia_protocol: TARGET_PROTOCOL,
    complete_protocol_emulation: false,
};

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
        assert_eq!(FE_1_2_PROFILE.tfs_reference, "1.2");
        assert_eq!(FE_1_2_PROFILE.tibia_protocol, "10.98");
        assert!(!FE_1_2_PROFILE.complete_protocol_emulation);
    }

    #[test]
    fn rejects_truncated_frame_payloads() {
        assert!(matches!(decode(&[1, 0]), Err(ProtocolError::Truncated)));
    }
}
