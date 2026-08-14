//! Bounded transport contracts for Forgotten Engine profiles.
//!
//! The legacy 7.4 types below are a tested foundation, not a claim of official-client support.

use rand::{rngs::OsRng, RngCore};
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey};
use rsa::pkcs8::DecodePrivateKey;
use rsa::traits::{PrivateKeyParts, PublicKeyParts};
use rsa::{BigUint, RsaPrivateKey};
use std::{
    fs,
    net::IpAddr,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_FRAME_SIZE: usize = 8 * 1024;
pub const FE_1_2_RELEASE: &str = "1.2.0";
pub const FE_8_0_RELEASE: &str = "8.0.0";
pub const FE_7_4_RELEASE: &str = "7.4.0";
pub const LEGACY_RSA_BLOCK_SIZE: usize = 128;
pub const MAX_LOGIN_STRING_BYTES: usize = 128;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusRequestFlags(u16);
impl StatusRequestFlags {
    pub const BASIC: Self = Self(1);
    pub const MISC: Self = Self(4);
    pub const PLAYERS: Self = Self(8);
    pub const MAP: Self = Self(16);
    pub const EXTENDED_PLAYERS: Self = Self(32);
    pub const PLAYER_STATUS: Self = Self(64);
    pub const SOFTWARE: Self = Self(128);
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }
    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusRequest {
    XmlInfo,
    Binary {
        flags: StatusRequestFlags,
        player_name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub server_name: String,
    pub bind_ip: IpAddr,
    pub status_port: u16,
    pub uptime_seconds: u64,
    pub players_online: u32,
    pub max_players: u32,
    pub players_peak: u32,
    pub map_name: String,
    pub profile: CompatibilityProfile,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPlayer {
    pub name: String,
    pub level: u32,
}

pub fn decode_status_request(frame: &Frame) -> Result<StatusRequest, ProtocolError> {
    let mut reader = Reader::new(&frame.0);
    match reader.byte()? {
        0xff if reader.string(MAX_LOGIN_STRING_BYTES)? == "info" && reader.done() => {
            Ok(StatusRequest::XmlInfo)
        }
        0x01 => {
            let flags = StatusRequestFlags::from_bits(reader.u16()?);
            let player_name = if flags.contains(StatusRequestFlags::PLAYER_STATUS) {
                Some(reader.string(MAX_LOGIN_STRING_BYTES)?)
            } else {
                None
            };
            if reader.done() {
                Ok(StatusRequest::Binary { flags, player_name })
            } else {
                Err(ProtocolError::InvalidStatusRequest)
            }
        }
        _ => Err(ProtocolError::InvalidStatusRequest),
    }
}

pub fn encode_status_xml(snapshot: &StatusSnapshot) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?><tsqp version=\"1.0\"><serverinfo uptime=\"{}\" ip=\"{}\" servername=\"{}\" port=\"{}\" server=\"Forgotten Engine\" version=\"{}\" client=\"{}\"/><players online=\"{}\" max=\"{}\" peak=\"{}\"/><map name=\"{}\" author=\"Original Forgotten Engine content\" width=\"0\" height=\"0\"/></tsqp>", snapshot.uptime_seconds, xml(&snapshot.bind_ip.to_string()), xml(&snapshot.server_name), snapshot.status_port, snapshot.profile.fe_release, snapshot.profile.tibia_protocol, snapshot.players_online, snapshot.max_players, snapshot.players_peak, xml(&snapshot.map_name)).into_bytes()
}

pub fn encode_status_binary(
    snapshot: &StatusSnapshot,
    flags: StatusRequestFlags,
    players: &[StatusPlayer],
    player_is_online: bool,
) -> Frame {
    let mut writer = Writer::default();
    if flags.contains(StatusRequestFlags::BASIC) {
        writer.byte(0x10);
        writer.string(&snapshot.server_name);
        writer.string(&snapshot.bind_ip.to_string());
        writer.string(&snapshot.status_port.to_string());
    }
    if flags.contains(StatusRequestFlags::MISC) {
        writer.byte(0x12);
        writer.string("N/A");
        writer.string("N/A");
        writer.string("N/A");
        writer.u64(snapshot.uptime_seconds);
    }
    if flags.contains(StatusRequestFlags::PLAYERS) {
        writer.byte(0x20);
        writer.u32(snapshot.players_online);
        writer.u32(snapshot.max_players);
        writer.u32(snapshot.players_peak);
    }
    if flags.contains(StatusRequestFlags::MAP) {
        writer.byte(0x30);
        writer.string(&snapshot.map_name);
        writer.string("Original Forgotten Engine content");
        writer.u16(0);
        writer.u16(0);
    }
    if flags.contains(StatusRequestFlags::EXTENDED_PLAYERS) {
        writer.byte(0x21);
        writer.u32(players.len() as u32);
        for player in players {
            writer.string(&player.name);
            writer.u32(player.level);
        }
    }
    if flags.contains(StatusRequestFlags::PLAYER_STATUS) {
        writer.byte(0x22);
        writer.byte(u8::from(player_is_online));
    }
    if flags.contains(StatusRequestFlags::SOFTWARE) {
        writer.byte(0x23);
        writer.string("Forgotten Engine");
        writer.string(snapshot.profile.fe_release);
        writer.string(snapshot.profile.tibia_protocol);
    }
    Frame(writer.finish())
}

#[derive(Debug)]
pub struct LegacyRsaPrivateKey(RsaPrivateKey);
impl LegacyRsaPrivateKey {
    pub fn generate() -> Result<Self, ProtocolError> {
        RsaPrivateKey::new(&mut OsRng, LEGACY_RSA_BLOCK_SIZE * 8)
            .map(Self)
            .map_err(|_| ProtocolError::InvalidPrivateKey)
    }

    pub fn load_pem(path: impl AsRef<Path>) -> Result<Self, ProtocolError> {
        let pem = fs::read_to_string(path).map_err(ProtocolError::KeyIo)?;
        let key = RsaPrivateKey::from_pkcs1_pem(&pem)
            .or_else(|_| RsaPrivateKey::from_pkcs8_pem(&pem))
            .map_err(|_| ProtocolError::InvalidPrivateKey)?;
        if key.n().bits() != LEGACY_RSA_BLOCK_SIZE * 8 {
            return Err(ProtocolError::UnsupportedRsaKeySize(key.n().bits()));
        }
        Ok(Self(key))
    }

    pub fn write_pem(&self, path: impl AsRef<Path>) -> Result<(), ProtocolError> {
        let pem = self
            .0
            .to_pkcs1_pem(Default::default())
            .map_err(|_| ProtocolError::InvalidPrivateKey)?;
        fs::write(path, pem.as_bytes()).map_err(ProtocolError::KeyIo)
    }
    pub fn decrypt_raw_block(
        &self,
        encrypted: &[u8],
    ) -> Result<[u8; LEGACY_RSA_BLOCK_SIZE], ProtocolError> {
        if encrypted.len() != LEGACY_RSA_BLOCK_SIZE {
            return Err(ProtocolError::InvalidRsaBlockLength(encrypted.len()));
        }
        let ciphertext = BigUint::from_bytes_be(encrypted);
        if ciphertext >= *self.0.n() {
            return Err(ProtocolError::InvalidRsaCiphertext);
        }
        let raw = ciphertext.modpow(self.0.d(), self.0.n()).to_bytes_be();
        if raw.len() > LEGACY_RSA_BLOCK_SIZE {
            return Err(ProtocolError::InvalidRsaCiphertext);
        }
        let mut plaintext = [0; LEGACY_RSA_BLOCK_SIZE];
        plaintext[LEGACY_RSA_BLOCK_SIZE - raw.len()..].copy_from_slice(&raw);
        Ok(plaintext)
    }

    /// Produces a fixed raw RSA block only for FE's local interoperability harnesses.
    pub fn encrypt_raw_block_for_harness(
        &self,
        plaintext: &[u8; LEGACY_RSA_BLOCK_SIZE],
    ) -> Result<[u8; LEGACY_RSA_BLOCK_SIZE], ProtocolError> {
        let encrypted = BigUint::from_bytes_be(plaintext)
            .modpow(self.0.e(), self.0.n())
            .to_bytes_be();
        if encrypted.len() > LEGACY_RSA_BLOCK_SIZE {
            return Err(ProtocolError::InvalidRsaCiphertext);
        }
        let mut padded = [0; LEGACY_RSA_BLOCK_SIZE];
        padded[LEGACY_RSA_BLOCK_SIZE - encrypted.len()..].copy_from_slice(&encrypted);
        Ok(padded)
    }
}

pub type XteaKey = [u32; 4];
pub fn xtea_encrypt_in_place(payload: &mut [u8], key: XteaKey) -> Result<(), ProtocolError> {
    xtea(payload, key, false)
}
pub fn xtea_decrypt_in_place(payload: &mut [u8], key: XteaKey) -> Result<(), ProtocolError> {
    xtea(payload, key, true)
}
fn xtea(payload: &mut [u8], key: XteaKey, decrypt: bool) -> Result<(), ProtocolError> {
    if payload.is_empty() || payload.len() % 8 != 0 {
        return Err(ProtocolError::InvalidXteaLength(payload.len()));
    }
    for block in payload.chunks_exact_mut(8) {
        let (mut left, mut right) = (
            u32::from_le_bytes(block[..4].try_into().expect("8-byte block")),
            u32::from_le_bytes(block[4..].try_into().expect("8-byte block")),
        );
        const DELTA: u32 = 0x9e37_79b9;
        let mut sum = if decrypt { DELTA.wrapping_mul(32) } else { 0 };
        for _ in 0..32 {
            if decrypt {
                right = right.wrapping_sub(
                    ((left << 4 ^ left >> 5).wrapping_add(left))
                        ^ sum.wrapping_add(key[((sum >> 11) & 3) as usize]),
                );
                sum = sum.wrapping_sub(DELTA);
                left = left.wrapping_sub(
                    ((right << 4 ^ right >> 5).wrapping_add(right))
                        ^ sum.wrapping_add(key[(sum & 3) as usize]),
                );
            } else {
                left = left.wrapping_add(
                    ((right << 4 ^ right >> 5).wrapping_add(right))
                        ^ sum.wrapping_add(key[(sum & 3) as usize]),
                );
                sum = sum.wrapping_add(DELTA);
                right = right.wrapping_add(
                    ((left << 4 ^ left >> 5).wrapping_add(left))
                        ^ sum.wrapping_add(key[((sum >> 11) & 3) as usize]),
                );
            }
        }
        block[..4].copy_from_slice(&left.to_le_bytes());
        block[4..].copy_from_slice(&right.to_le_bytes());
    }
    Ok(())
}
pub fn xtea_encrypt_packet(payload: &[u8], key: XteaKey) -> Result<Vec<u8>, ProtocolError> {
    let mut packet = (payload.len() as u16).to_le_bytes().to_vec();
    packet.extend_from_slice(payload);
    while packet.len() % 8 != 0 {
        packet.push(0);
    }
    xtea_encrypt_in_place(&mut packet, key)?;
    Ok(packet)
}
pub fn xtea_decrypt_packet(payload: &[u8], key: XteaKey) -> Result<Vec<u8>, ProtocolError> {
    let mut packet = payload.to_vec();
    xtea_decrypt_in_place(&mut packet, key)?;
    if packet.len() < 2 {
        return Err(ProtocolError::Truncated);
    }
    let length = u16::from_le_bytes([packet[0], packet[1]]) as usize;
    if length + 2 > packet.len() {
        return Err(ProtocolError::LengthMismatch {
            declared: length,
            actual: packet.len() - 2,
        });
    }
    Ok(packet[2..length + 2].to_vec())
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterListEntry {
    pub name: String,
    pub world_name: String,
    pub address: IpAddr,
    pub port: u16,
}
pub fn encode_legacy_74_character_list(
    motd: &str,
    entries: &[CharacterListEntry],
) -> Result<Frame, ProtocolError> {
    let mut writer = Writer::default();
    writer.byte(0x64);
    writer.string(motd);
    writer.byte(u8::try_from(entries.len()).map_err(|_| ProtocolError::TooManyCharacters)?);
    for entry in entries {
        writer.string(&entry.name);
        writer.string(&entry.world_name);
        let IpAddr::V4(ip) = entry.address else {
            return Err(ProtocolError::UnsupportedAddressFamily);
        };
        writer.bytes(&ip.octets());
        writer.u16(entry.port);
    }
    writer.u16(0);
    Ok(Frame(writer.finish()))
}
pub fn encode_login_error(message: &str) -> Frame {
    let mut writer = Writer::default();
    writer.byte(0x0a);
    writer.string(message);
    Frame(writer.finish())
}

#[derive(Debug)]
pub enum ProtocolError {
    InvalidLength(usize),
    LengthMismatch { declared: usize, actual: usize },
    Truncated,
    InvalidStatusRequest,
    KeyIo(std::io::Error),
    InvalidPrivateKey,
    UnsupportedRsaKeySize(usize),
    InvalidRsaBlockLength(usize),
    InvalidRsaCiphertext,
    InvalidXteaLength(usize),
    InvalidLoginEnvelope,
    InvalidLoginMarker,
    MissingLoginCredential,
    InvalidGameSessionRequest,
    UnsupportedGameSessionVersion(u16),
    ChallengeMismatch,
    TooManyCharacters,
    UnsupportedAddressFamily,
    StringTooLong(usize),
    InvalidString,
}
impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyIo(error) => write!(f, "private-key I/O error: {error}"),
            other => write!(f, "{other:?}"),
        }
    }
}
impl std::error::Error for ProtocolError {}

