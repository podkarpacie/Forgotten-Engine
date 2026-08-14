//! Persistent TCP host and bounded diagnostic session foundation for Forgotten Engine.
//!
//! This crate deliberately exposes an engine probe protocol, not a claimed Tibia wire protocol.

use forgotten_core::{CardinalDirection, EmptyWorldManifest, Player, Position, WorldState};
use forgotten_persistence::EngineDatabase;
use forgotten_protocol::{
    decode, decode_fe_otclient_capability_ack, decode_fe_otclient_move_request,
    decode_legacy_74_envelope, decode_legacy_74_game_session_bootstrap_plaintext,
    decode_legacy_74_game_session_envelope, decode_legacy_74_login_plaintext,
    decode_native_otclient_cardinal_move_request, decode_native_otclient_game_request,
    decode_native_otclient_login_request, decode_status_request, encode,
    encode_fe_otclient_capability_offer, encode_fe_otclient_empty_viewport,
    encode_fe_otclient_initial_world, encode_fe_otclient_movement_ack,
    encode_fe_otclient_world_tick, encode_legacy_74_character_list,
    encode_legacy_74_game_challenge, encode_legacy_74_game_session_error,
    encode_legacy_74_game_session_ready, encode_login_error, encode_native_otclient_character_list,
    encode_native_otclient_empty_world_map, encode_native_otclient_game_login_error,
    encode_native_otclient_game_login_state, encode_native_otclient_login_error,
    encode_native_otclient_move_creature, encode_status_binary, encode_status_xml,
    generate_legacy_74_game_challenge, xtea_encrypt_packet, CharacterListEntry,
    CompatibilityProfile, EmptyWorldMovementAck, Frame, InitialWorldSnapshot,
    Legacy74GameSessionState, LegacyRsaPrivateKey, NativeOtClientCardinalDirection,
    NativeOtClientEmptyWorldSnapshot, NativeOtClientPosition, NativeOtClientProfile,
    OtClientEndpoint, ProtocolError, StatusPlayer, StatusRequest, StatusSnapshot, MAX_FRAME_SIZE,
    NATIVE_OTCLIENT_PLAYER_ID_END, NATIVE_OTCLIENT_PLAYER_ID_START,
};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const PROBE_MAGIC: &[u8; 4] = b"FEHS";
pub const PROBE_RESPONSE_MAGIC: &[u8; 4] = b"FEOK";
pub const PROBE_ERROR_MAGIC: &[u8; 4] = b"FEER";
pub const PROBE_VERSION: u8 = 1;
const MAX_EMPTY_WORLD_MOVES_PER_SESSION: usize = 64;
const MAX_NATIVE_EMPTY_WORLD_MOVES_PER_SESSION: usize = 64;

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

