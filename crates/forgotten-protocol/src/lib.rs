//! Bounded transport contracts for Forgotten Engine profiles.
//!
//! The legacy 7.4 types below are a tested foundation, not a claim of official-client support.

use forgotten_core::{
    CardinalDirection, EmptyWorldViewport, EquipmentSlot, FeTfsStaticEntity,
    FeTfsStaticSpawnCollection, PlayerSkills, Position, WorldMap,
};
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
    /// FE-specific operator metrics request. Classic clients never send this magic, so the
    /// existing XML and binary responses stay byte-identical for them.
    Metrics,
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

/// Encodes the classic quest-line response: the requested quest ID, one bounded mission count,
/// then per mission a name and description pair. Unknown or not-started quests encode an empty
/// mission list so the client window opens without inventing content.
pub fn encode_native_otclient_quest_line(
    profile: &NativeOtClientProfile,
    quest_id: u16,
    missions: &[(String, String)],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() || missions.len() > u8::MAX as usize {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    for (name, description) in missions {
        if name.is_empty()
            || name.len() > MAX_LOGIN_STRING_BYTES
            || description.is_empty()
            || description.len() > MAX_LOGIN_STRING_BYTES
        {
            return Err(ProtocolError::InvalidLength(name.len()));
        }
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_QUEST_LINE);
    writer.u16(quest_id);
    writer.byte(missions.len() as u8);
    for (name, description) in missions {
        writer.string(name);
        writer.string(description);
    }
    Ok(Frame(writer.finish()))
}

pub fn decode_status_request(frame: &Frame) -> Result<StatusRequest, ProtocolError> {
    let mut reader = Reader::new(&frame.0);
    match reader.byte()? {
        0xff => {
            let tag = reader.string(MAX_LOGIN_STRING_BYTES)?;
            if !reader.done() {
                return Err(ProtocolError::InvalidStatusRequest);
            }
            match tag.as_str() {
                "info" => Ok(StatusRequest::XmlInfo),
                "fe-metrics" => Ok(StatusRequest::Metrics),
                _ => Err(ProtocolError::InvalidStatusRequest),
            }
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

/// Encodes the FE-specific operator metrics document. Field order is stable so operators can
/// scrape it; values come from one authoritative database read plus process-local clocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMetrics {
    pub uptime_seconds: u64,
    pub registered_accounts: u32,
    pub registered_characters: u32,
    pub schema_version: i64,
    pub players_online: u32,
    pub players_online_cap: u32,
}

pub fn encode_status_metrics(metrics: &StatusMetrics) -> Vec<u8> {
    format!(
        "{{\"uptime_seconds\":{},\"registered_accounts\":{},\"registered_characters\":{},\"schema_version\":{},\"players_online\":{},\"players_online_cap\":{}}}\n",
        metrics.uptime_seconds,
        metrics.registered_accounts,
        metrics.registered_characters,
        metrics.schema_version,
        metrics.players_online,
        metrics.players_online_cap
    )
    .into_bytes()
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

/// Maximum plaintext bytes accepted by the bounded FE 8.0 transport envelope. The two-byte
/// encrypted-packet length must remain within the outer FE frame budget; padding is applied only
/// after this limit is checked.
pub const FE_8_0_XTEA_TRANSPORT_MAX_PLAINTEXT_BYTES: usize = MAX_FRAME_SIZE - 2;

/// Encodes an opaque outbound XTEA envelope for the explicitly classified FE 8.0 transport
/// foundation. This is intentionally a transport primitive only: it does not accept client input,
/// decrypt packets, parse login or game records, start a listener, or expose key material.
pub fn encode_fe_8_0_xtea_transport_envelope(
    profile: &NativeOtClientProfile,
    plaintext: &Frame,
    key: XteaKey,
) -> Result<Frame, ProtocolError> {
    if profile.foundation() != NativeOtClientFoundation::Classic800RequiresRsaXtea
        || !profile.login_packet_encryption
        || plaintext.0.is_empty()
        || plaintext.0.len() > FE_8_0_XTEA_TRANSPORT_MAX_PLAINTEXT_BYTES
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let encrypted = xtea_encrypt_packet(&plaintext.0, key)?;
    if encrypted.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(encrypted.len()));
    }
    Ok(Frame(encrypted))
}

/// The minimal encrypted-transport context that future parser-backed FE 8.0 work may consume.
/// It contains no account, character, password, or other login field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fe80RsaXteaBootstrap {
    pub xtea_key: XteaKey,
}

/// Extracts only the RSA-protected marker and four XTEA words from an already decrypted fixed-size
/// block. This function does not perform RSA decryption, accept a network frame, parse credentials
/// or login fields, or validate any protocol-800 packet layout beyond this generic bootstrap prefix.
pub fn decode_fe_8_0_rsa_xtea_bootstrap(
    plaintext: &[u8; LEGACY_RSA_BLOCK_SIZE],
) -> Result<Fe80RsaXteaBootstrap, ProtocolError> {
    let mut reader = Reader::new(plaintext);
    if reader.byte()? != 0 {
        return Err(ProtocolError::InvalidLoginMarker);
    }
    Ok(Fe80RsaXteaBootstrap {
        xtea_key: [reader.u32()?, reader.u32()?, reader.u32()?, reader.u32()?],
    })
}

/// Produces a fixed bootstrap-prefix block only for FE protocol regressions and local harnesses.
/// It is not a network encoder and it does not append credentials or other login fields.
pub fn encode_fe_8_0_rsa_xtea_bootstrap_for_harness(
    bootstrap: Fe80RsaXteaBootstrap,
) -> [u8; LEGACY_RSA_BLOCK_SIZE] {
    let mut plaintext = [0; LEGACY_RSA_BLOCK_SIZE];
    plaintext[0] = 0;
    for (index, word) in bootstrap.xtea_key.into_iter().enumerate() {
        let start = 1 + index * 4;
        plaintext[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }
    plaintext
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

/// OTClient-oriented extended-opcode transport. A custom OTClient module must explicitly opt in;
/// this is not a general Tibia-client compatibility claim.
pub const FE_OTCLIENT_EXTENDED_OPCODE: u8 = 0x32;
pub const FE_OTCLIENT_CAPABILITY_SUBOPCODE: u8 = 0xf0;
pub const FE_OTCLIENT_WORLD_SUBOPCODE: u8 = 0xf1;
pub const FE_OTCLIENT_CAPABILITY_ACK: &str = "fe.otclient.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtClientEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialWorldSnapshot {
    pub character_name: String,
    pub start_x: u16,
    pub start_y: u16,
    pub start_z: u8,
    pub endpoint: OtClientEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyWorldMovementAck {
    pub tick: u64,
    pub from: Position,
    pub to: Position,
}

pub fn encode_fe_otclient_capability_offer(endpoint: &OtClientEndpoint) -> Frame {
    let offer = format!(
        "fe.capabilities.v1;session=challenge-rsa-xtea;world=empty-gated;endpoint={}:{}",
        endpoint.host, endpoint.port
    );
    encode_fe_otclient_extended(FE_OTCLIENT_CAPABILITY_SUBOPCODE, offer.as_bytes())
}

pub fn encode_fe_otclient_capability_ack_for_harness() -> Frame {
    encode_fe_otclient_extended(
        FE_OTCLIENT_CAPABILITY_SUBOPCODE,
        FE_OTCLIENT_CAPABILITY_ACK.as_bytes(),
    )
}

pub fn decode_fe_otclient_capability_ack(frame: &Frame) -> Result<(), ProtocolError> {
    let (subopcode, payload) = decode_fe_otclient_extended(frame)?;
    if subopcode == FE_OTCLIENT_CAPABILITY_SUBOPCODE
        && payload == FE_OTCLIENT_CAPABILITY_ACK.as_bytes()
    {
        Ok(())
    } else {
        Err(ProtocolError::InvalidOtClientCapabilityAck)
    }
}

pub fn encode_fe_otclient_initial_world(snapshot: &InitialWorldSnapshot) -> Frame {
    let payload = format!(
        "fe.world.v1;character={};position={},{},{};endpoint={}:{};world=empty-gated",
        snapshot.character_name,
        snapshot.start_x,
        snapshot.start_y,
        snapshot.start_z,
        snapshot.endpoint.host,
        snapshot.endpoint.port
    );
    encode_fe_otclient_extended(FE_OTCLIENT_CAPABILITY_SUBOPCODE, payload.as_bytes())
}

pub fn encode_fe_otclient_empty_viewport(viewport: &EmptyWorldViewport) -> Frame {
    let payload = format!(
        "fe.viewport.v1;tick={};center={},{},{};manifest={};radius={},{};world=empty",
        viewport.tick,
        viewport.center.x,
        viewport.center.y,
        viewport.center.z,
        viewport.manifest.identifier,
        viewport.manifest.viewport_radius_x,
        viewport.manifest.viewport_radius_y
    );
    encode_fe_otclient_extended(FE_OTCLIENT_WORLD_SUBOPCODE, payload.as_bytes())
}

pub fn encode_fe_otclient_world_tick(tick: u64) -> Frame {
    encode_fe_otclient_extended(
        FE_OTCLIENT_WORLD_SUBOPCODE,
        format!("fe.tick.v1;tick={tick}").as_bytes(),
    )
}

pub fn encode_fe_otclient_movement_ack(ack: &EmptyWorldMovementAck) -> Frame {
    let payload = format!(
        "fe.move.ack.v1;tick={};from={},{},{};to={},{},{};world=empty",
        ack.tick, ack.from.x, ack.from.y, ack.from.z, ack.to.x, ack.to.y, ack.to.z
    );
    encode_fe_otclient_extended(FE_OTCLIENT_WORLD_SUBOPCODE, payload.as_bytes())
}

pub fn encode_fe_otclient_move_request_for_harness(direction: CardinalDirection) -> Frame {
    encode_fe_otclient_extended(
        FE_OTCLIENT_WORLD_SUBOPCODE,
        format!("fe.move.v1;direction={}", direction_name(direction)).as_bytes(),
    )
}

pub fn decode_fe_otclient_move_request(frame: &Frame) -> Result<CardinalDirection, ProtocolError> {
    let (subopcode, payload) = decode_fe_otclient_extended(frame)?;
    if subopcode != FE_OTCLIENT_WORLD_SUBOPCODE {
        return Err(ProtocolError::InvalidOtClientMessage);
    }
    let payload =
        std::str::from_utf8(&payload).map_err(|_| ProtocolError::InvalidOtClientMessage)?;
    match payload {
        "fe.move.v1;direction=north" => Ok(CardinalDirection::North),
        "fe.move.v1;direction=east" => Ok(CardinalDirection::East),
        "fe.move.v1;direction=south" => Ok(CardinalDirection::South),
        "fe.move.v1;direction=west" => Ok(CardinalDirection::West),
        _ => Err(ProtocolError::InvalidOtClientMessage),
    }
}

fn direction_name(direction: CardinalDirection) -> &'static str {
    match direction {
        CardinalDirection::North => "north",
        CardinalDirection::East => "east",
        CardinalDirection::South => "south",
        CardinalDirection::West => "west",
    }
}

fn encode_fe_otclient_extended(subopcode: u8, payload: &[u8]) -> Frame {
    let mut writer = Writer::default();
    writer.byte(FE_OTCLIENT_EXTENDED_OPCODE);
    writer.byte(subopcode);
    let payload = String::from_utf8_lossy(payload);
    writer.string(&payload);
    Frame(writer.finish())
}

fn decode_fe_otclient_extended(frame: &Frame) -> Result<(u8, Vec<u8>), ProtocolError> {
    let mut reader = Reader::new(&frame.0);
    if reader.byte()? != FE_OTCLIENT_EXTENDED_OPCODE {
        return Err(ProtocolError::InvalidOtClientCapabilityAck);
    }
    let subopcode = reader.byte()?;
    let payload = reader.string(MAX_FRAME_SIZE)?.into_bytes();
    if !reader.done() {
        return Err(ProtocolError::InvalidOtClientCapabilityAck);
    }
    Ok((subopcode, payload))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterListEntry {
    pub name: String,
    pub world_name: String,
    pub address: IpAddr,
    pub port: u16,
}

pub const NATIVE_OTCLIENT_ENTER_ACCOUNT: u8 = 0x01;
pub const NATIVE_OTCLIENT_PENDING_GAME: u8 = 0x0a;
pub const NATIVE_OTCLIENT_LOGIN_ERROR: u8 = 0x0a;
pub const NATIVE_OTCLIENT_LOGIN_CHARACTER_LIST: u8 = 0x64;
pub const NATIVE_OTCLIENT_GAME_LOGIN_ERROR: u8 = 0x14;
pub const NATIVE_OTCLIENT_GAME_LOGIN_STATE: u8 = 0x0a;
pub const NATIVE_OTCLIENT_GAME_DEATH: u8 = 0x28;
pub const NATIVE_OTCLIENT_GAME_FULL_MAP: u8 = 0x64;
pub const NATIVE_OTCLIENT_GAME_MOVE_CREATURE: u8 = 0x6d;
pub const NATIVE_OTCLIENT_GAME_OPEN_CONTAINER: u8 = 0x6e;
pub const NATIVE_OTCLIENT_GAME_SET_INVENTORY: u8 = 0x78;
pub const NATIVE_OTCLIENT_GAME_DELETE_INVENTORY: u8 = 0x79;
pub const NATIVE_OTCLIENT_GAME_PING_BACK: u8 = 0x1d;
pub const NATIVE_OTCLIENT_GAME_PING: u8 = 0x1e;
pub const NATIVE_OTCLIENT_GAME_PLAYER_STATS: u8 = 0xa0;
pub const NATIVE_OTCLIENT_GAME_PLAYER_SKILLS: u8 = 0xa1;
pub const NATIVE_OTCLIENT_GAME_PLAYER_STATE: u8 = 0xa2;
pub const NATIVE_OTCLIENT_GAME_CLEAR_TARGET: u8 = 0xa3;
pub const NATIVE_OTCLIENT_GAME_PLAYER_MODES: u8 = 0xa7;
pub const NATIVE_OTCLIENT_GAME_TALK: u8 = 0xaa;
pub const NATIVE_OTCLIENT_GAME_CLOSE_CONTAINER: u8 = 0x6f;
pub const NATIVE_OTCLIENT_GAME_CREATURE_HEALTH: u8 = 0x8c;
pub const NATIVE_OTCLIENT_GAME_CREATURE_OUTFIT: u8 = 0x8e;
pub const NATIVE_OTCLIENT_GAME_CREATURE_PARTY: u8 = 0x91;
pub const NATIVE_OTCLIENT_GAME_EDIT_TEXT: u8 = 0x96;
pub const NATIVE_OTCLIENT_GAME_TEXT_MESSAGE: u8 = 0xb4;
pub const NATIVE_OTCLIENT_MESSAGE_STATUS_DEFAULT: u8 = 0x15;
pub const NATIVE_OTCLIENT_MESSAGE_SAY: u8 = 0x01;
/// Classic server-talk mode for a whispered message delivered only to nearby listeners.
pub const NATIVE_OTCLIENT_MESSAGE_WHISPER: u8 = 0x02;
/// Classic server-talk mode for a yelled message delivered to a wider same-floor audience.
pub const NATIVE_OTCLIENT_MESSAGE_YELL: u8 = 0x03;
pub const NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT: u8 = 0xc8;
pub const NATIVE_OTCLIENT_GAME_VIP_ADD: u8 = 0xd2;
pub const NATIVE_OTCLIENT_GAME_VIP_STATE: u8 = 0xd3;
pub const NATIVE_OTCLIENT_GAME_VIP_LOGOUT: u8 = 0xd4;
pub const NATIVE_OTCLIENT_GAME_CANCEL_WALK: u8 = 0xb5;
pub const NATIVE_OTCLIENT_ENTER_GAME: u8 = 0x0f;
pub const NATIVE_OTCLIENT_LEAVE_GAME: u8 = 0x14;
pub const NATIVE_OTCLIENT_CLIENT_PING: u8 = 0x1d;
pub const NATIVE_OTCLIENT_CLIENT_PING_BACK: u8 = 0x1e;
pub const NATIVE_OTCLIENT_CLIENT_AUTO_WALK: u8 = 0x64;
pub const NATIVE_OTCLIENT_CLIENT_WALK_NORTH: u8 = 0x65;
pub const NATIVE_OTCLIENT_CLIENT_WALK_EAST: u8 = 0x66;
pub const NATIVE_OTCLIENT_CLIENT_WALK_SOUTH: u8 = 0x67;
pub const NATIVE_OTCLIENT_CLIENT_WALK_WEST: u8 = 0x68;
pub const NATIVE_OTCLIENT_CLIENT_STOP: u8 = 0x69;
pub const NATIVE_OTCLIENT_CLIENT_THROW_ITEM: u8 = 0x78;
pub const NATIVE_OTCLIENT_CLIENT_WALK_NORTH_EAST: u8 = 0x6a;
pub const NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_EAST: u8 = 0x6b;
pub const NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_WEST: u8 = 0x6c;
pub const NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST: u8 = 0x6d;
pub const NATIVE_OTCLIENT_CLIENT_TURN_NORTH: u8 = 0x6f;
pub const NATIVE_OTCLIENT_CLIENT_TURN_EAST: u8 = 0x70;
pub const NATIVE_OTCLIENT_CLIENT_TURN_SOUTH: u8 = 0x71;
pub const NATIVE_OTCLIENT_CLIENT_TURN_WEST: u8 = 0x72;
pub const NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES: u8 = 0xa0;
pub const NATIVE_OTCLIENT_CLIENT_SELECT_TARGET: u8 = 0xa1;
pub const NATIVE_OTCLIENT_CLIENT_SELECT_FOLLOW: u8 = 0xa2;
pub const NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY: u8 = 0xa3;
pub const NATIVE_OTCLIENT_CLIENT_JOIN_PARTY: u8 = 0xa4;
pub const NATIVE_OTCLIENT_CLIENT_REVOKE_PARTY_INVITATION: u8 = 0xa5;
pub const NATIVE_OTCLIENT_CLIENT_PASS_PARTY_LEADERSHIP: u8 = 0xa6;
pub const NATIVE_OTCLIENT_CLIENT_LEAVE_PARTY: u8 = 0xa7;
pub const NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE: u8 = 0xa8;
pub const NATIVE_OTCLIENT_CLIENT_CANCEL_ATTACK_AND_FOLLOW: u8 = 0xbe;
pub const NATIVE_OTCLIENT_CLIENT_TALK: u8 = 0x96;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS: u8 = 0x97;
pub const NATIVE_OTCLIENT_CLIENT_JOIN_CHANNEL: u8 = 0x98;
pub const NATIVE_OTCLIENT_CLIENT_LEAVE_CHANNEL: u8 = 0x99;
pub const NATIVE_OTCLIENT_CLIENT_USE_ITEM: u8 = 0x82;
pub const NATIVE_OTCLIENT_CLIENT_USE_ITEM_EX: u8 = 0x83;
pub const NATIVE_OTCLIENT_CLIENT_USE_ITEM_ON_CREATURE: u8 = 0x84;
pub const NATIVE_OTCLIENT_CLIENT_ROTATE_ITEM: u8 = 0x85;
pub const NATIVE_OTCLIENT_CLIENT_CLOSE_CONTAINER: u8 = 0x87;
pub const NATIVE_OTCLIENT_CLIENT_UP_ARROW_CONTAINER: u8 = 0x88;
pub const NATIVE_OTCLIENT_CLIENT_UPDATE_CONTAINER: u8 = 0xca;
pub const NATIVE_OTCLIENT_CLIENT_LOOK_MAP: u8 = 0x8c;
pub const NATIVE_OTCLIENT_CLIENT_LOOK_CREATURE: u8 = 0x8d;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT: u8 = 0xd2;
pub const NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT: u8 = 0xd3;
pub const NATIVE_OTCLIENT_CLIENT_ADD_VIP: u8 = 0xdc;
pub const NATIVE_OTCLIENT_CLIENT_REMOVE_VIP: u8 = 0xdd;
pub const NATIVE_OTCLIENT_CLIENT_EDIT_VIP: u8 = 0xde;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG: u8 = 0xf0;
pub const NATIVE_OTCLIENT_GAME_QUEST_LOG: u8 = 0xf0;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LINE: u8 = 0xf1;
pub const NATIVE_OTCLIENT_GAME_QUEST_LINE: u8 = 0xf1;
pub const NATIVE_OTCLIENT_GAME_CHANNELS: u8 = 0xab;
pub const NATIVE_OTCLIENT_GAME_OPEN_CHANNEL: u8 = 0xac;
pub const NATIVE_OTCLIENT_MAX_IGNORED_INTERACTION_BYTES: usize = 512;
pub const NATIVE_OTCLIENT_UNKNOWN_CREATURE: u16 = 0x0061;
pub const NATIVE_OTCLIENT_MAPPED_CREATURE: u16 = 0xffff;
pub const NATIVE_OTCLIENT_TILE_END: u16 = 0xff00;
pub const NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH: usize = 18;
pub const NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT: usize = 14;
pub const NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS: usize = 8;
pub const NATIVE_OTCLIENT_MAX_EXTRA_TILE_ITEMS: usize = 8;
pub const NATIVE_OTCLIENT_MAX_STATIC_ENTITIES_PER_VIEWPORT: usize = 32;
pub const NATIVE_OTCLIENT_MAX_SHARED_PLAYERS_PER_VIEWPORT: usize = 32;
pub const NATIVE_OTCLIENT_PLAYER_ID_START: u32 = 0x1000_0000;
pub const NATIVE_OTCLIENT_PLAYER_ID_END: u32 = 0x4000_0000;
pub const NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientProfile {
    pub protocol_version: u16,
    pub numeric_account_ids: bool,
    pub login_packet_encryption: bool,
    pub protocol_checksum: bool,
    pub challenge_on_login: bool,
    pub max_padding_bytes: usize,
}

/// The native transport boundary selected by a protocol profile. This is intentionally narrower
/// than a claim of full protocol compatibility: FE currently has one runnable plain 7.4
/// foundation and recognizes the distinct encrypted transport required by classic 8.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientFoundation {
    PlainClassic740,
    Classic800RequiresRsaXtea,
    Unsupported,
}

impl NativeOtClientFoundation {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainClassic740 => "plain-classic-740",
            Self::Classic800RequiresRsaXtea => "classic-800-requires-rsa-xtea",
            Self::Unsupported => "unsupported",
        }
    }
}

