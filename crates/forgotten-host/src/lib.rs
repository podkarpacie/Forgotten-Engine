//! Persistent TCP host and bounded diagnostic session foundation for Forgotten Engine.
//!
//! This crate deliberately exposes an engine probe protocol, not a claimed Tibia wire protocol.

use forgotten_persistence::EngineDatabase;
use forgotten_protocol::{
    decode, decode_legacy_74_envelope, decode_legacy_74_login_plaintext, decode_status_request,
    encode, encode_legacy_74_character_list, encode_login_error, encode_status_binary,
    encode_status_xml, xtea_encrypt_packet, CharacterListEntry, CompatibilityProfile, Frame,
    LegacyRsaPrivateKey, ProtocolError, StatusPlayer, StatusRequest, StatusSnapshot,
    MAX_FRAME_SIZE,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const PROBE_MAGIC: &[u8; 4] = b"FEHS";
pub const PROBE_RESPONSE_MAGIC: &[u8; 4] = b"FEOK";
pub const PROBE_ERROR_MAGIC: &[u8; 4] = b"FEER";
pub const PROBE_VERSION: u8 = 1;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub bind_addr: SocketAddr,
    pub profile: CompatibilityProfile,
    pub max_connections: usize,
    pub session_timeout: Duration,
    pub legacy_login: Option<LegacyLoginConfig>,
}

#[derive(Debug, Clone)]
pub struct LegacyLoginConfig {
    pub rsa_private_key: Arc<LegacyRsaPrivateKey>,
    pub server_name: String,
    pub message_of_the_day: String,
}

#[derive(Debug, Clone)]
pub struct StatusHostConfig {
    pub bind_addr: SocketAddr,
    pub profile: CompatibilityProfile,
    pub server_name: String,
    pub map_name: String,
    pub max_players: u32,
    pub max_connections: usize,
    pub session_timeout: Duration,
}

impl HostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if self.max_connections == 0 {
            return Err(HostError::InvalidConfiguration(
                "max_connections must be greater than zero".into(),
            ));
        }
        if self.session_timeout.is_zero() {
            return Err(HostError::InvalidConfiguration(
                "session_timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

impl StatusHostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if self.max_connections == 0 {
            return Err(HostError::InvalidConfiguration(
                "max_connections must be greater than zero".into(),
            ));
        }
        if self.session_timeout.is_zero() {
            return Err(HostError::InvalidConfiguration(
                "session_timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

pub struct HostHandle {
    local_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<(), HostError>>>,
}

impl HostHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn shutdown_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }

    pub fn shutdown(mut self) -> Result<(), HostError> {
        self.shutdown.store(true, Ordering::SeqCst);
        match self.thread.take().expect("host thread exists").join() {
            Ok(result) => result,
            Err(_) => Err(HostError::HostThreadPanicked),
        }
    }
}

pub fn start(config: HostConfig, database_path: impl AsRef<Path>) -> Result<HostHandle, HostError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
        )
    });

    Ok(HostHandle {
        local_addr,
        shutdown,
        thread: Some(thread),
    })
}

pub fn start_status(
    config: StatusHostConfig,
    database_path: impl AsRef<Path>,
) -> Result<HostHandle, HostError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve_status(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
            Instant::now(),
        )
    });
    Ok(HostHandle {
        local_addr,
        shutdown,
        thread: Some(thread),
    })
}

fn serve(
    listener: TcpListener,
    config: HostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "network host started addr={} profile={}",
            listener.local_addr()?,
            config.profile.id
        ),
    );

    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let active = active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= config.max_connections {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    let _ = write_frame(&mut stream, &error_frame(b"busy"));
                    record_event(
                        &database_path,
                        "warn",
                        &format!("connection rejected peer={peer} reason=connection-limit"),
                    );
                    continue;
                }

                let session_config = config.clone();
                let session_database_path = database_path.clone();
                let session_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let result =
                        handle_session(&mut stream, peer, &session_config, &session_database_path);
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("session rejected peer={peer} reason={error}"),
                        );
                    }
                    session_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(HostError::Io(error)),
        }
    }

    record_event(&database_path, "info", "network host stopped");
    Ok(())
}

