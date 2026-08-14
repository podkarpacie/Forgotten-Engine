//! Persistent TCP host and bounded diagnostic session foundation for Forgotten Engine.
//!
//! This crate deliberately exposes an engine probe protocol, not a claimed Tibia wire protocol.

use forgotten_persistence::EngineDatabase;
use forgotten_protocol::{
    decode, encode, CompatibilityProfile, Frame, ProtocolError, MAX_FRAME_SIZE,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

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

fn handle_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &HostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;

    let request = read_frame(stream)?;
    match decode_probe(&request) {
        Ok(()) => {
            write_frame(stream, &probe_response(config.profile))?;
            record_event(
                database_path,
                "info",
                &format!("probe accepted peer={peer} profile={}", config.profile.id),
            );
            Ok(())
        }
        Err(error) => {
            let _ = write_frame(stream, &error_frame(error.code()));
            Err(error)
        }
    }
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
    InvalidConfiguration(String),
    InvalidProbe(&'static str),
    HostThreadPanicked,
}

impl HostError {
    fn code(&self) -> &'static [u8] {
        match self {
            Self::InvalidProbe(_) => b"invalid-probe",
            Self::Protocol(_) => b"invalid-frame",
            Self::Io(_) => b"io-error",
            Self::InvalidConfiguration(_) => b"invalid-config",
            Self::HostThreadPanicked => b"host-panic",
        }
    }
}

impl From<std::io::Error> for HostError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
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
}
