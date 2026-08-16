//! Persistent TCP host and bounded diagnostic session foundation for Forgotten Engine.
//!
//! This crate deliberately exposes an engine probe protocol, not a claimed Tibia wire protocol.

use forgotten_core::{
    CardinalDirection, EmptyWorldManifest, FeTfsStaticSpawnCollection, Player,
    PlayerInteractionIntent, Position, StaticCreatureDecisionBatch, StaticCreatureDecisionPolicy,
    WorldMap, WorldState,
};
use forgotten_persistence::EngineDatabase;
use forgotten_protocol::{
    decode, decode_fe_otclient_capability_ack, decode_fe_otclient_move_request,
    decode_legacy_74_envelope, decode_legacy_74_game_session_bootstrap_plaintext,
    decode_legacy_74_game_session_envelope, decode_legacy_74_login_plaintext,
    decode_native_otclient_game_action, decode_native_otclient_game_request,
    decode_native_otclient_login_request, decode_status_request, encode,
    encode_fe_otclient_capability_offer, encode_fe_otclient_empty_viewport,
    encode_fe_otclient_initial_world, encode_fe_otclient_movement_ack,
    encode_fe_otclient_world_tick, encode_legacy_74_character_list,
    encode_legacy_74_game_challenge, encode_legacy_74_game_session_error,
    encode_legacy_74_game_session_ready, encode_login_error, encode_native_otclient_animated_text,
    encode_native_otclient_character_list, encode_native_otclient_game_cancel_walk_facing,
    encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players,
    encode_native_otclient_game_login_error, encode_native_otclient_game_ping,
    encode_native_otclient_game_ping_back, encode_native_otclient_game_status_message,
    encode_native_otclient_login_error,
    encode_native_otclient_map_step_with_static_spawns_and_players,
    encode_native_otclient_map_viewport_with_static_spawns,
    encode_native_otclient_map_viewport_with_static_spawns_and_players,
    encode_native_otclient_move_creature_at, encode_status_binary, encode_status_xml,
    generate_legacy_74_game_challenge, xtea_encrypt_packet, CharacterListEntry,
    CompatibilityProfile, EmptyWorldMovementAck, Frame, InitialWorldSnapshot,
    Legacy74GameSessionState, LegacyRsaPrivateKey, NativeOtClientAutoWalkDirection,
    NativeOtClientCardinalDirection, NativeOtClientEmptyWorldSnapshot, NativeOtClientGameAction,
    NativeOtClientPosition, NativeOtClientProfile, NativeOtClientVisiblePlayer, OtClientEndpoint,
    ProtocolError, StatusPlayer, StatusRequest, StatusSnapshot, MAX_FRAME_SIZE,
    NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES, NATIVE_OTCLIENT_PLAYER_ID_END,
    NATIVE_OTCLIENT_PLAYER_ID_START,
};
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const PROBE_MAGIC: &[u8; 4] = b"FEHS";
pub const PROBE_RESPONSE_MAGIC: &[u8; 4] = b"FEOK";
pub const PROBE_ERROR_MAGIC: &[u8; 4] = b"FEER";
pub const PROBE_VERSION: u8 = 1;
const MAX_EMPTY_WORLD_MOVES_PER_SESSION: usize = 64;
const NATIVE_OTCLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const NATIVE_OTCLIENT_DEFAULT_GROUND_SPEED: u64 = 150;
const NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY: Duration = Duration::from_secs(2);
const NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY: usize = 64;