fn serve_status(
    listener: TcpListener,
    config: StatusHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    started_at: Instant,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "status service started addr={} profile={}",
            listener.local_addr()?,
            config.profile.id
        ),
    );
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let active = active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= config.max_connections {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let session_config = config.clone();
                let session_database_path = database_path.clone();
                let session_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let result = handle_status_session(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                        started_at,
                    );
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("status session rejected peer={peer} reason={error}"),
                        );
                    }
                    session_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(HostError::Io(error)),
        }
    }
    record_event(&database_path, "info", "status service stopped");
    Ok(())
}

fn handle_status_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &StatusHostConfig,
    database_path: &Path,
    started_at: Instant,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let request = decode_status_request(&read_frame(stream)?).map_err(HostError::Protocol)?;
    let snapshot = StatusSnapshot {
        server_name: config.server_name.clone(),
        bind_ip: config.bind_addr.ip(),
        status_port: config.bind_addr.port(),
        uptime_seconds: started_at.elapsed().as_secs(),
        players_online: 0,
        max_players: config.max_players,
        players_peak: 0,
        map_name: config.map_name.clone(),
        profile: config.profile,
    };
    match request {
        StatusRequest::XmlInfo => {
            stream.write_all(&encode_status_xml(&snapshot))?;
            stream.flush()?;
        }
        StatusRequest::Binary { flags, .. } => {
            let response = encode_status_binary(&snapshot, flags, &[] as &[StatusPlayer], false);
            write_frame(stream, &response)?;
        }
    }
    record_event(
        database_path,
        "info",
        &format!("status query accepted peer={peer}"),
    );
    Ok(())
}

fn handle_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &HostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;

    let request = read_frame(stream)?;
    if decode_probe(&request).is_ok() {
        write_frame(stream, &probe_response(config.profile))?;
        record_event(
            database_path,
            "info",
            &format!("probe accepted peer={peer} profile={}", config.profile.id),
        );
        Ok(())
    } else if let Some(login) = &config.legacy_login {
        handle_legacy_login(stream, peer, config, login, database_path, &request)
    } else {
        let error = decode_probe(&request).expect_err("non-probe request must be rejected");
        let _ = write_frame(stream, &error_frame(error.code()));
        Err(error)
    }
}

fn handle_legacy_login(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &HostConfig,
    login: &LegacyLoginConfig,
    database_path: &Path,
    request: &Frame,
) -> Result<(), HostError> {
    if config.profile.id != "fe-7.4" {
        return Err(HostError::LegacyLoginUnavailable);
    }
    let envelope = decode_legacy_74_envelope(request).map_err(HostError::Protocol)?;
    let plaintext = login
        .rsa_private_key
        .decrypt_raw_block(&envelope.encrypted_block)
        .map_err(HostError::Protocol)?;
    let request = decode_legacy_74_login_plaintext(envelope.client_version, &plaintext)
        .map_err(HostError::Protocol)?;
    if request.client_version != 740 {
        return send_legacy_login_error(
            stream,
            request.xtea_key,
            "Only clients with protocol 7.4 are allowed.",
        );
    }
    let database = EngineDatabase::open(database_path)?;
    let Some(account) = database.authenticate_account(&request.account_name, &request.password)?
    else {
        return send_legacy_login_error(
            stream,
            request.xtea_key,
            "Account name or password is not correct.",
        );
    };
    let entries = account
        .characters
        .iter()
        .map(|character| CharacterListEntry {
            name: character.name.clone(),
            world_name: login.server_name.clone(),
            address: config.bind_addr.ip(),
            port: config.bind_addr.port(),
        })
        .collect::<Vec<_>>();
    let response = encode_legacy_74_character_list(&login.message_of_the_day, &entries)
        .map_err(HostError::Protocol)?;
    write_legacy_login_response(stream, request.xtea_key, &response)?;
    database.record_event(
        "info",
        &format!(
            "legacy login foundation accepted peer={peer} account={}",
            account.id
        ),
    )?;
    Ok(())
}

