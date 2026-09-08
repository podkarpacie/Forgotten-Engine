//! Bounded transport contracts for Forgotten Engine profiles.
//!
//! The legacy 7.4 types below are a tested foundation, not a claim of official-client support.

#![deny(clippy::unwrap_used, clippy::expect_used)]

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

mod status;
pub use status::*;
mod legacy74;
pub use legacy74::*;

mod fe_otclient;
pub use fe_otclient::*;

mod native_types;
pub use native_types::*;
mod native;
pub use native::*;

pub const MAX_FRAME_SIZE: usize = 8 * 1024;
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

pub const FE_7_4_PROFILE: CompatibilityProfile = CompatibilityProfile {
    id: "fe-7.4",
    fe_release: FE_7_4_RELEASE,
    compatibility_reference: "Tibia 7.4 protocol",
    tibia_protocol: "7.4",
    complete_protocol_emulation: false,
};
pub const COMPATIBILITY_PROFILES: [CompatibilityProfile; 1] = [FE_7_4_PROFILE];

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
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| ProtocolError::Truncated)?;
        Ok(u16::from_le_bytes(bytes))
    }
    fn u32(&mut self) -> Result<u32, ProtocolError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ProtocolError::Truncated)?;
        Ok(u32::from_le_bytes(bytes))
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
        assert_eq!(profile_by_id("fe-7.4"), Some(FE_7_4_PROFILE));
        assert_eq!(profile_by_id("fe-8.0"), None);
        assert_eq!(profile_by_id("fe-1.2"), None);
    }

    #[test]
    fn native_profile_foundations_distinguish_runnable_classic_from_encrypted_800() {
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
                process_memory_kib: Some(4096),
            }))
            .unwrap(),
            "{\"uptime_seconds\":12,\"registered_accounts\":3,\"registered_characters\":7,\"schema_version\":27,\"players_online\":2,\"players_online_cap\":100,\"process_memory_kib\":4096}\n"
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
            name_description: String::new(),
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
            encode_native_otclient_create_in_container(
                &profile,
                3,
                NativeOtClientClassicItemRecord {
                    client_thing_id: 3584,
                    subtype: None,
                },
            )
            .unwrap()
            .0,
            vec![NATIVE_OTCLIENT_GAME_CREATE_IN_CONTAINER, 3, 0x00, 0x0e,]
        );
        assert_eq!(
            encode_native_otclient_change_in_container(
                &profile,
                1,
                4,
                NativeOtClientClassicItemRecord {
                    client_thing_id: 3577,
                    subtype: Some(37),
                },
            )
            .unwrap()
            .0,
            vec![
                NATIVE_OTCLIENT_GAME_CHANGE_IN_CONTAINER,
                1,
                4,
                0xf9,
                0x0d,
                37,
            ]
        );
        assert_eq!(
            encode_native_otclient_delete_in_container(&profile, 2, 6)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_DELETE_IN_CONTAINER, 2, 6]
        );
        assert_eq!(
            encode_native_otclient_distance_effect(
                &profile,
                NativeOtClientPosition {
                    x: 100,
                    y: 100,
                    z: 7
                },
                NativeOtClientPosition {
                    x: 105,
                    y: 103,
                    z: 7
                },
                29,
            )
            .unwrap()
            .0,
            vec![
                NATIVE_OTCLIENT_GAME_DISTANCE_EFFECT,
                100,
                0,
                100,
                0,
                7,
                105,
                0,
                103,
                0,
                7,
                29,
            ]
        );
        assert!(encode_native_otclient_distance_effect(
            &profile,
            NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7
            },
            NativeOtClientPosition {
                x: 101,
                y: 100,
                z: 7
            },
            0,
        )
        .is_err());
        assert_eq!(
            encode_native_otclient_creature_skull(&profile, 0x7000_0001, 3)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_CREATURE_SKULL, 1, 0, 0, 0x70, 3]
        );
        assert!(encode_native_otclient_creature_skull(&profile, 1, 7).is_err());
        assert_eq!(
            encode_native_otclient_creature_unpass(&profile, 0x4000_0002, true)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_CREATURE_UNPASS, 2, 0, 0, 0x40, 1]
        );
        assert_eq!(
            encode_native_otclient_player_state_bits(&profile, 0x0005)
                .unwrap()
                .0,
            vec![NATIVE_OTCLIENT_GAME_PLAYER_STATE, 0x05, 0x00]
        );
        assert!(encode_native_otclient_create_in_container(
            &NativeOtClientProfile {
                protocol_version: 860,
                numeric_account_ids: true,
                login_packet_encryption: false,
                protocol_checksum: false,
                challenge_on_login: false,
                max_padding_bytes: 128,
            },
            3,
            NativeOtClientClassicItemRecord {
                client_thing_id: 3584,
                subtype: None,
            },
        )
        .is_err());
        assert!(encode_native_otclient_change_in_container(
            &profile,
            1,
            0,
            NativeOtClientClassicItemRecord {
                client_thing_id: 0,
                subtype: None,
            },
        )
        .is_err());
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

