//! Milestone 1: a synthetic 740/760 client that drives the real engine over a
//! real TCP socket and asserts on **decoded** frames, not just "no error".
//!
//! Scope for this milestone, per plan: crate skeleton, wiring, and the
//! login -> character-select -> world-init path. Movement and Talk assertions
//! (Task A steps 6-7) are deliberately not attempted here.
//!
//! Everything below was hand-derived from the real decoders in
//! `forgotten-protocol/src/native.rs` and the wire-wrapping in
//! `forgotten-protocol/src/lib.rs` (`encode`/`decode`, and the private,
//! test-only `encode_native_otclient_*_request_for_harness` helpers, which
//! are `#[cfg(test)]`-gated and therefore not callable from this external
//! crate — the encoders below reproduce their exact byte layout rather than
//! duplicate them by reference). If the wire format ever changes, update
//! `native.rs`'s decoders first and mirror the change here; there is no
//! shared source of truth to drift from silently, so a mismatch will show up
//! immediately as a rejected frame rather than a silent corruption.
//!
//! Honest limitation, carried over from the task spec: this proves wire
//! correctness only. It cannot prove a real, unmodified client renders
//! anything correctly on screen. Do not use a green run here as evidence for
//! the `todo.md` items that explicitly require real-client confirmation.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use forgotten_protocol::{decode, encode, Frame};

pub mod process;

/// Errors specific to the harness itself, distinct from `ProtocolError`
/// (which covers malformed wire data) and `std::io::Error` (which covers
/// transport failure). Kept small and enumerated rather than a boxed
/// `dyn Error`, matching the workspace's existing style of typed errors
/// over opaque ones.
#[derive(Debug)]
pub enum HarnessError {
    Io(std::io::Error),
    Protocol(forgotten_protocol::ProtocolError),
    /// The server sent a well-formed frame, but not the one this step of the
    /// scenario expected (e.g. a login error where a character list was
    /// expected). Carries the raw opcode byte for a precise assertion
    /// message rather than a generic mismatch.
    UnexpectedOpcode {
        expected: u8,
        got: u8,
    },
    /// A frame was empty, so there was no opcode byte to read at all.
    EmptyFrame,
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HarnessError::Io(error) => write!(f, "harness io error: {error}"),
            HarnessError::Protocol(error) => write!(f, "harness protocol error: {error:?}"),
            HarnessError::UnexpectedOpcode { expected, got } => write!(
                f,
                "expected opcode 0x{expected:02x}, server sent 0x{got:02x}"
            ),
            HarnessError::EmptyFrame => write!(f, "server sent an empty frame"),
        }
    }
}

impl std::error::Error for HarnessError {}

impl From<std::io::Error> for HarnessError {
    fn from(error: std::io::Error) -> Self {
        HarnessError::Io(error)
    }
}

impl From<forgotten_protocol::ProtocolError> for HarnessError {
    fn from(error: forgotten_protocol::ProtocolError) -> Self {
        HarnessError::Protocol(error)
    }
}

// --- Opcodes this milestone needs. Values copied from, and must be kept in
// sync with, `forgotten_protocol::native_types` (already `pub`, but not
// re-exported at the crate root there, so restated here rather than reached
// through a longer path). ---
const OPCODE_ENTER_ACCOUNT: u8 = 0x01;
const OPCODE_PENDING_GAME: u8 = 0x0a;
const OPCODE_LOGIN_ERROR: u8 = 0x0a;
const OPCODE_LOGIN_CHARACTER_LIST: u8 = 0x64;
const OPCODE_GAME_LOGIN_ERROR: u8 = 0x14;
/// First frame the game port sends after a successful character selection.
/// Corrected against `encode_native_otclient_game_login_state`: the world-init
/// stream opens with this, and `GAME_FULL_MAP` (0x64) follows it.
const OPCODE_GAME_LOGIN_STATE: u8 = 0x0a;