fn send_legacy_login_error(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    message: &str,
) -> Result<(), HostError> {
    write_legacy_login_response(stream, key, &encode_login_error(message))
}

fn write_legacy_login_response(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    response: &Frame,
) -> Result<(), HostError> {
    let encrypted = xtea_encrypt_packet(&response.0, key).map_err(HostError::Protocol)?;
    write_frame(stream, &Frame(encrypted))
}

pub fn probe_request() -> Frame {
    Frame([PROBE_MAGIC.as_slice(), &[PROBE_VERSION]].concat())
}

pub fn probe_response(profile: CompatibilityProfile) -> Frame {
    let mut payload = [PROBE_RESPONSE_MAGIC.as_slice(), &[PROBE_VERSION]].concat();
    payload.extend_from_slice(profile.id.as_bytes());
    Frame(payload)
}

pub fn error_frame(reason: &[u8]) -> Frame {
    let mut payload = PROBE_ERROR_MAGIC.to_vec();
    payload.extend_from_slice(reason);
    Frame(payload)
}

pub fn read_frame(stream: &mut TcpStream) -> Result<Frame, HostError> {
    let mut header = [0_u8; 2];
    stream.read_exact(&mut header)?;
    let declared = u16::from_le_bytes(header) as usize;
    if declared == 0 || declared > MAX_FRAME_SIZE {
        return Err(HostError::Protocol(ProtocolError::InvalidLength(declared)));
    }
    let mut encoded = Vec::with_capacity(declared + 2);
    encoded.extend_from_slice(&header);
    encoded.resize(declared + 2, 0);
    stream.read_exact(&mut encoded[2..])?;
    decode(&encoded).map_err(HostError::Protocol)
}

pub fn write_frame(stream: &mut TcpStream, frame: &Frame) -> Result<(), HostError> {
    let encoded = encode(frame).map_err(HostError::Protocol)?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    Ok(())
}

fn decode_probe(frame: &Frame) -> Result<(), HostError> {
    if frame.0.len() != PROBE_MAGIC.len() + 1 {
        return Err(HostError::InvalidProbe("unexpected probe length"));
    }
    if &frame.0[..4] != PROBE_MAGIC {
        return Err(HostError::InvalidProbe("unexpected probe magic"));
    }
    if frame.0[4] != PROBE_VERSION {
        return Err(HostError::InvalidProbe("unsupported probe version"));
    }
    Ok(())
}

fn record_event(database_path: &Path, level: &str, message: &str) {
    let _ = EngineDatabase::open(database_path)
        .and_then(|database| database.record_event(level, message));
}

#[derive(Debug)]
pub enum HostError {
    Io(std::io::Error),
    Protocol(ProtocolError),
    Persistence(forgotten_persistence::PersistenceError),
    InvalidConfiguration(String),
    InvalidProbe(&'static str),
    LegacyLoginUnavailable,
    HostThreadPanicked,
}

impl HostError {
    fn code(&self) -> &'static [u8] {
        match self {
            Self::InvalidProbe(_) => b"invalid-probe",
            Self::Protocol(_) => b"invalid-frame",
            Self::Persistence(_) => b"persistence-error",
            Self::Io(_) => b"io-error",
            Self::InvalidConfiguration(_) => b"invalid-config",
            Self::LegacyLoginUnavailable => b"legacy-login-unavailable",
            Self::HostThreadPanicked => b"host-panic",
        }
    }
}

impl From<std::io::Error> for HostError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<forgotten_persistence::PersistenceError> for HostError {
    fn from(value: forgotten_persistence::PersistenceError) -> Self {
        Self::Persistence(value)
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid configuration: {message}")
            }
            Self::InvalidProbe(message) => write!(formatter, "invalid probe: {message}"),
            other => write!(formatter, "{other:?}"),
        }
    }
}

impl std::error::Error for HostError {}