fn truncate_native_chat_text(message: &str) -> String {
    let mut output = String::new();
    for character in message.chars() {
        if output.len() + character.len_utf8() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

fn native_diagnostic_record(enabled: bool, peer: SocketAddr, event: &str) -> Option<String> {
    enabled.then(|| format!("> Native OTCv8 trace peer={peer} {event}"))
}

fn native_diagnostic(enabled: bool, peer: SocketAddr, event: &str) {
    if let Some(record) = native_diagnostic_record(enabled, peer, event) {
        eprintln!("{record}");
    }
}

fn native_action_diagnostic_summary(action: &NativeOtClientGameAction) -> String {
    match action {
        NativeOtClientGameAction::Ping => "action=ping".into(),
        NativeOtClientGameAction::PingBack => "action=ping-back".into(),
        NativeOtClientGameAction::EnterGame => "action=enter-game".into(),
        NativeOtClientGameAction::LeaveGame => "action=leave-game".into(),
        NativeOtClientGameAction::Stop => "action=stop".into(),
        NativeOtClientGameAction::Turn(direction) => format!("action=turn direction={direction:?}"),
        NativeOtClientGameAction::CardinalMove(direction) => {
            format!("action=cardinal-move direction={direction:?}")
        }
        NativeOtClientGameAction::DiagonalMove(direction) => {
            format!("action=diagonal-move direction={direction:?}")
        }
        NativeOtClientGameAction::AutoWalk(path) => format!(
            "action=auto-walk path-directions={} expanded-steps={}",
            path.len(),
            path.iter()
                .map(|direction| direction.cardinal_steps().len())
                .sum::<usize>()
        ),
        NativeOtClientGameAction::Talk(message) => {
            format!("action=talk text-bytes={}", message.len())
        }
        NativeOtClientGameAction::ChangeFightModes => "action=change-fight-modes".into(),
        NativeOtClientGameAction::UseItem => "action=use-item".into(),
        NativeOtClientGameAction::RequestOutfit => "action=request-outfit".into(),
        NativeOtClientGameAction::ChangeOutfit => "action=change-outfit".into(),
        NativeOtClientGameAction::IgnoredInteraction(opcode) => {
            format!("action=ignored-interaction opcode=0x{opcode:02x}")
        }
        NativeOtClientGameAction::SelectTarget(native_id) => {
            format!("action=select-target native-id={native_id}")
        }
        NativeOtClientGameAction::SelectFollow(native_id) => {
            format!("action=select-follow native-id={native_id}")
        }
    }
}

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
    /// Emits bounded session metadata only. Packet bodies and credentials are never logged.
    pub extended_diagnostics: bool,
    pub empty_world: Option<NativeOtClientEmptyWorldConfig>,
    pub world_map: Option<Arc<WorldMap>>,
    /// Immutable display-only TFS spawn entities. No AI, combat, movement, or Lua behavior is
    /// attached at this host boundary.
    pub static_spawns: Option<Arc<FeTfsStaticSpawnCollection>>,
}

#[derive(Debug, Clone)]
pub struct NativeOtClientEmptyWorldConfig {
    pub ground_thing_id: u16,
    pub player_look_type: u8,
    pub player_speed: u16,
    pub server_beat: u16,
}

/// One synchronized authoritative world for all native game sessions started by a host. It owns
/// no automatic scheduler: callers advance ticks and apply creature policy explicitly.
#[derive(Debug, Clone)]
pub struct SharedNativeWorld {
    world: Arc<Mutex<WorldState>>,
    visibility_epoch: Arc<AtomicU64>,
    chat_recipients: Arc<Mutex<BTreeMap<u64, mpsc::SyncSender<SharedPublicChatEvent>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedPublicChatEvent {
    speaker_name: String,
    speaker_position: NativeOtClientPosition,
    text: String,
}

impl SharedNativeWorld {
    pub fn from_static_spawns(
        static_spawns: Option<&FeTfsStaticSpawnCollection>,
    ) -> Result<Self, HostError> {
        let mut world = WorldState::default();
        if let Some(static_spawns) = static_spawns {
            world
                .install_static_creatures(static_spawns)
                .map_err(HostError::Core)?;
        }
        Ok(Self {
            world: Arc::new(Mutex::new(world)),
            visibility_epoch: Arc::new(AtomicU64::new(0)),
            chat_recipients: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    pub fn advance_tick(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.advance_tick())
    }

    pub fn tick(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.tick())
    }

    pub fn visibility_epoch(&self) -> u64 {
        self.visibility_epoch.load(Ordering::SeqCst)
    }

    pub fn active_static_spawns(&self) -> Result<FeTfsStaticSpawnCollection, HostError> {
        Ok(self.lock()?.active_static_spawn_collection())
    }

    pub fn player_interaction_intent(
        &self,
        player_id: u64,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .player_interaction_intent(player_id)
            .map_err(HostError::Core)
    }

    pub fn set_player_target(
        &self,
        player_id: u64,
        target_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_target(player_id, target_player_id)
            .map_err(HostError::Core)
    }

    pub fn set_player_follow(
        &self,
        player_id: u64,
        follow_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_follow(player_id, follow_player_id)
            .map_err(HostError::Core)
    }

    pub fn visible_players(
        &self,
        observer_id: u64,
        look_type: u8,
        speed: u16,
    ) -> Result<Vec<NativeOtClientVisiblePlayer>, HostError> {
        self.lock()?
            .player_render_snapshots()
            .into_iter()
            .filter(|player| player.id != observer_id)
            .map(|player| {
                Ok(NativeOtClientVisiblePlayer {
                    player_id: native_player_id(player.id)?,
                    name: player.name,
                    position: native_position(player.position),
                    look_type,
                    speed,
                })
            })
            .collect()
    }

    fn register_public_chat_recipient(
        &self,
        player_id: u64,
    ) -> Result<mpsc::Receiver<SharedPublicChatEvent>, HostError> {
        let (sender, receiver) = mpsc::sync_channel(NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY);
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        if recipients.insert(player_id, sender).is_some() {
            return Err(HostError::InvalidConfiguration(
                "shared chat recipient already registered for player".into(),
            ));
        }
        Ok(receiver)
    }

    fn unregister_public_chat_recipient(&self, player_id: u64) {
        if let Ok(mut recipients) = self.chat_recipients.lock() {
            recipients.remove(&player_id);
        }
    }

    fn broadcast_public_chat(&self, sender_id: u64, message: &str) -> Result<usize, HostError> {
        let sender = self
            .lock()?
            .player(sender_id)
            .cloned()
            .ok_or(forgotten_core::CoreError::UnknownPlayer(sender_id))
            .map_err(HostError::Core)?;
        let body = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if body.is_empty() {
            return Ok(0);
        }
        let event = SharedPublicChatEvent {
            speaker_name: sender.name,
            speaker_position: native_position(sender.position),
            text: truncate_native_chat_text(&body),
        };
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        recipients.retain(|_, recipient| match recipient.try_send(event.clone()) {
            Ok(()) => {
                delivered += 1;
                true
            }
            Err(mpsc::TrySendError::Full(_)) => true,
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        });
        Ok(delivered)
    }

    pub fn register_player_at_available_position(
        &self,
        mut player: Player,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let mut world = self.lock()?;
        let position = [player.position, world_map.spawn()]
            .into_iter()
            .find(|position| {
                world_map.is_walkable(*position)
                    && !world.is_static_creature_occupied(*position)
                    && !world.is_player_occupied(*position)
            })
            .or_else(|| {
                world_map.tiles().find_map(|(position, tile)| {
                    (tile.walkable
                        && !world.is_static_creature_occupied(position)
                        && !world.is_player_occupied(position))
                    .then_some(position)
                })
            })
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "native map has no walkable tile unoccupied by a player or static creature"
                        .into(),
                )
            })?;
        player.position = position;
        world.add_player(player).map_err(HostError::Core)?;
        self.mark_visibility_changed();
        Ok(position)
    }

    pub fn remove_player(&self, id: u64) -> Result<(), HostError> {
        self.lock()?.remove_player(id).map_err(HostError::Core)?;
        self.mark_visibility_changed();
        Ok(())
    }

    fn mark_visibility_changed(&self) {
        self.visibility_epoch.fetch_add(1, Ordering::SeqCst);
    }

    fn lock(&self) -> Result<MutexGuard<'_, WorldState>, HostError> {
        self.world
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)
    }
}

#[derive(Debug)]
struct SharedNativePlayerRegistration {
    world: SharedNativeWorld,
    player_id: u64,
}

