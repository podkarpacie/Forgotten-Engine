//! Status protocol records, player/software metrics, RSA key material, XTEA,
//! and the FE 8.0 RSA/XTEA transport bootstrap.

use super::*;

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
    /// Resident set size in KiB when the platform exposes it; serialized as null otherwise.
    pub process_memory_kib: Option<u32>,
}

pub fn encode_status_metrics(metrics: &StatusMetrics) -> Vec<u8> {
    let memory = match metrics.process_memory_kib {
        Some(kib) => kib.to_string(),
        None => "null".into(),
    };
    format!(
        "{{\"uptime_seconds\":{},\"registered_accounts\":{},\"registered_characters\":{},\"schema_version\":{},\"players_online\":{},\"players_online_cap\":{},\"process_memory_kib\":{}}}\n",
        metrics.uptime_seconds,
        metrics.registered_accounts,
        metrics.registered_characters,
        metrics.schema_version,
        metrics.players_online,
        metrics.players_online_cap,
        memory
    )
    .into_bytes()
}

/// Resident set size in KiB from the Linux status interface; `None` on platforms without it.
/// Cloud panels scrape this for the server_metrics model.
pub fn process_memory_kib() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
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
        let block: &mut [u8; 8] = block
            .try_into()
            .map_err(|_| ProtocolError::InvalidXteaLength(8))?;
        let (mut left, mut right) = (
            u32::from_le_bytes(
                block[..4]
                    .try_into()
                    .map_err(|_| ProtocolError::InvalidXteaLength(4))?,
            ),
            u32::from_le_bytes(
                block[4..]
                    .try_into()
                    .map_err(|_| ProtocolError::InvalidXteaLength(4))?,
            ),
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