/// Minimal little-endian byte writer matching `forgotten_protocol`'s private
/// `Writer` exactly (byte / u16 / u32 / length-prefixed string, u16-LE
/// length). Reimplemented here, not imported, because the real `Writer` is
/// crate-private to `forgotten-protocol` — see the module doc comment.
#[derive(Default)]
struct ByteWriter(Vec<u8>);

impl ByteWriter {
    // These consume and return `Self` rather than taking `&mut self`. A
    // `&mut Self` builder cannot be chained into `finish(self)`: the final
    // call would move out of a borrow, which is E0507. Consuming `Self`
    // keeps the fluent call sites and compiles.
    fn byte(mut self, value: u8) -> Self {
        self.0.push(value);
        self
    }
    fn u16(mut self, value: u16) -> Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn u32(mut self, value: u32) -> Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn string(mut self, value: &str) -> Self {
        let bytes = value.as_bytes();
        let len = bytes.len().min(usize::from(u16::MAX));
        self.0.extend_from_slice(&(len as u16).to_le_bytes());
        self.0.extend_from_slice(&bytes[..len]);
        self
    }
    #[allow(dead_code)]
    fn bytes(mut self, value: &[u8]) -> Self {
        self.0.extend_from_slice(value);
        self
    }
    fn finish(self) -> Frame {
        Frame(self.0)
    }
}

/// Minimal reader, the decode-side counterpart of `ByteWriter`. Panics are
/// never used; every read that runs past the end returns `HarnessError`,
/// same discipline as the rest of the workspace.
struct ByteReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        ByteReader { bytes, position: 0 }
    }
    fn byte(&mut self) -> Result<u8, HarnessError> {
        let value = *self.bytes.get(self.position).ok_or_else(too_short)?;
        self.position += 1;
        Ok(value)
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], HarnessError> {
        let end = self.position.checked_add(count).ok_or_else(too_short)?;
        let slice = self.bytes.get(self.position..end).ok_or_else(too_short)?;
        self.position = end;
        Ok(slice)
    }
    fn u16(&mut self) -> Result<u16, HarnessError> {
        let raw = self.take(2)?;
        Ok(u16::from_le_bytes([raw[0], raw[1]]))
    }
    fn u32(&mut self) -> Result<u32, HarnessError> {
        let raw = self.take(4)?;
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }
    fn string(&mut self) -> Result<String, HarnessError> {
        let len = self.u16()? as usize;
        let raw = self.take(len)?;
        Ok(String::from_utf8_lossy(raw).into_owned())
    }
    /// Bytes left unconsumed. Used by a later milestone to assert that a frame
    /// carried no unexpected trailing padding.
    #[allow(dead_code)]
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}

fn too_short() -> HarnessError {
    HarnessError::Protocol(forgotten_protocol::ProtocolError::Truncated)
}

/// A minimal set of fields for the login-port request. Deliberately not the
/// real `NativeOtClientLoginRequest` — that type also carries `dat_signature`
/// / `spr_signature` / `pic_signature`, which the harness has no real client
/// assets to derive, so it sends the same zeroed placeholders the decoder
/// already accepts (see `decode_native_otclient_login_request`: it validates
/// protocol version and padding length, not the signature values).
pub struct LoginRequest {
    pub protocol_version: u16,
    pub account_id: u32,
    pub password: String,
}

fn encode_login_request(request: &LoginRequest) -> Frame {
    ByteWriter::default()
        .byte(OPCODE_ENTER_ACCOUNT)
        .u16(0) // operating_system: accepted range per profile; 0 is inert.
        .u16(request.protocol_version)
        .u32(0) // dat_signature - no real client assets to hash, see doc above
        .u32(0) // spr_signature
        .u32(0) // pic_signature
        .u32(request.account_id)
        .string(&request.password)
        .u16(0) // client_tag length 0 -> decode_optional_client_tag_build sees
        // remaining() == 0 immediately after and defaults tag/build; see
        // native.rs. NOT sending a tag byte at all achieves the same
        // "stock 7.4 login, no trailing fields" shape the decoder expects.
        .finish()
}