#[derive(Debug, Clone)]
pub struct GameSessionHostConfig {
    pub bind_addr: SocketAddr,
    pub profile: CompatibilityProfile,
    pub rsa_private_key: Arc<LegacyRsaPrivateKey>,
    pub advertised_endpoint: OtClientEndpoint,
    pub max_connections: usize,
    pub session_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct NativeOtClientHostConfig {
    pub bind_addr: SocketAddr,
    pub client_profile: NativeOtClientProfile,
    pub server_name: String,
    pub advertised_game_addr: SocketAddr,
    pub max_connections: usize,
    pub session_timeout: Duration,
    pub empty_world: Option<NativeOtClientEmptyWorldConfig>,
}

#[derive(Debug, Clone)]
pub struct NativeOtClientEmptyWorldConfig {
    pub ground_thing_id: u16,
    pub player_look_type: u8,
    pub player_speed: u16,
    pub server_beat: u16,
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

impl GameSessionHostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if self.profile.id != "fe-7.4" {
            return Err(HostError::LegacyLoginUnavailable);
        }
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

impl NativeOtClientHostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if !self.client_profile.supports_current_native_foundation() {
            return Err(HostError::InvalidConfiguration(
                "selected native client profile is not supported by the current foundation".into(),
            ));
        }
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
        if let Some(empty_world) = &self.empty_world {
            if empty_world.ground_thing_id == 0
                || empty_world.player_look_type == 0
                || empty_world.player_speed == 0
                || empty_world.server_beat == 0
            {
                return Err(HostError::InvalidConfiguration(
                    "native empty-world fixture requires nonzero thing, look, speed, and beat values"
                        .into(),
                ));
            }
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

pub fn start_game_session(
    config: GameSessionHostConfig,
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
        serve_game_session(
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

pub fn start_native_otclient_login(
    config: NativeOtClientHostConfig,
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
        serve_native_otclient_login(
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

pub fn start_native_otclient_game(
    config: NativeOtClientHostConfig,
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
        serve_native_otclient_game(
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

fn serve_game_session(
    listener: TcpListener,
    config: GameSessionHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "game session foundation started addr={} profile={}",
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
                    let result = handle_game_session(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                    );
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("game session rejected peer={peer} reason={error}"),
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
    record_event(&database_path, "info", "game session foundation stopped");
    Ok(())
}

fn serve_native_otclient_login(
    listener: TcpListener,
    config: NativeOtClientHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "native client login service started addr={} protocol={}",
            listener.local_addr()?,
            config.client_profile.protocol_version
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
                    let result = handle_native_otclient_login(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                    );
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("native login rejected peer={peer} reason={error}"),
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
    record_event(
        &database_path,
        "info",
        "native client login service stopped",
    );
    Ok(())
}

fn serve_native_otclient_game(
    listener: TcpListener,
    config: NativeOtClientHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "native client game service started addr={} protocol={}",
            listener.local_addr()?,
            config.client_profile.protocol_version
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
                    let result = handle_native_otclient_game(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                    );
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("native game rejected peer={peer} reason={error}"),
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
    record_event(&database_path, "info", "native client game service stopped");
    Ok(())
}

fn handle_native_otclient_login(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &NativeOtClientHostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let request =
        decode_native_otclient_login_request(&read_frame(stream)?, &config.client_profile)
            .map_err(HostError::Protocol)?;
    let database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    let Some(account) = database
        .authenticate_account_id(request.account_id, &request.password)
        .map_err(HostError::Persistence)?
    else {
        write_frame(
            stream,
            &encode_native_otclient_login_error("Account name or password is not correct."),
        )?;
        return Ok(());
    };
    let IpAddr::V4(address) = config.advertised_game_addr.ip() else {
        write_frame(
            stream,
            &encode_native_otclient_login_error(
                "This native client profile requires an IPv4 game endpoint.",
            ),
        )?;
        return Ok(());
    };
    let entries = account
        .characters
        .iter()
        .map(|character| CharacterListEntry {
            name: character.name.clone(),
            world_name: config.server_name.clone(),
            address: IpAddr::V4(address),
            port: config.advertised_game_addr.port(),
        })
        .collect::<Vec<_>>();
    write_frame(
        stream,
        &encode_native_otclient_character_list(&entries).map_err(HostError::Protocol)?,
    )?;
    record_event(
        database_path,
        "info",
        &format!(
            "native client login accepted peer={peer} account={} protocol={}",
            account.id, request.protocol_version
        ),
    );
    Ok(())
}

fn handle_native_otclient_game(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &NativeOtClientHostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let request = decode_native_otclient_game_request(&read_frame(stream)?, &config.client_profile)
        .map_err(HostError::Protocol)?;
    let database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    let Some(account) = database
        .authenticate_account_id(request.account_id, &request.password)
        .map_err(HostError::Persistence)?
    else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error("Account name or password is not correct."),
        )?;
        return Ok(());
    };
    let Some(character) = account
        .characters
        .iter()
        .find(|character| character.name == request.character_name)
    else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error("Character does not belong to this account."),
        )?;
        return Ok(());
    };
    let Some(empty_world) = &config.empty_world else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error(
                "Forgotten Engine native map initialization is not enabled for this selected client profile.",
            ),
        )?;
        return Ok(());
    };
    let player_id = native_player_id(character.id)?;
    let snapshot = NativeOtClientEmptyWorldSnapshot {
        player_id,
        player_name: character.name.clone(),
        player_position: native_position(character.position),
        ground_thing_id: empty_world.ground_thing_id,
        player_look_type: empty_world.player_look_type,
        player_speed: empty_world.player_speed,
        server_beat: empty_world.server_beat,
    };
    write_frame(
        stream,
        &encode_native_otclient_game_login_state(&config.client_profile, &snapshot)
            .map_err(HostError::Protocol)?,
    )?;
    write_frame(
        stream,
        &encode_native_otclient_empty_world_map(&config.client_profile, &snapshot)
            .map_err(HostError::Protocol)?,
    )?;

    let account_id = u64::try_from(account.id).map_err(|_| {
        HostError::InvalidConfiguration("native numeric account IDs must be non-negative".into())
    })?;
    let mut world = WorldState::default();
    world
        .add_player(Player {
            id: character.id,
            account_id,
            name: character.name.clone(),
            position: character.position,
            level: character.level,
            experience: 0,
            skill_points: 0,
        })
        .map_err(HostError::Core)?;
    let initial_position = character.position;
    for _ in 0..MAX_NATIVE_EMPTY_WORLD_MOVES_PER_SESSION {
        let request = match read_frame(stream) {
            Ok(request) => request,
            Err(HostError::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        };
        let direction =
            decode_native_otclient_cardinal_move_request(&request, &config.client_profile)
                .map_err(HostError::Protocol)?;
        let (_, destination) = world
            .move_player_cardinal(character.id, native_cardinal_direction(direction))
            .map_err(HostError::Core)?;
        if !native_empty_world_position_is_visible(initial_position, destination) {
            write_frame(
                stream,
                &encode_native_otclient_game_login_error(
                    "Native empty-world viewport boundary reached; map-row streaming is not available yet.",
                ),
            )?;
            return Ok(());
        }
        database.update_player_position(character.id, destination)?;
        write_frame(
            stream,
            &encode_native_otclient_move_creature(
                &config.client_profile,
                player_id,
                native_position(destination),
            )
            .map_err(HostError::Protocol)?,
        )?;
    }
    record_event(
        database_path,
        "info",
        &format!(
            "native empty-world session completed peer={peer} account={} character={} protocol={}",
            account.id, request.character_name, request.protocol_version
        ),
    );
    Ok(())
}

fn native_player_id(character_id: u64) -> Result<u32, HostError> {
    let character_id = u32::try_from(character_id).map_err(|_| {
        HostError::InvalidConfiguration("character ID exceeds the native player-ID range".into())
    })?;
    let player_id = NATIVE_OTCLIENT_PLAYER_ID_START
        .checked_add(character_id)
        .ok_or_else(|| {
            HostError::InvalidConfiguration(
                "character ID exceeds the native player-ID range".into(),
            )
        })?;
    if player_id >= NATIVE_OTCLIENT_PLAYER_ID_END {
        return Err(HostError::InvalidConfiguration(
            "character ID exceeds the native player-ID range".into(),
        ));
    }
    Ok(player_id)
}

fn native_position(position: Position) -> NativeOtClientPosition {
    NativeOtClientPosition {
        x: position.x,
        y: position.y,
        z: position.z,
    }
}

fn native_cardinal_direction(direction: NativeOtClientCardinalDirection) -> CardinalDirection {
    match direction {
        NativeOtClientCardinalDirection::North => CardinalDirection::North,
        NativeOtClientCardinalDirection::East => CardinalDirection::East,
        NativeOtClientCardinalDirection::South => CardinalDirection::South,
        NativeOtClientCardinalDirection::West => CardinalDirection::West,
    }
}

fn native_empty_world_position_is_visible(center: Position, position: Position) -> bool {
    position.z == center.z
        && position.x >= center.x.saturating_sub(8)
        && position.x <= center.x.saturating_add(9)
        && position.y >= center.y.saturating_sub(6)
        && position.y <= center.y.saturating_add(7)
}

fn handle_game_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &GameSessionHostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let challenge = generate_legacy_74_game_challenge();
    write_frame(stream, &encode_legacy_74_game_challenge(challenge))?;
    let envelope = decode_legacy_74_game_session_envelope(&read_frame(stream)?)
        .map_err(HostError::Protocol)?;
    let plaintext = config
        .rsa_private_key
        .decrypt_raw_block(&envelope.encrypted_block)
        .map_err(HostError::Protocol)?;
    let bootstrap = decode_legacy_74_game_session_bootstrap_plaintext(
        envelope.client_version,
        &plaintext,
        challenge,
    )
    .map_err(HostError::Protocol)?;
    let database = EngineDatabase::open(database_path)?;
    let Some(account) = database
        .authenticate_account(&bootstrap.request.account_name, &bootstrap.request.password)?
    else {
        return send_game_session_error(
            stream,
            bootstrap.xtea_key,
            "Account name or password is not correct.",
        );
    };
    let Some(character) = account
        .characters
        .iter()
        .find(|character| character.name == bootstrap.request.character_name)
    else {
        return send_game_session_error(
            stream,
            bootstrap.xtea_key,
            "Character is not available on this account.",
        );
    };
    let authenticated = Legacy74GameSessionState::Authenticated {
        account_id: account.id,
        character_name: bootstrap.request.character_name.clone(),
    };
    database.record_event(
        "info",
        &format!("game session state peer={peer} state={authenticated:?}"),
    )?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_legacy_74_game_session_ready(&bootstrap.request.character_name),
    )?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_fe_otclient_capability_offer(&config.advertised_endpoint),
    )?;
    let acknowledgement = read_frame(stream)?;
    let acknowledgement =
        forgotten_protocol::xtea_decrypt_packet(&acknowledgement.0, bootstrap.xtea_key)
            .map_err(HostError::Protocol)?;
    if let Err(error) = decode_fe_otclient_capability_ack(&Frame(acknowledgement)) {
        let _ = send_game_session_error(
            stream,
            bootstrap.xtea_key,
            "A compatible FE OTClient module must acknowledge fe.otclient.v1.",
        );
        return Err(HostError::Protocol(error));
    }
    let custom_client = Legacy74GameSessionState::CustomClientNegotiated {
        character_name: bootstrap.request.character_name.clone(),
    };
    database.record_event(
        "info",
        &format!("game session state peer={peer} state={custom_client:?}"),
    )?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_fe_otclient_initial_world(&InitialWorldSnapshot {
            character_name: bootstrap.request.character_name.clone(),
            start_x: character.position.x,
            start_y: character.position.y,
            start_z: character.position.z,
            endpoint: config.advertised_endpoint.clone(),
        }),
    )?;
    let mut world = WorldState::default();
    world
        .add_player(Player {
            id: character.id,
            account_id: account.id as u64,
            name: character.name.clone(),
            position: character.position,
            level: character.level,
            experience: 0,
            skill_points: 0,
        })
        .map_err(HostError::Core)?;
    let manifest = EmptyWorldManifest::default();
    let viewport = world
        .empty_world_viewport(character.id, manifest.clone())
        .map_err(HostError::Core)?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_fe_otclient_empty_viewport(&viewport),
    )?;
    for _ in 0..MAX_EMPTY_WORLD_MOVES_PER_SESSION {
        let request = match read_frame(stream) {
            Ok(request) => request,
            Err(HostError::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        };
        let request = forgotten_protocol::xtea_decrypt_packet(&request.0, bootstrap.xtea_key)
            .map_err(HostError::Protocol)?;
        let direction =
            decode_fe_otclient_move_request(&Frame(request)).map_err(HostError::Protocol)?;
        let (from, to) = match world.move_player_cardinal(character.id, direction) {
            Ok(movement) => movement,
            Err(error) => {
                send_game_session_error(
                    stream,
                    bootstrap.xtea_key,
                    "Movement rejected by empty-world bounds.",
                )?;
                return Err(HostError::Core(error));
            }
        };
        let tick = world.advance_tick();
        database.update_player_position(character.id, to)?;
        write_game_session_response(
            stream,
            bootstrap.xtea_key,
            &encode_fe_otclient_movement_ack(&EmptyWorldMovementAck { tick, from, to }),
        )?;
        write_game_session_response(
            stream,
            bootstrap.xtea_key,
            &encode_fe_otclient_world_tick(tick),
        )?;
        let viewport = world
            .empty_world_viewport(character.id, manifest.clone())
            .map_err(HostError::Core)?;
        write_game_session_response(
            stream,
            bootstrap.xtea_key,
            &encode_fe_otclient_empty_viewport(&viewport),
        )?;
    }
    let feature_gate = Legacy74GameSessionState::FeatureGated {
        character_name: bootstrap.request.character_name,
    };
    database.record_event(
        "info",
        &format!("game session state peer={peer} state={feature_gate:?}"),
    )?;
    Ok(())
}