#[derive(Default)]
struct Writer(Vec<u8>);
impl Writer {
    fn byte(&mut self, value: u8) {
        self.0.push(value);
    }
    fn bytes(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }
    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }
    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
    fn string(&mut self, value: &str) {
        let bytes = value.as_bytes();
        self.u16(bytes.len().min(u16::MAX as usize) as u16);
        self.bytes(&bytes[..bytes.len().min(u16::MAX as usize)]);
    }
    fn finish(self) -> Vec<u8> {
        self.0
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn byte(&mut self) -> Result<u8, ProtocolError> {
        let value = *self
            .bytes
            .get(self.position)
            .ok_or(ProtocolError::Truncated)?;
        self.position += 1;
        Ok(value)
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(ProtocolError::Truncated)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(ProtocolError::Truncated)?;
        self.position = end;
        Ok(value)
    }
    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn string(&mut self, max: usize) -> Result<String, ProtocolError> {
        let length = self.u16()? as usize;
        if length > max {
            return Err(ProtocolError::StringTooLong(length));
        }
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| ProtocolError::InvalidString)
    }
    fn done(&self) -> bool {
        self.position == self.bytes.len()
    }
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> StatusSnapshot {
        StatusSnapshot {
            server_name: "Forgotten & Engine".into(),
            bind_ip: "127.0.0.1".parse().unwrap(),
            status_port: 7171,
            uptime_seconds: 42,
            players_online: 1,
            max_players: 100,
            players_peak: 3,
            map_name: "forgotten".into(),
            profile: FE_7_4_PROFILE,
        }
    }
    #[test]
    fn frame_round_trip() {
        let frame = Frame(vec![1, 2, 3]);
        assert_eq!(decode(&encode(&frame).unwrap()).unwrap(), frame);
    }
    #[test]
    fn profiles_remain_explicit_and_limited() {
        assert_eq!(profile_by_id("fe-8.0"), Some(FE_8_0_PROFILE));
        assert!(!profile_by_id("fe-1.2").unwrap().complete_protocol_emulation);
    }
    #[test]
    fn status_contract_decodes_and_escapes_xml() {
        assert!(matches!(
            decode_status_request(&Frame(vec![1, 9, 0])),
            Ok(StatusRequest::Binary { .. })
        ));
        assert!(String::from_utf8(encode_status_xml(&snapshot()))
            .unwrap()
            .contains("&amp;"));
        assert_eq!(
            encode_status_binary(&snapshot(), StatusRequestFlags::BASIC, &[], false).0[0],
            0x10
        );
    }
    #[test]
    fn xtea_round_trip_restores_inner_packet() {
        let encrypted = xtea_encrypt_packet(b"status-payload", [1, 2, 3, 4]).unwrap();
        assert_eq!(
            xtea_decrypt_packet(&encrypted, [1, 2, 3, 4]).unwrap(),
            b"status-payload"
        );
    }
    #[test]
    fn login_plaintext_contract_is_bounded() {
        let mut bytes = [0; LEGACY_RSA_BLOCK_SIZE];
        bytes[1..5].copy_from_slice(&1_u32.to_le_bytes());
        bytes[5..9].copy_from_slice(&2_u32.to_le_bytes());
        bytes[9..13].copy_from_slice(&3_u32.to_le_bytes());
        bytes[13..17].copy_from_slice(&4_u32.to_le_bytes());
        bytes[17..19].copy_from_slice(&5_u16.to_le_bytes());
        bytes[19..24].copy_from_slice(b"admin");
        bytes[24..26].copy_from_slice(&6_u16.to_le_bytes());
        bytes[26..32].copy_from_slice(b"secret");
        let login = decode_legacy_74_login_plaintext(740, &bytes).unwrap();
        assert_eq!(login.xtea_key, [1, 2, 3, 4]);
        assert_eq!(login.account_name, "admin");
    }
    #[test]
    fn character_list_requires_ipv4() {
        let response = encode_legacy_74_character_list(
            "Welcome",
            &[CharacterListEntry {
                name: "Knight".into(),
                world_name: "Forgotten".into(),
                address: "127.0.0.1".parse().unwrap(),
                port: 7172,
            }],
        )
        .unwrap();
        assert_eq!(response.0[0], 0x64);
    }

    #[test]
    fn locally_generated_key_round_trips_a_raw_harness_block() {
        let key = LegacyRsaPrivateKey::generate().unwrap();
        let mut plaintext = [0; LEGACY_RSA_BLOCK_SIZE];
        plaintext[127] = 1;
        let encrypted = key.encrypt_raw_block_for_harness(&plaintext).unwrap();
        assert_eq!(key.decrypt_raw_block(&encrypted).unwrap(), plaintext);
    }

    #[test]
    fn encrypted_game_session_request_validates_its_challenge() {
        let challenge = Legacy74GameChallenge {
            timestamp: 1_700_000_000,
            random: 42,
        };
        let request = Legacy74GameSessionRequest {
            client_version: 740,
            account_name: "admin".into(),
            password: "secret".into(),
            character_name: "Knight".into(),
            challenge,
        };
        let frame = encode_legacy_74_game_session_request(&request, [1, 2, 3, 4]).unwrap();
        assert_eq!(
            decode_legacy_74_game_session_request(&frame, [1, 2, 3, 4], challenge).unwrap(),
            request
        );
        assert!(matches!(
            decode_legacy_74_game_session_request(
                &frame,
                [1, 2, 3, 4],
                Legacy74GameChallenge {
                    timestamp: challenge.timestamp,
                    random: 43,
                }
            ),
            Err(ProtocolError::ChallengeMismatch)
        ));
    }

    #[test]
    fn game_session_ready_response_declares_the_world_feature_gate() {
        let response = encode_legacy_74_game_session_ready("Knight");
        assert_eq!(response.0[0], LEGACY_74_GAME_SESSION_READY_OPCODE);
        assert!(response
            .0
            .windows(b"feature-gated".len())
            .any(|window| window == b"feature-gated"));
    }

    #[test]
    fn raw_rsa_game_session_bootstrap_binds_the_expected_challenge() {
        let key = LegacyRsaPrivateKey::generate().unwrap();
        let challenge = Legacy74GameChallenge {
            timestamp: 1_700_000_000,
            random: 7,
        };
        let bootstrap = Legacy74GameSessionBootstrap {
            xtea_key: [1, 2, 3, 4],
            request: Legacy74GameSessionRequest {
                client_version: 740,
                account_name: "admin".into(),
                password: "secret".into(),
                character_name: "Knight".into(),
                challenge,
            },
        };
        let envelope =
            encode_legacy_74_game_session_bootstrap_for_harness(&key, &bootstrap).unwrap();
        let envelope = decode_legacy_74_game_session_envelope(&envelope).unwrap();
        let plaintext = key.decrypt_raw_block(&envelope.encrypted_block).unwrap();
        assert_eq!(
            decode_legacy_74_game_session_bootstrap_plaintext(
                envelope.client_version,
                &plaintext,
                challenge
            )
            .unwrap(),
            bootstrap
        );
    }
}