impl NativeOtClientProfile {
    pub fn foundation(&self) -> NativeOtClientFoundation {
        if self.protocol_version == 0
            || !self.numeric_account_ids
            || self.max_padding_bytes > MAX_FRAME_SIZE
        {
            return NativeOtClientFoundation::Unsupported;
        }
        match self.protocol_version {
            740 if !self.login_packet_encryption
                && !self.protocol_checksum
                && !self.challenge_on_login =>
            {
                NativeOtClientFoundation::PlainClassic740
            }
            800 => NativeOtClientFoundation::Classic800RequiresRsaXtea,
            _ => NativeOtClientFoundation::Unsupported,
        }
    }

    pub fn supports_current_native_foundation(&self) -> bool {
        self.foundation() == NativeOtClientFoundation::PlainClassic740
    }

    /// Classic equipment records have been verified only for the selected 740 native profile.
    /// Other configured protocol versions must add their own parser-backed layout before reuse.
    pub fn supports_classic_740_inventory_records(&self) -> bool {
        self.supports_current_native_foundation() && self.protocol_version == 740
    }
}

/// One classic wire item record. `subtype` is present only when the validated operator-supplied
/// item catalog identifies the client thing as stackable, chargeable, fluid, or splash. FE does
/// not infer that field from a server item ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientClassicItemRecord {
    pub client_thing_id: u16,
    pub subtype: Option<u8>,
}

/// Parser-verified classic 740 `OpenContainer` (`0x6e`) payload. Pagination and modern quick-loot
/// fields are deliberately absent because they are feature-gated outside the selected profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientClassicOpenContainer {
    pub container_id: u8,
    pub container_item: NativeOtClientClassicItemRecord,
    pub name: String,
    pub capacity: u8,
    pub has_parent: bool,
    pub items: Vec<NativeOtClientClassicItemRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientLoginRequest {
    pub operating_system: u16,
    pub protocol_version: u16,
    pub dat_signature: u32,
    pub spr_signature: u32,
    pub pic_signature: u32,
    pub account_id: u32,
    pub password: String,
    pub client_tag: String,
    pub client_build: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientGameRequest {
    pub operating_system: u16,
    pub protocol_version: u16,
    pub account_id: u32,
    pub character_name: String,
    pub password: String,
    pub client_tag: String,
    pub client_build: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientPosition {
    pub x: u16,
    pub y: u16,
    pub z: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientEmptyWorldSnapshot {
    pub player_id: u32,
    pub player_name: String,
    pub player_position: NativeOtClientPosition,
    pub player_level: u16,
    pub player_experience: u64,
    pub player_vitals: NativeOtClientPlayerVitals,
    pub player_skills: PlayerSkills,
    pub ground_thing_id: u16,
    pub player_look_type: u8,
    pub player_direction: u8,
    pub player_speed: u16,
    pub server_beat: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientPlayerVitals {
    pub health: u16,
    pub max_health: u16,
    pub mana: u16,
    pub max_mana: u16,
    pub capacity: u16,
    pub magic_level: u8,
}

impl Default for NativeOtClientPlayerVitals {
    fn default() -> Self {
        Self {
            health: 150,
            max_health: 150,
            mana: 50,
            max_mana: 50,
            capacity: 40_000,
            magic_level: 0,
        }
    }
}

/// A bounded classic 7.4 creature appearance. The selected 740 OTCv8 feature profile uses an
/// 8-bit look type followed by four color bytes and has no addon or mount fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientClassicOutfit {
    pub look_type: u8,
    pub head: u8,
    pub body: u8,
    pub legs: u8,
    pub feet: u8,
}

impl NativeOtClientClassicOutfit {
    pub fn from_snapshot(snapshot: &NativeOtClientEmptyWorldSnapshot) -> Self {
        Self {
            look_type: snapshot.player_look_type,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
        }
    }
}

/// The audited classic party shield values FE can represent without shared-experience state.
/// Invitation, leader, and member values are intentionally limited to the TFS-compatible basic
/// display states; shared-experience, blink, and unrelated social shield values remain deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientClassicPartyShield {
    None,
    InvitationFromLeader,
    InvitationToLeader,
    Member,
    Leader,
}

impl NativeOtClientClassicPartyShield {
    const fn classic_value(self) -> u8 {
        match self {
            Self::None => 0,
            Self::InvitationFromLeader => 1,
            Self::InvitationToLeader => 2,
            Self::Member => 3,
            Self::Leader => 4,
        }
    }
}

/// Immutable rendering data for an active player other than the local snapshot owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientVisiblePlayer {
    pub player_id: u32,
    pub name: String,
    pub position: NativeOtClientPosition,
    pub health_percent: u8,
    pub outfit: NativeOtClientClassicOutfit,
    pub direction: u8,
    pub speed: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientCardinalDirection {
    North,
    East,
    South,
    West,
}

impl NativeOtClientCardinalDirection {
    fn from_client_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH => Some(Self::North),
            NATIVE_OTCLIENT_CLIENT_WALK_EAST => Some(Self::East),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH => Some(Self::South),
            NATIVE_OTCLIENT_CLIENT_WALK_WEST => Some(Self::West),
            _ => None,
        }
    }

    fn from_turn_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_TURN_NORTH => Some(Self::North),
            NATIVE_OTCLIENT_CLIENT_TURN_EAST => Some(Self::East),
            NATIVE_OTCLIENT_CLIENT_TURN_SOUTH => Some(Self::South),
            NATIVE_OTCLIENT_CLIENT_TURN_WEST => Some(Self::West),
            _ => None,
        }
    }

    pub fn protocol_direction(self) -> u8 {
        match self {
            Self::North => 0,
            Self::East => 1,
            Self::South => 2,
            Self::West => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientAutoWalkDirection {
    East,
    NorthEast,
    North,
    NorthWest,
    West,
    SouthWest,
    South,
    SouthEast,
}

impl NativeOtClientAutoWalkDirection {
    fn from_direct_diagonal_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH_EAST => Some(Self::NorthEast),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_EAST => Some(Self::SouthEast),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_WEST => Some(Self::SouthWest),
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST => Some(Self::NorthWest),
            _ => None,
        }
    }

    fn from_native_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::East),
            2 => Some(Self::NorthEast),
            3 => Some(Self::North),
            4 => Some(Self::NorthWest),
            5 => Some(Self::West),
            6 => Some(Self::SouthWest),
            7 => Some(Self::South),
            8 => Some(Self::SouthEast),
            _ => None,
        }
    }

    pub fn cardinal_steps(self) -> &'static [NativeOtClientCardinalDirection] {
        match self {
            Self::East => &[NativeOtClientCardinalDirection::East],
            Self::NorthEast => &[
                NativeOtClientCardinalDirection::North,
                NativeOtClientCardinalDirection::East,
            ],
            Self::North => &[NativeOtClientCardinalDirection::North],
            Self::NorthWest => &[
                NativeOtClientCardinalDirection::North,
                NativeOtClientCardinalDirection::West,
            ],
            Self::West => &[NativeOtClientCardinalDirection::West],
            Self::SouthWest => &[
                NativeOtClientCardinalDirection::South,
                NativeOtClientCardinalDirection::West,
            ],
            Self::South => &[NativeOtClientCardinalDirection::South],
            Self::SouthEast => &[
                NativeOtClientCardinalDirection::South,
                NativeOtClientCardinalDirection::East,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeOtClientGameAction {
    EnterGame,
    LeaveGame,
    Ping,
    PingBack,
    Stop,
    AutoWalk(Vec<NativeOtClientAutoWalkDirection>),
    Talk(NativeOtClientTalkRequest),
    ThrowItem {
        source_position: NativeOtClientPosition,
        source_client_thing_id: u16,
        source_stack_position: u8,
        target_position: NativeOtClientPosition,
        count: u8,
    },
    UseItem {
        position: NativeOtClientPosition,
        client_thing_id: u16,
        stack_position: u8,
        index: u8,
    },
    UseItemEx {
        source_position: NativeOtClientPosition,
        source_client_thing_id: u16,
        source_stack_position: u8,
        target_position: NativeOtClientPosition,
        target_client_thing_id: u16,
        target_stack_position: u8,
    },
    UseItemOnCreature {
        source_position: NativeOtClientPosition,
        source_client_thing_id: u16,
        source_stack_position: u8,
        target_creature_id: u32,
    },
    RotateItem {
        position: NativeOtClientPosition,
        client_thing_id: u16,
        stack_position: u8,
    },
    LookMap {
        position: NativeOtClientPosition,
        thing_id: u16,
        stack_position: u8,
    },
    LookCreature {
        creature_id: u32,
    },
    RequestOutfit,
    RequestQuestLog,
    RequestQuestLine {
        quest_id: u16,
    },
    RequestChannels,
    JoinChannel(u16),
    LeaveChannel(u16),
    AddVip(String),
    RemoveVip(u32),
    EditVip {
        target_player_id: u32,
        description: String,
        icon: u32,
        notify: bool,
    },
    ChangeOutfit(NativeOtClientClassicOutfit),
    CloseContainer(u8),
    UpArrowContainer(u8),
    UpdateContainer(u8),
    SelectTarget(u32),
    SelectFollow(u32),
    PartyInvite(u32),
    PartyJoin(u32),
    PartyRevokeInvitation(u32),
    PartyPassLeadership(u32),
    PartyLeave,
    PartySharedExperience(bool),
    CancelAttackAndFollow,
    IgnoredInteraction(u8),
    Turn(NativeOtClientCardinalDirection),
    ChangeFightModes(NativeOtClientFightModeRequest),
    CardinalMove(NativeOtClientCardinalDirection),
    DiagonalMove(NativeOtClientAutoWalkDirection),
}

/// One classic channel-list entry prepared by an authoritative caller. The protocol layer only
/// serializes a bounded list; it does not model joining, membership, messages, or permissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientClassicChannel {
    pub id: u16,
    pub name: String,
}

/// Parsed bounded client talk input. Classic mode `5` retains its recipient string for a separate
/// host-owned private-message route, while configured channels retain their classic channel ID.
/// Authorization and delivery are deliberately outside this protocol boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientTalkRequest {
    pub mode: u8,
    pub channel_id: Option<u16>,
    pub recipient: Option<String>,
    pub message: String,
}