/// Tests for the classic-760 runnable profile and the visible-text encoder set. The 760
/// foundation exists because an unmodified OTCv8 at protocol 740 keeps an empty message-mode
/// map and discards every 0xAA/0xB4 record; 760 is byte-identical on the wire but renders.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod classic_760_visible_text_tests {
    use super::*;

    fn profile_740() -> NativeOtClientProfile {
        NativeOtClientProfile {
            protocol_version: 740,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        }
    }

    fn profile_760() -> NativeOtClientProfile {
        NativeOtClientProfile {
            protocol_version: 760,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        }
    }

    #[test]
    fn seven_sixty_is_a_runnable_classic_foundation_with_visible_text() {
        assert!(profile_740().supports_current_native_foundation());
        assert!(!profile_740().supports_visible_text_messages());
        assert_eq!(
            profile_760().foundation(),
            NativeOtClientFoundation::PlainClassic760
        );
        assert!(profile_760().supports_current_native_foundation());
        assert!(profile_760().supports_classic_740_inventory_records());
        assert!(profile_760().supports_visible_text_messages());
    }

    #[test]
    fn gm_broadcast_record_matches_the_classic_mode_nine_layout() {
        let frame =
            encode_native_otclient_gm_broadcast(&profile_760(), "Console", "server restart")
                .unwrap();
        let mut expected = vec![NATIVE_OTCLIENT_GAME_TALK, 7, 0];
        expected.extend_from_slice(b"Console");
        expected.push(NATIVE_OTCLIENT_MESSAGE_GM_BROADCAST);
        expected.extend_from_slice(&[14, 0]);
        expected.extend_from_slice(b"server restart");
        assert_eq!(frame.0, expected);
    }

    #[test]
    fn look_failure_login_records_carry_their_classes() {
        let look = encode_native_otclient_look_message(&profile_760(), "You see a rat.").unwrap();
        assert_eq!(look.0[0], NATIVE_OTCLIENT_GAME_TEXT_MESSAGE);
        assert_eq!(look.0[1], NATIVE_OTCLIENT_MESSAGE_LOOK);
        let failure =
            encode_native_otclient_failure_message(&profile_760(), "Not possible.").unwrap();
        assert_eq!(failure.0[1], NATIVE_OTCLIENT_MESSAGE_FAILURE);
        let login = encode_native_otclient_login_message(&profile_760(), "Welcome.").unwrap();
        assert_eq!(login.0[1], NATIVE_OTCLIENT_MESSAGE_LOGIN);
    }

    #[test]
    fn animated_text_and_magic_effect_match_parser_layouts() {
        let position = NativeOtClientPosition {
            x: 100,
            y: 200,
            z: 7,
        };
        let animated =
            encode_native_otclient_animated_text(&profile_760(), position, 180, "12").unwrap();
        assert_eq!(
            animated.0,
            vec![
                NATIVE_OTCLIENT_GAME_ANIMATED_TEXT,
                100,
                0,
                200,
                0,
                7,
                180,
                2,
                0,
                b'1',
                b'2'
            ]
        );
        let effect = encode_native_otclient_magic_effect(&profile_760(), position, 3).unwrap();
        assert_eq!(
            effect.0,
            vec![NATIVE_OTCLIENT_GAME_MAGIC_EFFECT, 100, 0, 200, 0, 7, 3]
        );
    }

    #[test]
    fn encoders_still_reject_profiles_without_a_runnable_foundation() {
        let mut unsupported = profile_740();
        unsupported.numeric_account_ids = false;
        assert!(encode_native_otclient_gm_broadcast(&unsupported, "A", "b").is_err());
        assert!(encode_native_otclient_look_message(&unsupported, "x").is_err());
        assert!(encode_native_otclient_animated_text(
            &unsupported,
            NativeOtClientPosition { x: 0, y: 0, z: 0 },
            1,
            "t"
        )
        .is_err());
    }
}