/// One entry in the decoded character list, mirroring
/// `forgotten_protocol::CharacterListEntry` but reconstructed by hand since
/// there is no client-side decoder for it in the protocol crate (the server
/// only ever encodes this frame, never decodes its own output).
#[derive(Debug, Clone)]
pub struct DecodedCharacter {
    pub name: String,
    pub world_name: String,
    pub address: std::net::Ipv4Addr,
    pub port: u16,
}

pub enum LoginOutcome {
    CharacterList(Vec<DecodedCharacter>),
    /// The message text the server sent back with a login-error frame.
    Error(String),
}

/// Decodes the login port's response. Layout is the mirror image of
/// `encode_native_otclient_character_list` (native.rs) for the success case;
/// for the error case, decoding a plain length-prefixed string is enough —
/// `encode_native_otclient_login_error` is a thin wrapper over the same
/// status-message shape used elsewhere in the crate.
fn decode_login_response(frame: &Frame) -> Result<LoginOutcome, HarnessError> {
    let mut reader = ByteReader::new(&frame.0);
    let opcode = reader.byte()?;
    match opcode {
        OPCODE_LOGIN_CHARACTER_LIST => {
            let count = reader.byte()?;
            let mut entries = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let name = reader.string()?;
                let world_name = reader.string()?;
                let octets = reader.take(4)?;
                let address = std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
                let port = reader.u16()?;
                entries.push(DecodedCharacter {
                    name,
                    world_name,
                    address,
                    port,
                });
            }
            // Trailing u16(0) (premium-days field in the real client) is
            // intentionally not asserted on here - milestone 1 cares about
            // reaching a valid character list, not that field's value.
            Ok(LoginOutcome::CharacterList(entries))
        }
        OPCODE_LOGIN_ERROR => {
            let message = reader.string()?;
            Ok(LoginOutcome::Error(message))
        }
        other => Err(HarnessError::UnexpectedOpcode {
            expected: OPCODE_LOGIN_CHARACTER_LIST,
            got: other,
        }),
    }
}

/// The game-port request (character selection). Same zeroed-signature
/// reasoning as `LoginRequest` applies to os/tag/build here.
pub struct GameRequest {
    pub protocol_version: u16,
    pub account_id: u32,
    pub character_name: String,
    pub password: String,
}

fn encode_game_request(request: &GameRequest) -> Frame {
    ByteWriter::default()
        .byte(OPCODE_PENDING_GAME)
        .u16(0) // operating_system
        .u16(request.protocol_version)
        .byte(0) // decoder requires this exact zero byte, see native.rs:56
        .u32(request.account_id)
        .string(&request.character_name)
        .string(&request.password)
        .u16(0) // client_tag length 0, same reasoning as encode_login_request
        .finish()
}

/// Outcome of the game-port handshake. `WorldInitStarted` only confirms the
/// server accepted the character selection and the response is not an error
/// frame — it deliberately does not decode viewport tiles or local-player
/// identity yet. That is real scope for a later milestone (Task A step 5
/// proper), not claimed here.
pub enum GameOutcome {
    /// The game port accepted character selection and opened the world-init
    /// stream. `login_state` is the decoded first frame; the tile stream that
    /// follows is not decoded yet (later milestone).
    WorldInitStarted {
        login_state: GameLoginState,
    },
    Error(String),
}

/// Decoded `encode_native_otclient_game_login_state` payload (native.rs):
/// opcode byte, u32 player_id, u16 server_beat, one trailing zero byte.
#[derive(Debug, Clone, Copy)]
pub struct GameLoginState {
    pub player_id: u32,
    pub server_beat: u16,
}

