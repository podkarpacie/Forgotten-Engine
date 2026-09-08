//! Legacy 7.4 login envelope, game-session challenge/bootstrap, and session error/ready codecs.

use super::*;

/// `0x01`, client version, then a raw 128-byte RSA block. The decrypted block has a zero marker,
/// four XTEA words, then bounded account and password strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legacy74LoginEnvelope {
    pub client_version: u16,
    pub encrypted_block: [u8; LEGACY_RSA_BLOCK_SIZE],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legacy74LoginRequest {
    pub client_version: u16,
    pub xtea_key: XteaKey,
    pub account_name: String,
    pub password: String,
}
pub fn decode_legacy_74_envelope(frame: &Frame) -> Result<Legacy74LoginEnvelope, ProtocolError> {
    if frame.0.len() != LEGACY_RSA_BLOCK_SIZE + 3 || frame.0[0] != 1 {
        return Err(ProtocolError::InvalidLoginEnvelope);
    }
    let mut encrypted_block = [0; LEGACY_RSA_BLOCK_SIZE];
    encrypted_block.copy_from_slice(&frame.0[3..]);
    Ok(Legacy74LoginEnvelope {
        client_version: u16::from_le_bytes([frame.0[1], frame.0[2]]),
        encrypted_block,
    })
}
pub fn decode_legacy_74_login_plaintext(
    client_version: u16,
    plaintext: &[u8; LEGACY_RSA_BLOCK_SIZE],
) -> Result<Legacy74LoginRequest, ProtocolError> {
    let mut reader = Reader::new(plaintext);
    if reader.byte()? != 0 {
        return Err(ProtocolError::InvalidLoginMarker);
    }
    let xtea_key = [reader.u32()?, reader.u32()?, reader.u32()?, reader.u32()?];
    let account_name = reader.string(MAX_LOGIN_STRING_BYTES)?;
    let password = reader.string(MAX_LOGIN_STRING_BYTES)?;
    if account_name.is_empty() || password.is_empty() {
        return Err(ProtocolError::MissingLoginCredential);
    }
    Ok(Legacy74LoginRequest {
        client_version,
        xtea_key,
        account_name,
        password,
    })
}

pub const LEGACY_74_GAME_CHALLENGE_OPCODE: u8 = 0x1f;
pub const LEGACY_74_GAME_SESSION_REQUEST_OPCODE: u8 = 0x02;
pub const LEGACY_74_GAME_SESSION_READY_OPCODE: u8 = 0xf0;
pub const LEGACY_74_GAME_SESSION_ERROR_OPCODE: u8 = 0xf1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Legacy74GameChallenge {
    pub timestamp: u32,
    pub random: u8,
}

pub fn generate_legacy_74_game_challenge() -> Legacy74GameChallenge {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32;
    Legacy74GameChallenge {
        timestamp,
        random: OsRng.next_u32() as u8,
    }
}