/// NPC shop window coverage: 0x7A catalog record layout, 0x7B player-goods record, and the
/// close-shop zero-payload frame.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod npc_shop_window_tests {
    use super::*;

    fn profile_760() -> NativeOtClientProfile {
        NativeOtClientProfile {
            protocol_version: 760,
            numeric_account_ids: true,
            login_packet_encryption: false,
            protocol_checksum: false,
            challenge_on_login: false,
            max_padding_bytes: 128,
        }
    }

    #[test]
    fn open_npc_trade_record_matches_classic_layout() {
        let items = vec![
            NativeOtClientShopItem {
                client_thing_id: 2666,
                subtype: None,
                name: "ham".into(),
                weight: 800,
                buy_price: 8,
                sell_price: 4,
            },
            NativeOtClientShopItem {
                client_thing_id: 2854,
                subtype: None,
                name: "backpack".into(),
                weight: 1800,
                buy_price: 20,
                sell_price: 10,
            },
        ];
        let frame = encode_native_otclient_open_npc_trade(&profile_760(), &items).unwrap();
        let mut expected = vec![
            NATIVE_OTCLIENT_GAME_OPEN_NPC_TRADE,
            2, // list count (classic u8)
        ];
        // Item 1: id + name + weight + buy + sell (no subtype for non-stackables)
        expected.extend_from_slice(&[
            u8::try_from(2666 & 0xff).unwrap(),
            u8::try_from((2666 >> 8) & 0xff).unwrap(),
        ]);
        expected.extend_from_slice(&[3, 0]);
        expected.extend_from_slice(b"ham");
        expected.extend_from_slice(&[32, 3, 0, 0]); // weight 800
        expected.extend_from_slice(&[8, 0, 0, 0]); // buy 8
        expected.extend_from_slice(&[4, 0, 0, 0]); // sell 4
                                                   // Item 2
        expected.extend_from_slice(&[
            u8::try_from(2854 & 0xff).unwrap(),
            u8::try_from((2854 >> 8) & 0xff).unwrap(),
        ]);
        expected.extend_from_slice(&[8, 0]);
        expected.extend_from_slice(b"backpack");
        expected.extend_from_slice(&[8, 7, 0, 0]); // weight 1800
        expected.extend_from_slice(&[20, 0, 0, 0]);
        expected.extend_from_slice(&[10, 0, 0, 0]);
        assert_eq!(frame.0, expected);
    }

    #[test]
    fn player_goods_record_carries_gold_and_sellable_list() {
        let goods = vec![NativeOtClientPlayerGood {
            client_thing_id: 3031,
            amount: 250,
        }];
        let frame = encode_native_otclient_player_goods(&profile_760(), 12345, &goods).unwrap();
        let mut expected = vec![NATIVE_OTCLIENT_GAME_PLAYER_GOODS];
        expected.extend_from_slice(&[57, 48, 0, 0]); // 12345 LE
        expected.push(1); // one good
        expected.extend_from_slice(&[
            u8::try_from(3031 & 0xff).unwrap(),
            u8::try_from((3031 >> 8) & 0xff).unwrap(),
        ]);
        expected.push(250);
        assert_eq!(frame.0, expected);
    }

    #[test]
    fn close_npc_trade_is_a_zero_payload_frame() {
        let frame = encode_native_otclient_close_npc_trade(&profile_760()).unwrap();
        assert_eq!(frame.0, vec![NATIVE_OTCLIENT_GAME_CLOSE_NPC_TRADE]);
    }
}