/// Parsed classic fight-mode intent. This is an inbound state request only; its use in combat and
/// client output must be established separately by the host and profile-specific evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientFightModeRequest {
    pub mode: NativeOtClientFightMode,
    pub chase: bool,
    pub secure: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientFightMode {
    Attack,
    Balanced,
    Defense,
}

impl NativeOtClientFightMode {
    const fn from_classic_value(value: u8) -> Self {
        match value {
            1 => Self::Attack,
            2 => Self::Balanced,
            _ => Self::Defense,
        }
    }
}

pub fn decode_native_otclient_login_request(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientLoginRequest, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut reader = Reader::new(&frame.0);
    if reader.byte()? != NATIVE_OTCLIENT_ENTER_ACCOUNT {
        return Err(ProtocolError::InvalidNativeLoginRequest);
    }
    let request = NativeOtClientLoginRequest {
        operating_system: reader.u16()?,
        protocol_version: reader.u16()?,
        dat_signature: reader.u32()?,
        spr_signature: reader.u32()?,
        pic_signature: reader.u32()?,
        account_id: reader.u32()?,
        password: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_tag: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_build: reader.u16()?,
    };
    if request.protocol_version != profile.protocol_version
        || reader.remaining() > profile.max_padding_bytes
    {
        return Err(ProtocolError::InvalidNativeLoginRequest);
    }
    Ok(request)
}

pub fn decode_native_otclient_game_request(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientGameRequest, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut reader = Reader::new(&frame.0);
    if reader.byte()? != NATIVE_OTCLIENT_PENDING_GAME {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    let operating_system = reader.u16()?;
    let protocol_version = reader.u16()?;
    if reader.byte()? != 0 {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    let request = NativeOtClientGameRequest {
        operating_system,
        protocol_version,
        account_id: reader.u32()?,
        character_name: reader.string(MAX_LOGIN_STRING_BYTES)?,
        password: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_tag: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_build: reader.u16()?,
    };
    if request.protocol_version != profile.protocol_version
        || reader.remaining() > profile.max_padding_bytes
    {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    Ok(request)
}

pub fn encode_native_otclient_character_list(
    entries: &[CharacterListEntry],
) -> Result<Frame, ProtocolError> {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_LOGIN_CHARACTER_LIST);
    writer.byte(u8::try_from(entries.len()).map_err(|_| ProtocolError::TooManyCharacters)?);
    for entry in entries {
        writer.string(&entry.name);
        writer.string(&entry.world_name);
        let IpAddr::V4(address) = entry.address else {
            return Err(ProtocolError::UnsupportedAddressFamily);
        };
        writer.bytes(&address.octets());
        writer.u16(entry.port);
    }
    writer.u16(0);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_login_error(message: &str) -> Frame {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_LOGIN_ERROR);
    writer.string(message);
    Frame(writer.finish())
}

pub fn encode_native_otclient_game_login_error(message: &str) -> Frame {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_LOGIN_ERROR);
    writer.string(message);
    Frame(writer.finish())
}

/// Encodes the parser-verified classic 740 status-message record. The message class is the
/// independently verified `MSG_STATUS_DEFAULT` value, which clients render in the default status
/// area and console. Other protocol profiles require their own verified record layout.
pub fn encode_native_otclient_status_message(
    profile: &NativeOtClientProfile,
    message: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if message.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
        return Err(ProtocolError::StringTooLong(message.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TEXT_MESSAGE);
    writer.byte(NATIVE_OTCLIENT_MESSAGE_STATUS_DEFAULT);
    writer.string(message);
    Ok(Frame(writer.finish()))
}

/// Encodes the classic 740 server-talk record for one visible public `Say` message. The local
/// OTCv8 parser maps server mode `1` to `MessageSay` for protocol 740, then reads the speaker
/// position before the text. Channels, private messages, yell/whisper variants, levels, and later
/// statement fields require separate profile-backed layouts.
pub fn encode_native_otclient_public_say(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 8usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(NATIVE_OTCLIENT_MESSAGE_SAY);
    writer.u16(speaker_position.x);
    writer.u16(speaker_position.y);
    writer.byte(speaker_position.z);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 whispered Talk record. The local OTCv8 parser maps server mode `2`
/// to `MessageWhisper` for protocol 740 and reads the speaker position before the text, exactly
/// like the ordinary Say layout.
pub fn encode_native_otclient_whisper(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_positioned_talk(
        profile,
        NATIVE_OTCLIENT_MESSAGE_WHISPER,
        speaker_name,
        speaker_position,
        text,
    )
}

/// Encodes one classic 740 yelled Talk record. The local OTCv8 parser maps server mode `3`
/// to `MessageYell` for protocol 740 and reads the speaker position before the text, exactly
/// like the ordinary Say layout.
pub fn encode_native_otclient_yell(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_positioned_talk(
        profile,
        NATIVE_OTCLIENT_MESSAGE_YELL,
        speaker_name,
        speaker_position,
        text,
    )
}

fn encode_native_otclient_positioned_talk(
    profile: &NativeOtClientProfile,
    mode: u8,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 8usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(mode);
    writer.u16(speaker_position.x);
    writer.u16(speaker_position.y);
    writer.byte(speaker_position.z);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 normal channel Talk record. The selected OTCv8 profile maps server
/// mode `7` to a normal channel message and reads a 16-bit channel ID before the text.
pub fn encode_native_otclient_public_channel_say(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    channel_id: u16,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || channel_id == 0
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 7usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(7);
    writer.u16(channel_id);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 private message received from another player. The local OTCv8 parser
/// maps server mode `4` to `MessagePrivateFrom` and reads no recipient or channel field before
/// the text.
pub fn encode_native_otclient_private_message_from(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 6usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(4);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes the verified classic 740 read-only text-window layout. The selected profile reads the
/// item as a single client item ID, then the maximum length, text, and writer. Writable date and
/// traded fields belong to later protocol features and are deliberately absent.
pub fn encode_native_otclient_read_only_text_window(
    profile: &NativeOtClientProfile,
    window_id: u32,
    client_thing_id: u16,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || client_thing_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    const CLASSIC_740_TEXT_WINDOW_FIXED_BYTES: usize = 13;
    if text.is_empty() || text.len() + CLASSIC_740_TEXT_WINDOW_FIXED_BYTES > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let text_length = text
        .len()
        .try_into()
        .map_err(|_| ProtocolError::StringTooLong(text.len()))?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_EDIT_TEXT);
    writer.u32(window_id);
    writer.u16(client_thing_id);
    writer.u16(text_length);
    writer.string(text);
    writer.string("");
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_game_login_state(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_LOGIN_STATE);
    writer.u32(snapshot.player_id);
    writer.u16(snapshot.server_beat);
    writer.byte(0);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_game_initialization(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    let mut payload = encode_native_otclient_game_login_state(profile, snapshot)?.0;
    payload.extend_from_slice(&encode_native_otclient_empty_world_map(profile, snapshot)?.0);
    payload.extend_from_slice(&encode_native_otclient_player_bootstrap(profile, snapshot)?.0);
    Ok(Frame(payload))
}

pub fn encode_native_otclient_game_initialization_with_map(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_game_initialization_with_map_and_static_spawns(
        profile, snapshot, world_map, None,
    )
}

pub fn encode_native_otclient_game_initialization_with_map_and_static_spawns(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players(
        profile,
        snapshot,
        world_map,
        static_spawns,
        None,
    )
}

pub fn encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
    visible_players: Option<&[NativeOtClientVisiblePlayer]>,
) -> Result<Frame, ProtocolError> {
    let mut payload = encode_native_otclient_game_login_state(profile, snapshot)?.0;
    payload.extend_from_slice(
        &encode_native_otclient_map_viewport_with_static_spawns_and_players(
            profile,
            snapshot,
            world_map,
            static_spawns,
            visible_players,
        )?
        .0,
    );
    payload.extend_from_slice(&encode_native_otclient_player_bootstrap(profile, snapshot)?.0);
    Ok(Frame(payload))
}

/// Encodes the fixed-width classic 7.4 player-stats record. Health, mana, capacity, experience,
/// level, and magic level come from persisted player state. The classic record carries a u16
/// level and trailing soul byte; newer-protocol total-capacity and stamina fields are excluded.
pub fn encode_native_otclient_player_stats(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    let level = snapshot.player_level.max(1);
    let vitals = snapshot.player_vitals;
    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_STATS);
    writer.u16(vitals.health);
    writer.u16(vitals.max_health);
    writer.u16(vitals.capacity);
    writer.u32(snapshot.player_experience.min(i32::MAX as u64) as u32);
    writer.u16(level);
    writer.byte(0);
    writer.u16(vitals.mana);
    writer.u16(vitals.max_mana);
    writer.byte(vitals.magic_level);
    writer.byte(0);
    writer.byte(0);
    Ok(Frame(writer.finish()))
}

/// Encodes the fixed-width classic 7.4 typed player-skills record. The selected protocol has one
/// byte each for a skill level and percentage. Authoritative levels higher than the packet range
/// are saturated only for client presentation; the persisted core value remains unchanged.
pub fn encode_native_otclient_player_skills(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_SKILLS);
    for (_, progress) in snapshot.player_skills.iter() {
        writer.byte(progress.level.min(u16::from(u8::MAX)) as u8);
        writer.byte(progress.percent);
    }
    Ok(Frame(writer.finish()))
}

/// Encodes the fixed-width classic 7.4 local-player records expected immediately after map
/// delivery, including authoritative typed skills supplied by the core runtime.
pub fn encode_native_otclient_player_bootstrap(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    let mut payload = encode_native_otclient_player_stats(profile, snapshot)?.0;
    let mut writer = Writer::default();
    payload.extend_from_slice(&encode_native_otclient_player_skills(profile, snapshot)?.0);

    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_STATE);
    writer.byte(0);
    payload.extend_from_slice(&writer.finish());
    Ok(Frame(payload))
}

/// Encodes classic `SetInventory` (`0x78`) for the parser-verified native 740 layout. The caller
/// must obtain `client_thing_id` and subtype semantics from a validated operator-supplied item
/// catalog; this codec does not infer either property from a server item ID.
pub fn encode_native_otclient_set_inventory(
    profile: &NativeOtClientProfile,
    slot: EquipmentSlot,
    item: NativeOtClientClassicItemRecord,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || item.client_thing_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_SET_INVENTORY);
    writer.byte(slot.code());
    write_native_otclient_classic_item_record(&mut writer, item);
    Ok(Frame(writer.finish()))
}

/// Encodes classic `DeleteInventory` (`0x79`) for a fixed player equipment slot.
pub fn encode_native_otclient_delete_inventory(
    profile: &NativeOtClientProfile,
    slot: EquipmentSlot,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![
        NATIVE_OTCLIENT_GAME_DELETE_INVENTORY,
        slot.code(),
    ]))
}

/// Encodes classic `OpenContainer` (`0x6e`) in the exact non-pagination field order consumed by
/// the selected 740 profile: container ID, item record, name, capacity, parent flag, item count,
/// then item records. It does not enable client requests or runtime container ownership.
pub fn encode_native_otclient_open_container(
    profile: &NativeOtClientProfile,
    container: &NativeOtClientClassicOpenContainer,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records()
        || container.name.is_empty()
        || container.name.len() > MAX_LOGIN_STRING_BYTES
        || container.items.len() > u8::MAX as usize
        || container.container_item.client_thing_id == 0
        || container.items.iter().any(|item| item.client_thing_id == 0)
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_OPEN_CONTAINER);
    writer.byte(container.container_id);
    write_native_otclient_classic_item_record(&mut writer, container.container_item);
    writer.string(&container.name);
    writer.byte(container.capacity);
    writer.byte(u8::from(container.has_parent));
    writer.byte(container.items.len() as u8);
    for item in &container.items {
        write_native_otclient_classic_item_record(&mut writer, *item);
    }
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

pub fn encode_native_otclient_empty_world_map(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_FULL_MAP);
    write_native_otclient_position(&mut writer, snapshot.player_position);
    let asset_free = snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0;

    for z in (0..NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8).rev() {
        for x in 0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH {
            for y in 0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT {
                if snapshot.ground_thing_id != 0 {
                    writer.u16(snapshot.ground_thing_id);
                }
                let is_player_tile = z == snapshot.player_position.z
                    && x == NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1
                    && y == NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1;
                if is_player_tile && !asset_free {
                    write_native_otclient_unknown_player(&mut writer, snapshot);
                }
                writer.u16(NATIVE_OTCLIENT_TILE_END);
            }
        }
    }

    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes an 18Ă—14Ă—8 classic viewport using original operator-supplied map data.
/// A map tile with `ground_thing_id = 0` inherits the profile-configured fallback so a world
/// document can remain portable across lawful client asset sets.
pub fn encode_native_otclient_map_viewport(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_map_viewport_with_static_spawns(profile, snapshot, world_map, None)
}