impl Drop for SharedNativePlayerRegistration {
    fn drop(&mut self) {
        self.world.unregister_public_chat_recipient(self.player_id);
        let _ = self.world.remove_player(self.player_id);
    }
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
            if empty_world.player_speed == 0 || empty_world.server_beat == 0 {
                return Err(HostError::InvalidConfiguration(
                    "native empty-world fixture requires nonzero speed and beat values".into(),
                ));
            }
            if self.world_map.is_none() {
                return Err(HostError::InvalidConfiguration(
                    "native map initialization requires a loaded world map".into(),
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
    let shared_world = SharedNativeWorld::from_static_spawns(config.static_spawns.as_deref())?;
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve_native_otclient_game(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
            shared_world,
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
    shared_world: SharedNativeWorld,
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
                let session_world = shared_world.clone();
                thread::spawn(move || {
                    let result = handle_native_otclient_game(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                        &session_world,
                    );
                    if let Err(error) = result {
                        eprintln!("> Native OTCv8 game session ended peer={peer} reason={error}");
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
    stream.set_nonblocking(false)?;
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
    shared_world: &SharedNativeWorld,
) -> Result<(), HostError> {
    stream.set_nonblocking(false)?;
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
    let Some(world_map) = &config.world_map else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error(
                "Forgotten Engine native map initialization requires a selected world map.",
            ),
        )?;
        return Ok(());
    };
    let account_id = u64::try_from(account.id).map_err(|_| {
        HostError::InvalidConfiguration("native numeric account IDs must be non-negative".into())
    })?;
    let initial_position = match shared_world.register_player_at_available_position(
        Player {
            id: character.id,
            account_id,
            name: character.name.clone(),
            position: character.position,
            level: character.level,
            experience: 0,
            skill_points: 0,
        },
        world_map,
    ) {
        Ok(position) => position,
        Err(HostError::Core(forgotten_core::CoreError::DuplicatePlayer(_))) => {
            write_frame(
                stream,
                &encode_native_otclient_game_login_error(
                    "Character is already active in the shared world.",
                ),
            )?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let _registration = SharedNativePlayerRegistration {
        world: shared_world.clone(),
        player_id: character.id,
    };
    let chat_events = shared_world.register_public_chat_recipient(character.id)?;
    if initial_position != character.position {
        database.update_player_position(character.id, initial_position)?;
    }
    let player_id = native_player_id(character.id)?;
    let snapshot = NativeOtClientEmptyWorldSnapshot {
        player_id,
        player_name: character.name.clone(),
        player_position: native_position(initial_position),
        player_level: character.level.try_into().unwrap_or(u16::MAX),
        ground_thing_id: empty_world.ground_thing_id,
        player_look_type: empty_world.player_look_type,
        player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
        player_speed: empty_world.player_speed,
        server_beat: empty_world.server_beat,
    };
    let active_static_spawns = shared_world.active_static_spawns()?;
    let visible_players = shared_world.visible_players(
        character.id,
        empty_world.player_look_type,
        empty_world.player_speed,
    )?;
    let initialization =
        encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players(
            &config.client_profile,
            &snapshot,
            world_map,
            Some(&active_static_spawns),
            Some(&visible_players),
        )
        .map_err(HostError::Protocol)?;
    write_frame(stream, &initialization)?;
    stream.set_read_timeout(Some(NATIVE_OTCLIENT_HEARTBEAT_INTERVAL))?;
    if config.extended_diagnostics {
        eprintln!(
            "> Native OTCv8 map init sent peer={peer} player={} record-bytes={} map={} tiles={} static-spawns={} login-state-opcode=0x0a map-opcode=0x64 asset-free={}",
            character.name,
            initialization.0.len(),
            world_map.identifier(),
            world_map.tile_count(),
            active_static_spawns.entities.len(),
            snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0,
        );
    }

    let mut player_position = initial_position;
    let mut facing = NativeOtClientCardinalDirection::South;
    let mut active_click_walk: Option<NativeActiveClickWalk> = None;
    let mut observed_visibility_epoch = shared_world.visibility_epoch();
    loop {
        drain_shared_public_chat(
            stream,
            &config.client_profile,
            &chat_events,
            config.extended_diagnostics,
            peer,
        )?;
        let read_timeout = active_click_walk
            .as_ref()
            .map(|task| {
                task.next_step_deadline
                    .saturating_duration_since(Instant::now())
                    .min(NATIVE_OTCLIENT_HEARTBEAT_INTERVAL)
                    .max(Duration::from_millis(1))
            })
            .unwrap_or(NATIVE_OTCLIENT_HEARTBEAT_INTERVAL);
        stream.set_read_timeout(Some(read_timeout))?;
        let action = {
            let request = match read_frame(stream) {
                Ok(request) => request,
                Err(HostError::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    drain_shared_public_chat(
                        stream,
                        &config.client_profile,
                        &chat_events,
                        config.extended_diagnostics,
                        peer,
                    )?;
                    let visibility_epoch = shared_world.visibility_epoch();
                    if visibility_epoch != observed_visibility_epoch {
                        let mut refreshed_snapshot = snapshot.clone();
                        refreshed_snapshot.player_position = native_position(player_position);
                        refreshed_snapshot.player_direction = facing.protocol_direction();
                        let refreshed_viewport = encode_shared_native_world_viewport(
                            &config.client_profile,
                            &refreshed_snapshot,
                            world_map,
                            shared_world,
                            character.id,
                        )?;
                        write_frame(stream, &refreshed_viewport)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=viewport-refresh reason=visibility-epoch epoch={visibility_epoch} bytes={}",
                                refreshed_viewport.0.len()
                            ),
                        );
                        observed_visibility_epoch = visibility_epoch;
                        continue;
                    }
                    if active_click_walk
                        .as_ref()
                        .is_some_and(|task| task.next_step_deadline <= Instant::now())
                    {
                        let next_step = active_click_walk
                            .as_mut()
                            .and_then(|task| task.queued_steps.pop_front());
                        let Some(direction) = next_step else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "scheduler=click-walk-complete queued-steps=0",
                            );
                            active_click_walk = None;
                            continue;
                        };
                        if move_native_map_player(
                            stream,
                            &config.client_profile,
                            &snapshot,
                            &database,
                            shared_world,
                            character.id,
                            world_map,
                            &mut player_position,
                            &mut facing,
                            direction,
                        )? {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=moved position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                            observed_visibility_epoch = shared_world.visibility_epoch();
                            if let Some(task) = active_click_walk.as_mut() {
                                task.next_step_deadline = Instant::now()
                                    + native_autowalk_step_delay(
                                        snapshot.player_speed,
                                        snapshot.server_beat,
                                    );
                            }
                        } else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=blocked position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                            active_click_walk = None;
                        }
                        continue;
                    }
                    write_frame(
                        stream,
                        &encode_native_otclient_game_ping(&config.client_profile)
                            .map_err(HostError::Protocol)?,
                    )?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "outbound=ping opcode=0x1e",
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            let opcode = request.0.first().copied().unwrap_or_default();
            if config.extended_diagnostics {
                eprintln!(
                    "> Native OTCv8 frame peer={peer} opcode=0x{opcode:02x} len={}",
                    request.0.len()
                );
            }
            let decoded = decode_native_otclient_game_action(&request, &config.client_profile)
                .map_err(HostError::Protocol)?;
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                &native_action_diagnostic_summary(&decoded),
            );
            decoded
        };
        match action {
            NativeOtClientGameAction::Ping => write_frame(
                stream,
                &encode_native_otclient_game_ping_back(&config.client_profile)
                    .map_err(HostError::Protocol)?,
            )?,
            NativeOtClientGameAction::PingBack
            | NativeOtClientGameAction::EnterGame
            | NativeOtClientGameAction::ChangeFightModes
            | NativeOtClientGameAction::UseItem
            | NativeOtClientGameAction::RequestOutfit
            | NativeOtClientGameAction::ChangeOutfit => {}
            NativeOtClientGameAction::IgnoredInteraction(opcode) => {
                if config.extended_diagnostics {
                    eprintln!("> Native OTCv8 compatibility action ignored opcode=0x{opcode:02x}");
                }
            }
            NativeOtClientGameAction::SelectTarget(native_selected_id) => {
                apply_native_player_interaction(
                    shared_world,
                    character.id,
                    native_selected_id,
                    NativePlayerInteractionKind::Target,
                    config.extended_diagnostics,
                )?;
            }
            NativeOtClientGameAction::SelectFollow(native_selected_id) => {
                apply_native_player_interaction(
                    shared_world,
                    character.id,
                    native_selected_id,
                    NativePlayerInteractionKind::Follow,
                    config.extended_diagnostics,
                )?;
            }
            NativeOtClientGameAction::Talk(message) => {
                let recipient_count = shared_world.broadcast_public_chat(character.id, &message)?;
                if config.extended_diagnostics {
                    eprintln!(
                        "> Native OTCv8 public chat received bytes={} recipients={recipient_count}",
                        message.len()
                    );
                }
                drain_shared_public_chat(
                    stream,
                    &config.client_profile,
                    &chat_events,
                    config.extended_diagnostics,
                    peer,
                )?;
            }
            NativeOtClientGameAction::LeaveGame => break,
            NativeOtClientGameAction::Stop => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "scheduler=click-walk-cancel reason=stop active={cancelled_click_walk}"
                    ),
                );
                write_frame(
                    stream,
                    &encode_native_otclient_game_cancel_walk_facing(
                        &config.client_profile,
                        facing.protocol_direction(),
                    )
                    .map_err(HostError::Protocol)?,
                )?;
            }
            NativeOtClientGameAction::Turn(direction) => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "scheduler=click-walk-cancel reason=turn active={cancelled_click_walk} direction={direction:?}"
                    ),
                );
                facing = direction;
                write_frame(
                    stream,
                    &encode_native_otclient_game_cancel_walk_facing(
                        &config.client_profile,
                        facing.protocol_direction(),
                    )
                    .map_err(HostError::Protocol)?,
                )?;
            }
            NativeOtClientGameAction::AutoWalk(path) => {
                if let Some(task) = active_click_walk.as_mut() {
                    let previous_steps = task.queued_steps.len();
                    let replacement_steps = native_click_walk_steps(path.clone()).len();
                    task.replace_path(path);
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "scheduler=click-walk-replace previous-steps={previous_steps} queued-steps={replacement_steps}"
                        ),
                    );
                } else {
                    let step_delay =
                        native_autowalk_step_delay(snapshot.player_speed, snapshot.server_beat);
                    let mut task =
                        NativeActiveClickWalk::from_path(path, Instant::now() + step_delay);
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "scheduler=click-walk-create queued-steps={} step-delay-ms={}",
                            task.queued_steps.len(),
                            step_delay.as_millis()
                        ),
                    );
                    if task.queued_steps.is_empty() {
                        continue;
                    }
                    if task.queued_steps.len() == 1 {
                        let direction = task
                            .queued_steps
                            .pop_front()
                            .expect("single queued click-walk step");
                        if move_native_map_player(
                            stream,
                            &config.client_profile,
                            &snapshot,
                            &database,
                            shared_world,
                            character.id,
                            world_map,
                            &mut player_position,
                            &mut facing,
                            direction,
                        )? {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=moved position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                            observed_visibility_epoch = shared_world.visibility_epoch();
                            active_click_walk = Some(task);
                        } else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=blocked position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                        }
                    } else {
                        active_click_walk = Some(task);
                    }
                }
            }
            NativeOtClientGameAction::CardinalMove(direction) => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                let moved = move_native_map_player(
                    stream,
                    &config.client_profile,
                    &snapshot,
                    &database,
                    shared_world,
                    character.id,
                    world_map,
                    &mut player_position,
                    &mut facing,
                    direction,
                )?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "movement=cardinal direction={direction:?} outcome={} position={},{},{} map-update={}",
                        if moved { "moved" } else { "blocked" },
                        player_position.x,
                        player_position.y,
                        player_position.z,
                        if moved { "step" } else { "cancel-walk" }
                    ),
                );
                if cancelled_click_walk {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "scheduler=click-walk-cancel reason=manual-cardinal active=true",
                    );
                }
                if moved {
                    observed_visibility_epoch = shared_world.visibility_epoch();
                }
            }
            NativeOtClientGameAction::DiagonalMove(direction) => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                let moved = move_native_map_player_diagonal(
                    stream,
                    &config.client_profile,
                    &snapshot,
                    &database,
                    shared_world,
                    character.id,
                    world_map,
                    &mut player_position,
                    &mut facing,
                    direction,
                )?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "movement=diagonal direction={direction:?} outcome={} position={},{},{} map-update={}",
                        if moved { "moved" } else { "blocked" },
                        player_position.x,
                        player_position.y,
                        player_position.z,
                        if moved { "double-step" } else { "cancel-walk" }
                    ),
                );
                if cancelled_click_walk {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "scheduler=click-walk-cancel reason=manual-diagonal active=true",
                    );
                }
                if moved {
                    observed_visibility_epoch = shared_world.visibility_epoch();
                }
            }
        }
    }
    record_event(
        database_path,
        "info",
        &format!(
            "native map session completed peer={peer} account={} character={} protocol={}",
            account.id, request.character_name, request.protocol_version
        ),
    );
    Ok(())
}