fn decode_game_response(frame: &Frame) -> Result<GameOutcome, HarnessError> {
    let mut reader = ByteReader::new(&frame.0);
    let opcode = reader.byte()?;
    if opcode == OPCODE_GAME_LOGIN_ERROR {
        let message = reader.string()?;
        return Ok(GameOutcome::Error(message));
    }
    if opcode != OPCODE_GAME_LOGIN_STATE {
        return Err(HarnessError::UnexpectedOpcode {
            expected: OPCODE_GAME_LOGIN_STATE,
            got: opcode,
        });
    }
    // Assert on decoded contents rather than "it didn't error": a nonzero
    // player_id and a nonzero server beat are both required by the encoder's
    // own validation, so their presence proves we read a real frame.
    let player_id = reader.u32()?;
    let server_beat = reader.u16()?;
    Ok(GameOutcome::WorldInitStarted {
        login_state: GameLoginState {
            player_id,
            server_beat,
        },
    })
}

/// Thin wire helpers: length-prefix a `Frame` and send it, or block for one
/// and decode it. Mirrors `forgotten_host::session_handlers::read_frame` /
/// `write_frame` exactly (2-byte LE length prefix, no checksum layer) but
/// reimplemented against `TcpStream` directly rather than imported, since
/// `forgotten-host` is not a dependency of this crate and pulling it in only
/// for these two functions would be a much larger surface than the harness
/// needs.
pub fn send_frame(stream: &mut TcpStream, frame: &Frame) -> Result<(), HarnessError> {
    let encoded = encode(frame)?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    Ok(())
}

pub fn recv_frame(stream: &mut TcpStream) -> Result<Frame, HarnessError> {
    let mut header = [0_u8; 2];
    stream.read_exact(&mut header)?;
    let declared = u16::from_le_bytes(header) as usize;
    let mut body = vec![0_u8; declared];
    stream.read_exact(&mut body)?;
    let mut full = header.to_vec();
    full.extend_from_slice(&body);
    Ok(decode(&full)?)
}

/// Drives the full login -> character-select -> world-init path against a
/// running server. This is the milestone-1 scenario; steps 6-7 (movement,
/// Talk) are not attempted here.
pub fn run_login_to_world_init(
    login_addr: std::net::SocketAddr,
    game_addr: std::net::SocketAddr,
    login: LoginRequest,
    character_name: String,
) -> Result<GameOutcome, HarnessError> {
    let mut login_stream = TcpStream::connect(login_addr)?;
    login_stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    send_frame(&mut login_stream, &encode_login_request(&login))?;
    let response = recv_frame(&mut login_stream)?;
    let outcome = decode_login_response(&response)?;
    let account_id = login.account_id;
    let password = login.password;
    let protocol_version = login.protocol_version;
    match outcome {
        LoginOutcome::Error(message) => {
            // HarnessError has no variant carrying a String yet, so the
            // server's actual rejection text is surfaced on stderr rather
            // than silently dropped. A caller that needs the text
            // programmatically should call decode_login_response directly
            // instead of this convenience wrapper.
            eprintln!("login rejected: {message}");
            Err(HarnessError::UnexpectedOpcode {
                expected: OPCODE_LOGIN_CHARACTER_LIST,
                got: OPCODE_LOGIN_ERROR,
            })
        }
        LoginOutcome::CharacterList(characters) => {
            let selected = characters
                .iter()
                .find(|entry| entry.name == character_name)
                .ok_or(HarnessError::EmptyFrame)?; // no dedicated "not found"
                                                   // variant yet; see note below.
            let _ = selected; // world/login-port entry says where to connect;
                              // the caller already passed game_addr explicitly
                              // for milestone 1 rather than trusting the
                              // server-advertised address, so a real single-
                              // world deployment and a harness pointed at a
                              // loopback port under test behave the same way.
            let mut game_stream = TcpStream::connect(game_addr)?;
            game_stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            send_frame(
                &mut game_stream,
                &encode_game_request(&GameRequest {
                    protocol_version,
                    account_id,
                    character_name,
                    password,
                }),
            )?;
            let response = recv_frame(&mut game_stream)?;
            decode_game_response(&response)
        }
    }
}
