//! Session serve/login handlers and the per-peer auth rate limiter: probe/status/game
//! listener accept loops, legacy and native login serving, persisted-player relog
//! respawn, static-creature runtime persistence, and brute-force rate limiting.

use super::*;

pub(crate) fn serve(
    listener: TcpListener,
    config: HostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    _online_players: Arc<AtomicU64>,
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

pub(crate) fn serve_status(
    listener: TcpListener,
    config: StatusHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    online_players: Arc<AtomicU64>,
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
                let session_online = Arc::clone(&online_players);
                thread::spawn(move || {
                    let result = handle_status_session(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                        &session_online,
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

pub(crate) fn serve_game_session(
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

/// Best-effort reattach of a relogging player to their persisted party. The stored relation
/// is only consumed when the stored leader is online; otherwise the row is left in place so
/// a later leader login can still reform the party through the same path.
pub(crate) fn try_hydrate_persisted_party(
    shared_world: &SharedNativeWorld,
    database: &EngineDatabase,
    player_id: u64,
) -> Result<(), HostError> {
    let Some(leader_id) = database.party_leader_of(player_id)? else {
        return Ok(());
    };
    if leader_id == player_id {
        return Ok(());
    }
    let leader_online = shared_world.player_and_vitals(leader_id).is_ok();
    if !leader_online {
        return Ok(());
    }
    let leader_leads_live_party = matches!(
        shared_world.lock()?.player_party_leader(leader_id),
        Ok(Some(_))
    );
    if leader_leads_live_party {
        shared_world.add_existing_party_member(leader_id, player_id)?;
    } else {
        shared_world.restore_party_snapshot(leader_id, &[player_id])?;
    }
    Ok(())
}

pub(crate) fn serve_native_otclient_login(
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

/// Tracks one native session's authoritative lifecycle state. A session that joins while already
/// dead does not synthesize a historical death packet; a future authoritative respawn resets the
/// observation so a subsequent real death can be delivered once. Respawn timing, teleportation,
/// loss, and client-side recovery records remain separate deferred work.
pub(crate) fn observe_native_death_transition(
    shared_world: &SharedNativeWorld,
    player_id: u64,
    observed_dead: &mut bool,
) -> Result<bool, HostError> {
    let dead = shared_world.player_respawn_state(player_id)?.dead;
    let should_notify = dead && !*observed_dead;
    *observed_dead = dead;
    Ok(should_notify)
}

pub(crate) fn serve_native_otclient_game(
    listener: TcpListener,
    config: NativeOtClientHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    shared_world: SharedNativeWorld,
    shared_map: Option<Arc<SharedNativeMap>>,
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
    let heartbeat_shutdown = Arc::clone(&shutdown);
    let heartbeat_world = shared_world.clone();
    let heartbeat_pursuit_policy = config.static_target_pursuit_policy;
    let heartbeat_attack_policy = config.static_target_attack_policy;
    let heartbeat_wander_policy = config.static_creature_wander_policy;
    let heartbeat_wander_every_ticks = config.static_creature_wander_every_ticks;
    let heartbeat_map_owner = shared_map.clone();
    let heartbeat_database_path = database_path.clone();
    let heartbeat_death_loss_policy = config.death_loss_policy;
    let heartbeat_progression_rules = config.progression_rules.clone();
    let heartbeat_corpse_despawn_seconds = config.corpse_despawn_seconds;
    let auth_rate_limiter = Arc::new(NativeAuthRateLimiter::default());
    let heartbeat = thread::spawn(move || {
        run_native_shared_world_heartbeat(
            heartbeat_world,
            heartbeat_shutdown,
            NativeHeartbeatConfig {
                pursuit_policy: heartbeat_pursuit_policy,
                attack_policy: heartbeat_attack_policy,
                wander_policy: heartbeat_wander_policy,
                wander_every_ticks: heartbeat_wander_every_ticks,
                map_owner: heartbeat_map_owner,
                database_path: heartbeat_database_path,
                death_loss_policy: heartbeat_death_loss_policy,
                progression_rules: heartbeat_progression_rules,
                corpse_despawn_seconds: heartbeat_corpse_despawn_seconds,
            },
        )
    });
    let service_result = loop {
        if shutdown.load(Ordering::SeqCst) {
            break Ok(());
        }
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
                let session_map = shared_map.clone();
                let session_rate_limiter = Arc::clone(&auth_rate_limiter);
                thread::spawn(move || {
                    let result = (|| {
                        let session_config = native_session_config_with_map_snapshot(
                            session_config,
                            session_map.as_deref(),
                        )?;
                        handle_native_otclient_game(
                            &mut stream,
                            peer,
                            &session_config,
                            &session_database_path,
                            &session_world,
                            session_map.as_deref(),
                            &session_rate_limiter,
                        )
                    })();
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
            Err(error) => break Err(HostError::Io(error)),
        }
    };
    shutdown.store(true, Ordering::SeqCst);
    let heartbeat_result = match heartbeat.join() {
        Ok(result) => result,
        Err(_) => Err(HostError::HostThreadPanicked),
    };
    service_result?;
    heartbeat_result?;
    persist_static_creature_runtime_to_database(&shared_world, &database_path)?;
    record_event(&database_path, "info", "native client game service stopped");
    Ok(())
}

/// Builds one session-local native configuration with a detached map snapshot from the live map
/// owner. The immutable configuration map is an initialization input only; each accepted session
/// receives the synchronized owner's current map state without retaining its lock.
pub(crate) fn native_session_config_with_map_snapshot(
    mut config: NativeOtClientHostConfig,
    map_owner: Option<&SharedNativeMap>,
) -> Result<NativeOtClientHostConfig, HostError> {
    config.world_map = map_owner
        .map(SharedNativeMap::render_snapshot)
        .transpose()?;
    Ok(config)
}

pub(crate) fn restore_static_creature_runtime_from_database(
    shared_world: &SharedNativeWorld,
    database_path: &Path,
) -> Result<StaticCreatureRuntimeRestoreSummary, HostError> {
    let database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    let records = database
        .static_creature_runtime()
        .map_err(HostError::Persistence)?;
    let snapshots = records
        .into_iter()
        .map(|record| StaticCreatureRuntimeSnapshot {
            id: record.creature_id,
            position: record.position,
            active: record.active,
            health_percent: record.health_percent,
            reactivation_remaining_seconds: record.reactivation_remaining_seconds,
            direct_melee_cooldown_remaining_ticks: record.direct_melee_cooldown_remaining_ticks,
            direct_melee_damage_sequence: record.direct_melee_damage_sequence,
        })
        .collect::<Vec<_>>();
    shared_world.restore_static_creature_runtime(&snapshots)
}

pub(crate) fn persist_static_creature_runtime_to_database(
    shared_world: &SharedNativeWorld,
    database_path: &Path,
) -> Result<(), HostError> {
    let mut database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    persist_static_creature_runtime_to_open_database(shared_world, &mut database)
}

pub(crate) fn persist_static_creature_runtime_to_open_database(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
) -> Result<(), HostError> {
    let snapshots = shared_world.static_creature_runtime_snapshot()?;
    let records = snapshots
        .into_iter()
        .map(|snapshot| StaticCreatureRuntimeRecord {
            creature_id: snapshot.id,
            position: snapshot.position,
            active: snapshot.active,
            health_percent: snapshot.health_percent,
            reactivation_remaining_seconds: snapshot.reactivation_remaining_seconds,
            direct_melee_cooldown_remaining_ticks: snapshot.direct_melee_cooldown_remaining_ticks,
            direct_melee_damage_sequence: snapshot.direct_melee_damage_sequence,
        })
        .collect::<Vec<_>>();
    database
        .replace_static_creature_runtime(&records)
        .map_err(HostError::Persistence)
}

/// Applies the existing authoritative temple-respawn transition before a new native game session
/// registers the persisted character. Stock OTCv8 740's death dialog logs out and starts another
/// character login; there is no separate revival request to route on the prior dead session.
/// This boundary does not add a timer, death-loss record, teleport effect, or in-session revive.
pub(crate) fn respawn_persisted_native_player_for_relog(
    database: &mut EngineDatabase,
    player_id: u64,
    world_map: &WorldMap,
) -> Result<(), HostError> {
    let character = database
        .player_by_id(player_id)
        .map_err(HostError::Persistence)?;
    let mut world = WorldState::default();
    world
        .add_player_with_vitals_and_progression(
            Player {
                id: character.id,
                account_id: 0,
                name: character.name,
                position: character.position,
                level: character.level,
                experience: character.experience,
                skill_points: character.skill_points,
            },
            PlayerVitals {
                health: character.vitals.health,
                max_health: character.vitals.max_health,
                mana: character.vitals.mana,
                max_mana: character.vitals.max_mana,
                capacity: character.vitals.capacity,
                magic_level: character.vitals.magic_level,
            },
            character.progression,
        )
        .map_err(HostError::Core)?;
    world
        .replace_player_town(player_id, character.town_id)
        .map_err(HostError::Core)?;
    world
        .hydrate_player_respawn_state(player_id, character.respawn_state)
        .map_err(HostError::Core)?;
    let outcome = world.respawn_player(player_id).map_err(HostError::Core)?;
    if !world_map
        .tile(outcome.position)
        .is_some_and(|tile| tile.walkable)
    {
        return Err(HostError::InvalidConfiguration(
            "native temple respawn destination is missing or not walkable in the selected world map"
                .into(),
        ));
    }
    database
        .update_player_position_vitals_and_respawn_state(
            player_id,
            outcome.position,
            PersistedPlayerVitals {
                health: outcome.vitals.health,
                max_health: outcome.vitals.max_health,
                mana: outcome.vitals.mana,
                max_mana: outcome.vitals.max_mana,
                capacity: outcome.vitals.capacity,
                magic_level: outcome.vitals.magic_level,
            },
            PlayerRespawnState::default(),
        )
        .map_err(HostError::Persistence)?;
    Ok(())
}

pub(crate) fn handle_native_otclient_login(
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
    // Plan v49 slice 17: banned accounts get a clean login error instead of a character list.
    if let Some(reason) = database
        .active_account_ban(u64::try_from(account.id).unwrap_or(0))
        .map_err(HostError::Persistence)?
    {
        write_frame(
            stream,
            &encode_native_otclient_login_error(&format!("Your account is banned. {reason}")),
        )?;
        record_event(
            database_path,
            "info",
            &format!(
                "native client login rejected banned-account={} peer={peer}",
                account.id
            ),
        );
        return Ok(());
    }
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

/// Bounded fixed-window brute-force guard for native game authentication. Failures are counted
/// per peer IP; once a peer exceeds the window budget every further attempt is rejected before
/// any database work happens until the window elapses.
pub(crate) const NATIVE_AUTH_MAX_FAILURES_PER_WINDOW: u32 = 8;
pub(crate) const NATIVE_AUTH_WINDOW: Duration = Duration::from_secs(60);
/// Bounded fixed-window chat flood control: at most this many routed talk records per window;
/// anything beyond is suppressed before shared delivery.
pub(crate) const CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW: usize = 10;
pub(crate) const CHAT_FLOOD_WINDOW: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct NativeAuthRateLimiter {
    failures: Mutex<BTreeMap<IpAddr, (u32, Instant)>>,
}

impl NativeAuthRateLimiter {
    pub(crate) fn is_blocked(&self, peer: IpAddr) -> bool {
        // A poisoned limiter must not lock every peer out; availability wins here because the
        // database password check still runs for unblocked peers.
        let Ok(failures) = self.failures.lock() else {
            return false;
        };
        match failures.get(&peer) {
            Some((count, window_start)) => {
                *count >= NATIVE_AUTH_MAX_FAILURES_PER_WINDOW
                    && window_start.elapsed() < NATIVE_AUTH_WINDOW
            }
            None => false,
        }
    }

    pub(crate) fn register_failure(&self, peer: IpAddr) {
        // A poisoned mutex must not crash the accept loop; skipping one failure record
        // degrades rate-limiting accuracy but keeps the listener alive.
        let Ok(mut failures) = self.failures.lock() else {
            return;
        };
        let now = Instant::now();
        let entry = failures.entry(peer).or_insert((0, now));
        if entry.1.elapsed() >= NATIVE_AUTH_WINDOW {
            *entry = (0, now);
        }
        entry.0 = entry.0.saturating_add(1);
    }
}