pub fn encode_native_otclient_map_viewport_with_static_spawns(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_map_viewport_with_static_spawns_and_players(
        profile,
        snapshot,
        world_map,
        static_spawns,
        None,
    )
}

pub fn encode_native_otclient_map_viewport_with_static_spawns_and_players(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
    visible_players: Option<&[NativeOtClientVisiblePlayer]>,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_FULL_MAP);
    write_native_otclient_position(&mut writer, snapshot.player_position);
    let asset_free = snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0;
    let center_x = (NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1) as i16;
    let center_y = (NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1) as i16;
    let viewport_cells = NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH
        * NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT
        * NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS;
    let mut encoded_cells = 0usize;
    let mut static_entity_count = 0usize;
    let mut visible_player_count = 0usize;

    for z in (0..NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8).rev() {
        for x in 0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH {
            for y in 0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT {
                let remaining_cells = viewport_cells.saturating_sub(encoded_cells + 1);
                let position = Position {
                    x: snapshot
                        .player_position
                        .x
                        .saturating_add_signed(x as i16 - center_x),
                    y: snapshot
                        .player_position
                        .y
                        .saturating_add_signed(y as i16 - center_y),
                    z,
                };
                let ground_thing_id = world_map
                    .tile(position)
                    .map(|tile| tile.ground_thing_id)
                    .filter(|ground_thing_id| *ground_thing_id != 0)
                    .unwrap_or(snapshot.ground_thing_id);
                if ground_thing_id != 0
                    && native_map_record_fits_budget(&writer, 2, remaining_cells)
                {
                    writer.u16(ground_thing_id);
                }
                if let Some(items) = world_map.tile_items(position) {
                    for item in items
                        .iter()
                        .skip(1)
                        .take(NATIVE_OTCLIENT_MAX_EXTRA_TILE_ITEMS)
                    {
                        let thing_id = item.client_thing_id.unwrap_or(item.server_id);
                        if thing_id != 0
                            && native_map_record_fits_budget(&writer, 2, remaining_cells)
                        {
                            writer.u16(thing_id);
                        } else if thing_id != 0 {
                            break;
                        }
                    }
                }
                if let Some(static_spawns) = static_spawns {
                    for entity in static_spawns.at(position) {
                        if static_entity_count >= NATIVE_OTCLIENT_MAX_STATIC_ENTITIES_PER_VIEWPORT {
                            break;
                        }
                        let mut entity_record = Writer::default();
                        write_native_otclient_unknown_static_entity(&mut entity_record, entity);
                        if native_map_record_fits_budget(
                            &writer,
                            entity_record.len(),
                            remaining_cells,
                        ) {
                            writer.bytes(&entity_record.finish());
                            static_entity_count += 1;
                        } else {
                            break;
                        }
                    }
                }
                if !asset_free {
                    if let Some(visible_players) = visible_players {
                        for player in visible_players.iter().filter(|player| {
                            player.player_id != snapshot.player_id
                                && player.position.x == position.x
                                && player.position.y == position.y
                                && player.position.z == position.z
                        }) {
                            if visible_player_count
                                >= NATIVE_OTCLIENT_MAX_SHARED_PLAYERS_PER_VIEWPORT
                            {
                                break;
                            }
                            let mut player_record = Writer::default();
                            write_native_otclient_unknown_visible_player(
                                &mut player_record,
                                player,
                            );
                            if native_map_record_fits_budget(
                                &writer,
                                player_record.len(),
                                remaining_cells,
                            ) {
                                writer.bytes(&player_record.finish());
                                visible_player_count += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
                let is_player_tile = z == snapshot.player_position.z
                    && x == NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1
                    && y == NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1;
                if is_player_tile && !asset_free {
                    let mut player_record = Writer::default();
                    write_native_otclient_unknown_player(&mut player_record, snapshot);
                    if native_map_record_fits_budget(&writer, player_record.len(), remaining_cells)
                    {
                        writer.bytes(&player_record.finish());
                    }
                }
                writer.u16(NATIVE_OTCLIENT_TILE_END);
                encoded_cells += 1;
            }
        }
    }

    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes the single newly exposed classic viewport edge after one confirmed cardinal step.
/// It is deliberately limited to map rendering; it does not add movement, combat, or AI behavior.
pub fn encode_native_otclient_map_step_with_static_spawns_and_players(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
    visible_players: Option<&[NativeOtClientVisiblePlayer]>,
    direction: NativeOtClientCardinalDirection,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    let center_x = (NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1) as i16;
    let center_y = (NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1) as i16;
    let asset_free = snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0;
    let mut static_entity_count = 0usize;
    let mut visible_player_count = 0usize;
    let positions = match direction {
        NativeOtClientCardinalDirection::North => (0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH)
            .map(|x| Position {
                x: snapshot
                    .player_position
                    .x
                    .saturating_add_signed(x as i16 - center_x),
                y: snapshot.player_position.y.saturating_add_signed(-center_y),
                z: 0,
            })
            .collect::<Vec<_>>(),
        NativeOtClientCardinalDirection::East => (0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT)
            .map(|y| Position {
                x: snapshot
                    .player_position
                    .x
                    .saturating_add_signed(NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH as i16 - 1 - center_x),
                y: snapshot
                    .player_position
                    .y
                    .saturating_add_signed(y as i16 - center_y),
                z: 0,
            })
            .collect::<Vec<_>>(),
        NativeOtClientCardinalDirection::South => (0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH)
            .map(|x| Position {
                x: snapshot
                    .player_position
                    .x
                    .saturating_add_signed(x as i16 - center_x),
                y: snapshot.player_position.y.saturating_add_signed(
                    NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT as i16 - 1 - center_y,
                ),
                z: 0,
            })
            .collect::<Vec<_>>(),
        NativeOtClientCardinalDirection::West => (0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT)
            .map(|y| Position {
                x: snapshot.player_position.x.saturating_add_signed(-center_x),
                y: snapshot
                    .player_position
                    .y
                    .saturating_add_signed(y as i16 - center_y),
                z: 0,
            })
            .collect::<Vec<_>>(),
    };
    writer.byte(match direction {
        NativeOtClientCardinalDirection::North => 0x65,
        NativeOtClientCardinalDirection::East => 0x66,
        NativeOtClientCardinalDirection::South => 0x67,
        NativeOtClientCardinalDirection::West => 0x68,
    });
    let step_cells = positions.len() * NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS;
    let mut encoded_cells = 0usize;
    for z in (0..NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8).rev() {
        for position in &positions {
            let remaining_cells = step_cells.saturating_sub(encoded_cells + 1);
            let position = Position { z, ..*position };
            let ground_thing_id = world_map
                .tile(position)
                .map(|tile| tile.ground_thing_id)
                .filter(|ground_thing_id| *ground_thing_id != 0)
                .unwrap_or(snapshot.ground_thing_id);
            if ground_thing_id != 0 && native_map_record_fits_budget(&writer, 2, remaining_cells) {
                writer.u16(ground_thing_id);
            }
            if let Some(items) = world_map.tile_items(position) {
                for item in items
                    .iter()
                    .skip(1)
                    .take(NATIVE_OTCLIENT_MAX_EXTRA_TILE_ITEMS)
                {
                    let thing_id = item.client_thing_id.unwrap_or(item.server_id);
                    if thing_id != 0 && native_map_record_fits_budget(&writer, 2, remaining_cells) {
                        writer.u16(thing_id);
                    } else if thing_id != 0 {
                        break;
                    }
                }
            }
            if let Some(static_spawns) = static_spawns {
                for entity in static_spawns.at(position) {
                    if static_entity_count >= NATIVE_OTCLIENT_MAX_STATIC_ENTITIES_PER_VIEWPORT {
                        break;
                    }
                    let mut entity_record = Writer::default();
                    write_native_otclient_unknown_static_entity(&mut entity_record, entity);
                    if native_map_record_fits_budget(&writer, entity_record.len(), remaining_cells)
                    {
                        writer.bytes(&entity_record.finish());
                        static_entity_count += 1;
                    } else {
                        break;
                    }
                }
            }
            if !asset_free {
                if let Some(visible_players) = visible_players {
                    for player in visible_players.iter().filter(|player| {
                        player.position.x == position.x
                            && player.position.y == position.y
                            && player.position.z == position.z
                    }) {
                        if visible_player_count >= NATIVE_OTCLIENT_MAX_SHARED_PLAYERS_PER_VIEWPORT {
                            break;
                        }
                        let mut player_record = Writer::default();
                        write_native_otclient_unknown_visible_player(&mut player_record, player);
                        if native_map_record_fits_budget(
                            &writer,
                            player_record.len(),
                            remaining_cells,
                        ) {
                            writer.bytes(&player_record.finish());
                            visible_player_count += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            writer.u16(NATIVE_OTCLIENT_TILE_END);
            encoded_cells += 1;
        }
    }
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

fn native_map_record_fits_budget(
    writer: &Writer,
    record_bytes: usize,
    remaining_cells: usize,
) -> bool {
    writer
        .len()
        .saturating_add(record_bytes)
        .saturating_add(2)
        .saturating_add(remaining_cells.saturating_mul(4))
        <= MAX_FRAME_SIZE
}

pub fn decode_native_otclient_cardinal_move_request(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientCardinalDirection, ProtocolError> {
    match decode_native_otclient_game_action(frame, profile)? {
        NativeOtClientGameAction::CardinalMove(direction) => Ok(direction),
        _ => Err(ProtocolError::InvalidNativeGameRequest),
    }
}

pub fn decode_native_otclient_game_action(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientGameAction, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut reader = Reader::new(&frame.0);
    let action = match reader.byte()? {
        NATIVE_OTCLIENT_ENTER_GAME => NativeOtClientGameAction::EnterGame,
        NATIVE_OTCLIENT_LEAVE_GAME => NativeOtClientGameAction::LeaveGame,
        NATIVE_OTCLIENT_CLIENT_PING => NativeOtClientGameAction::Ping,
        NATIVE_OTCLIENT_CLIENT_PING_BACK => NativeOtClientGameAction::PingBack,
        NATIVE_OTCLIENT_CLIENT_STOP => NativeOtClientGameAction::Stop,
        NATIVE_OTCLIENT_CLIENT_CANCEL_ATTACK_AND_FOLLOW => {
            NativeOtClientGameAction::CancelAttackAndFollow
        }
        NATIVE_OTCLIENT_CLIENT_AUTO_WALK => {
            let length = usize::from(reader.byte()?);
            if length > 64 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            let mut path = Vec::with_capacity(length);
            for _ in 0..length {
                path.push(
                    NativeOtClientAutoWalkDirection::from_native_byte(reader.byte()?)
                        .ok_or(ProtocolError::InvalidNativeGameRequest)?,
                );
            }
            NativeOtClientGameAction::AutoWalk(path)
        }
        NATIVE_OTCLIENT_CLIENT_THROW_ITEM => {
            if reader.remaining() != 14 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::ThrowItem {
                source_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                source_client_thing_id: reader.u16()?,
                source_stack_position: reader.byte()?,
                target_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                count: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_USE_ITEM => {
            if reader.remaining() != 9 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::UseItem {
                position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                client_thing_id: reader.u16()?,
                stack_position: reader.byte()?,
                index: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_USE_ITEM_EX => {
            if reader.remaining() != 16 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::UseItemEx {
                source_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                source_client_thing_id: reader.u16()?,
                source_stack_position: reader.byte()?,
                target_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                target_client_thing_id: reader.u16()?,
                target_stack_position: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_USE_ITEM_ON_CREATURE => {
            if reader.remaining() != 12 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::UseItemOnCreature {
                source_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                source_client_thing_id: reader.u16()?,
                source_stack_position: reader.byte()?,
                target_creature_id: reader.u32()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_ROTATE_ITEM => {
            if reader.remaining() != 8 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::RotateItem {
                position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                client_thing_id: reader.u16()?,
                stack_position: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_LOOK_MAP => {
            if reader.remaining() != 8 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::LookMap {
                position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                thing_id: reader.u16()?,
                stack_position: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_LOOK_CREATURE => {
            if reader.remaining() != 4 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::LookCreature {
                creature_id: reader.u32()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT => {
            if reader.remaining() != 5 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::ChangeOutfit(NativeOtClientClassicOutfit {
                look_type: reader.byte()?,
                head: reader.byte()?,
                body: reader.byte()?,
                legs: reader.byte()?,
                feet: reader.byte()?,
            })
        }
        NATIVE_OTCLIENT_CLIENT_CLOSE_CONTAINER => {
            NativeOtClientGameAction::CloseContainer(reader.byte()?)
        }
        NATIVE_OTCLIENT_CLIENT_UP_ARROW_CONTAINER => {
            NativeOtClientGameAction::UpArrowContainer(reader.byte()?)
        }
        NATIVE_OTCLIENT_CLIENT_UPDATE_CONTAINER => {
            NativeOtClientGameAction::UpdateContainer(reader.byte()?)
        }
        NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT => NativeOtClientGameAction::RequestOutfit,
        NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG => NativeOtClientGameAction::RequestQuestLog,
        NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LINE => {
            if reader.remaining() != 2 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::RequestQuestLine {
                quest_id: reader.u16()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS => NativeOtClientGameAction::RequestChannels,
        NATIVE_OTCLIENT_CLIENT_JOIN_CHANNEL => NativeOtClientGameAction::JoinChannel(reader.u16()?),
        NATIVE_OTCLIENT_CLIENT_LEAVE_CHANNEL => {
            NativeOtClientGameAction::LeaveChannel(reader.u16()?)
        }
        NATIVE_OTCLIENT_CLIENT_ADD_VIP => {
            NativeOtClientGameAction::AddVip(reader.string(MAX_LOGIN_STRING_BYTES)?)
        }
        NATIVE_OTCLIENT_CLIENT_REMOVE_VIP => NativeOtClientGameAction::RemoveVip(reader.u32()?),
        NATIVE_OTCLIENT_CLIENT_EDIT_VIP => {
            let target_player_id = reader.u32()?;
            let description = reader.string(MAX_LOGIN_STRING_BYTES)?;
            let icon = reader.u32()?;
            let notify = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::InvalidNativeGameRequest),
            };
            NativeOtClientGameAction::EditVip {
                target_player_id,
                description,
                icon,
                notify,
            }
        }
        NATIVE_OTCLIENT_CLIENT_TALK => {
            let mode = reader.byte()?;
            let (channel_id, recipient, message) = match mode {
                4 | 5 | 11 => {
                    let recipient = reader.string(MAX_LOGIN_STRING_BYTES)?;
                    (
                        None,
                        Some(recipient),
                        reader.string(MAX_LOGIN_STRING_BYTES)?,
                    )
                }
                6 | 7 | 8 | 10 | 12 => {
                    let channel_id = reader.u16()?;
                    (
                        Some(channel_id),
                        None,
                        reader.string(MAX_LOGIN_STRING_BYTES)?,
                    )
                }
                _ => (None, None, reader.string(MAX_LOGIN_STRING_BYTES)?),
            };
            NativeOtClientGameAction::Talk(NativeOtClientTalkRequest {
                mode,
                channel_id,
                recipient,
                message,
            })
        }
        NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES => {
            NativeOtClientGameAction::ChangeFightModes(NativeOtClientFightModeRequest {
                mode: NativeOtClientFightMode::from_classic_value(reader.byte()?),
                chase: reader.byte()? != 0,
                secure: reader.byte()? != 0,
            })
        }
        NATIVE_OTCLIENT_CLIENT_SELECT_TARGET => {
            NativeOtClientGameAction::SelectTarget(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_SELECT_FOLLOW => {
            NativeOtClientGameAction::SelectFollow(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY => {
            NativeOtClientGameAction::PartyInvite(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_JOIN_PARTY => NativeOtClientGameAction::PartyJoin(reader.u32()?),
        NATIVE_OTCLIENT_CLIENT_REVOKE_PARTY_INVITATION => {
            NativeOtClientGameAction::PartyRevokeInvitation(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_PASS_PARTY_LEADERSHIP => {
            NativeOtClientGameAction::PartyPassLeadership(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_LEAVE_PARTY => NativeOtClientGameAction::PartyLeave,
        NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE => {
            let active = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::InvalidNativeGameRequest),
            };
            if reader.byte()? != 0 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::PartySharedExperience(active)
        }
        opcode if is_native_otclient_compatibility_interaction(opcode) => {
            if reader.remaining() > NATIVE_OTCLIENT_MAX_IGNORED_INTERACTION_BYTES {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            reader.take(reader.remaining())?;
            NativeOtClientGameAction::IgnoredInteraction(opcode)
        }
        opcode => NativeOtClientCardinalDirection::from_client_opcode(opcode)
            .map(NativeOtClientGameAction::CardinalMove)
            .or_else(|| {
                NativeOtClientAutoWalkDirection::from_direct_diagonal_opcode(opcode)
                    .map(NativeOtClientGameAction::DiagonalMove)
            })
            .or_else(|| {
                NativeOtClientCardinalDirection::from_turn_opcode(opcode)
                    .map(NativeOtClientGameAction::Turn)
            })
            .ok_or(ProtocolError::InvalidNativeGameRequest)?,
    };
    if !reader.done() {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    Ok(action)
}

fn is_native_otclient_compatibility_interaction(opcode: u8) -> bool {
    matches!(
        opcode,
            0x77
            | 0x86..=0x8b
            | 0x97..=0x9f
            | 0xa3..=0xad
            | 0xca
    )
}

pub fn encode_native_otclient_game_ping_back(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_PING_BACK]))
}

pub fn encode_native_otclient_game_ping(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_PING]))
}

/// Encodes the classic zero-payload `ClearTarget` (`0xA3`) record for the supported native 740
/// profile. Later profiles may append an attack-sequence field and are intentionally excluded.
pub fn encode_native_otclient_clear_target(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_CLEAR_TARGET]))
}

/// Encodes the classic three-byte `PlayerModes` (`0xA7`) record for the supported native 740
/// profile. Later protocol variants may append a PvP-mode byte and are intentionally excluded.
pub fn encode_native_otclient_player_modes(
    profile: &NativeOtClientProfile,
    modes: NativeOtClientFightModeRequest,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mode = match modes.mode {
        NativeOtClientFightMode::Attack => 1,
        NativeOtClientFightMode::Balanced => 2,
        NativeOtClientFightMode::Defense => 3,
    };
    Ok(Frame(vec![
        NATIVE_OTCLIENT_GAME_PLAYER_MODES,
        mode,
        u8::from(modes.chase),
        u8::from(modes.secure),
    ]))
}

/// Encodes classic `CloseContainer` (`0x6F`) for the supported native 740 profile. This closes
/// only the client view; authoritative container ownership and persistence are separate.
pub fn encode_native_otclient_close_container(
    profile: &NativeOtClientProfile,
    container_id: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![
        NATIVE_OTCLIENT_GAME_CLOSE_CONTAINER,
        container_id,
    ]))
}

/// Encodes the classic native death notification. The supported 7.4 profile admits only the
/// opcode: death type and penalty fields belong to later client feature sets and are not emitted.
pub fn encode_native_otclient_game_death(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_DEATH]))
}

/// Encodes an explicitly empty classic 7.4 Quest Log response. FE does not yet claim quest
/// content, persistence, mission lines, scripts, or gameplay semantics.
pub fn encode_native_otclient_empty_quest_log(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_QUEST_LOG);
    writer.u16(0);
    Ok(Frame(writer.finish()))
}

/// Encodes the classic quest-list response: one u16 entry count followed by per-entry
/// u16 quest ID and bounded display name. Entries must be sorted by ID and carry nonzero IDs
/// with nonempty names; the caller owns that ordering.
pub fn encode_native_otclient_quest_list(
    profile: &NativeOtClientProfile,
    quests: &[(u16, String)],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if quests.len() > u16::MAX as usize
        || quests
            .iter()
            .any(|(id, name)| *id == 0 || name.is_empty() || name.len() > MAX_LOGIN_STRING_BYTES)
    {
        return Err(ProtocolError::InvalidLength(quests.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_QUEST_LOG);
    writer.u16(quests.len() as u16);
    for (id, name) in quests {
        writer.u16(*id);
        writer.string(name);
    }
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes an explicitly empty classic 7.4 channel list. FE does not yet claim channel
/// membership, private messages, moderation, persistence, guild, party, or gameplay semantics.
pub fn encode_native_otclient_empty_channel_list(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_channel_list(profile, &[])
}

/// Encodes one bounded classic 7.4 channel list. The entries must originate from a validated
/// authoritative catalog; membership and social-system behavior are outside this codec.
pub fn encode_native_otclient_channel_list(
    profile: &NativeOtClientProfile,
    channels: &[NativeOtClientClassicChannel],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let count = u8::try_from(channels.len()).map_err(|_| ProtocolError::TooManyChannels)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CHANNELS);
    writer.byte(count);
    for channel in channels {
        if channel.name.len() > u16::MAX as usize {
            return Err(ProtocolError::StringTooLong(channel.name.len()));
        }
        writer.u16(channel.id);
        writer.string(&channel.name);
    }
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 VIP entry for the profile's no-additional-info parser branch. The
/// record contains only the character ID, name, and explicit online/offline status; description,
/// icon, notification, and VIP group fields belong to later client features.
pub fn encode_native_otclient_classic_vip_entry(
    profile: &NativeOtClientProfile,
    target_player_id: u32,
    target_player_name: &str,
    online: bool,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records()
        || target_player_id == 0
        || target_player_name.is_empty()
        || target_player_name.len() > MAX_LOGIN_STRING_BYTES
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let fixed_bytes = 8usize;
    if target_player_name.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(target_player_name.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_VIP_ADD);
    writer.u32(target_player_id);
    writer.string(target_player_name);
    writer.byte(u8::from(online));
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 live VIP presence transition. This profile predates login-pending
/// state, so `0xd3 + id` means online and `0xd4 + id` means offline; no status byte is emitted.
/// Newer client layouts remain outside this profile-gated codec.
pub fn encode_native_otclient_classic_vip_presence(
    profile: &NativeOtClientProfile,
    target_player_id: u32,
    online: bool,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || target_player_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(if online {
        NATIVE_OTCLIENT_GAME_VIP_STATE
    } else {
        NATIVE_OTCLIENT_GAME_VIP_LOGOUT
    });
    writer.u32(target_player_id);
    Ok(Frame(writer.finish()))
}

/// Opens one validated configured public channel with explicit empty player and invitation lists.
/// This codec does not create session membership or enable channel messages.
pub fn encode_native_otclient_open_public_channel(
    profile: &NativeOtClientProfile,
    channel: &NativeOtClientClassicChannel,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if channel.name.len() > u16::MAX as usize {
        return Err(ProtocolError::StringTooLong(channel.name.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_OPEN_CHANNEL);
    writer.u16(channel.id);
    writer.string(&channel.name);
    writer.u16(0);
    writer.u16(0);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_game_cancel_walk(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_game_cancel_walk_facing(profile, 0)
}

pub fn encode_native_otclient_game_cancel_walk_facing(
    profile: &NativeOtClientProfile,
    direction: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_CANCEL_WALK, direction]))
}

pub fn encode_native_otclient_choose_outfit(
    profile: &NativeOtClientProfile,
    current_outfit: NativeOtClientClassicOutfit,
    first_look_type: u8,
    last_look_type: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || current_outfit.look_type == 0
        || first_look_type == 0
        || first_look_type > last_look_type
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT);
    write_native_otclient_classic_outfit(&mut writer, current_outfit);
    writer.byte(first_look_type);
    writer.byte(last_look_type);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_creature_outfit(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    outfit: NativeOtClientClassicOutfit,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&creature_id)
        || outfit.look_type == 0
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_OUTFIT);
    writer.u32(creature_id);
    write_native_otclient_classic_outfit(&mut writer, outfit);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_creature_health(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    health: u16,
    max_health: u16,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !is_native_otclient_creature_id(creature_id)
        || max_health == 0
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let health_percent = ((u32::from(health.min(max_health)) * 100) / u32::from(max_health)) as u8;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_HEALTH);
    writer.u32(creature_id);
    writer.byte(health_percent);
    Ok(Frame(writer.finish()))
}

/// Encodes the exact classic creature-party (`0x91`) record for one active native player. The
/// caller owns party relationships, spectator visibility, and delivery timing; this protocol
/// function only permits the audited basic shield values for the supported classic profile.
pub fn encode_native_otclient_creature_party_shield(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    shield: NativeOtClientClassicPartyShield,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&creature_id)
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_PARTY);
    writer.u32(creature_id);
    writer.byte(shield.classic_value());
    Ok(Frame(writer.finish()))
}

fn is_native_otclient_creature_id(creature_id: u32) -> bool {
    (NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&creature_id)
        || creature_id > NATIVE_OTCLIENT_PLAYER_ID_END
}

pub fn encode_native_otclient_move_creature(
    profile: &NativeOtClientProfile,
    player_id: u32,
    position: NativeOtClientPosition,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&player_id)
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_MOVE_CREATURE);
    writer.u16(NATIVE_OTCLIENT_MAPPED_CREATURE);
    writer.u32(player_id);
    write_native_otclient_position(&mut writer, position);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_move_creature_at(
    profile: &NativeOtClientProfile,
    old_position: NativeOtClientPosition,
    old_stack_position: u8,
    new_position: NativeOtClientPosition,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() || old_stack_position == u8::MAX {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_MOVE_CREATURE);
    write_native_otclient_position(&mut writer, old_position);
    writer.byte(old_stack_position);
    write_native_otclient_position(&mut writer, new_position);
    Ok(Frame(writer.finish()))
}

fn validate_native_empty_world_snapshot(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<(), ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END)
            .contains(&snapshot.player_id)
        || snapshot.player_position.z >= NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8
        || snapshot.player_direction > 3
        || snapshot.player_speed == 0
        || snapshot.server_beat == 0
        || snapshot.player_name.len() > MAX_LOGIN_STRING_BYTES
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(())
}

fn write_native_otclient_position(writer: &mut Writer, position: NativeOtClientPosition) {
    writer.u16(position.x);
    writer.u16(position.y);
    writer.byte(position.z);
}

fn write_native_otclient_classic_item_record(
    writer: &mut Writer,
    item: NativeOtClientClassicItemRecord,
) {
    writer.u16(item.client_thing_id);
    if let Some(subtype) = item.subtype {
        writer.byte(subtype);
    }
}

fn write_native_otclient_unknown_player(
    writer: &mut Writer,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) {
    writer.u16(NATIVE_OTCLIENT_UNKNOWN_CREATURE);
    writer.u32(0);
    writer.u32(snapshot.player_id);
    writer.string(&snapshot.player_name);
    writer.byte(100);
    writer.byte(snapshot.player_direction);
    writer.byte(snapshot.player_look_type);
    if snapshot.player_look_type == 0 {
        writer.u16(0);
    } else {
        writer.byte(0);
        writer.byte(0);
        writer.byte(0);
        writer.byte(0);
    }
    writer.byte(0);
    writer.byte(0);
    writer.u16(snapshot.player_speed);
    writer.byte(0);
    writer.byte(0);
}

fn write_native_otclient_classic_outfit(writer: &mut Writer, outfit: NativeOtClientClassicOutfit) {
    writer.byte(outfit.look_type);
    writer.byte(outfit.head);
    writer.byte(outfit.body);
    writer.byte(outfit.legs);
    writer.byte(outfit.feet);
}

fn write_native_otclient_unknown_visible_player(
    writer: &mut Writer,
    player: &NativeOtClientVisiblePlayer,
) {
    writer.u16(NATIVE_OTCLIENT_UNKNOWN_CREATURE);
    writer.u32(0);
    writer.u32(player.player_id);
    writer.string(&player.name);
    writer.byte(player.health_percent);
    writer.byte(player.direction);
    if player.outfit.look_type == 0 {
        writer.u16(0);
    } else {
        write_native_otclient_classic_outfit(writer, player.outfit);
        writer.byte(0);
    }
    writer.u16(player.speed);
    writer.byte(0);
    writer.byte(0);
}

fn write_native_otclient_unknown_static_entity(writer: &mut Writer, entity: &FeTfsStaticEntity) {
    writer.u16(NATIVE_OTCLIENT_UNKNOWN_CREATURE);
    writer.u32(0);
    writer.u32(entity.id);
    writer.string(&entity.name);
    writer.byte(entity.health_percent);
    writer.byte(entity.direction);
    writer.byte(entity.look_type);
    writer.byte(entity.head);
    writer.byte(entity.body);
    writer.byte(entity.legs);
    writer.byte(entity.feet);
    writer.byte(entity.addons);
    writer.byte(0);
    writer.u16(entity.speed);
    writer.byte(0);
    writer.byte(0);
}

#[cfg(test)]
fn encode_native_otclient_login_request_for_harness(request: &NativeOtClientLoginRequest) -> Frame {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_ENTER_ACCOUNT);
    writer.u16(request.operating_system);
    writer.u16(request.protocol_version);
    writer.u32(request.dat_signature);
    writer.u32(request.spr_signature);
    writer.u32(request.pic_signature);
    writer.u32(request.account_id);
    writer.string(&request.password);
    writer.string(&request.client_tag);
    writer.u16(request.client_build);
    writer.bytes(&[0; 8]);
    Frame(writer.finish())
}

#[cfg(test)]
fn encode_native_otclient_game_request_for_harness(request: &NativeOtClientGameRequest) -> Frame {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_PENDING_GAME);
    writer.u16(request.operating_system);
    writer.u16(request.protocol_version);
    writer.byte(0);
    writer.u32(request.account_id);
    writer.string(&request.character_name);
    writer.string(&request.password);
    writer.string(&request.client_tag);
    writer.u16(request.client_build);
    writer.bytes(&[0; 8]);
    Frame(writer.finish())
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
    InvalidOtClientCapabilityAck,
    InvalidOtClientMessage,
    InvalidNativeLoginRequest,
    InvalidNativeGameRequest,
    UnsupportedNativeClientProfile,
    TooManyCharacters,
    TooManyChannels,
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
    fn len(&self) -> usize {
        self.0.len()
    }

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
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
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
    #[test]
    fn native_decoders_survive_deterministic_garbage_and_truncation() {
        // Fuzz-lite robustness sweep on stable tooling: a seeded LCG drives both random
        // payloads and every truncation of a valid frame through the public decoders. The
        // contract is no panic and no hang - any outcome must be a typed ProtocolError.
        let profile = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..256_u32 {
            let len = (next() % 96) as usize;
            let payload: Vec<u8> = (0..len).map(|_| (next() & 0xff) as u8).collect();
            let garbage = Frame(payload);
            let _ = decode_status_request(&garbage);
            let _ = decode_legacy_74_envelope(&garbage);
            let _ = decode_native_otclient_game_request(&garbage, &profile);
            let _ = decode_native_otclient_game_action(&garbage, &profile);
            let _ = decode_native_otclient_cardinal_move_request(&garbage, &profile);

            if round == 0 {
                continue;
            }
            // Truncation sweep of a well-formed game-action prefix.
            let mut valid = vec![NATIVE_OTCLIENT_CLIENT_AUTO_WALK];
            valid.extend_from_slice(&(next() as u16).to_be_bytes());
            for cut in 0..valid.len() {
                let truncated = Frame(valid[..cut].to_vec());
                let _ = decode_native_otclient_game_action(&truncated, &profile);
                let _ = decode_native_otclient_cardinal_move_request(&truncated, &profile);
            }
        }
    }
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
    fn native_profile_foundations_distinguish_runnable_740_from_encrypted_800() {
        let plain_740 = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        assert_eq!(
            plain_740.foundation(),
            NativeOtClientFoundation::PlainClassic740
        );
        assert!(plain_740.supports_current_native_foundation());

        let encrypted_800 = NativeOtClientProfile {
            protocol_version: 800,
            login_packet_encryption: true,
            protocol_checksum: false,
            challenge_on_login: false,
            ..plain_740
        };
        assert_eq!(
            encrypted_800.foundation(),
            NativeOtClientFoundation::Classic800RequiresRsaXtea
        );
        assert_eq!(
            encrypted_800.foundation().label(),
            "classic-800-requires-rsa-xtea"
        );
        assert!(!encrypted_800.supports_current_native_foundation());
    }

    #[test]
    fn fe_8_0_xtea_transport_envelope_is_outbound_only_and_profile_bounded() {
        let plain_740 = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        let encrypted_800 = NativeOtClientProfile {
            protocol_version: 800,
            login_packet_encryption: true,
            ..plain_740
        };
        let key = [1, 2, 3, 4];
        let plaintext = Frame(vec![0x0a, 0x34, 0x12]);
        let envelope =
            encode_fe_8_0_xtea_transport_envelope(&encrypted_800, &plaintext, key).unwrap();
        assert_ne!(envelope.0, plaintext.0);
        assert_eq!(envelope.0.len() % 8, 0);
        assert_eq!(xtea_decrypt_packet(&envelope.0, key).unwrap(), plaintext.0);
        assert!(matches!(
            encode_fe_8_0_xtea_transport_envelope(&plain_740, &plaintext, key),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
        assert!(matches!(
            encode_fe_8_0_xtea_transport_envelope(&encrypted_800, &Frame(Vec::new()), key),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
        assert!(matches!(
            encode_fe_8_0_xtea_transport_envelope(
                &encrypted_800,
                &Frame(vec![0; FE_8_0_XTEA_TRANSPORT_MAX_PLAINTEXT_BYTES + 1]),
                key,
            ),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
    }

    #[test]
    fn status_contract_decodes_and_escapes_xml() {
        let bootstrap = Fe80RsaXteaBootstrap {
            xtea_key: [0x1122_3344, 0x5566_7788, 0x99aa_bbcc, 0xddee_ff00],
        };
        let mut bootstrap_plaintext = encode_fe_8_0_rsa_xtea_bootstrap_for_harness(bootstrap);
        bootstrap_plaintext[17..22].copy_from_slice(&[9, 8, 7, 6, 5]);
        assert_eq!(
            decode_fe_8_0_rsa_xtea_bootstrap(&bootstrap_plaintext).unwrap(),
            bootstrap
        );
        bootstrap_plaintext[0] = 1;
        assert!(matches!(
            decode_fe_8_0_rsa_xtea_bootstrap(&bootstrap_plaintext),
            Err(ProtocolError::InvalidLoginMarker)
        ));
        assert!(matches!(
            decode_status_request(&Frame(vec![1, 9, 0])),
            Ok(StatusRequest::Binary { .. })
        ));
        let metrics_frame = {
            let mut payload = vec![0xff, 0x0a, 0x00];
            payload.extend_from_slice(b"fe-metrics");
            Frame(payload)
        };
        assert_eq!(
            decode_status_request(&metrics_frame).unwrap(),
            StatusRequest::Metrics
        );
        assert_eq!(
            String::from_utf8(encode_status_metrics(&StatusMetrics {
                uptime_seconds: 12,
                registered_accounts: 3,
                registered_characters: 7,
                schema_version: 27,
                players_online: 2,
                players_online_cap: 100,
            }))
            .unwrap(),
            "{\"uptime_seconds\":12,\"registered_accounts\":3,\"registered_characters\":7,\"schema_version\":27,\"players_online\":2,\"players_online_cap\":100}\n"
        );
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

    #[test]
    fn otclient_capability_ack_and_initial_world_payload_are_bounded() {
        let endpoint = OtClientEndpoint {
            host: "fe.example.test".into(),
            port: 443,
        };
        let offer = encode_fe_otclient_capability_offer(&endpoint);
        assert_eq!(offer.0[0], FE_OTCLIENT_EXTENDED_OPCODE);
        assert!(
            decode_fe_otclient_capability_ack(&encode_fe_otclient_capability_ack_for_harness())
                .is_ok()
        );
        let world = encode_fe_otclient_initial_world(&InitialWorldSnapshot {
            character_name: "Knight".into(),
            start_x: 100,
            start_y: 100,
            start_z: 7,
            endpoint,
        });
        assert_eq!(world.0[0], FE_OTCLIENT_EXTENDED_OPCODE);
        assert!(world
            .0
            .windows(b"fe.world.v1".len())
            .any(|window| window == b"fe.world.v1"));
    }

    #[test]
    fn empty_world_viewport_tick_and_movement_contracts_are_explicit() {
        let viewport = EmptyWorldViewport {
            tick: 9,
            center: Position {
                x: 101,
                y: 99,
                z: 7,
            },
            manifest: forgotten_core::EmptyWorldManifest::default(),
        };
        let viewport = encode_fe_otclient_empty_viewport(&viewport);
        assert!(viewport
            .0
            .windows(b"fe.viewport.v1;tick=9".len())
            .any(|window| window == b"fe.viewport.v1;tick=9"));
        let tick = encode_fe_otclient_world_tick(10);
        assert!(tick
            .0
            .windows(b"fe.tick.v1;tick=10".len())
            .any(|window| window == b"fe.tick.v1;tick=10"));
        let move_request = encode_fe_otclient_move_request_for_harness(CardinalDirection::West);
        assert_eq!(
            decode_fe_otclient_move_request(&move_request).unwrap(),
            CardinalDirection::West
        );
        let acknowledgement = encode_fe_otclient_movement_ack(&EmptyWorldMovementAck {
            tick: 10,
            from: Position {
                x: 101,
                y: 99,
                z: 7,
            },
            to: Position {
                x: 100,
                y: 99,
                z: 7,
            },
        });
        assert!(acknowledgement
            .0
            .windows(b"fe.move.ack.v1;tick=10".len())
            .any(|window| window == b"fe.move.ack.v1;tick=10"));
    }

    #[test]
    fn native_otclient_login_character_list_and_game_selection_are_profile_driven() {
        let profile = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: LEGACY_RSA_BLOCK_SIZE,
        };
        let login = NativeOtClientLoginRequest {
            operating_system: 2,
            protocol_version: profile.protocol_version,
            dat_signature: 0x1122_3344,
            spr_signature: 0x5566_7788,
            pic_signature: 0x99aa_bbcc,
            account_id: 42,
            password: "correct horse".into(),
            client_tag: "OTCv8".into(),
            client_build: 412,
        };
        assert_eq!(
            decode_native_otclient_login_request(
                &encode_native_otclient_login_request_for_harness(&login),
                &profile,
            )
            .unwrap(),
            login
        );
        let list = encode_native_otclient_character_list(&[CharacterListEntry {
            name: "Knight".into(),
            world_name: "Forgotten Engine".into(),
            address: "127.0.0.1".parse().unwrap(),
            port: 7172,
        }])
        .unwrap();
        assert_eq!(list.0[0], NATIVE_OTCLIENT_LOGIN_CHARACTER_LIST);
        assert_eq!(list.0[1], 1);
        assert!(list.0.windows(4).any(|bytes| bytes == [127, 0, 0, 1]));
        let game = NativeOtClientGameRequest {
            operating_system: 2,
            protocol_version: profile.protocol_version,
            account_id: 42,
            character_name: "Knight".into(),
            password: "correct horse".into(),
            client_tag: "OTCv8".into(),
            client_build: 412,
        };
        assert_eq!(
            decode_native_otclient_game_request(
                &encode_native_otclient_game_request_for_harness(&game),
                &profile,
            )
            .unwrap(),
            game
        );
        let mut missing_rsa_leading_byte = encode_native_otclient_game_request_for_harness(&game);
        missing_rsa_leading_byte.0.remove(5);
        assert!(decode_native_otclient_game_request(&missing_rsa_leading_byte, &profile).is_err());
        assert_eq!(
            encode_native_otclient_game_login_error("Map initialization is pending.").0[0],
            NATIVE_OTCLIENT_GAME_LOGIN_ERROR
        );
    }

    #[test]
    fn native_otclient_empty_world_packets_follow_the_selected_classic_profile() {
        let profile = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START + 42,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            player_experience: 4_900,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: PlayerSkills::default(),
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };

        let login = encode_native_otclient_game_login_state(&profile, &snapshot).unwrap();
        assert_eq!(login.0[0], NATIVE_OTCLIENT_GAME_LOGIN_STATE);
        assert_eq!(
            u32::from_le_bytes(login.0[1..5].try_into().unwrap()),
            snapshot.player_id
        );
        assert_eq!(
            u16::from_le_bytes(login.0[5..7].try_into().unwrap()),
            snapshot.server_beat
        );
        assert_eq!(login.0[7], 0);

        let map = encode_native_otclient_empty_world_map(&profile, &snapshot).unwrap();
        assert_eq!(map.0[0], NATIVE_OTCLIENT_GAME_FULL_MAP);
        assert_eq!(
            &map.0[1..6],
            &[100, 0, 100, 0, 7],
            "map center uses x, y, z little-endian coordinates"
        );
        let cells = NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH
            * NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT
            * NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS;
        assert_eq!(map.0.len(), 1 + 5 + cells * 4 + 31);
        let player_tile = 1
            + 5
            + ((NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1) * NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT
                + (NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1))
                * 4;
        assert_eq!(
            u16::from_le_bytes(map.0[player_tile..player_tile + 2].try_into().unwrap()),
            snapshot.ground_thing_id
        );
        assert_eq!(
            u16::from_le_bytes(map.0[player_tile + 2..player_tile + 4].try_into().unwrap()),
            NATIVE_OTCLIENT_UNKNOWN_CREATURE
        );
        assert!(map
            .0
            .windows(snapshot.player_name.len())
            .any(|bytes| bytes == snapshot.player_name.as_bytes()));

        let mut world_map = WorldMap::new(
            "viewport-test",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        world_map
            .set_tile(
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                forgotten_core::WorldMapTile {
                    ground_thing_id: 555,
                    walkable: true,
                },
            )
            .unwrap();
        world_map
            .set_tile_items(
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                vec![
                    forgotten_core::WorldMapItem {
                        server_id: 4526,
                        client_thing_id: Some(555),
                        count: 1,
                        action_id: None,
                        unique_id: None,
                        text: None,
                        description: None,
                        teleport_destination: None,
                        duration: None,
                        charges: None,
                        children: Vec::new(),
                    },
                    forgotten_core::WorldMapItem {
                        server_id: 4527,
                        client_thing_id: Some(556),
                        count: 1,
                        action_id: None,
                        unique_id: None,
                        text: None,
                        description: None,
                        teleport_destination: None,
                        duration: None,
                        charges: None,
                        children: Vec::new(),
                    },
                ],
            )
            .unwrap();
        let map_viewport =
            encode_native_otclient_map_viewport(&profile, &snapshot, &world_map).unwrap();
        assert_eq!(map_viewport.0[0], NATIVE_OTCLIENT_GAME_FULL_MAP);
        assert!(map_viewport
            .0
            .windows(2)
            .any(|bytes| bytes == 555u16.to_le_bytes()));
        assert!(map_viewport
            .0
            .windows(2)
            .any(|bytes| bytes == 556u16.to_le_bytes()));
        world_map
            .set_tile(
                Position {
                    x: 110,
                    y: 100,
                    z: 7,
                },
                forgotten_core::WorldMapTile {
                    ground_thing_id: 777,
                    walkable: true,
                },
            )
            .unwrap();
        let east_snapshot = NativeOtClientEmptyWorldSnapshot {
            player_position: NativeOtClientPosition {
                x: 101,
                ..snapshot.player_position
            },
            player_direction: NativeOtClientCardinalDirection::East.protocol_direction(),
            ..snapshot.clone()
        };
        let east_edge = encode_native_otclient_map_step_with_static_spawns_and_players(
            &profile,
            &east_snapshot,
            &world_map,
            None,
            None,
            NativeOtClientCardinalDirection::East,
        )
        .unwrap();
        assert_eq!(east_edge.0[0], 0x66);
        assert!(east_edge
            .0
            .windows(2)
            .any(|bytes| bytes == 777u16.to_le_bytes()));
        assert!(east_edge.0.len() < map_viewport.0.len());
        let static_position = Position {
            x: 99,
            y: 100,
            z: 7,
        };
        let static_spawns = FeTfsStaticSpawnCollection::new(vec![FeTfsStaticEntity {
            id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
            name: "Rat".into(),
            position: static_position,
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 220,
            health_percent: 100,
            direction: 2,
        }])
        .unwrap();
        let static_viewport = encode_native_otclient_map_viewport_with_static_spawns(
            &profile,
            &snapshot,
            &world_map,
            Some(&static_spawns),
        )
        .unwrap();
        let static_tile_index = 7 * NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT + 6;
        let static_tile_offset = 1 + 5 + static_tile_index * 4;
        assert_eq!(
            u16::from_le_bytes(
                static_viewport.0[static_tile_offset + 2..static_tile_offset + 4]
                    .try_into()
                    .unwrap()
            ),
            NATIVE_OTCLIENT_UNKNOWN_CREATURE
        );
        assert_eq!(
            u32::from_le_bytes(
                static_viewport.0[static_tile_offset + 8..static_tile_offset + 12]
                    .try_into()
                    .unwrap()
            ),
            NATIVE_OTCLIENT_PLAYER_ID_END + 1
        );
        assert!(static_viewport.0.windows(3).any(|bytes| bytes == b"Rat"));
        let shared_players = [
            NativeOtClientVisiblePlayer {
                player_id: snapshot.player_id,
                name: "Duplicate Local".into(),
                position: snapshot.player_position,
                health_percent: 100,
                outfit: NativeOtClientClassicOutfit::from_snapshot(&snapshot),
                direction: snapshot.player_direction,
                speed: snapshot.player_speed,
            },
            NativeOtClientVisiblePlayer {
                player_id: NATIVE_OTCLIENT_PLAYER_ID_START + 1,
                name: "Druid".into(),
                position: NativeOtClientPosition {
                    x: 99,
                    y: 100,
                    z: 7,
                },
                health_percent: 100,
                outfit: NativeOtClientClassicOutfit::from_snapshot(&snapshot),
                direction: snapshot.player_direction,
                speed: snapshot.player_speed,
            },
        ];
        let shared_viewport = encode_native_otclient_map_viewport_with_static_spawns_and_players(
            &profile,
            &snapshot,
            &world_map,
            None,
            Some(&shared_players),
        )
        .unwrap();
        assert!(shared_viewport.0.windows(5).any(|bytes| bytes == b"Druid"));
        assert!(!shared_viewport
            .0
            .windows("Duplicate Local".len())
            .any(|bytes| bytes == b"Duplicate Local"));
        assert!(shared_viewport
            .0
            .windows(4)
            .any(|bytes| bytes == (NATIVE_OTCLIENT_PLAYER_ID_START + 1).to_le_bytes()));
        let map_initialization =
            encode_native_otclient_game_initialization_with_map(&profile, &snapshot, &world_map)
                .unwrap();
        assert_eq!(&map_initialization.0[..login.0.len()], login.0.as_slice());
        assert_eq!(
            &map_initialization.0[login.0.len()..login.0.len() + map_viewport.0.len()],
            map_viewport.0.as_slice()
        );

        let initialization =
            encode_native_otclient_game_initialization(&profile, &snapshot).unwrap();
        assert_eq!(&initialization.0[..login.0.len()], login.0.as_slice());
        assert_eq!(
            initialization.0[login.0.len()],
            NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(
            &initialization.0[login.0.len()..login.0.len() + map.0.len()],
            map.0.as_slice()
        );
        let mut persisted_vitals_snapshot = snapshot.clone();
        persisted_vitals_snapshot.player_vitals = NativeOtClientPlayerVitals {
            health: 95,
            max_health: 150,
            mana: 42,
            max_mana: 50,
            capacity: 32_000,
            magic_level: 4,
        };
        let persisted_stats =
            encode_native_otclient_player_stats(&profile, &persisted_vitals_snapshot).unwrap();
        assert_eq!(
            u16::from_le_bytes(persisted_stats.0[5..7].try_into().unwrap()),
            32_000
        );
        assert_eq!(
            u16::from_le_bytes(persisted_stats.0[14..16].try_into().unwrap()),
            42
        );
        assert_eq!(
            u16::from_le_bytes(persisted_stats.0[16..18].try_into().unwrap()),
            50
        );
        assert_eq!(persisted_stats.0[18], 4);
        let bootstrap = encode_native_otclient_player_bootstrap(&profile, &snapshot).unwrap();
        assert_eq!(bootstrap.0[0], NATIVE_OTCLIENT_GAME_PLAYER_STATS);
        assert_eq!(
            u16::from_le_bytes(bootstrap.0[1..3].try_into().unwrap()),
            150
        );
        assert_eq!(
            u16::from_le_bytes(bootstrap.0[3..5].try_into().unwrap()),
            150
        );
        assert_eq!(
            u16::from_le_bytes(bootstrap.0[5..7].try_into().unwrap()),
            40_000
        );
        assert_eq!(
            u32::from_le_bytes(bootstrap.0[7..11].try_into().unwrap()),
            4_900
        );
        assert_eq!(
            u16::from_le_bytes(bootstrap.0[11..13].try_into().unwrap()),
            8
        );
        assert_eq!(bootstrap.0[13], 0);
        assert_eq!(
            u16::from_le_bytes(bootstrap.0[14..16].try_into().unwrap()),
            50
        );
        assert_eq!(
            u16::from_le_bytes(bootstrap.0[16..18].try_into().unwrap()),
            50
        );
        assert_eq!(bootstrap.0[18], 0);
        assert_eq!(bootstrap.0[19], 0);
        assert_eq!(bootstrap.0[20], 0);
        assert_eq!(bootstrap.0[21], NATIVE_OTCLIENT_GAME_PLAYER_SKILLS);
        assert_eq!(bootstrap.0[36], NATIVE_OTCLIENT_GAME_PLAYER_STATE);
        assert_eq!(bootstrap.0[37], 0);
        assert_eq!(
            &initialization.0[login.0.len() + map.0.len()..],
            bootstrap.0.as_slice()
        );
        let classic_outfit = NativeOtClientClassicOutfit {
            look_type: 128,
            head: 1,
            body: 2,
            legs: 3,
            feet: 4,
        };
        assert_eq!(
            encode_native_otclient_choose_outfit(&profile, classic_outfit, 128, 131)
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT,
                128,
                1,
                2,
                3,
                4,
                128,
                131
            ]
        );
        assert_eq!(
            encode_native_otclient_creature_outfit(&profile, snapshot.player_id, classic_outfit)
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_CREATURE_OUTFIT,
                42,
                0,
                0,
                16,
                128,
                1,
                2,
                3,
                4
            ]
        );
        assert_eq!(
            encode_native_otclient_creature_health(&profile, snapshot.player_id, 75, 150)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_CREATURE_HEALTH, 42, 0, 0, 16, 50]
        );
        for (shield, expected_value) in [
            (NativeOtClientClassicPartyShield::None, 0),
            (NativeOtClientClassicPartyShield::InvitationFromLeader, 1),
            (NativeOtClientClassicPartyShield::InvitationToLeader, 2),
            (NativeOtClientClassicPartyShield::Member, 3),
            (NativeOtClientClassicPartyShield::Leader, 4),
        ] {
            assert_eq!(
                encode_native_otclient_creature_party_shield(&profile, snapshot.player_id, shield)
                    .unwrap()
                    .0,
                vec![
                    NATIVE_OTCLIENT_GAME_CREATURE_PARTY,
                    42,
                    0,
                    0,
                    16,
                    expected_value,
                ]
            );
        }
        assert_eq!(
            encode_native_otclient_creature_health(
                &profile,
                NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                40,
                100,
            )
            .unwrap()
            .0,
            vec![NATIVE_OTCLIENT_GAME_CREATURE_HEALTH, 1, 0, 0, 64, 40]
        );
        assert_eq!(
            encode_native_otclient_status_message(&profile, "You see a rat.")
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_TEXT_MESSAGE,
                NATIVE_OTCLIENT_MESSAGE_STATUS_DEFAULT,
                14,
                0,
                b'Y',
                b'o',
                b'u',
                b' ',
                b's',
                b'e',
                b'e',
                b' ',
                b'a',
                b' ',
                b'r',
                b'a',
                b't',
                b'.',
            ]
        );
        assert!(matches!(
            encode_native_otclient_status_message(
                &profile,
                &"x".repeat(NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES + 1),
            ),
            Err(ProtocolError::StringTooLong(length))
                if length == NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES + 1
        ));
        assert!(
            encode_native_otclient_creature_health(&profile, snapshot.player_id, 1, 0).is_err()
        );
        assert!(encode_native_otclient_creature_health(
            &profile,
            NATIVE_OTCLIENT_PLAYER_ID_END,
            100,
            100,
        )
        .is_err());
        assert!(encode_native_otclient_creature_party_shield(
            &profile,
            NATIVE_OTCLIENT_PLAYER_ID_END,
            NativeOtClientClassicPartyShield::None,
        )
        .is_err());
        assert!(encode_native_otclient_choose_outfit(&profile, classic_outfit, 131, 128).is_err());
        assert!(encode_native_otclient_creature_outfit(
            &profile,
            snapshot.player_id,
            NativeOtClientClassicOutfit {
                look_type: 0,
                ..classic_outfit
            }
        )
        .is_err());

        assert_eq!(
            decode_native_otclient_cardinal_move_request(&Frame(vec![0x66]), &profile).unwrap(),
            NativeOtClientCardinalDirection::East
        );
        assert!(
            decode_native_otclient_cardinal_move_request(&Frame(vec![0x66, 0]), &profile).is_err()
        );
        assert_eq!(
            decode_native_otclient_game_action(&Frame(vec![NATIVE_OTCLIENT_CLIENT_PING]), &profile)
                .unwrap(),
            NativeOtClientGameAction::Ping
        );
        assert_eq!(
            decode_native_otclient_game_action(&Frame(vec![NATIVE_OTCLIENT_ENTER_GAME]), &profile)
                .unwrap(),
            NativeOtClientGameAction::EnterGame
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES, 1, 0, 1]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::ChangeFightModes(NativeOtClientFightModeRequest {
                mode: NativeOtClientFightMode::Attack,
                chase: false,
                secure: true,
            })
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES, 9, 2, 3]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::ChangeFightModes(NativeOtClientFightModeRequest {
                mode: NativeOtClientFightMode::Defense,
                chase: true,
                secure: true,
            })
        );
        assert_eq!(
            encode_native_otclient_player_modes(
                &profile,
                NativeOtClientFightModeRequest {
                    mode: NativeOtClientFightMode::Defense,
                    chase: true,
                    secure: false,
                },
            )
            .unwrap()
            .0,
            vec![NATIVE_OTCLIENT_GAME_PLAYER_MODES, 3, 1, 0]
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_CLOSE_CONTAINER, 2]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::CloseContainer(2)
        );
        assert_eq!(
            encode_native_otclient_close_container(&profile, 2)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_CLOSE_CONTAINER, 2]
        );
        assert_eq!(
            encode_native_otclient_read_only_text_window(&profile, 42, 1988, "Read me")
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_EDIT_TEXT,
                42,
                0,
                0,
                0,
                196,
                7,
                7,
                0,
                7,
                0,
                b'R',
                b'e',
                b'a',
                b'd',
                b' ',
                b'm',
                b'e',
                0,
                0,
            ]
        );
        assert!(matches!(
            encode_native_otclient_read_only_text_window(&profile, 42, 1988, ""),
            Err(ProtocolError::StringTooLong(0))
        ));
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_UP_ARROW_CONTAINER, 2]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::UpArrowContainer(2)
        );
        assert!(matches!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_UP_ARROW_CONTAINER, 2, 0]),
                &profile,
            ),
            Err(ProtocolError::InvalidNativeGameRequest)
        ));
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_UPDATE_CONTAINER, 2]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::UpdateContainer(2)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_AUTO_WALK, 2, 1, 3]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::AutoWalk(vec![
                NativeOtClientAutoWalkDirection::East,
                NativeOtClientAutoWalkDirection::North,
            ])
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_AUTO_WALK, 65]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                    255,
                    255,
                    5,
                    0,
                    1,
                    102,
                    0,
                    0,
                    255,
                    255,
                    6,
                    0,
                    2,
                    1,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::ThrowItem {
                source_position: NativeOtClientPosition {
                    x: 0xffff,
                    y: 5,
                    z: 1,
                },
                source_client_thing_id: 102,
                source_stack_position: 0,
                target_position: NativeOtClientPosition {
                    x: 0xffff,
                    y: 6,
                    z: 2,
                },
                count: 1,
            }
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_THROW_ITEM; 14]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                    100,
                    0,
                    101,
                    0,
                    7,
                    102,
                    0,
                    3,
                    1,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::UseItem {
                position: NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                client_thing_id: 102,
                stack_position: 3,
                index: 1,
            }
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_USE_ITEM_EX,
                    100,
                    0,
                    101,
                    0,
                    7,
                    102,
                    0,
                    3,
                    99,
                    0,
                    98,
                    0,
                    7,
                    103,
                    0,
                    4,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::UseItemEx {
                source_position: NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                source_client_thing_id: 102,
                source_stack_position: 3,
                target_position: NativeOtClientPosition { x: 99, y: 98, z: 7 },
                target_client_thing_id: 103,
                target_stack_position: 4,
            }
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_USE_ITEM_EX, 0]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_USE_ITEM_ON_CREATURE,
                    100,
                    0,
                    101,
                    0,
                    7,
                    102,
                    0,
                    3,
                    1,
                    0,
                    0,
                    64,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::UseItemOnCreature {
                source_position: NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                source_client_thing_id: 102,
                source_stack_position: 3,
                target_creature_id: 0x4000_0001,
            }
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_USE_ITEM_ON_CREATURE, 0]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_ROTATE_ITEM,
                    100,
                    0,
                    101,
                    0,
                    7,
                    102,
                    0,
                    3,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::RotateItem {
                position: NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                client_thing_id: 102,
                stack_position: 3,
            }
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_ROTATE_ITEM, 0]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_USE_ITEM, 0]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_LOOK_MAP,
                    100,
                    0,
                    101,
                    0,
                    7,
                    102,
                    0,
                    3,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::LookMap {
                position: NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                thing_id: 102,
                stack_position: 3,
            }
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_LOOK_CREATURE, 1, 0, 0, 16,]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::LookCreature {
                creature_id: 0x1000_0001,
            }
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_LOOK_MAP, 0, 0, 0, 0, 0, 0, 0]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![0x77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::IgnoredInteraction(0x77)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::RequestOutfit
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::RequestQuestLog
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::RequestChannels
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_JOIN_CHANNEL, 7, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::JoinChannel(7)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_LEAVE_CHANNEL, 7, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::LeaveChannel(7)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_ADD_VIP,
                    5,
                    0,
                    b'D',
                    b'r',
                    b'u',
                    b'i',
                    b'd',
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::AddVip("Druid".into())
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_REMOVE_VIP, 7, 0, 0, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::RemoveVip(7)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_EDIT_VIP,
                    7,
                    0,
                    0,
                    0,
                    4,
                    0,
                    b'n',
                    b'o',
                    b't',
                    b'e',
                    3,
                    0,
                    0,
                    0,
                    1,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::EditVip {
                target_player_id: 7,
                description: "note".into(),
                icon: 3,
                notify: true,
            }
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![
                NATIVE_OTCLIENT_CLIENT_EDIT_VIP,
                7,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                2,
            ]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_JOIN_CHANNEL, 7]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_LEAVE_CHANNEL, 7]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS, 0]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG, 0]),
            &profile,
        )
        .is_err());
        assert_eq!(
            encode_native_otclient_empty_quest_log(&profile).unwrap().0,
            vec![NATIVE_OTCLIENT_GAME_QUEST_LOG, 0, 0]
        );
        assert_eq!(
            encode_native_otclient_empty_channel_list(&profile)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_CHANNELS, 0]
        );
        assert_eq!(
            encode_native_otclient_classic_vip_entry(&profile, 7, "Druid", false)
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_VIP_ADD,
                7,
                0,
                0,
                0,
                5,
                0,
                b'D',
                b'r',
                b'u',
                b'i',
                b'd',
                0,
            ]
        );
        assert_eq!(
            encode_native_otclient_classic_vip_presence(&profile, 7, true)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_VIP_STATE, 7, 0, 0, 0]
        );
        assert_eq!(
            encode_native_otclient_classic_vip_presence(&profile, 7, false)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_VIP_LOGOUT, 7, 0, 0, 0]
        );
        assert!(encode_native_otclient_classic_vip_presence(&profile, 0, true).is_err());
        assert_eq!(
            encode_native_otclient_channel_list(
                &profile,
                &[
                    NativeOtClientClassicChannel {
                        id: 1,
                        name: "World Chat".into(),
                    },
                    NativeOtClientClassicChannel {
                        id: 7,
                        name: "Trade".into(),
                    },
                ],
            )
            .unwrap()
            .0,
            vec![
                NATIVE_OTCLIENT_GAME_CHANNELS,
                2,
                1,
                0,
                10,
                0,
                b'W',
                b'o',
                b'r',
                b'l',
                b'd',
                b' ',
                b'C',
                b'h',
                b'a',
                b't',
                7,
                0,
                5,
                0,
                b'T',
                b'r',
                b'a',
                b'd',
                b'e',
            ]
        );
        assert_eq!(
            encode_native_otclient_open_public_channel(
                &profile,
                &NativeOtClientClassicChannel {
                    id: 7,
                    name: "Trade".into(),
                },
            )
            .unwrap()
            .0,
            vec![
                NATIVE_OTCLIENT_GAME_OPEN_CHANNEL,
                7,
                0,
                5,
                0,
                b'T',
                b'r',
                b'a',
                b'd',
                b'e',
                0,
                0,
                0,
                0,
            ]
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT, 128, 1, 2, 3, 4,]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::ChangeOutfit(NativeOtClientClassicOutfit {
                look_type: 128,
                head: 1,
                body: 2,
                legs: 3,
                feet: 4,
            })
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT, 128, 1, 2, 3,]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_SELECT_TARGET, 1, 0, 0, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::SelectTarget(1)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_SELECT_FOLLOW, 2, 0, 0, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::SelectFollow(2)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY, 3, 0, 0, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::PartyInvite(3)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_JOIN_PARTY, 4, 0, 0, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::PartyJoin(4)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_REVOKE_PARTY_INVITATION,
                    5,
                    0,
                    0,
                    0,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::PartyRevokeInvitation(5)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_PASS_PARTY_LEADERSHIP,
                    6,
                    0,
                    0,
                    0,
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::PartyPassLeadership(6)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_LEAVE_PARTY]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::PartyLeave
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE, 1, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::PartySharedExperience(true)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE, 0, 0]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::PartySharedExperience(false)
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY, 3, 0, 0]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE, 2, 0]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE, 1, 1]),
            &profile,
        )
        .is_err());
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_SELECT_TARGET, 0, 0, 0, 0, 0]),
            &profile,
        )
        .is_err());
        let mut oversized_interaction = vec![0xa3];
        oversized_interaction.extend(vec![0; NATIVE_OTCLIENT_MAX_IGNORED_INTERACTION_BYTES + 1]);
        assert!(
            decode_native_otclient_game_action(&Frame(oversized_interaction), &profile).is_err()
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_TALK, 1, 2, 0, b'h', b'i']),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::Talk(NativeOtClientTalkRequest {
                mode: 1,
                channel_id: None,
                recipient: None,
                message: "hi".into(),
            })
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_TALK, 7, 7, 0, 2, 0, b'h', b'i']),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::Talk(NativeOtClientTalkRequest {
                mode: 7,
                channel_id: Some(7),
                recipient: None,
                message: "hi".into(),
            })
        );
        assert_eq!(
            encode_native_otclient_public_channel_say(&profile, "Knight", 7, "hi")
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_TALK,
                6,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                7,
                7,
                0,
                2,
                0,
                b'h',
                b'i',
            ]
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![
                    NATIVE_OTCLIENT_CLIENT_TALK,
                    5,
                    5,
                    0,
                    b'D',
                    b'r',
                    b'u',
                    b'i',
                    b'd',
                    2,
                    0,
                    b'h',
                    b'i',
                ]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::Talk(NativeOtClientTalkRequest {
                mode: 5,
                channel_id: None,
                recipient: Some("Druid".into()),
                message: "hi".into(),
            })
        );
        assert_eq!(
            encode_native_otclient_private_message_from(&profile, "Knight", "hi")
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_TALK,
                6,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                4,
                2,
                0,
                b'h',
                b'i',
            ]
        );
        let whisper_position = NativeOtClientPosition {
            x: 100,
            y: 200,
            z: 7,
        };
        assert_eq!(
            encode_native_otclient_whisper(&profile, "Knight", whisper_position, "hi")
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_TALK,
                6,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                NATIVE_OTCLIENT_MESSAGE_WHISPER,
                100,
                0,
                200,
                0,
                7,
                2,
                0,
                b'h',
                b'i',
            ]
        );
        assert_eq!(
            encode_native_otclient_yell(&profile, "Knight", whisper_position, "hi")
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_TALK,
                6,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                NATIVE_OTCLIENT_MESSAGE_YELL,
                100,
                0,
                200,
                0,
                7,
                2,
                0,
                b'h',
                b'i',
            ]
        );
        assert_eq!(
            decode_native_otclient_game_action(&Frame(vec![NATIVE_OTCLIENT_CLIENT_STOP]), &profile)
                .unwrap(),
            NativeOtClientGameAction::Stop
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_CANCEL_ATTACK_AND_FOLLOW]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::CancelAttackAndFollow
        );
        assert!(decode_native_otclient_game_action(
            &Frame(vec![NATIVE_OTCLIENT_CLIENT_CANCEL_ATTACK_AND_FOLLOW, 0]),
            &profile,
        )
        .is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_WALK_NORTH_EAST]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::DiagonalMove(NativeOtClientAutoWalkDirection::NorthEast)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_EAST]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::DiagonalMove(NativeOtClientAutoWalkDirection::SouthEast)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_WEST]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::DiagonalMove(NativeOtClientAutoWalkDirection::SouthWest)
        );
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::DiagonalMove(NativeOtClientAutoWalkDirection::NorthWest)
        );
        assert!(decode_native_otclient_game_action(&Frame(vec![0x6e]), &profile,).is_err());
        assert_eq!(
            decode_native_otclient_game_action(
                &Frame(vec![NATIVE_OTCLIENT_CLIENT_TURN_SOUTH]),
                &profile,
            )
            .unwrap(),
            NativeOtClientGameAction::Turn(NativeOtClientCardinalDirection::South)
        );
        assert_eq!(
            NativeOtClientCardinalDirection::West.protocol_direction(),
            3
        );
        assert_eq!(
            decode_native_otclient_game_action(&Frame(vec![NATIVE_OTCLIENT_LEAVE_GAME]), &profile)
                .unwrap(),
            NativeOtClientGameAction::LeaveGame
        );
        assert_eq!(
            encode_native_otclient_game_ping_back(&profile).unwrap().0,
            vec![NATIVE_OTCLIENT_GAME_PING_BACK]
        );
        assert_eq!(
            encode_native_otclient_game_ping(&profile).unwrap().0,
            vec![NATIVE_OTCLIENT_GAME_PING]
        );
        assert_eq!(
            encode_native_otclient_clear_target(&profile).unwrap().0,
            vec![NATIVE_OTCLIENT_GAME_CLEAR_TARGET]
        );
        assert_eq!(
            encode_native_otclient_game_death(&profile).unwrap().0,
            vec![NATIVE_OTCLIENT_GAME_DEATH]
        );
        let incompatible_death_profile = NativeOtClientProfile {
            protocol_version: 800,
            ..profile.clone()
        };
        assert!(matches!(
            encode_native_otclient_game_death(&incompatible_death_profile),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
        let movement = encode_native_otclient_move_creature(
            &profile,
            snapshot.player_id,
            NativeOtClientPosition {
                x: 101,
                y: 100,
                z: 7,
            },
        )
        .unwrap();
        assert_eq!(movement.0[0], NATIVE_OTCLIENT_GAME_MOVE_CREATURE);
        assert_eq!(
            u16::from_le_bytes(movement.0[1..3].try_into().unwrap()),
            NATIVE_OTCLIENT_MAPPED_CREATURE
        );
        assert_eq!(
            u32::from_le_bytes(movement.0[3..7].try_into().unwrap()),
            snapshot.player_id
        );
        assert_eq!(&movement.0[7..12], &[101, 0, 100, 0, 7]);
        let coordinate_movement = encode_native_otclient_move_creature_at(
            &profile,
            snapshot.player_position,
            1,
            NativeOtClientPosition {
                x: 101,
                y: 100,
                z: 7,
            },
        )
        .unwrap();
        assert_eq!(coordinate_movement.0[0], NATIVE_OTCLIENT_GAME_MOVE_CREATURE);
        assert_eq!(&coordinate_movement.0[1..7], &[100, 0, 100, 0, 7, 1]);
        assert_eq!(&coordinate_movement.0[7..12], &[101, 0, 100, 0, 7]);
        assert_eq!(
            encode_native_otclient_game_cancel_walk(&profile).unwrap().0,
            vec![NATIVE_OTCLIENT_GAME_CANCEL_WALK, 0]
        );
        assert_eq!(
            encode_native_otclient_game_cancel_walk_facing(
                &profile,
                NativeOtClientCardinalDirection::South.protocol_direction(),
            )
            .unwrap()
            .0,
            vec![NATIVE_OTCLIENT_GAME_CANCEL_WALK, 2]
        );

        let asset_free_snapshot = NativeOtClientEmptyWorldSnapshot {
            ground_thing_id: 0,
            player_look_type: 0,
            ..snapshot
        };
        let asset_free_map =
            encode_native_otclient_empty_world_map(&profile, &asset_free_snapshot).unwrap();
        assert_eq!(asset_free_map.0[0], NATIVE_OTCLIENT_GAME_FULL_MAP);
        assert_eq!(asset_free_map.0.len(), 1 + 5 + cells * 2);
        assert_eq!(
            u16::from_le_bytes(asset_free_map.0[6..8].try_into().unwrap()),
            NATIVE_OTCLIENT_TILE_END
        );
        assert!(!asset_free_map
            .0
            .windows(asset_free_snapshot.player_name.len())
            .any(|bytes| bytes == asset_free_snapshot.player_name.as_bytes()));
    }

    #[test]
    fn classic_740_container_open_record_is_profile_gated_and_parser_shaped() {
        let profile = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        let container = NativeOtClientClassicOpenContainer {
            container_id: 1,
            container_item: NativeOtClientClassicItemRecord {
                client_thing_id: 1988,
                subtype: None,
            },
            name: "Backpack".into(),
            capacity: 20,
            has_parent: false,
            items: vec![
                NativeOtClientClassicItemRecord {
                    client_thing_id: 102,
                    subtype: Some(25),
                },
                NativeOtClientClassicItemRecord {
                    client_thing_id: 2463,
                    subtype: None,
                },
            ],
        };
        assert_eq!(
            encode_native_otclient_open_container(&profile, &container)
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_OPEN_CONTAINER,
                1,
                196,
                7,
                8,
                0,
                b'B',
                b'a',
                b'c',
                b'k',
                b'p',
                b'a',
                b'c',
                b'k',
                20,
                0,
                2,
                102,
                0,
                25,
                159,
                9,
            ]
        );
        assert!(matches!(
            encode_native_otclient_open_container(
                &profile,
                &NativeOtClientClassicOpenContainer {
                    name: String::new(),
                    ..container.clone()
                }
            ),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
        let incompatible_profile = NativeOtClientProfile {
            protocol_version: 800,
            ..profile
        };
        assert!(matches!(
            encode_native_otclient_open_container(&incompatible_profile, &container),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
    }

    #[test]
    fn classic_740_inventory_records_are_profile_gated_and_parser_shaped() {
        let profile = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        let stackable = encode_native_otclient_set_inventory(
            &profile,
            EquipmentSlot::RightHand,
            NativeOtClientClassicItemRecord {
                client_thing_id: 102,
                subtype: Some(25),
            },
        )
        .unwrap();
        assert_eq!(
            stackable.0,
            vec![NATIVE_OTCLIENT_GAME_SET_INVENTORY, 5, 102, 0, 25]
        );
        let non_stackable = encode_native_otclient_set_inventory(
            &profile,
            EquipmentSlot::Armor,
            NativeOtClientClassicItemRecord {
                client_thing_id: 2463,
                subtype: None,
            },
        )
        .unwrap();
        assert_eq!(
            non_stackable.0,
            vec![NATIVE_OTCLIENT_GAME_SET_INVENTORY, 4, 159, 9]
        );
        assert_eq!(
            encode_native_otclient_delete_inventory(&profile, EquipmentSlot::LeftHand)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_DELETE_INVENTORY, 6]
        );
        assert!(matches!(
            encode_native_otclient_set_inventory(
                &profile,
                EquipmentSlot::Head,
                NativeOtClientClassicItemRecord {
                    client_thing_id: 0,
                    subtype: None,
                },
            ),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
        let incompatible_profile = NativeOtClientProfile {
            protocol_version: 800,
            ..profile
        };
        assert!(matches!(
            encode_native_otclient_delete_inventory(&incompatible_profile, EquipmentSlot::Head),
            Err(ProtocolError::UnsupportedNativeClientProfile)
        ));
    }

    #[test]
    fn classic_740_player_skills_use_typed_order_and_bounded_presentation() {
        let profile = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        let mut skills = PlayerSkills::default();
        skills.set(
            forgotten_core::PlayerSkill::Sword,
            forgotten_core::SkillProgress::new(65, 42).unwrap(),
        );
        skills.set(
            forgotten_core::PlayerSkill::Fishing,
            forgotten_core::SkillProgress::new(512, 100).unwrap(),
        );
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START + 7,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: skills,
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        assert_eq!(
            encode_native_otclient_player_skills(&profile, &snapshot)
                .unwrap()
                .0,
            vec![
                NATIVE_OTCLIENT_GAME_PLAYER_SKILLS,
                10,
                0,
                10,
                0,
                65,
                42,
                10,
                0,
                10,
                0,
                10,
                0,
                255,
                100
            ]
        );
    }

    #[test]
    fn dense_native_viewport_remains_within_the_frame_budget() {
        let profile = NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        };
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START + 7,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: PlayerSkills::default(),
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let mut world_map = WorldMap::new(
            "dense-viewport",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        let item = forgotten_core::WorldMapItem {
            server_id: 102,
            client_thing_id: Some(102),
            count: 1,
            action_id: None,
            unique_id: None,
            text: None,
            description: None,
            teleport_destination: None,
            duration: None,
            charges: None,
            children: Vec::new(),
        };
        let center_x = (NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1) as i16;
        let center_y = (NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1) as i16;
        for z in 0..NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8 {
            for x in 0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH {
                for y in 0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT {
                    let position = Position {
                        x: snapshot
                            .player_position
                            .x
                            .saturating_add_signed(x as i16 - center_x),
                        y: snapshot
                            .player_position
                            .y
                            .saturating_add_signed(y as i16 - center_y),
                        z,
                    };
                    world_map
                        .set_tile(
                            position,
                            forgotten_core::WorldMapTile {
                                ground_thing_id: 102,
                                walkable: true,
                            },
                        )
                        .unwrap();
                    world_map
                        .set_tile_items(position, vec![item.clone(); 9])
                        .unwrap();
                }
            }
        }
        let frame = encode_native_otclient_map_viewport(&profile, &snapshot, &world_map).unwrap();
        assert_eq!(frame.0[0], NATIVE_OTCLIENT_GAME_FULL_MAP);
        assert!(frame.0.len() <= MAX_FRAME_SIZE);
    }
}