#[cfg(test)]
mod tests {
    use super::*;
    use forgotten_core::{Player, Position};
    use forgotten_protocol::FE_7_4_PROFILE;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn database_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forgotten-engine-host-{name}-{nonce}.db"))
    }

    fn test_config() -> HostConfig {
        HostConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            profile: FE_7_4_PROFILE,
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
            legacy_login: None,
        }
    }

    fn status_config() -> StatusHostConfig {
        StatusHostConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            profile: FE_7_4_PROFILE,
            server_name: "Forgotten Engine Test".into(),
            map_name: "forgotten".into(),
            max_players: 100,
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
        }
    }

    #[test]
    fn accepts_a_bounded_probe_and_returns_the_selected_profile() {
        let database = database_path("probe");
        let host = start(test_config(), &database).unwrap();
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        write_frame(&mut stream, &probe_request()).unwrap();
        let response = read_frame(&mut stream).unwrap();
        assert_eq!(response, probe_response(FE_7_4_PROFILE));
        host.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn rejects_an_invalid_probe_with_an_error_frame() {
        let database = database_path("invalid");
        let host = start(test_config(), &database).unwrap();
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        write_frame(&mut stream, &Frame(b"BAD!\x01".to_vec())).unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap(),
            error_frame(b"invalid-probe")
        );
        host.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn rejects_an_unbounded_configuration() {
        let mut config = test_config();
        config.max_connections = 0;
        assert!(matches!(
            start(config, database_path("limit")),
            Err(HostError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn answers_a_raw_xml_status_request() {
        let database = database_path("status-xml");
        let status = start_status(status_config(), &database).unwrap();
        let mut stream = TcpStream::connect(status.local_addr()).unwrap();
        write_frame(
            &mut stream,
            &Frame(vec![0xff, 0x04, 0x00, b'i', b'n', b'f', b'o']),
        )
        .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.contains("<tsqp version=\"1.0\">"));
        assert!(response.contains("Forgotten Engine Test"));
        status.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn answers_a_binary_status_request() {
        let database = database_path("status-binary");
        let status = start_status(status_config(), &database).unwrap();
        let mut stream = TcpStream::connect(status.local_addr()).unwrap();
        write_frame(&mut stream, &Frame(vec![0x01, 0x88, 0x00])).unwrap();
        let response = read_frame(&mut stream).unwrap();
        assert_eq!(response.0[0], 0x20);
        assert!(response.0.contains(&0x23));
        status.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn authenticates_a_legacy_login_fixture_and_returns_an_encrypted_character_list() {
        let database_path = database_path("legacy-login");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("admin", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let key = Arc::new(LegacyRsaPrivateKey::generate().unwrap());
        let mut config = test_config();
        config.legacy_login = Some(LegacyLoginConfig {
            rsa_private_key: Arc::clone(&key),
            server_name: "Forgotten Test".into(),
            message_of_the_day: "Welcome".into(),
        });
        let host = start(config, &database_path).unwrap();
        let mut plaintext = [0; forgotten_protocol::LEGACY_RSA_BLOCK_SIZE];
        plaintext[1..5].copy_from_slice(&1_u32.to_le_bytes());
        plaintext[5..9].copy_from_slice(&2_u32.to_le_bytes());
        plaintext[9..13].copy_from_slice(&3_u32.to_le_bytes());
        plaintext[13..17].copy_from_slice(&4_u32.to_le_bytes());
        plaintext[17..19].copy_from_slice(&5_u16.to_le_bytes());
        plaintext[19..24].copy_from_slice(b"admin");
        plaintext[24..26].copy_from_slice(&28_u16.to_le_bytes());
        plaintext[26..54].copy_from_slice(b"correct horse battery staple");
        let encrypted = key.encrypt_raw_block_for_harness(&plaintext).unwrap();
        let mut payload = vec![0x01, 0xe4, 0x02];
        payload.extend_from_slice(&encrypted);
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        write_frame(&mut stream, &Frame(payload)).unwrap();
        let response = read_frame(&mut stream).unwrap();
        let response = forgotten_protocol::xtea_decrypt_packet(&response.0, [1, 2, 3, 4]).unwrap();
        assert_eq!(response[0], 0x64);
        assert!(response.windows(6).any(|window| window == b"Knight"));
        host.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }
}
