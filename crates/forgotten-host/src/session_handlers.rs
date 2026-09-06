//! Legacy probe/status/game-session handlers and the framed TCP IO primitives shared
//! by every session path (read_frame/write_frame, probe encode/decode, status metrics,
//! password-session and legacy 7.4 login handling).

use super::*;

pub(crate) fn handle_game_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &GameSessionHostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    // On Windows, accepted sockets inherit the listener's non-blocking mode,
    // which would surface transient WouldBlock reads as session rejections.
    // Restore blocking behavior for the per-connection stream.
    stream.set_nonblocking(false)?;
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

pub(crate) fn send_game_session_error(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    message: &str,
) -> Result<(), HostError> {
    write_game_session_response(stream, key, &encode_legacy_74_game_session_error(message))
}

pub(crate) fn write_game_session_response(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    response: &Frame,
) -> Result<(), HostError> {
    let encrypted = xtea_encrypt_packet(&response.0, key).map_err(HostError::Protocol)?;
    write_frame(stream, &Frame(encrypted))
}

pub(crate) fn handle_status_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &StatusHostConfig,
    database_path: &Path,
    online_players: &AtomicU64,
    started_at: Instant,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    // Windows accepted sockets inherit the listener's non-blocking mode; see
    // handle_game_session for why blocking mode is restored per connection.
    stream.set_nonblocking(false)?;
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
        StatusRequest::Metrics => {
            // One authoritative database read per metrics scrape keeps counters exact without
            // any shared-world locking on the status path.
            let database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
            let (registered_accounts, registered_characters) =
                database.metrics_counts().map_err(HostError::Persistence)?;
            let schema_version = database.schema_version().map_err(HostError::Persistence)?;
            let metrics = forgotten_protocol::StatusMetrics {
                uptime_seconds: snapshot.uptime_seconds,
                registered_accounts,
                registered_characters,
                schema_version,
                players_online: u32::try_from(online_players.load(Ordering::SeqCst))
                    .unwrap_or(u32::MAX),
                players_online_cap: config.max_players,
                process_memory_kib: forgotten_protocol::process_memory_kib(),
            };
            stream.write_all(&encode_status_metrics(&metrics))?;
            stream.flush()?;
        }
    }
    record_event(
        database_path,
        "info",
        &format!("status query accepted peer={peer}"),
    );
    Ok(())
}

pub(crate) fn handle_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &HostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;

    let request = read_frame(stream)?;
    // Single decode: the probe arm and the rejection arm share one parse instead of the
    // previous is_ok()/expect_err double-decode (which had a panic path on the rejection arm).
    match decode_probe(&request) {
        Ok(_) => {
            write_frame(stream, &probe_response(config.profile))?;
            record_event(
                database_path,
                "info",
                &format!("probe accepted peer={peer} profile={}", config.profile.id),
            );
            Ok(())
        }
        Err(error) => {
            if let Some(login) = &config.legacy_login {
                handle_legacy_login(stream, peer, config, login, database_path, &request)
            } else {
                let _ = write_frame(stream, &error_frame(error.code()));
                Err(error)
            }
        }
    }
}

pub(crate) fn handle_legacy_login(
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

pub(crate) fn send_legacy_login_error(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    message: &str,
) -> Result<(), HostError> {
    write_legacy_login_response(stream, key, &encode_login_error(message))
}

pub(crate) fn write_legacy_login_response(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    response: &Frame,
) -> Result<(), HostError> {
    let encrypted = xtea_encrypt_packet(&response.0, key).map_err(HostError::Protocol)?;
    write_frame(stream, &Frame(encrypted))
}

#[allow(dead_code)] // public probe-protocol helper; exercised by socket regressions
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
        // Oversized inbound frames happen legitimately when a stale client connection from a
        // previous session dumps buffered data (e.g. after an ERROR-2 kick). Drain exactly the
        // declared payload so the stream stays framed for the next real request, then report
        // the failure without leaving partial bytes in the socket.
        let mut drained = vec![0_u8; declared.min(1 << 20)];
        let mut remaining = declared;
        while remaining > 0 {
            let chunk = remaining.min(drained.len());
            stream.read_exact(&mut drained[..chunk])?;
            remaining -= chunk;
        }
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

pub(crate) fn decode_probe(frame: &Frame) -> Result<(), HostError> {
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

pub(crate) fn record_event(database_path: &Path, level: &str, message: &str) {
    let _ = EngineDatabase::open(database_path)
        .and_then(|database| database.record_event(level, message));
}