fn send_game_session_error(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    message: &str,
) -> Result<(), HostError> {
    write_game_session_response(stream, key, &encode_legacy_74_game_session_error(message))
}

fn write_game_session_response(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    response: &Frame,
) -> Result<(), HostError> {
    let encrypted = xtea_encrypt_packet(&response.0, key).map_err(HostError::Protocol)?;
    write_frame(stream, &Frame(encrypted))
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
    Core(forgotten_core::CoreError),
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
            Self::Core(_) => b"world-error",
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

    fn game_session_config(key: Arc<LegacyRsaPrivateKey>) -> GameSessionHostConfig {
        GameSessionHostConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            profile: FE_7_4_PROFILE,
            rsa_private_key: key,
            advertised_endpoint: OtClientEndpoint {
                host: "fe.example.test".into(),
                port: 443,
            },
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
        }
    }

    fn native_otclient_config(bind_addr: SocketAddr) -> NativeOtClientHostConfig {
        NativeOtClientHostConfig {
            bind_addr,
            client_profile: NativeOtClientProfile {
                protocol_version: 740,
                numeric_account_ids: true,
                login_packet_encryption: false,
                protocol_checksum: false,
                challenge_on_login: false,
                max_padding_bytes: 128,
            },
            server_name: "Forgotten Engine Test".into(),
            advertised_game_addr: "127.0.0.1:7265".parse().unwrap(),
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
            empty_world: None,
        }
    }

    fn native_empty_world_config(bind_addr: SocketAddr) -> NativeOtClientHostConfig {
        let mut config = native_otclient_config(bind_addr);
        config.empty_world = Some(NativeOtClientEmptyWorldConfig {
            ground_thing_id: 102,
            player_look_type: 128,
            player_speed: 220,
            server_beat: 50,
        });
        config
    }

    fn add_string(payload: &mut Vec<u8>, value: &str) {
        payload.extend_from_slice(&(value.len() as u16).to_le_bytes());
        payload.extend_from_slice(value.as_bytes());
    }

    fn native_login_request(account_id: u32, password: &str) -> Frame {
        let mut payload = vec![forgotten_protocol::NATIVE_OTCLIENT_ENTER_ACCOUNT];
        payload.extend_from_slice(&2_u16.to_le_bytes());
        payload.extend_from_slice(&740_u16.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.extend_from_slice(&account_id.to_le_bytes());
        add_string(&mut payload, password);
        add_string(&mut payload, "otcv8-test");
        payload.extend_from_slice(&1_u16.to_le_bytes());
        Frame(payload)
    }

    fn native_game_request(account_id: u32, character_name: &str, password: &str) -> Frame {
        let mut payload = vec![forgotten_protocol::NATIVE_OTCLIENT_PENDING_GAME];
        payload.extend_from_slice(&2_u16.to_le_bytes());
        payload.extend_from_slice(&740_u16.to_le_bytes());
        payload.extend_from_slice(&account_id.to_le_bytes());
        add_string(&mut payload, character_name);
        add_string(&mut payload, password);
        add_string(&mut payload, "otcv8-test");
        payload.extend_from_slice(&1_u16.to_le_bytes());
        Frame(payload)
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

    #[test]
    fn serves_a_profile_driven_native_otclient_character_list_and_game_gate() {
        let database_path = database_path("native-otclient");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
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

        let login = start_native_otclient_login(
            native_otclient_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();
        let game = start_native_otclient_game(
            native_otclient_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut login_stream = TcpStream::connect(login.local_addr()).unwrap();
        write_frame(
            &mut login_stream,
            &native_login_request(
                account_id.try_into().unwrap(),
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let character_list = read_frame(&mut login_stream).unwrap();
        assert_eq!(
            character_list.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_LOGIN_CHARACTER_LIST
        );
        assert!(character_list
            .0
            .windows(6)
            .any(|window| window == b"Knight"));
        assert!(character_list
            .0
            .windows(4)
            .any(|window| window == [127, 0, 0, 1]));

        let mut game_stream = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut game_stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let game_gate = read_frame(&mut game_stream).unwrap();
        assert_eq!(
            game_gate.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_ERROR
        );
        assert!(game_gate
            .0
            .windows(
                b"Forgotten Engine native map initialization is not enabled for this selected client profile."
                    .len(),
            )
            .any(|window| {
                window
                    == b"Forgotten Engine native map initialization is not enabled for this selected client profile."
            }));

        game.shutdown().unwrap();
        login.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn serves_a_native_empty_world_and_normal_cardinal_movement() {
        let database_path = database_path("native-empty-world");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
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
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut stream = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let login = read_frame(&mut stream).unwrap();
        assert_eq!(
            login.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        let map = read_frame(&mut stream).unwrap();
        assert_eq!(map.0[0], forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP);
        assert!(map.0.windows(6).any(|window| window == b"Knight"));

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let movement = read_frame(&mut stream).unwrap();
        assert_eq!(
            movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        assert_eq!(&movement.0[7..12], &[101, 0, 100, 0, 7]);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            101
        );

        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn completes_a_challenge_bound_game_session_and_returns_a_feature_gate() {
        let database_path = database_path("game-session");
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
        let host =
            start_game_session(game_session_config(Arc::clone(&key)), &database_path).unwrap();
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        let challenge = read_frame(&mut stream).unwrap();
        assert_eq!(
            challenge.0[0],
            forgotten_protocol::LEGACY_74_GAME_CHALLENGE_OPCODE
        );
        let challenge = forgotten_protocol::Legacy74GameChallenge {
            timestamp: u32::from_le_bytes(challenge.0[1..5].try_into().unwrap()),
            random: challenge.0[5],
        };
        let bootstrap = forgotten_protocol::Legacy74GameSessionBootstrap {
            xtea_key: [1, 2, 3, 4],
            request: forgotten_protocol::Legacy74GameSessionRequest {
                client_version: 740,
                account_name: "admin".into(),
                password: "correct horse battery staple".into(),
                character_name: "Knight".into(),
                challenge,
            },
        };
        let request = forgotten_protocol::encode_legacy_74_game_session_bootstrap_for_harness(
            &key, &bootstrap,
        )
        .unwrap();
        write_frame(&mut stream, &request).unwrap();
        let response = read_frame(&mut stream).unwrap();
        let response =
            forgotten_protocol::xtea_decrypt_packet(&response.0, bootstrap.xtea_key).unwrap();
        assert_eq!(
            response[0],
            forgotten_protocol::LEGACY_74_GAME_SESSION_READY_OPCODE
        );
        let offer = read_frame(&mut stream).unwrap();
        let offer = forgotten_protocol::xtea_decrypt_packet(&offer.0, bootstrap.xtea_key).unwrap();
        assert_eq!(offer[0], forgotten_protocol::FE_OTCLIENT_EXTENDED_OPCODE);
        let acknowledgement = forgotten_protocol::encode_fe_otclient_capability_ack_for_harness();
        let acknowledgement =
            forgotten_protocol::xtea_encrypt_packet(&acknowledgement.0, bootstrap.xtea_key)
                .unwrap();
        write_frame(&mut stream, &Frame(acknowledgement)).unwrap();
        let world = read_frame(&mut stream).unwrap();
        let world = forgotten_protocol::xtea_decrypt_packet(&world.0, bootstrap.xtea_key).unwrap();
        assert_eq!(world[0], forgotten_protocol::FE_OTCLIENT_EXTENDED_OPCODE);
        assert!(world
            .windows(b"fe.example.test:443".len())
            .any(|window| window == b"fe.example.test:443"));
        assert!(world
            .windows(b"position=100,100,7".len())
            .any(|window| window == b"position=100,100,7"));
        assert!(world
            .windows(b"empty-gated".len())
            .any(|window| window == b"empty-gated"));
        let initial_viewport = read_frame(&mut stream).unwrap();
        let initial_viewport =
            forgotten_protocol::xtea_decrypt_packet(&initial_viewport.0, bootstrap.xtea_key)
                .unwrap();
        assert!(initial_viewport
            .windows(b"fe.viewport.v1;tick=0".len())
            .any(|window| window == b"fe.viewport.v1;tick=0"));
        let movement = forgotten_protocol::encode_fe_otclient_move_request_for_harness(
            forgotten_core::CardinalDirection::East,
        );
        let movement =
            forgotten_protocol::xtea_encrypt_packet(&movement.0, bootstrap.xtea_key).unwrap();
        write_frame(&mut stream, &Frame(movement)).unwrap();
        let acknowledgement = read_frame(&mut stream).unwrap();
        let acknowledgement =
            forgotten_protocol::xtea_decrypt_packet(&acknowledgement.0, bootstrap.xtea_key)
                .unwrap();
        assert!(acknowledgement
            .windows(b"fe.move.ack.v1;tick=1".len())
            .any(|window| window == b"fe.move.ack.v1;tick=1"));
        assert!(acknowledgement
            .windows(b"to=101,100,7".len())
            .any(|window| window == b"to=101,100,7"));
        let tick = read_frame(&mut stream).unwrap();
        let tick = forgotten_protocol::xtea_decrypt_packet(&tick.0, bootstrap.xtea_key).unwrap();
        assert!(tick
            .windows(b"fe.tick.v1;tick=1".len())
            .any(|window| window == b"fe.tick.v1;tick=1"));
        let viewport = read_frame(&mut stream).unwrap();
        let viewport =
            forgotten_protocol::xtea_decrypt_packet(&viewport.0, bootstrap.xtea_key).unwrap();
        assert!(viewport
            .windows(b"center=101,100,7".len())
            .any(|window| window == b"center=101,100,7"));
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            101
        );
        host.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }
}