pub fn encode_legacy_74_game_challenge(challenge: Legacy74GameChallenge) -> Frame {
    let mut writer = Writer::default();
    writer.byte(LEGACY_74_GAME_CHALLENGE_OPCODE);
    writer.u32(challenge.timestamp);
    writer.byte(challenge.random);
    Frame(writer.finish())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legacy74GameSessionRequest {
    pub client_version: u16,
    pub account_name: String,
    pub password: String,
    pub character_name: String,
    pub challenge: Legacy74GameChallenge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Legacy74GameSessionState {
    ChallengeIssued(Legacy74GameChallenge),
    Authenticated {
        account_id: i64,
        character_name: String,
    },
    FeatureGated {
        character_name: String,
    },
    CustomClientNegotiated {
        character_name: String,
    },
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legacy74GameSessionBootstrap {
    pub xtea_key: XteaKey,
    pub request: Legacy74GameSessionRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legacy74GameSessionEnvelope {
    pub client_version: u16,
    pub encrypted_block: [u8; LEGACY_RSA_BLOCK_SIZE],
}

pub fn decode_legacy_74_game_session_envelope(
    frame: &Frame,
) -> Result<Legacy74GameSessionEnvelope, ProtocolError> {
    if frame.0.len() != LEGACY_RSA_BLOCK_SIZE + 3
        || frame.0[0] != LEGACY_74_GAME_SESSION_REQUEST_OPCODE
    {
        return Err(ProtocolError::InvalidGameSessionRequest);
    }
    let mut encrypted_block = [0; LEGACY_RSA_BLOCK_SIZE];
    encrypted_block.copy_from_slice(&frame.0[3..]);
    Ok(Legacy74GameSessionEnvelope {
        client_version: u16::from_le_bytes([frame.0[1], frame.0[2]]),
        encrypted_block,
    })
}

pub fn decode_legacy_74_game_session_bootstrap_plaintext(
    client_version: u16,
    plaintext: &[u8; LEGACY_RSA_BLOCK_SIZE],
    expected_challenge: Legacy74GameChallenge,
) -> Result<Legacy74GameSessionBootstrap, ProtocolError> {
    let mut reader = Reader::new(plaintext);
    if reader.byte()? != 0 {
        return Err(ProtocolError::InvalidLoginMarker);
    }
    let xtea_key = [reader.u32()?, reader.u32()?, reader.u32()?, reader.u32()?];
    let request = Legacy74GameSessionRequest {
        client_version,
        account_name: reader.string(MAX_LOGIN_STRING_BYTES)?,
        password: reader.string(MAX_LOGIN_STRING_BYTES)?,
        character_name: reader.string(MAX_LOGIN_STRING_BYTES)?,
        challenge: Legacy74GameChallenge {
            timestamp: reader.u32()?,
            random: reader.byte()?,
        },
    };
    if request.client_version != 740 {
        return Err(ProtocolError::UnsupportedGameSessionVersion(
            request.client_version,
        ));
    }
    if request.account_name.is_empty()
        || request.password.is_empty()
        || request.character_name.is_empty()
        || request.challenge != expected_challenge
    {
        return Err(ProtocolError::InvalidGameSessionRequest);
    }
    Ok(Legacy74GameSessionBootstrap { xtea_key, request })
}

pub fn encode_legacy_74_game_session_bootstrap_for_harness(
    key: &LegacyRsaPrivateKey,
    bootstrap: &Legacy74GameSessionBootstrap,
) -> Result<Frame, ProtocolError> {
    let mut writer = Writer::default();
    writer.byte(0);
    for word in bootstrap.xtea_key {
        writer.u32(word);
    }
    writer.string(&bootstrap.request.account_name);
    writer.string(&bootstrap.request.password);
    writer.string(&bootstrap.request.character_name);
    writer.u32(bootstrap.request.challenge.timestamp);
    writer.byte(bootstrap.request.challenge.random);
    let body = writer.finish();
    if body.len() > LEGACY_RSA_BLOCK_SIZE {
        return Err(ProtocolError::InvalidLength(body.len()));
    }
    let mut plaintext = [0; LEGACY_RSA_BLOCK_SIZE];
    plaintext[..body.len()].copy_from_slice(&body);
    let encrypted = key.encrypt_raw_block_for_harness(&plaintext)?;
    let mut envelope = vec![LEGACY_74_GAME_SESSION_REQUEST_OPCODE];
    envelope.extend_from_slice(&bootstrap.request.client_version.to_le_bytes());
    envelope.extend_from_slice(&encrypted);
    Ok(Frame(envelope))
}

pub fn encode_legacy_74_game_session_request(
    request: &Legacy74GameSessionRequest,
    key: XteaKey,
) -> Result<Frame, ProtocolError> {
    let mut writer = Writer::default();
    writer.byte(LEGACY_74_GAME_SESSION_REQUEST_OPCODE);
    writer.u16(request.client_version);
    writer.string(&request.account_name);
    writer.string(&request.password);
    writer.string(&request.character_name);
    writer.u32(request.challenge.timestamp);
    writer.byte(request.challenge.random);
    Ok(Frame(xtea_encrypt_packet(&writer.finish(), key)?))
}

pub fn decode_legacy_74_game_session_request(
    frame: &Frame,
    key: XteaKey,
    expected_challenge: Legacy74GameChallenge,
) -> Result<Legacy74GameSessionRequest, ProtocolError> {
    let decrypted = xtea_decrypt_packet(&frame.0, key)?;
    let mut reader = Reader::new(&decrypted);
    if reader.byte()? != LEGACY_74_GAME_SESSION_REQUEST_OPCODE {
        return Err(ProtocolError::InvalidGameSessionRequest);
    }
    let request = Legacy74GameSessionRequest {
        client_version: reader.u16()?,
        account_name: reader.string(MAX_LOGIN_STRING_BYTES)?,
        password: reader.string(MAX_LOGIN_STRING_BYTES)?,
        character_name: reader.string(MAX_LOGIN_STRING_BYTES)?,
        challenge: Legacy74GameChallenge {
            timestamp: reader.u32()?,
            random: reader.byte()?,
        },
    };
    if !reader.done()
        || request.account_name.is_empty()
        || request.password.is_empty()
        || request.character_name.is_empty()
    {
        return Err(ProtocolError::InvalidGameSessionRequest);
    }
    if request.client_version != 740 {
        return Err(ProtocolError::UnsupportedGameSessionVersion(
            request.client_version,
        ));
    }
    if request.challenge != expected_challenge {
        return Err(ProtocolError::ChallengeMismatch);
    }
    Ok(request)
}

pub fn encode_legacy_74_game_session_ready(character_name: &str) -> Frame {
    let mut writer = Writer::default();
    writer.byte(LEGACY_74_GAME_SESSION_READY_OPCODE);
    writer.string(character_name);
    writer.string("Game session authenticated; world/map simulation is feature-gated.");
    writer.byte(0);
    Frame(writer.finish())
}

pub fn encode_legacy_74_game_session_error(message: &str) -> Frame {
    let mut writer = Writer::default();
    writer.byte(LEGACY_74_GAME_SESSION_ERROR_OPCODE);
    writer.string(message);
    Frame(writer.finish())
}