fn encode_shared_native_world_viewport(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    shared_world: &SharedNativeWorld,
    observer_id: u64,
) -> Result<Frame, HostError> {
    let active_static_spawns = shared_world.active_static_spawns()?;
    let visible_players = shared_world.visible_players(
        observer_id,
        snapshot.player_look_type,
        snapshot.player_speed,
    )?;
    encode_native_otclient_map_viewport_with_static_spawns_and_players(
        profile,
        snapshot,
        world_map,
        Some(&active_static_spawns),
        Some(&visible_players),
    )
    .map_err(HostError::Protocol)
}

fn drain_shared_public_chat(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    events: &mpsc::Receiver<SharedPublicChatEvent>,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    loop {
        match events.try_recv() {
            Ok(event) => {
                let visible_text =
                    truncate_native_chat_text(&format!("{}: {}", event.speaker_name, event.text));
                let console = encode_native_otclient_game_status_message(profile, &visible_text)
                    .map_err(HostError::Protocol)?;
                write_frame(stream, &console)?;
                native_diagnostic(
                    extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=public-chat-console opcode=0xb4 bytes={} text-bytes={}",
                        console.0.len(),
                        visible_text.len()
                    ),
                );
                let map_label = encode_native_otclient_animated_text(
                    profile,
                    event.speaker_position,
                    &visible_text,
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &map_label)?;
                native_diagnostic(
                    extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=public-chat-map-label opcode=0x84 bytes={} position={},{},{} text-bytes={}",
                        map_label.0.len(),
                        event.speaker_position.x,
                        event.speaker_position.y,
                        event.speaker_position.z,
                        visible_text.len()
                    ),
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// Applies one externally selected static-creature step and returns a full native map refresh.
/// It deliberately makes no AI decision, schedules no autonomous movement, and performs no
/// combat, Lua, spell, or action behavior.
pub fn move_native_static_creature_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world: &mut WorldState,
    world_map: &WorldMap,
    creature_id: u32,
    direction: CardinalDirection,
) -> Result<Frame, HostError> {
    world
        .move_static_creature_cardinal(creature_id, direction, world_map)
        .map_err(HostError::Core)?;
    let active_static_spawns = world.active_static_spawn_collection();
    encode_native_otclient_map_viewport_with_static_spawns(
        profile,
        snapshot,
        world_map,
        Some(&active_static_spawns),
    )
    .map_err(HostError::Protocol)
}

/// Applies a caller-triggered deterministic static creature policy and emits a native map refresh
/// only if that policy made at least one move. It does not create an autonomous scheduler.
pub fn apply_native_static_creature_policy_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world: &mut WorldState,
    world_map: &WorldMap,
    policy: StaticCreatureDecisionPolicy,
) -> Result<(StaticCreatureDecisionBatch, Option<Frame>), HostError> {
    let batch = world
        .apply_static_creature_policy(policy, world_map)
        .map_err(HostError::Core)?;
    if batch.decisions.is_empty() {
        return Ok((batch, None));
    }
    let active_static_spawns = world.active_static_spawn_collection();
    let frame = encode_native_otclient_map_viewport_with_static_spawns(
        profile,
        snapshot,
        world_map,
        Some(&active_static_spawns),
    )
    .map_err(HostError::Protocol)?;
    Ok((batch, Some(frame)))
}

#[allow(clippy::too_many_arguments)]
fn move_native_map_player(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    database: &EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    world_map: &WorldMap,
    player_position: &mut Position,
    facing: &mut NativeOtClientCardinalDirection,
    direction: NativeOtClientCardinalDirection,
) -> Result<bool, HostError> {
    let moved = {
        let mut world = shared_world.lock()?;
        let source = world
            .player(character_id)
            .ok_or(forgotten_core::CoreError::UnknownPlayer(character_id))
            .map_err(HostError::Core)?
            .position;
        let destination = source
            .step(native_cardinal_direction(direction))
            .map_err(HostError::Core)?;
        if !world_map.is_walkable(destination)
            || world.is_static_creature_occupied(destination)
            || world.is_player_occupied(destination)
        {
            None
        } else {
            let (previous, destination) = world
                .move_player_cardinal(character_id, native_cardinal_direction(direction))
                .map_err(HostError::Core)?;
            let active_static_spawns = world.active_static_spawn_collection();
            Some((previous, destination, active_static_spawns))
        }
    };
    let Some((previous, destination, active_static_spawns)) = moved else {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    };
    shared_world.mark_visibility_changed();
    database.update_player_position(character_id, destination)?;
    *facing = direction;
    write_frame(
        stream,
        &encode_native_otclient_move_creature_at(
            profile,
            native_position(previous),
            1,
            native_position(destination),
        )
        .map_err(HostError::Protocol)?,
    )?;
    let mut refreshed_snapshot = snapshot.clone();
    refreshed_snapshot.player_position = native_position(destination);
    refreshed_snapshot.player_direction = facing.protocol_direction();
    let visible_players = shared_world.visible_players(
        character_id,
        snapshot.player_look_type,
        snapshot.player_speed,
    )?;
    write_frame(
        stream,
        &encode_native_otclient_map_step_with_static_spawns_and_players(
            profile,
            &refreshed_snapshot,
            world_map,
            Some(&active_static_spawns),
            Some(&visible_players),
            direction,
        )
        .map_err(HostError::Protocol)?,
    )?;
    *player_position = destination;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn move_native_map_player_diagonal(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    database: &EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    world_map: &WorldMap,
    player_position: &mut Position,
    facing: &mut NativeOtClientCardinalDirection,
    direction: NativeOtClientAutoWalkDirection,
) -> Result<bool, HostError> {
    let steps = direction.cardinal_steps();
    debug_assert_eq!(steps.len(), 2);
    let moved = {
        let mut world = shared_world.lock()?;
        let source = world
            .player(character_id)
            .ok_or(forgotten_core::CoreError::UnknownPlayer(character_id))
            .map_err(HostError::Core)?
            .position;
        let intermediate = source
            .step(native_cardinal_direction(steps[0]))
            .map_err(HostError::Core)?;
        let destination = intermediate
            .step(native_cardinal_direction(steps[1]))
            .map_err(HostError::Core)?;
        let blocked = [intermediate, destination].into_iter().any(|position| {
            !world_map.is_walkable(position)
                || world.is_static_creature_occupied(position)
                || world.is_player_occupied(position)
        });
        if blocked {
            None
        } else {
            world
                .move_player(character_id, destination)
                .map_err(HostError::Core)?;
            let active_static_spawns = world.active_static_spawn_collection();
            Some((source, intermediate, destination, active_static_spawns))
        }
    };
    let Some((previous, intermediate, destination, active_static_spawns)) = moved else {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    };
    shared_world.mark_visibility_changed();
    database.update_player_position(character_id, destination)?;
    *facing = steps[1];
    write_frame(
        stream,
        &encode_native_otclient_move_creature_at(
            profile,
            native_position(previous),
            1,
            native_position(destination),
        )
        .map_err(HostError::Protocol)?,
    )?;
    let visible_players = shared_world.visible_players(
        character_id,
        snapshot.player_look_type,
        snapshot.player_speed,
    )?;
    for (step, position) in [(steps[0], intermediate), (steps[1], destination)] {
        let mut refreshed_snapshot = snapshot.clone();
        refreshed_snapshot.player_position = native_position(position);
        refreshed_snapshot.player_direction = step.protocol_direction();
        write_frame(
            stream,
            &encode_native_otclient_map_step_with_static_spawns_and_players(
                profile,
                &refreshed_snapshot,
                world_map,
                Some(&active_static_spawns),
                Some(&visible_players),
                step,
            )
            .map_err(HostError::Protocol)?,
        )?;
    }
    *player_position = destination;
    Ok(true)
}

fn native_autowalk_step_delay(player_speed: u16, server_beat: u16) -> Duration {
    let speed = u64::from(player_speed).max(1);
    let server_beat = u64::from(server_beat).max(1);
    let interval_millis = (1000 * NATIVE_OTCLIENT_DEFAULT_GROUND_SPEED / speed)
        .max(server_beat)
        .min(NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY.as_millis() as u64);
    Duration::from_millis(interval_millis)
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

#[derive(Debug, Clone, Copy)]
enum NativePlayerInteractionKind {
    Target,
    Follow,
}

/// A single server-owned native click-walk task. Client paths may replace its queued directions,
/// but never its next-step deadline. This mirrors the classic one-active-event behavior without
/// importing implementation code from another server.
struct NativeActiveClickWalk {
    queued_steps: VecDeque<NativeOtClientCardinalDirection>,
    next_step_deadline: Instant,
}

impl NativeActiveClickWalk {
    fn from_path(path: Vec<NativeOtClientAutoWalkDirection>, next_step_deadline: Instant) -> Self {
        Self {
            queued_steps: native_click_walk_steps(path),
            next_step_deadline,
        }
    }

    fn replace_path(&mut self, path: Vec<NativeOtClientAutoWalkDirection>) {
        self.queued_steps = native_click_walk_steps(path);
    }
}

fn native_click_walk_steps(
    path: Vec<NativeOtClientAutoWalkDirection>,
) -> VecDeque<NativeOtClientCardinalDirection> {
    path.into_iter()
        .flat_map(|direction| direction.cardinal_steps().iter().copied())
        .collect()
}

fn apply_native_player_interaction(
    shared_world: &SharedNativeWorld,
    source_player_id: u64,
    native_selected_id: u32,
    kind: NativePlayerInteractionKind,
    extended_diagnostics: bool,
) -> Result<(), HostError> {
    let selected_player_id = if native_selected_id == 0 {
        Some(None)
    } else {
        native_player_id_to_character_id(native_selected_id).map(Some)
    };
    let Some(selected_player_id) = selected_player_id else {
        if extended_diagnostics {
            eprintln!(
                "> Native OTCv8 {:?} selection deferred native-id={native_selected_id}",
                kind
            );
        }
        return Ok(());
    };
    let result = match kind {
        NativePlayerInteractionKind::Target => {
            shared_world.set_player_target(source_player_id, selected_player_id)
        }
        NativePlayerInteractionKind::Follow => {
            shared_world.set_player_follow(source_player_id, selected_player_id)
        }
    };
    match result {
        Ok(_) => Ok(()),
        Err(HostError::Core(forgotten_core::CoreError::UnknownPlayer(_)))
        | Err(HostError::Core(forgotten_core::CoreError::SelfInteractionNotAllowed(_))) => {
            if extended_diagnostics {
                eprintln!(
                    "> Native OTCv8 {:?} selection ignored native-id={native_selected_id}",
                    kind
                );
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn native_player_id_to_character_id(native_id: u32) -> Option<u64> {
    (NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END)
        .contains(&native_id)
        .then(|| u64::from(native_id - NATIVE_OTCLIENT_PLAYER_ID_START))
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
    SharedWorldUnavailable,
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
            Self::SharedWorldUnavailable => b"shared-world-unavailable",
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
    use forgotten_core::{Player, Position, WorldMapTile};
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
            extended_diagnostics: false,
            empty_world: None,
            world_map: None,
            static_spawns: None,
        }
    }

    fn native_world_map() -> Arc<WorldMap> {
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("native-test", spawn);
        for x in 80..=120 {
            for y in 80..=120 {
                map.set_tile(
                    Position { x, y, z: 7 },
                    WorldMapTile {
                        ground_thing_id: 102,
                        walkable: true,
                    },
                )
                .unwrap();
            }
        }
        map.validate().unwrap();
        Arc::new(map)
    }

    #[test]
    fn shared_native_world_synchronizes_concurrent_player_registration_and_cleanup() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let first_world = shared.clone();
        let first_map = Arc::clone(&map);
        let first = thread::spawn(move || {
            first_world
                .register_player_at_available_position(
                    Player {
                        id: 101,
                        account_id: 1,
                        name: "Knight".into(),
                        position: first_map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &first_map,
                )
                .unwrap()
        });
        let second_world = shared.clone();
        let second_map = Arc::clone(&map);
        let second = thread::spawn(move || {
            second_world
                .register_player_at_available_position(
                    Player {
                        id: 102,
                        account_id: 2,
                        name: "Druid".into(),
                        position: second_map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &second_map,
                )
                .unwrap()
        });
        let first_position = first.join().unwrap();
        let second_position = second.join().unwrap();
        assert_ne!(first_position, second_position);
        assert_eq!(shared.tick().unwrap(), 0);
        assert_eq!(shared.advance_tick().unwrap(), 1);
        assert_eq!(shared.tick().unwrap(), 1);
        shared.remove_player(101).unwrap();
        let recycled = shared
            .register_player_at_available_position(
                Player {
                    id: 103,
                    account_id: 3,
                    name: "Sorcerer".into(),
                    position: first_position,
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        assert_eq!(recycled, first_position);
        shared.remove_player(102).unwrap();
        shared.remove_player(103).unwrap();
    }

    #[test]
    fn shared_native_world_tracks_and_clears_player_interaction_intent() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        for (id, name) in [(101, "Knight"), (102, "Druid")] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position: map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap();
        }
        assert_eq!(
            shared.set_player_target(101, Some(102)).unwrap(),
            PlayerInteractionIntent {
                target_player_id: Some(102),
                follow_player_id: None,
            }
        );
        assert_eq!(
            shared.set_player_follow(101, Some(102)).unwrap(),
            PlayerInteractionIntent {
                target_player_id: Some(102),
                follow_player_id: Some(102),
            }
        );
        shared.remove_player(102).unwrap();
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent::default()
        );
    }

    #[test]
    fn native_player_interaction_ids_only_accept_the_reserved_player_range() {
        assert_eq!(
            native_player_id_to_character_id(NATIVE_OTCLIENT_PLAYER_ID_START + 101),
            Some(101)
        );
        assert_eq!(native_player_id_to_character_id(0), None);
        assert_eq!(
            native_player_id_to_character_id(NATIVE_OTCLIENT_PLAYER_ID_END),
            None
        );
    }

    #[test]
    fn native_player_interaction_application_preserves_follow_and_defers_non_players() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        for (id, name) in [(101, "Knight"), (102, "Druid")] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position: map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap();
        }

        apply_native_player_interaction(
            &shared,
            101,
            NATIVE_OTCLIENT_PLAYER_ID_START + 102,
            NativePlayerInteractionKind::Target,
            false,
        )
        .unwrap();
        apply_native_player_interaction(
            &shared,
            101,
            NATIVE_OTCLIENT_PLAYER_ID_START + 102,
            NativePlayerInteractionKind::Follow,
            false,
        )
        .unwrap();
        apply_native_player_interaction(
            &shared,
            101,
            0,
            NativePlayerInteractionKind::Target,
            false,
        )
        .unwrap();
        apply_native_player_interaction(
            &shared,
            101,
            NATIVE_OTCLIENT_PLAYER_ID_END,
            NativePlayerInteractionKind::Follow,
            false,
        )
        .unwrap();
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent {
                target_player_id: None,
                follow_player_id: Some(102),
            }
        );
    }

    #[test]
    fn shared_player_visibility_tracks_join_move_and_leave() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let knight_position = shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let druid_position = shared
            .register_player_at_available_position(
                Player {
                    id: 102,
                    account_id: 2,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        assert_eq!(knight_position, map.spawn());
        assert_eq!(druid_position.x, 101);
        assert_eq!(shared.visibility_epoch(), 2);
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(knight_position),
            player_level: 8,
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let joined =
            encode_shared_native_world_viewport(&profile, &snapshot, &map, &shared, 101).unwrap();
        assert!(joined.0.windows(5).any(|window| window == b"Druid"));
        {
            let mut world = shared.lock().unwrap();
            world
                .move_player_cardinal(102, CardinalDirection::East)
                .unwrap();
        }
        shared.mark_visibility_changed();
        assert_eq!(shared.visibility_epoch(), 3);
        assert_eq!(
            shared.visible_players(101, 128, 220).unwrap()[0].position,
            native_position(Position {
                x: 102,
                y: 100,
                z: 7,
            })
        );
        let moved =
            encode_shared_native_world_viewport(&profile, &snapshot, &map, &shared, 101).unwrap();
        assert!(moved.0.windows(5).any(|window| window == b"Druid"));
        shared.remove_player(102).unwrap();
        assert_eq!(shared.visibility_epoch(), 4);
        let left =
            encode_shared_native_world_viewport(&profile, &snapshot, &map, &shared, 101).unwrap();
        assert!(!left.0.windows(5).any(|window| window == b"Druid"));
        shared.remove_player(101).unwrap();
    }

    #[test]
    fn shared_public_chat_broadcasts_sanitized_events_and_releases_recipients() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let _knight_position = shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 102,
                    account_id: 2,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let knight_events = shared.register_public_chat_recipient(101).unwrap();
        let druid_events = shared.register_public_chat_recipient(102).unwrap();
        assert_eq!(
            shared
                .broadcast_public_chat(101, "  hello\n world  ")
                .unwrap(),
            2
        );
        let expected = SharedPublicChatEvent {
            speaker_name: "Knight".into(),
            speaker_position: native_position(map.spawn()),
            text: "hello world".into(),
        };
        assert_eq!(knight_events.try_recv().unwrap(), expected);
        assert_eq!(druid_events.try_recv().unwrap(), expected);
        assert_eq!(shared.broadcast_public_chat(101, "   ").unwrap(), 0);
        assert_eq!(
            shared
                .broadcast_public_chat(101, &"x".repeat(NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES))
                .unwrap(),
            2
        );
        let capped = knight_events.try_recv().unwrap();
        assert_eq!(capped.speaker_name, "Knight");
        assert_eq!(capped.speaker_position, native_position(map.spawn()));
        assert_eq!(capped.text.len(), NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES);
        assert_eq!(druid_events.try_recv().unwrap(), capped);
        shared.unregister_public_chat_recipient(102);
        assert_eq!(shared.broadcast_public_chat(101, "again").unwrap(), 1);
        assert_eq!(knight_events.try_recv().unwrap().text, "again".to_string());
        assert!(matches!(
            druid_events.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        shared.unregister_public_chat_recipient(101);
        shared.remove_player(101).unwrap();
        shared.remove_player(102).unwrap();
    }

    #[test]
    fn shared_public_chat_bounds_a_slow_recipient_queue_without_unregistering_it() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let events = shared.register_public_chat_recipient(101).unwrap();
        for index in 0..NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY {
            assert_eq!(
                shared
                    .broadcast_public_chat(101, &format!("queued-{index}"))
                    .unwrap(),
                1
            );
        }
        assert_eq!(shared.broadcast_public_chat(101, "dropped").unwrap(), 0);
        assert!(events.try_recv().is_ok());
        assert_eq!(shared.broadcast_public_chat(101, "resumed").unwrap(), 1);
        shared.unregister_public_chat_recipient(101);
        shared.remove_player(101).unwrap();
    }

    fn native_empty_world_config(bind_addr: SocketAddr) -> NativeOtClientHostConfig {
        let mut config = native_otclient_config(bind_addr);
        config.empty_world = Some(NativeOtClientEmptyWorldConfig {
            ground_thing_id: 102,
            player_look_type: 128,
            player_speed: 220,
            server_beat: 50,
        });
        config.world_map = Some(native_world_map());
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
        payload.push(0);
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
        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 102,
                    z: 7,
                },
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
            .unwrap(),
        ));
        Arc::get_mut(native_config.world_map.as_mut().unwrap())
            .unwrap()
            .set_tile(
                Position {
                    x: 103,
                    y: 101,
                    z: 7,
                },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: false,
                },
            )
            .unwrap();
        Arc::get_mut(native_config.world_map.as_mut().unwrap())
            .unwrap()
            .set_tile(
                Position {
                    x: 101,
                    y: 99,
                    z: 7,
                },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: false,
                },
            )
            .unwrap();
        let game = start_native_otclient_game(native_config, &database_path).unwrap();

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
        let initialization = read_frame(&mut stream).unwrap();
        assert_eq!(
            initialization.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        assert_eq!(
            initialization.0[8],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(initialization
            .0
            .windows(6)
            .any(|window| window == b"Knight"));
        assert!(initialization.0.windows(3).any(|window| window == b"Rat"));
        assert!(initialization
            .0
            .contains(&forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_STATS));
        assert!(initialization
            .0
            .contains(&forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_SKILLS));

        let heartbeat = read_frame(&mut stream).unwrap();
        assert_eq!(
            heartbeat.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_PING]
        );
        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING_BACK]),
        )
        .unwrap();
        write_frame(&mut stream, &Frame(vec![0xa0, 1, 0, 1])).unwrap();
        write_frame(&mut stream, &Frame(vec![0x1d])).unwrap();
        let ping_back = read_frame(&mut stream).unwrap();
        assert_eq!(ping_back.0, vec![0x1d]);

        let auto_walk_started = Instant::now();
        write_frame(&mut stream, &Frame(vec![0x64, 2, 1, 3])).unwrap();
        let auto_walk_east = read_frame(&mut stream).unwrap();
        assert!(auto_walk_started.elapsed() >= Duration::from_millis(500));
        assert_eq!(&auto_walk_east.0[1..7], &[100, 0, 100, 0, 7, 1]);
        assert_eq!(&auto_walk_east.0[7..12], &[101, 0, 100, 0, 7]);
        let auto_walk_edge = read_frame(&mut stream).unwrap();
        assert_eq!(
            auto_walk_edge.0[0],
            NativeOtClientCardinalDirection::East.protocol_direction() + 0x65
        );
        assert_ne!(
            auto_walk_edge.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        let replacement_started = Instant::now();
        write_frame(&mut stream, &Frame(vec![0x64, 1, 7])).unwrap();
        write_frame(&mut stream, &Frame(vec![0x64, 1, 5])).unwrap();
        let latest_path_movement = read_frame(&mut stream).unwrap();
        assert!(replacement_started.elapsed() >= Duration::from_millis(500));
        assert_eq!(&latest_path_movement.0[1..7], &[101, 0, 100, 0, 7, 1]);
        assert_eq!(&latest_path_movement.0[7..12], &[100, 0, 100, 0, 7]);
        let latest_path_edge = read_frame(&mut stream).unwrap();
        assert_eq!(latest_path_edge.0[0], 0x68);
        write_frame(&mut stream, &Frame(vec![0x67])).unwrap();
        let manual_movement = read_frame(&mut stream).unwrap();
        assert_eq!(&manual_movement.0[1..7], &[100, 0, 100, 0, 7, 1]);
        assert_eq!(&manual_movement.0[7..12], &[100, 0, 101, 0, 7]);
        let manual_edge = read_frame(&mut stream).unwrap();
        assert_eq!(
            manual_edge.0[0],
            NativeOtClientCardinalDirection::South.protocol_direction() + 0x65
        );
        assert_ne!(
            manual_edge.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let movement = read_frame(&mut stream).unwrap();
        assert_eq!(
            movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        assert_eq!(&movement.0[1..7], &[100, 0, 101, 0, 7, 1]);
        assert_eq!(&movement.0[7..12], &[101, 0, 101, 0, 7]);
        let movement_edge = read_frame(&mut stream).unwrap();
        assert_eq!(
            movement_edge.0[0],
            NativeOtClientCardinalDirection::East.protocol_direction() + 0x65
        );
        assert_ne!(
            movement_edge.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            101
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .y,
            101
        );

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let second_east = read_frame(&mut stream).unwrap();
        assert_eq!(&second_east.0[1..7], &[101, 0, 101, 0, 7, 1]);
        assert_eq!(&second_east.0[7..12], &[102, 0, 101, 0, 7]);
        let second_east_edge = read_frame(&mut stream).unwrap();
        assert_eq!(second_east_edge.0[0], 0x66);

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let blocked_movement = read_frame(&mut stream).unwrap();
        assert_eq!(
            blocked_movement.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 1]
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            102
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST,
            ]),
        )
        .unwrap();
        let diagonal_movement = read_frame(&mut stream).unwrap();
        assert_eq!(
            diagonal_movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        assert_eq!(&diagonal_movement.0[1..7], &[102, 0, 101, 0, 7, 1]);
        assert_eq!(&diagonal_movement.0[7..12], &[101, 0, 100, 0, 7]);
        let north_edge = read_frame(&mut stream).unwrap();
        assert_eq!(north_edge.0[0], 0x65);
        let west_edge = read_frame(&mut stream).unwrap();
        assert_eq!(west_edge.0[0], 0x68);
        let diagonal_position = database.characters_for_account(account_id).unwrap()[0].position;
        assert_eq!(diagonal_position.x, 101);
        assert_eq!(diagonal_position.y, 100);
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST,
            ]),
        )
        .unwrap();
        let blocked_diagonal = read_frame(&mut stream).unwrap();
        assert_eq!(
            blocked_diagonal.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 3]
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            diagonal_position
        );

        write_frame(&mut stream, &Frame(vec![0x96, 1, 2, 0, b'h', b'i'])).unwrap();
        let chat_console = read_frame(&mut stream).unwrap();
        assert_eq!(
            chat_console.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE,
                forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_STATUS_CONSOLE_BLUE,
                10,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                b':',
                b' ',
                b'h',
                b'i',
            ]
        );
        assert!(!chat_console.0.contains(&0xaa));
        let chat_map_label = read_frame(&mut stream).unwrap();
        assert_eq!(
            chat_map_label.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_ANIMATED_TEXT,
                101,
                0,
                100,
                0,
                7,
                215,
                10,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                b':',
                b' ',
                b'h',
                b'i',
            ]
        );
        write_frame(
            &mut stream,
            &Frame(vec![0x78, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]),
        )
        .unwrap();
        write_frame(&mut stream, &Frame(vec![0xa1, 1, 0, 0, 0])).unwrap();
        write_frame(&mut stream, &Frame(vec![0x69])).unwrap();
        let cancelled = read_frame(&mut stream).unwrap();
        assert_eq!(
            cancelled.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 3]
        );
        write_frame(&mut stream, &Frame(vec![0x71])).unwrap();
        let turned = read_frame(&mut stream).unwrap();
        assert_eq!(
            turned.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 2]
        );

        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn static_creature_occupancy_cancels_native_player_movement() {
        let database_path = database_path("native-static-occupancy");
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
        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }])
            .unwrap(),
        ));
        let game = start_native_otclient_game(native_config, &database_path).unwrap();

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
        let initialization = read_frame(&mut stream).unwrap();
        assert!(initialization.0.windows(3).any(|window| window == b"Rat"));
        let heartbeat = read_frame(&mut stream).unwrap();
        assert_eq!(
            heartbeat.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_PING]
        );
        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING_BACK]),
        )
        .unwrap();
        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let blocked = read_frame(&mut stream).unwrap();
        assert_eq!(
            blocked.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 2]
        );
        let character = database
            .characters_for_account(account_id)
            .unwrap()
            .remove(0);
        assert_eq!(
            character.position,
            Position {
                x: 100,
                y: 100,
                z: 7,
            }
        );

        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn server_owned_static_creature_move_refreshes_native_visibility() {
        let map = native_world_map();
        let creature = forgotten_core::FeTfsStaticEntity {
            id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let frame = move_native_static_creature_and_refresh(
            &profile,
            &snapshot,
            &mut world,
            &map,
            NATIVE_OTCLIENT_PLAYER_ID_END + 1,
            CardinalDirection::East,
        )
        .unwrap();
        assert_eq!(
            frame.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(frame.0.windows(3).any(|window| window == b"Rat"));
        assert_eq!(
            world
                .static_creature(NATIVE_OTCLIENT_PLAYER_ID_END + 1)
                .unwrap()
                .position,
            Position {
                x: 102,
                y: 100,
                z: 7,
            }
        );
    }

    #[test]
    fn opt_in_static_creature_policy_moves_and_refreshes_native_visibility() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let (batch, refresh) = apply_native_static_creature_policy_and_refresh(
            &profile,
            &snapshot,
            &mut world,
            &map,
            StaticCreatureDecisionPolicy::ClockwiseAdjacent,
        )
        .unwrap();
        assert_eq!(batch.decisions.len(), 1);
        let refresh = refresh.expect("an applied move must refresh the map");
        assert_eq!(
            refresh.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(refresh.0.windows(3).any(|window| window == b"Rat"));
        assert_eq!(
            world.static_creature(creature_id).unwrap().position,
            Position {
                x: 102,
                y: 100,
                z: 7,
            }
        );
        let (disabled, refresh) = apply_native_static_creature_policy_and_refresh(
            &profile,
            &snapshot,
            &mut world,
            &map,
            StaticCreatureDecisionPolicy::Disabled,
        )
        .unwrap();
        assert!(disabled.decisions.is_empty());
        assert!(refresh.is_none());
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

#[cfg(test)]
mod native_timing_tests {
    use super::{native_autowalk_step_delay, NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY};
    use std::time::Duration;

    #[test]
    fn auto_walk_delay_scales_with_player_speed_and_server_beat() {
        assert_eq!(
            native_autowalk_step_delay(220, 50),
            Duration::from_millis(681)
        );
        assert!(native_autowalk_step_delay(440, 50) < native_autowalk_step_delay(220, 50));
        assert_eq!(
            native_autowalk_step_delay(1, 50),
            NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY
        );
        assert_eq!(
            native_autowalk_step_delay(1000, 750),
            Duration::from_millis(750)
        );
    }
}

#[cfg(test)]
mod native_diagnostics_tests {
    use super::{
        native_action_diagnostic_summary, native_diagnostic_record, NativeOtClientGameAction,
    };
    use forgotten_protocol::NativeOtClientCardinalDirection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn diagnostic_records_are_strictly_opt_in() {
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7175);
        assert!(native_diagnostic_record(false, peer, "action=ping").is_none());
        assert_eq!(
            native_diagnostic_record(true, peer, "action=ping").as_deref(),
            Some("> Native OTCv8 trace peer=127.0.0.1:7175 action=ping")
        );
    }

    #[test]
    fn action_summaries_report_metadata_without_chat_text_or_raw_bytes() {
        assert_eq!(
            native_action_diagnostic_summary(&NativeOtClientGameAction::CardinalMove(
                NativeOtClientCardinalDirection::North
            )),
            "action=cardinal-move direction=North"
        );
        let secret_message = "correct horse battery staple".to_owned();
        let talk_summary =
            native_action_diagnostic_summary(&NativeOtClientGameAction::Talk(secret_message));
        assert_eq!(talk_summary, "action=talk text-bytes=28");
        assert!(!talk_summary.contains("correct"));
        assert!(!talk_summary.contains("68 6f 72"));
    }
}
