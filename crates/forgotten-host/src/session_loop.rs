//! Native 740 OTClient game session loop: bootstrap, shared-state drain, and the
//! per-action dispatch for one connected game session.

use super::*;

/// A resolved consumable item location for one UseItem request: the item's server id plus the
/// single owning inventory position it was addressed from (equipment slot or container content).
struct ConsumableSource {
    server_id: u16,
    slot: Option<EquipmentSlot>,
    container_ref: Option<(u8, usize)>,
}

pub(crate) fn handle_native_otclient_game(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &NativeOtClientHostConfig,
    database_path: &Path,
    shared_world: &SharedNativeWorld,
    map_owner: Option<&SharedNativeMap>,
    auth_rate_limiter: &NativeAuthRateLimiter,
) -> Result<(), HostError> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let request = decode_native_otclient_game_request(&read_frame(stream)?, &config.client_profile)
        .map_err(HostError::Protocol)?;
    if auth_rate_limiter.is_blocked(peer.ip()) {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error(
                "Too many failed attempts from your address. Try again later.",
            ),
        )?;
        return Ok(());
    }
    let mut database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    let Some(account) = database
        .authenticate_account_id(request.account_id, &request.password)
        .map_err(HostError::Persistence)?
    else {
        auth_rate_limiter.register_failure(peer.ip());
        write_frame(
            stream,
            &encode_native_otclient_game_login_error("Account name or password is not correct."),
        )?;
        return Ok(());
    };
    let Some(selected_character) = account
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
    let mut character = selected_character.clone();
    let Some(empty_world) = &config.empty_world else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error(
                "Forgotten Engine native map initialization is not enabled for this selected client profile.",
            ),
        )?;
        return Ok(());
    };
    let Some(map_owner) = map_owner else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error(
                "Forgotten Engine native map initialization requires a selected world map.",
            ),
        )?;
        return Ok(());
    };
    let world_map = map_owner.render_snapshot()?;
    if character.respawn_state.dead {
        respawn_persisted_native_player_for_relog(&mut database, character.id, world_map.as_ref())?;
        character = database
            .player_by_id(character.id)
            .map_err(HostError::Persistence)?;
    }
    let account_id = u64::try_from(account.id).map_err(|_| {
        HostError::InvalidConfiguration("native numeric account IDs must be non-negative".into())
    })?;
    let equipment = database
        .player_equipment(character.id)
        .map_err(HostError::Persistence)?;
    let containers = database
        .player_containers(character.id)
        .map_err(HostError::Persistence)?;
    let conditions = database
        .player_conditions(character.id)
        .map_err(HostError::Persistence)?;
    let bootstrap_equipment = equipment.clone();
    let bootstrap_containers = containers.clone();
    let initial_position = match shared_world
        .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
            Player {
                id: character.id,
                account_id,
                name: character.name.clone(),
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
            NativePlayerHydration {
                progression: character.progression,
                progression_attempts: character.progression_attempts,
                town_id: character.town_id,
                respawn_state: character.respawn_state,
                equipment,
                containers,
                conditions,
            },
            world_map.as_ref(),
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
    let mut registration = SharedNativePlayerRegistration {
        world: shared_world.clone(),
        player_id: character.id,
        vip_presence_announced: false,
    };
    // Persisted-party hydration: if this character's stored leader is online, reattach to
    // that live party (or reform it when the online leader has none yet). Failures are
    // non-fatal; the stored row survives until the next bounded snapshot flush prunes it.
    if let Err(error) = try_hydrate_persisted_party(shared_world, &database, character.id) {
        native_diagnostic(
            config.extended_diagnostics,
            peer,
            &format!("party=hydration-failed error={error}"),
        );
    }
    let hydrated_armor_defense = sync_native_equipment_armor_defense(
        shared_world,
        character.id,
        config.item_armor_by_server_id.as_deref(),
        config.item_shield_defense_by_server_id.as_deref(),
        config.armor_multiplier_by_vocation.as_deref(),
        &bootstrap_equipment,
    )?;
    native_diagnostic(
        config.extended_diagnostics,
        peer,
        &format!(
            "combat=equipment-armor-hydration physical-flat-reduction={} changed={hydrated_armor_defense}",
            native_equipment_armor_defense(
                config.item_armor_by_server_id.as_deref(),
                config.item_shield_defense_by_server_id.as_deref(),
                &bootstrap_equipment,
                config
                    .armor_multiplier_by_vocation
                    .as_deref()
                    .and_then(|multipliers| multipliers.get(&character.progression.vocation))
                    .copied()
                    .unwrap_or(DEFAULT_NATIVE_ARMOR_MULTIPLIER_MILLI),
            )
            .physical_flat_reduction
        ),
    );
    let chat_events = shared_world.register_public_chat_recipient(character.id, &character.name)?;
    let persisted_vip_entries = database
        .account_vip_entries(request.account_id)
        .map_err(HostError::Persistence)?;
    let watched_vip_player_ids = persisted_vip_entries
        .iter()
        .take(MAX_NATIVE_OTCLIENT_LOGIN_VIP_ENTRIES)
        .filter_map(|entry| u32::try_from(entry.target_player_id).ok())
        .filter(|target_player_id| *target_player_id != 0)
        .collect();
    let vip_presence_events =
        shared_world.register_vip_presence_recipient(character.id, watched_vip_player_ids)?;
    if initial_position != character.position {
        database.update_player_position(character.id, initial_position)?;
    }
    let mut player_outfit = native_hydrated_classic_outfit(
        empty_world.player_look_type,
        empty_world.outfit_first_look_type,
        empty_world.outfit_last_look_type,
        character.outfit,
    );
    shared_world.update_player_outfit(character.id, player_outfit)?;
    // Characters created before starter provisioning get their backpack on first login so
    // operator /give and loot pickup always have a destination.
    database
        .provision_starter_backpack(character.id)
        .map_err(HostError::Persistence)?;
    // Restore the persisted cardinal rotation so relog keeps the character facing the same
    // way it was left; south remains the fallback for legacy rows.
    let persisted_facing = NativeOtClientCardinalDirection::from_protocol_direction(
        database
            .player_facing(character.id)
            .map_err(HostError::Persistence)?,
    )
    .unwrap_or(NativeOtClientCardinalDirection::South);
    shared_world.update_player_facing(character.id, persisted_facing)?;
    let player_id = native_player_id(character.id)?;
    let (authoritative_player, authoritative_vitals) =
        shared_world.player_and_vitals(character.id)?;
    let mut snapshot = NativeOtClientEmptyWorldSnapshot {
        player_id,
        player_name: character.name.clone(),
        player_position: native_position(initial_position),
        player_level: 1,
        player_experience: 0,
        player_vitals: NativeOtClientPlayerVitals::default(),
        player_skills: character.progression.skills,
        ground_thing_id: empty_world.ground_thing_id,
        player_look_type: player_outfit.look_type,
        player_direction: persisted_facing.protocol_direction(),
        player_speed: empty_world.player_speed,
        server_beat: empty_world.server_beat,
    };
    refresh_native_player_stats_snapshot(
        &mut snapshot,
        &authoritative_player,
        authoritative_vitals,
    );
    let active_static_spawns = shared_world.active_static_spawns()?;
    let visible_players = shared_world.visible_players(
        character.id,
        empty_world.player_look_type,
        empty_world.player_speed,
    )?;
    let mut initialization =
        encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players(
            &config.client_profile,
            &snapshot,
            world_map.as_ref(),
            Some(&active_static_spawns),
            Some(&visible_players),
        )
        .map_err(HostError::Protocol)?;
    let fight_mode_state = shared_world.player_fight_mode_state(character.id)?;
    let mode = match fight_mode_state.mode {
        PlayerFightMode::Attack => NativeOtClientFightMode::Attack,
        PlayerFightMode::Balanced => NativeOtClientFightMode::Balanced,
        PlayerFightMode::Defense => NativeOtClientFightMode::Defense,
    };
    initialization.0.extend_from_slice(
        &encode_native_otclient_player_modes(
            &config.client_profile,
            NativeOtClientFightModeRequest {
                mode,
                chase: fight_mode_state.chase,
                secure: fight_mode_state.secure,
            },
        )
        .map_err(HostError::Protocol)?
        .0,
    );
    let equipment_frames = native_classic_equipment_frames(
        &config.client_profile,
        config.item_presentation_catalog.as_deref(),
        &bootstrap_equipment,
    )
    .map_err(HostError::Protocol)?;
    let mut observed_mapped_equipment = native_classic_mapped_equipment(
        config.item_presentation_catalog.as_deref(),
        &bootstrap_equipment,
    );
    let container_frames = native_classic_container_frames(
        &config.client_profile,
        config.item_presentation_catalog.as_deref(),
        &bootstrap_containers,
        &BTreeSet::new(),
    )
    .map_err(HostError::Protocol)?;
    let mut sent_container_windows = native_rendered_container_windows(
        &config.client_profile,
        config.item_presentation_catalog.as_deref(),
        &bootstrap_containers,
        &BTreeSet::new(),
    );
    // Slice 11: the white-skull award record is emitted once per session when the attacker's
    // first unjustified kill flips their skull state.
    let mut observed_white_skull_sent = false;
    // Slice 13: hydrated condition icons are read now but written after the bootstrap so the
    // login-state frame stays first on the wire.
    let mut observed_state_bits = shared_world
        .lock()
        .map(|world| {
            native_condition_state_bits(
                world
                    .player_conditions(character.id)
                    .unwrap_or(&BTreeMap::new()),
            )
        })
        .unwrap_or(0);
    let static_health_frames =
        native_static_creature_health_frames(&config.client_profile, &active_static_spawns)?;
    // Establish all shared-state baselines before any initialization frame becomes observable by
    // the peer. A concurrent mutation during bootstrap must remain pending for the first session
    // loop rather than being silently absorbed by a post-write baseline read.
    let mut observed_visibility_epoch = shared_world.visibility_epoch();
    let mut observed_vitals_epoch = shared_world.vitals_epoch();
    let mut observed_progression_epoch = shared_world.progression_epoch();
    let mut observed_equipment_epoch = shared_world.equipment_epoch();
    let mut observed_containers_epoch = shared_world.containers_epoch();
    let mut observed_party_epoch = shared_world.party_epoch();
    let mut observed_dead = shared_world.player_respawn_state(character.id)?.dead;
    write_frame(stream, &initialization)?;
    if character.outfit.look_type == player_outfit.look_type && player_outfit.look_type != 0 {
        let hydrated_outfit = encode_native_otclient_creature_outfit(
            &config.client_profile,
            snapshot.player_id,
            player_outfit,
        )
        .map_err(HostError::Protocol)?;
        write_frame(stream, &hydrated_outfit)?;
        native_diagnostic(
            config.extended_diagnostics,
            peer,
            &format!(
                "outbound=hydrated-creature-outfit opcode=0x8e bytes={} look-type={}",
                hydrated_outfit.0.len(),
                player_outfit.look_type
            ),
        );
    }
    for frame in &equipment_frames {
        write_frame(stream, frame)?;
    }
    for frame in &container_frames {
        write_frame(stream, frame)?;
    }
    for frame in &static_health_frames {
        write_frame(stream, frame)?;
    }
    // Slice 13: hydrated condition icons arrive after the bootstrap so the login-state record
    // stays first on the wire; zero bits match the bootstrap default and skip the write.
    if observed_state_bits != 0 {
        let state_frame =
            encode_native_otclient_player_state_bits(&config.client_profile, observed_state_bits)
                .map_err(HostError::Protocol)?;
        write_frame(stream, &state_frame)?;
    }
    // Slice 19: guild members receive their guild channel context and message-of-the-day as a
    // login-time status record, matching classic server behavior.
    if let Some((channel, motd)) = native_guild_channel_context(&database, character.id) {
        if !motd.trim().is_empty() {
            let motd_frame = encode_native_otclient_status_message(
                &config.client_profile,
                &format!("Message of the day: {motd}"),
            )
            .map_err(HostError::Protocol)?;
            write_frame(stream, &motd_frame)?;
        }
        native_diagnostic(
            config.extended_diagnostics,
            peer,
            &format!(
                "login=guild-context channel-id={} name={} motd-bytes={}",
                channel.id,
                channel.name,
                motd.len()
            ),
        );
    }
    let mut delivered_vip_entries = 0usize;
    let mut skipped_vip_entries = 0usize;
    for entry in persisted_vip_entries
        .iter()
        .take(MAX_NATIVE_OTCLIENT_LOGIN_VIP_ENTRIES)
    {
        let Ok(target_player_id) = u32::try_from(entry.target_player_id) else {
            skipped_vip_entries = skipped_vip_entries.saturating_add(1);
            continue;
        };
        if target_player_id == 0 {
            skipped_vip_entries = skipped_vip_entries.saturating_add(1);
            continue;
        }
        let frame = encode_native_otclient_classic_vip_entry(
            &config.client_profile,
            target_player_id,
            &entry.target_player_name,
            shared_world
                .lock()?
                .player(u64::from(target_player_id))
                .is_some(),
        )
        .map_err(HostError::Protocol)?;
        write_frame(stream, &frame)?;
        delivered_vip_entries = delivered_vip_entries.saturating_add(1);
    }
    skipped_vip_entries = skipped_vip_entries.saturating_add(
        persisted_vip_entries
            .len()
            .saturating_sub(MAX_NATIVE_OTCLIENT_LOGIN_VIP_ENTRIES),
    );
    let delivered_vip_presence_updates = shared_world.publish_vip_presence(character.id, true)?;
    registration.vip_presence_announced = true;
    stream.set_read_timeout(Some(NATIVE_OTCLIENT_HEARTBEAT_INTERVAL))?;
    if config.extended_diagnostics {
        eprintln!(
            "> Native OTCv8 map init sent peer={peer} player={} record-bytes={} equipment-records={}/{} skipped-unmapped={} container-records={}/{} skipped-unmapped-or-nested={} static-health-records={} vip-records={} skipped-vip-records={} vip-presence-updates={} map={} tiles={} static-spawns={} login-state-opcode=0x0a map-opcode=0x64 asset-free={}",
            character.name,
            initialization.0.len(),
            equipment_frames.len(),
            bootstrap_equipment.len(),
            bootstrap_equipment.len().saturating_sub(equipment_frames.len()),
            container_frames.len(),
            bootstrap_containers.len(),
            bootstrap_containers.len().saturating_sub(container_frames.len()),
            static_health_frames.len(),
            delivered_vip_entries,
            skipped_vip_entries,
            delivered_vip_presence_updates,
            world_map.identifier(),
            world_map.tile_count(),
            active_static_spawns.entities.len(),
            snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0,
        );
    }

    let mut player_position = initial_position;
    let mut facing = NativeOtClientCardinalDirection::South;
    let mut active_click_walk: Option<NativeActiveClickWalk> = None;
    let mut last_regeneration_tick = Instant::now();
    let mut last_condition_tick = Instant::now();
    let mut closed_container_ids = BTreeSet::new();
    let mut open_corpse_windows: BTreeMap<u8, (Position, usize)> = BTreeMap::new();
    // Ephemeral windows presenting depth-one nested content: window id -> (parent
    // container id, parent item index).
    let mut open_content_windows: BTreeMap<u8, (u8, usize)> = BTreeMap::new();
    let mut open_public_channel_ids = BTreeSet::new();
    let mut talk_windows: VecDeque<Instant> = VecDeque::new();
    loop {
        drain_shared_vip_presence(
            stream,
            &config.client_profile,
            &vip_presence_events,
            config.extended_diagnostics,
            peer,
        )?;
        drain_shared_public_chat(
            stream,
            &config.client_profile,
            &chat_events,
            &open_public_channel_ids,
            config.extended_diagnostics,
            peer,
        )?;
        refresh_native_party_shields(
            stream,
            shared_world,
            &config.client_profile,
            character.id,
            &mut observed_party_epoch,
            config.extended_diagnostics,
            peer,
        )?;
        if observe_native_death_transition(shared_world, character.id, &mut observed_dead)? {
            let death = encode_native_otclient_game_death(&config.client_profile)
                .map_err(HostError::Protocol)?;
            write_frame(stream, &death)?;
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                "lifecycle=death-notification profile=740 fields=none source=shared-world-transition",
            );
        }
        // Operator kicks: an entry in pending_kicks ends this session at the next loop pass.
        // Returning drops the SharedNativePlayerRegistration guard, which unregisters the
        // chat/VIP recipients and removes the player from the shared world before the socket
        // closes, so the disconnect is authoritative and the vitals/position stay persisted.
        if shared_world.take_pending_kick(character.id)? {
            let rejection = encode_native_otclient_failure_message(
                &config.client_profile,
                "You were disconnected by a gamemaster.",
            )
            .map_err(HostError::Protocol)?;
            let _ = write_frame(stream, &rejection);
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                "lifecycle=kicked-by-operator",
            );
            break;
        }
        // Trade window lifecycle: close this side's windows when a swap completed, the
        // counterparty rejected, or any other path signalled the trade ended for this player.
        if shared_world.take_pending_trade_close(character.id)? {
            let close = encode_native_otclient_close_trade(&config.client_profile)
                .map_err(HostError::Protocol)?;
            write_frame(stream, &close)?;
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                "trade=window-closed signal=shared-world",
            );
        }
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
                    drain_shared_vip_presence(
                        stream,
                        &config.client_profile,
                        &vip_presence_events,
                        config.extended_diagnostics,
                        peer,
                    )?;
                    drain_shared_public_chat(
                        stream,
                        &config.client_profile,
                        &chat_events,
                        &open_public_channel_ids,
                        config.extended_diagnostics,
                        peer,
                    )?;
                    refresh_native_party_shields(
                        stream,
                        shared_world,
                        &config.client_profile,
                        character.id,
                        &mut observed_party_epoch,
                        config.extended_diagnostics,
                        peer,
                    )?;
                    let now = Instant::now();
                    let condition_elapsed_seconds =
                        now.saturating_duration_since(last_condition_tick)
                            .as_secs()
                            .min(u64::from(u16::MAX)) as u16;
                    if condition_elapsed_seconds > 0 {
                        last_condition_tick +=
                            Duration::from_secs(u64::from(condition_elapsed_seconds));
                        let (outcome, mut vitals, death_state) = match shared_world
                            .apply_player_conditions_with_death(
                                character.id,
                                world_map.as_ref(),
                                condition_elapsed_seconds,
                            ) {
                            Ok(result) => result,
                            Err(HostError::Core(
                                forgotten_core::CoreError::PlayerTownUnassigned(_)
                                | forgotten_core::CoreError::UnknownTown(_),
                            )) => {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "lifecycle=conditions outcome=paused-invalid-death-temple",
                                );
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        persist_runtime_player_conditions(
                            &mut database,
                            shared_world,
                            character.id,
                        )?;
                        // Slice 13: condition icons refresh whenever the active bit set
                        // changes (new poison, expiry, etc.). Expired kinds drop out of
                        // the authoritative map, so the diff catches both directions.
                        let current_state_bits = shared_world
                            .lock()
                            .map(|world| {
                                native_condition_state_bits(
                                    world
                                        .player_conditions(character.id)
                                        .unwrap_or(&BTreeMap::new()),
                                )
                            })
                            .unwrap_or(observed_state_bits);
                        if current_state_bits != observed_state_bits {
                            let state_frame = encode_native_otclient_player_state_bits(
                                &config.client_profile,
                                current_state_bits,
                            )
                            .map_err(HostError::Protocol)?;
                            write_frame(stream, &state_frame)?;
                            observed_state_bits = current_state_bits;
                        }
                        if outcome.applied_damage > 0 {
                            let died = death_state.is_some();
                            let loss_persisted = if died {
                                apply_configured_native_death_loss(
                                    &mut database,
                                    shared_world,
                                    character.id,
                                    config.death_loss_policy,
                                    config.progression_rules.as_deref(),
                                )?
                            } else {
                                false
                            };
                            if loss_persisted {
                                vitals = shared_world.player_vitals(character.id)?;
                            }
                            let persisted_vitals = PersistedPlayerVitals {
                                health: vitals.health,
                                max_health: vitals.max_health,
                                mana: vitals.mana,
                                max_mana: vitals.max_mana,
                                capacity: vitals.capacity,
                                magic_level: vitals.magic_level,
                            };
                            if loss_persisted {
                                // The complete post-loss snapshot was already committed atomically.
                            } else if let Some(death_state) = death_state {
                                database.update_player_vitals_and_respawn_state(
                                    character.id,
                                    persisted_vitals,
                                    death_state,
                                )?;
                            } else {
                                database.update_player_vitals(character.id, persisted_vitals)?;
                            }
                            if died {
                                let death =
                                    encode_native_otclient_game_death(&config.client_profile)
                                        .map_err(HostError::Protocol)?;
                                write_frame(stream, &death)?;
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "lifecycle=death-notification profile=740 fields=none",
                                );
                                observed_dead = true;
                            }
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "lifecycle=conditions damage={} health={} expired={} dead={}",
                                    outcome.applied_damage,
                                    outcome.remaining_health,
                                    outcome.expired_conditions,
                                    death_state.is_some(),
                                ),
                            );
                        }
                    }
                    if let Some(rules_by_vocation) = config.regeneration_rules.as_deref() {
                        let elapsed_seconds =
                            now.saturating_duration_since(last_regeneration_tick)
                                .as_secs()
                                .min(u64::from(u16::MAX)) as u16;
                        if elapsed_seconds > 0 {
                            last_regeneration_tick +=
                                Duration::from_secs(u64::from(elapsed_seconds));
                            let vocation = shared_world.player_progression(character.id)?.vocation;
                            if let Some(rules) = rules_by_vocation.get(&vocation).copied() {
                                let outcome = shared_world.apply_player_regeneration(
                                    character.id,
                                    rules,
                                    elapsed_seconds,
                                )?;
                                if outcome.health_gained > 0 || outcome.mana_gained > 0 {
                                    database.update_player_vitals(
                                        character.id,
                                        PersistedPlayerVitals {
                                            health: outcome.vitals.health,
                                            max_health: outcome.vitals.max_health,
                                            mana: outcome.vitals.mana,
                                            max_mana: outcome.vitals.max_mana,
                                            capacity: outcome.vitals.capacity,
                                            magic_level: outcome.vitals.magic_level,
                                        },
                                    )?;
                                    native_diagnostic(
                                        config.extended_diagnostics,
                                        peer,
                                        &format!(
                                            "lifecycle=regeneration vocation={} health-gained={} mana-gained={}",
                                            vocation.value(),
                                            outcome.health_gained,
                                            outcome.mana_gained
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    if let Some((target_native_id, target_vitals, outcome)) =
                        apply_native_selected_player_melee_for_world_type(
                            &mut database,
                            shared_world,
                            character.id,
                            world_map.as_ref(),
                            config.world_type,
                            NativeSelectedPlayerMeleePolicy {
                                progression_rules: config.progression_rules.as_deref(),
                                skill_rate: config.skill_rate,
                                death_loss_policy: config.death_loss_policy,
                                armor_by_server_id: config.item_armor_by_server_id.as_deref(),
                                shield_defense_by_server_id: config
                                    .item_shield_defense_by_server_id
                                    .as_deref(),
                                armor_multiplier_by_vocation: config
                                    .armor_multiplier_by_vocation
                                    .as_deref(),
                                declarative_weapon_catalog: config
                                    .declarative_weapon_catalog
                                    .as_deref(),
                            },
                        )?
                    {
                        let health_update = encode_native_otclient_creature_health(
                            &config.client_profile,
                            target_native_id,
                            target_vitals.health,
                            target_vitals.max_health,
                        )
                        .map_err(HostError::Protocol)?;
                        write_frame(stream, &health_update)?;
                        if outcome.defeated {
                            for frame in native_selected_player_death_target_frames(
                                &config.client_profile,
                                true,
                            )
                            .map_err(HostError::Protocol)?
                            {
                                write_frame(stream, &frame)?;
                            }
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "outbound=clear-target opcode=0xa3 fields=none reason=selected-player-death",
                            );
                            let death = encode_native_otclient_game_death(&config.client_profile)
                                .map_err(HostError::Protocol)?;
                            write_frame(stream, &death)?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "lifecycle=death-notification profile=740 fields=none",
                            );
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "combat=selected-player-melee target={} damage={} health={}/{}",
                                outcome.target_id,
                                outcome.applied_damage,
                                target_vitals.health,
                                target_vitals.max_health
                            ),
                        );
                    }
                    if let Some(outcome) = apply_native_selected_static_creature_melee(
                        shared_world,
                        character.id,
                        world_map.as_ref(),
                    )? {
                        if outcome.applied_damage > 0 {
                            let _ = shared_world
                                .record_party_shared_experience_activity(character.id)?;
                        }
                        persist_static_creature_runtime_to_open_database(
                            shared_world,
                            &mut database,
                        )?;
                        if outcome.deactivated {
                            // Slice 11: tell this client the defeated creature no longer blocks
                            // its tile before the corpse appears.
                            let unpass = encode_native_otclient_creature_unpass(
                                &config.client_profile,
                                outcome.target_id,
                                false,
                            )
                            .map_err(HostError::Protocol)?;
                            write_frame(stream, &unpass)?;
                            let creature_name = shared_world.lock().ok().and_then(|world| {
                                world
                                    .static_creature(outcome.target_id)
                                    .map(|creature| creature.name.clone())
                            });
                            let corpse_server_id = native_declared_corpse_server_id(
                                config.corpse_server_id_by_creature_name.as_deref(),
                                creature_name,
                            );
                            let loot_split_targets = if config.party_loot_split_enabled {
                                shared_world.party_loot_split_targets(character.id)?
                            } else {
                                Vec::new()
                            };
                            if let Some(corpse_position) =
                                spawn_native_static_defeat_corpse(NativeDefeatCorpseRequest {
                                    shared_world,
                                    map_owner,
                                    database: &mut database,
                                    creature_id: outcome.target_id,
                                    seed: shared_world.tick()?,
                                    corpse_server_id,
                                    corpse_despawn_seconds: config.corpse_despawn_seconds,
                                    loot_split_targets: &loot_split_targets,
                                })?
                            {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "loot=corpse-spawned creature={} position={},{},{}",
                                        outcome.target_id,
                                        corpse_position.x,
                                        corpse_position.y,
                                        corpse_position.z
                                    ),
                                );
                            }
                            apply_and_persist_native_static_defeat_experience(
                                &mut database,
                                shared_world,
                                character.id,
                                outcome.target_id,
                                config.experience_award_policy.as_deref(),
                                config.vocation_level_up_gains.as_deref(),
                                config.party_shared_experience_rules,
                            )?;
                            for frame in native_static_target_deactivation_frames(
                                &config.client_profile,
                                outcome.deactivated,
                            )
                            .map_err(HostError::Protocol)?
                            {
                                write_frame(stream, &frame)?;
                            }
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "outbound=clear-target opcode=0xa3 fields=none reason=static-target-deactivated",
                            );
                        }
                        let health_update = encode_native_otclient_creature_health(
                            &config.client_profile,
                            outcome.target_id,
                            u16::from(outcome.remaining_health_percent),
                            100,
                        )
                        .map_err(HostError::Protocol)?;
                        write_frame(stream, &health_update)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "combat=selected-static-melee target={} damage={} health-percent={} deactivated={} delivery=creature-health persistence=static-runtime",
                                outcome.target_id,
                                outcome.applied_damage,
                                outcome.remaining_health_percent,
                                outcome.deactivated
                            ),
                        );
                    }
                    let vitals_epoch = shared_world.vitals_epoch();
                    if vitals_epoch != observed_vitals_epoch {
                        let (player, vitals) = shared_world.player_and_vitals(character.id)?;
                        refresh_native_player_stats_snapshot(&mut snapshot, &player, vitals);
                        let stats_update =
                            encode_native_otclient_player_stats(&config.client_profile, &snapshot)
                                .map_err(HostError::Protocol)?;
                        write_frame(stream, &stats_update)?;
                        let mut refreshed_snapshot = snapshot.clone();
                        refreshed_snapshot.player_position = native_position(player_position);
                        refreshed_snapshot.player_direction = facing.protocol_direction();
                        let refreshed_viewport = encode_shared_native_world_viewport(
                            &config.client_profile,
                            &refreshed_snapshot,
                            world_map.as_ref(),
                            shared_world,
                            character.id,
                        )?;
                        write_frame(stream, &refreshed_viewport)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=player-stats-refresh epoch={vitals_epoch} bytes={} viewport-bytes={}",
                                stats_update.0.len(),
                                refreshed_viewport.0.len()
                            ),
                        );
                        observed_vitals_epoch = vitals_epoch;
                    }
                    let progression_epoch = shared_world.progression_epoch();
                    if progression_epoch != observed_progression_epoch {
                        snapshot.player_skills =
                            shared_world.player_progression(character.id)?.skills;
                        let skills_update =
                            encode_native_otclient_player_skills(&config.client_profile, &snapshot)
                                .map_err(HostError::Protocol)?;
                        write_frame(stream, &skills_update)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=player-skills-refresh epoch={progression_epoch} bytes={}",
                                skills_update.0.len()
                            ),
                        );
                        observed_progression_epoch = progression_epoch;
                    }
                    let equipment_epoch = shared_world.equipment_epoch();
                    if equipment_epoch != observed_equipment_epoch {
                        let equipment = shared_world.player_equipment(character.id)?;
                        let current_mapped_equipment = native_classic_mapped_equipment(
                            config.item_presentation_catalog.as_deref(),
                            &equipment,
                        );
                        let equipment_updates = native_classic_equipment_delta_frames(
                            &config.client_profile,
                            &observed_mapped_equipment,
                            &current_mapped_equipment,
                        )
                        .map_err(HostError::Protocol)?;
                        for frame in &equipment_updates {
                            write_frame(stream, frame)?;
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=equipment-refresh epoch={equipment_epoch} records={}",
                                equipment_updates.len()
                            ),
                        );
                        observed_mapped_equipment = current_mapped_equipment;
                        observed_equipment_epoch = equipment_epoch;
                    }
                    let containers_epoch = shared_world.containers_epoch();
                    if containers_epoch != observed_containers_epoch {
                        let containers = shared_world.player_containers(character.id)?;
                        // Bounded slot deltas replace the blanket full refresh: only windows
                        // whose client-visible rendering changed emit records, and windows
                        // that cannot be expressed as deltas fall back to one exact
                        // OpenContainer resend.
                        let container_updates = native_container_delta_frames(
                            &config.client_profile,
                            config.item_presentation_catalog.as_deref(),
                            &containers,
                            &closed_container_ids,
                            &mut sent_container_windows,
                        )
                        .map_err(HostError::Protocol)?;
                        for frame in &container_updates {
                            write_frame(stream, frame)?;
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=container-refresh epoch={containers_epoch} records={}",
                                container_updates.len()
                            ),
                        );
                        observed_containers_epoch = containers_epoch;
                    }
                    let visibility_epoch = shared_world.visibility_epoch();
                    if visibility_epoch != observed_visibility_epoch {
                        let mut refreshed_snapshot = snapshot.clone();
                        refreshed_snapshot.player_position = native_position(player_position);
                        refreshed_snapshot.player_direction = facing.protocol_direction();
                        let refreshed_viewport = encode_shared_native_world_viewport(
                            &config.client_profile,
                            &refreshed_snapshot,
                            world_map.as_ref(),
                            shared_world,
                            character.id,
                        )?;
                        let refreshed_static_spawns = shared_world.active_static_spawns()?;
                        let refreshed_static_health_frames = native_static_creature_health_frames(
                            &config.client_profile,
                            &refreshed_static_spawns,
                        )?;
                        write_frame(stream, &refreshed_viewport)?;
                        for frame in &refreshed_static_health_frames {
                            write_frame(stream, frame)?;
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=viewport-refresh reason=visibility-epoch epoch={visibility_epoch} bytes={} static-health-records={}",
                                refreshed_viewport.0.len(),
                                refreshed_static_health_frames.len()
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
                            world_map.as_ref(),
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
                                let equipment = shared_world.player_equipment(character.id)?;
                                let effective_speed = native_hasted_speed(
                                    native_effective_player_speed(
                                        snapshot.player_speed,
                                        &equipment,
                                        config.item_speed_bonus_by_server_id.as_deref(),
                                    ),
                                    shared_world.player_speed_bonus_percent(character.id),
                                );
                                task.next_step_deadline = Instant::now()
                                    + native_autowalk_step_delay(
                                        effective_speed,
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
            NativeOtClientGameAction::PingBack | NativeOtClientGameAction::EnterGame => {}
            NativeOtClientGameAction::AddVip(target_player_name) => {
                let entry = match database.add_account_vip_entry(
                    request.account_id,
                    &target_player_name,
                    "",
                    0,
                    false,
                ) {
                    Ok(entry) => entry,
                    Err(_) => {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=vip-add outcome=rejected",
                        );
                        continue;
                    }
                };
                let target_player_id = match u32::try_from(entry.target_player_id) {
                    Ok(target_player_id) if target_player_id != 0 => target_player_id,
                    _ => {
                        let _ = database
                            .remove_account_vip_entry(request.account_id, entry.target_player_id);
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=vip-add outcome=deferred-target-id-out-of-classic-range",
                        );
                        continue;
                    }
                };
                write_frame(
                    stream,
                    &encode_native_otclient_classic_vip_entry(
                        &config.client_profile,
                        target_player_id,
                        &entry.target_player_name,
                        false,
                    )
                    .map_err(HostError::Protocol)?,
                )?;
            }
            NativeOtClientGameAction::RemoveVip(target_player_id) => {
                if database
                    .remove_account_vip_entry(request.account_id, u64::from(target_player_id))
                    .is_err()
                {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=vip-remove outcome=rejected",
                    );
                }
            }
            NativeOtClientGameAction::EditVip {
                target_player_id,
                description,
                icon,
                notify,
            } => {
                if database
                    .edit_account_vip_entry(
                        request.account_id,
                        u64::from(target_player_id),
                        &description,
                        icon,
                        notify,
                    )
                    .is_err()
                {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=vip-edit outcome=rejected",
                    );
                }
            }
            NativeOtClientGameAction::ThrowItem {
                source_position,
                source_client_thing_id,
                source_stack_position,
                target_position,
                count,
            } => {
                let source_slot = (source_position.x == 0xffff
                    && source_position.y & 0x40 == 0
                    && source_position.z == 0
                    && source_stack_position == 0)
                    .then(|| EquipmentSlot::from_code(source_position.y as u8))
                    .flatten();
                let source_container =
                    (source_position.x == 0xffff && source_position.y & 0x40 != 0).then_some((
                        (source_position.y & 0x0f) as u8,
                        usize::from(source_position.z),
                    ));
                let target_slot = (target_position.x == 0xffff
                    && target_position.y & 0x40 == 0
                    && target_position.z == 0)
                    .then(|| EquipmentSlot::from_code(target_position.y as u8))
                    .flatten();
                // Classic clients address open container windows with the high container flag
                // in y and the window identifier in its lower four bits. For a whole item the
                // destination index remains a client-side drop location and FE appends to the
                // already-owned top-level container. A requested partial stack is narrower: it
                // must name an existing matching top-level container item at that exact index.
                let target_container_id = (target_position.x == 0xffff
                    && target_position.y & 0x40 != 0)
                    .then_some((target_position.y & 0x0f) as u8);
                let Some(catalog) = config.item_presentation_catalog.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=throw-item outcome=deferred-no-item-presentation-catalog",
                    );
                    continue;
                };
                if count == 0 {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=throw-item outcome=deferred-zero-count",
                    );
                    continue;
                }
                // Owned-inventory drops onto a real ground tile route through the durable
                // runtime registry. Map-source and unknown sources stay deferred.
                if target_position.x != 0xffff {
                    let target_tile = Position {
                        x: target_position.x,
                        y: target_position.y,
                        z: target_position.z,
                    };
                    let drop_source = if let Some(slot) = source_slot {
                        Some(forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot))
                    } else if let Some((container_id, item_index)) = source_container {
                        if closed_container_ids.contains(&container_id) {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-closed-container-ground-drop",
                            );
                            continue;
                        }
                        // Nested content window: translate the ephemeral window address back
                        // to its parent container item and content index.
                        if let Some(&(parent_container_id, parent_item_index)) =
                            open_content_windows.get(&container_id)
                        {
                            Some(forgotten_core::PlayerGroundDropSource::ContainerContent {
                                container_id: parent_container_id,
                                item_index: parent_item_index,
                                content_index: item_index,
                            })
                        } else {
                            Some(forgotten_core::PlayerGroundDropSource::ContainerItem {
                                container_id,
                                item_index,
                            })
                        }
                    } else {
                        None
                    };
                    let Some(drop_source) = drop_source.filter(|_| !observed_dead) else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-unsupported-ground-drop-source",
                        );
                        continue;
                    };
                    // Validate the requested stack identity before any authoritative mutation.
                    let identity_ok = match &drop_source {
                        forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot) => shared_world
                            .player_equipment(character.id)
                            .ok()
                            .and_then(|equipment| equipment.item(*slot).cloned())
                            .is_some_and(|item| {
                                native_classic_item_record(Some(catalog), &item).is_some_and(
                                    |record| record.client_thing_id == source_client_thing_id,
                                )
                            }),
                        forgotten_core::PlayerGroundDropSource::ContainerItem {
                            container_id,
                            item_index,
                        } => shared_world
                            .player_containers(character.id)
                            .ok()
                            .and_then(|containers| containers.container(*container_id).cloned())
                            .and_then(|container| container.items.item(*item_index).cloned())
                            .is_some_and(|item| {
                                native_classic_item_record(Some(catalog), &item).is_some_and(
                                    |record| record.client_thing_id == source_client_thing_id,
                                )
                            }),
                        forgotten_core::PlayerGroundDropSource::ContainerContent {
                            container_id,
                            item_index,
                            content_index,
                        } => shared_world
                            .player_containers(character.id)
                            .ok()
                            .and_then(|containers| containers.container(*container_id).cloned())
                            .and_then(|container| container.items.item(*item_index).cloned())
                            .and_then(|item| item.contents().get(*content_index).cloned())
                            .is_some_and(|item| {
                                native_classic_item_record(Some(catalog), &item).is_some_and(
                                    |record| record.client_thing_id == source_client_thing_id,
                                )
                            }),
                    };
                    if !identity_ok {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-ground-drop-identity-mismatch",
                        );
                        continue;
                    }
                    match map_owner.move_player_stack_to_ground(
                        shared_world,
                        &mut database,
                        character.id,
                        drop_source,
                        target_tile,
                        u16::from(count),
                        config.item_weight_by_server_id.as_deref(),
                    ) {
                        Ok(Some(outcome)) => {
                            if matches!(
                                outcome.source,
                                forgotten_core::PlayerGroundDropSource::ContainerContent { .. }
                            ) {
                                native_refresh_open_content_windows(
                                    stream,
                                    &config.client_profile,
                                    config.item_presentation_catalog.as_deref(),
                                    &shared_world.player_containers(character.id)?,
                                    &mut open_content_windows,
                                )?;
                            }
                            if let forgotten_core::PlayerGroundDropSource::EquipmentSlot(_) =
                                outcome.source
                            {
                                let equipment = shared_world.player_equipment(character.id)?;
                                let current_mapped_equipment =
                                    native_classic_mapped_equipment(Some(catalog), &equipment);
                                let equipment_updates = native_classic_equipment_delta_frames(
                                    &config.client_profile,
                                    &observed_mapped_equipment,
                                    &current_mapped_equipment,
                                )
                                .map_err(HostError::Protocol)?;
                                for frame in &equipment_updates {
                                    write_frame(stream, frame)?;
                                }
                                observed_mapped_equipment = current_mapped_equipment;
                                observed_equipment_epoch = shared_world.equipment_epoch();
                            }
                            if let forgotten_core::PlayerGroundDropSource::ContainerItem {
                                container_id,
                                ..
                            } = outcome.source
                            {
                                if !closed_container_ids.contains(&container_id) {
                                    let containers =
                                        shared_world.player_containers(character.id)?;
                                    if let Some(container) = containers.container(container_id) {
                                        if let Some(frame) = native_classic_container_frame(
                                            &config.client_profile,
                                            Some(catalog),
                                            container,
                                        )
                                        .map_err(HostError::Protocol)?
                                        {
                                            write_frame(stream, &frame)?;
                                        }
                                        sent_container_windows.insert(
                                            container_id,
                                            native_rendered_container_window(
                                                &config.client_profile,
                                                Some(catalog),
                                                container,
                                            ),
                                        );
                                    }
                                }
                                observed_containers_epoch = shared_world.containers_epoch();
                            }
                            let mut refreshed_snapshot = snapshot.clone();
                            refreshed_snapshot.player_position = native_position(player_position);
                            refreshed_snapshot.player_direction = facing.protocol_direction();
                            let map_snapshot = map_owner.render_snapshot()?;
                            let refreshed_viewport = encode_shared_native_world_viewport(
                                &config.client_profile,
                                &refreshed_snapshot,
                                map_snapshot.as_ref(),
                                shared_world,
                                character.id,
                            )?;
                            write_frame(stream, &refreshed_viewport)?;
                            observed_visibility_epoch = shared_world.visibility_epoch();
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=throw-item outcome=inventory-to-ground target={},{},{} server-id={} count={} moved={} remaining={:?} map-revision={}",
                                    target_tile.x,
                                    target_tile.y,
                                    target_tile.z,
                                    outcome.moved_item.server_id,
                                    source_client_thing_id,
                                    outcome.moved_item.count,
                                    outcome.source_remaining_count,
                                    map_owner.revision(),
                                ),
                            );
                        }
                        Ok(None) => native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-ground-drop-rejected",
                        ),
                        Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-ground-drop-failed",
                            );
                        }
                        Err(error) => return Err(error),
                    }
                    continue;
                }
                if source_position.x != 0xffff {
                    let core_source_position = Position {
                        x: source_position.x,
                        y: source_position.y,
                        z: source_position.z,
                    };
                    // Runtime ground items (dropped stacks) are picked up from the durable
                    // registry; imported source items keep their own transfer paths below.
                    let runtime_pickup = map_owner.runtime_tile_item(
                        core_source_position,
                        usize::from(source_stack_position),
                    )?;
                    if let Some(runtime_item) = runtime_pickup.filter(|_| !observed_dead) {
                        let identity_ok = source_client_thing_id == runtime_item.server_id
                            || catalog.presentation(runtime_item.server_id).is_some_and(
                                |presentation| {
                                    presentation.client_thing_id == source_client_thing_id
                                },
                            );
                        if !identity_ok {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-runtime-pickup-identity-mismatch",
                            );
                            continue;
                        }
                        let destination = if let Some(slot) = target_slot {
                            Some(forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot))
                        } else if let Some(container_id) = target_container_id {
                            if closed_container_ids.contains(&container_id) {
                                native_diagnostic(
                                        config.extended_diagnostics,
                                        peer,
                                        "action=throw-item outcome=deferred-closed-container-pickup-target",
                                    );
                                continue;
                            }
                            Some(forgotten_core::PlayerGroundDropSource::ContainerItem {
                                container_id,
                                item_index: 0,
                            })
                        } else {
                            None
                        };
                        let Some(destination) = destination else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-unsupported-runtime-pickup-target",
                            );
                            continue;
                        };
                        match map_owner.move_runtime_item_to_inventory(
                            shared_world,
                            &mut database,
                            character.id,
                            core_source_position,
                            usize::from(source_stack_position),
                            None,
                            u16::from(count),
                            destination,
                            config.item_weight_by_server_id.as_deref(),
                        ) {
                            Ok(Some(outcome)) => {
                                if let forgotten_core::PlayerGroundDropSource::EquipmentSlot(_) =
                                    outcome.source
                                {
                                    let equipment = shared_world.player_equipment(character.id)?;
                                    let current_mapped_equipment =
                                        native_classic_mapped_equipment(Some(catalog), &equipment);
                                    let equipment_updates = native_classic_equipment_delta_frames(
                                        &config.client_profile,
                                        &observed_mapped_equipment,
                                        &current_mapped_equipment,
                                    )
                                    .map_err(HostError::Protocol)?;
                                    for frame in &equipment_updates {
                                        write_frame(stream, frame)?;
                                    }
                                    observed_mapped_equipment = current_mapped_equipment;
                                    observed_equipment_epoch = shared_world.equipment_epoch();
                                }
                                if let forgotten_core::PlayerGroundDropSource::ContainerItem {
                                    container_id,
                                    ..
                                } = outcome.source
                                {
                                    if !closed_container_ids.contains(&container_id) {
                                        let containers =
                                            shared_world.player_containers(character.id)?;
                                        if let Some(container) = containers.container(container_id)
                                        {
                                            if let Some(frame) = native_classic_container_frame(
                                                &config.client_profile,
                                                Some(catalog),
                                                container,
                                            )
                                            .map_err(HostError::Protocol)?
                                            {
                                                write_frame(stream, &frame)?;
                                            }
                                            sent_container_windows.insert(
                                                container_id,
                                                native_rendered_container_window(
                                                    &config.client_profile,
                                                    Some(catalog),
                                                    container,
                                                ),
                                            );
                                        }
                                    }
                                    observed_containers_epoch = shared_world.containers_epoch();
                                }
                                let mut refreshed_snapshot = snapshot.clone();
                                refreshed_snapshot.player_position =
                                    native_position(player_position);
                                refreshed_snapshot.player_direction = facing.protocol_direction();
                                let map_snapshot = map_owner.render_snapshot()?;
                                let refreshed_viewport = encode_shared_native_world_viewport(
                                    &config.client_profile,
                                    &refreshed_snapshot,
                                    map_snapshot.as_ref(),
                                    shared_world,
                                    character.id,
                                )?;
                                write_frame(stream, &refreshed_viewport)?;
                                observed_visibility_epoch = shared_world.visibility_epoch();
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=throw-item outcome=runtime-ground-pickup source={},{},{} server-id={} count={} moved={} remaining={:?} index={}",
                                        core_source_position.x,
                                        core_source_position.y,
                                        core_source_position.z,
                                        runtime_item.server_id,
                                        source_client_thing_id,
                                        outcome.moved_item.count,
                                        outcome.source_remaining_count,
                                        source_stack_position,
                                    ),
                                );
                            }
                            Ok(None) => native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-runtime-pickup-rejected",
                            ),
                            Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "action=throw-item outcome=deferred-runtime-pickup-failed",
                                );
                            }
                            Err(error) => return Err(error),
                        }
                        continue;
                    }
                    let Some(intent) = native_map_item_use_intent(
                        Some(catalog),
                        character.id,
                        source_position,
                        source_client_thing_id,
                        source_stack_position,
                    ) else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-unmapped-or-ambiguous-map-source-item",
                        );
                        continue;
                    };
                    let map_snapshot = map_owner.render_snapshot()?;
                    let source = match shared_world.validate_player_item_use(&map_snapshot, intent)
                    {
                        Ok(source) => source,
                        Err(HostError::Core(_)) => {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-invalid-server-owned-map-source",
                            );
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    let source_position = Position {
                        x: source_position.x,
                        y: source_position.y,
                        z: source_position.z,
                    };
                    if let Some(container_id) = target_container_id {
                        if closed_container_ids.contains(&container_id) {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-closed-map-source-container-target",
                            );
                            continue;
                        }
                        let transfer = match map_owner
                            .move_source_item_stack_to_top_level_container(
                                shared_world,
                                &mut database,
                                character.id,
                                source_position,
                                usize::from(source_stack_position),
                                u16::from(count),
                                container_id,
                            ) {
                            Ok(transfer) => transfer,
                            Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "action=throw-item outcome=deferred-map-source-container-transfer-rejected",
                                );
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        let containers = shared_world.player_containers(character.id)?;
                        let Some(container) = containers.container(container_id) else {
                            return Err(HostError::InvalidConfiguration(
                                "published map-source container transfer lost its container".into(),
                            ));
                        };
                        let Some(container_frame) = native_classic_container_frame(
                            &config.client_profile,
                            Some(catalog),
                            container,
                        )
                        .map_err(HostError::Protocol)?
                        else {
                            return Err(HostError::InvalidConfiguration(
                                "published map-source container transfer is not client-mapped"
                                    .into(),
                            ));
                        };
                        write_frame(stream, &container_frame)?;
                        sent_container_windows.insert(
                            container_id,
                            native_rendered_container_window(
                                &config.client_profile,
                                Some(catalog),
                                container,
                            ),
                        );
                        observed_containers_epoch = shared_world.containers_epoch();
                        let mut refreshed_snapshot = snapshot.clone();
                        refreshed_snapshot.player_position = native_position(player_position);
                        refreshed_snapshot.player_direction = facing.protocol_direction();
                        let map_snapshot = map_owner.render_snapshot()?;
                        let refreshed_viewport = encode_shared_native_world_viewport(
                            &config.client_profile,
                            &refreshed_snapshot,
                            map_snapshot.as_ref(),
                            shared_world,
                            character.id,
                        )?;
                        write_frame(stream, &refreshed_viewport)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=throw-item outcome=map-source-to-top-level-container source={:?} container-id={} client-thing-id={} count={} source-index={} map-revision={} container-refresh-bytes={} map-refresh-bytes={}",
                                transfer.source_identity.position,
                                container_id,
                                source_client_thing_id,
                                count,
                                transfer.source_identity.item_index,
                                transfer.map_revision,
                                container_frame.0.len(),
                                refreshed_viewport.0.len(),
                            ),
                        );
                        continue;
                    }
                    let Some(target_slot) = target_slot else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-unsupported-map-source-target",
                        );
                        continue;
                    };
                    if !native_legacy_slot_types_allow_equipment_slot(
                        config.item_slot_types_by_server_id.as_deref(),
                        source.server_id,
                        target_slot,
                    ) {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-map-source-slot-type-mismatch",
                        );
                        continue;
                    }
                    let transfer = match map_owner.move_source_item_stack_to_equipment(
                        shared_world,
                        &mut database,
                        character.id,
                        source_position,
                        usize::from(source_stack_position),
                        u16::from(count),
                        target_slot,
                    ) {
                        Ok(transfer) => transfer,
                        Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-map-source-transfer-rejected",
                            );
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    let equipment = shared_world.player_equipment(character.id)?;
                    let current_mapped_equipment =
                        native_classic_mapped_equipment(Some(catalog), &equipment);
                    let equipment_updates = native_classic_equipment_delta_frames(
                        &config.client_profile,
                        &observed_mapped_equipment,
                        &current_mapped_equipment,
                    )
                    .map_err(HostError::Protocol)?;
                    for frame in &equipment_updates {
                        write_frame(stream, frame)?;
                    }
                    observed_mapped_equipment = current_mapped_equipment;
                    observed_equipment_epoch = shared_world.equipment_epoch();
                    let mut refreshed_snapshot = snapshot.clone();
                    refreshed_snapshot.player_position = native_position(player_position);
                    refreshed_snapshot.player_direction = facing.protocol_direction();
                    let map_snapshot = map_owner.render_snapshot()?;
                    let refreshed_viewport = encode_shared_native_world_viewport(
                        &config.client_profile,
                        &refreshed_snapshot,
                        map_snapshot.as_ref(),
                        shared_world,
                        character.id,
                    )?;
                    write_frame(stream, &refreshed_viewport)?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=throw-item outcome=map-source-to-equipment source={:?} target-slot={} client-thing-id={} count={} source-index={} map-revision={} equipment-records={} map-refresh-bytes={}",
                            transfer.source_identity.position,
                            target_slot.code(),
                            source_client_thing_id,
                            count,
                            transfer.source_identity.item_index,
                            transfer.map_revision,
                            equipment_updates.len(),
                            refreshed_viewport.0.len(),
                        ),
                    );
                    continue;
                }
                if let Some((container_id, item_index)) = source_container {
                    // Open corpse windows are session-local views over durable runtime registry
                    // items; taking loot routes through the registry composite instead of
                    // player-owned container storage.
                    if let Some((corpse_position, corpse_item_index)) =
                        open_corpse_windows.get(&container_id).copied()
                    {
                        let destination = if let Some(slot) = target_slot {
                            Some(forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot))
                        } else if let Some(target_container) = target_container_id {
                            if closed_container_ids.contains(&target_container)
                                || target_container == container_id
                            {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "action=throw-item outcome=deferred-closed-corpse-take-target",
                                );
                                continue;
                            }
                            Some(forgotten_core::PlayerGroundDropSource::ContainerItem {
                                container_id: target_container,
                                item_index: 0,
                            })
                        } else {
                            None
                        };
                        let Some(destination) = destination.filter(|_| !observed_dead) else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-unsupported-corpse-take-target",
                            );
                            continue;
                        };
                        match map_owner.move_runtime_item_to_inventory(
                            shared_world,
                            &mut database,
                            character.id,
                            corpse_position,
                            corpse_item_index,
                            Some(usize::from(source_stack_position)),
                            u16::from(count),
                            destination,
                            config.item_weight_by_server_id.as_deref(),
                        ) {
                            Ok(Some(outcome)) => {
                                // Re-send the refreshed corpse window so remaining loot stays accurate.
                                if let Some(runtime_corpse) = map_owner
                                    .runtime_tile_item(corpse_position, corpse_item_index)?
                                {
                                    if let Some(frame) = native_corpse_window_frame(
                                        &config.client_profile,
                                        Some(catalog),
                                        container_id,
                                        &runtime_corpse,
                                        config.item_name_by_server_id.as_deref(),
                                    )
                                    .map_err(HostError::Protocol)?
                                    {
                                        write_frame(stream, &frame)?;
                                    }
                                } else {
                                    open_corpse_windows.remove(&container_id);
                                    open_content_windows.remove(&container_id);
                                }
                                if let forgotten_core::PlayerGroundDropSource::EquipmentSlot(_) =
                                    outcome.source
                                {
                                    let equipment = shared_world.player_equipment(character.id)?;
                                    let current_mapped_equipment =
                                        native_classic_mapped_equipment(Some(catalog), &equipment);
                                    let equipment_updates = native_classic_equipment_delta_frames(
                                        &config.client_profile,
                                        &observed_mapped_equipment,
                                        &current_mapped_equipment,
                                    )
                                    .map_err(HostError::Protocol)?;
                                    for frame in &equipment_updates {
                                        write_frame(stream, frame)?;
                                    }
                                    observed_mapped_equipment = current_mapped_equipment;
                                    observed_equipment_epoch = shared_world.equipment_epoch();
                                }
                                if let forgotten_core::PlayerGroundDropSource::ContainerItem {
                                    container_id: target_container,
                                    ..
                                } = outcome.source
                                {
                                    if !closed_container_ids.contains(&target_container) {
                                        let containers =
                                            shared_world.player_containers(character.id)?;
                                        if let Some(container) =
                                            containers.container(target_container)
                                        {
                                            if let Some(frame) = native_classic_container_frame(
                                                &config.client_profile,
                                                Some(catalog),
                                                container,
                                            )
                                            .map_err(HostError::Protocol)?
                                            {
                                                write_frame(stream, &frame)?;
                                            }
                                            sent_container_windows.insert(
                                                target_container,
                                                native_rendered_container_window(
                                                    &config.client_profile,
                                                    Some(catalog),
                                                    container,
                                                ),
                                            );
                                        }
                                    }
                                    observed_containers_epoch = shared_world.containers_epoch();
                                }
                                let mut refreshed_snapshot = snapshot.clone();
                                refreshed_snapshot.player_position =
                                    native_position(player_position);
                                refreshed_snapshot.player_direction = facing.protocol_direction();
                                let map_snapshot = map_owner.render_snapshot()?;
                                let refreshed_viewport = encode_shared_native_world_viewport(
                                    &config.client_profile,
                                    &refreshed_snapshot,
                                    map_snapshot.as_ref(),
                                    shared_world,
                                    character.id,
                                )?;
                                write_frame(stream, &refreshed_viewport)?;
                                observed_visibility_epoch = shared_world.visibility_epoch();
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=throw-item outcome=corpse-loot-taken window-id={} child-index={} server-id={} moved={} remaining={:?}",
                                        container_id,
                                        source_stack_position,
                                        outcome.moved_item.server_id,
                                        outcome.moved_item.count,
                                        outcome.source_remaining_count,
                                    ),
                                );
                            }
                            Ok(None) => native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-corpse-take-rejected",
                            ),
                            Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "action=throw-item outcome=deferred-corpse-take-failed",
                                );
                            }
                            Err(error) => return Err(error),
                        }
                        continue;
                    } // All containerâ†”equipment and containerâ†”container throw paths below
                      // persist through the atomic replace_player_inventory boundary so a
                      // torn two-transaction inventory can never be observed or crash-duplicated.
                    if let Some(target_container_id) = target_container_id {
                        // Nested content window source: translate the ephemeral window address
                        // and move the whole content item into the target owned container.
                        if let Some(&(parent_container_id, parent_item_index)) =
                            open_content_windows.get(&container_id)
                        {
                            shared_world.move_content_item_to_container(
                                character.id,
                                parent_container_id,
                                parent_item_index,
                                item_index,
                                target_container_id,
                            )?;
                            let next_equipment = shared_world.player_equipment(character.id)?;
                            let next_containers = shared_world.player_containers(character.id)?;
                            database.replace_player_inventory(
                                character.id,
                                &next_equipment,
                                &next_containers,
                            )?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=throw-item outcome=content-item-to-container parent-container-id={} parent-item-index={} content-index={item_index} target-container-id={} client-thing-id={}",
                                    parent_container_id,
                                    parent_item_index,
                                    target_container_id,
                                    source_client_thing_id
                                ),
                            );
                            native_refresh_open_content_windows(
                                stream,
                                &config.client_profile,
                                config.item_presentation_catalog.as_deref(),
                                &shared_world.player_containers(character.id)?,
                                &mut open_content_windows,
                            )?;
                            continue;
                        }
                        let containers = shared_world.player_containers(character.id)?;
                        let Some(source_container) = containers.container(container_id) else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-unknown-container-source",
                            );
                            continue;
                        };
                        let Some(target_container) = containers.container(target_container_id)
                        else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-unknown-container-target",
                            );
                            continue;
                        };
                        if closed_container_ids.contains(&container_id)
                            || closed_container_ids.contains(&target_container_id)
                            || container_id == target_container_id
                            || source_container.has_parent
                            || target_container.has_parent
                        {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-invalid-container-to-container-boundary",
                            );
                            continue;
                        }
                        let Some(item) = source_container.items.item(item_index) else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-unknown-container-source-item",
                            );
                            continue;
                        };
                        if item.count < u16::from(count)
                            || catalog
                                .presentation(item.server_id)
                                .map(|entry| entry.client_thing_id)
                                != Some(source_client_thing_id)
                        {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-invalid-container-item-identity-or-source-count",
                            );
                            continue;
                        }
                        shared_world.move_container_stack_to_container(
                            character.id,
                            container_id,
                            item_index,
                            target_container_id,
                            u16::from(count),
                        )?;
                        let next_containers = shared_world.player_containers(character.id)?;
                        database.replace_player_containers(character.id, &next_containers)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=throw-item outcome=top-level-container-to-container-stack source-container-id={} item-index={} target-container-id={} client-thing-id={} count={}",
                                container_id,
                                item_index,
                                target_container_id,
                                source_client_thing_id,
                                count
                            ),
                        );
                        continue;
                    }
                    let Some(target_slot) = target_slot else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-unsupported-container-source-target",
                        );
                        continue;
                    };
                    // Nested content window source: translate the ephemeral window address and
                    // move the whole content item into an empty equipment slot.
                    if let Some(&(parent_container_id, parent_item_index)) =
                        open_content_windows.get(&container_id)
                    {
                        shared_world.move_content_item_to_equipment(
                            character.id,
                            parent_container_id,
                            parent_item_index,
                            item_index,
                            target_slot,
                        )?;
                        let next_equipment = shared_world.player_equipment(character.id)?;
                        let next_containers = shared_world.player_containers(character.id)?;
                        database.replace_player_inventory(
                            character.id,
                            &next_equipment,
                            &next_containers,
                        )?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=throw-item outcome=content-item-to-equipment parent-container-id={} parent-item-index={} content-index={item_index} target-slot={} client-thing-id={}",
                                parent_container_id,
                                parent_item_index,
                                target_slot.code(),
                                source_client_thing_id
                            ),
                        );
                        native_refresh_open_content_windows(
                            stream,
                            &config.client_profile,
                            config.item_presentation_catalog.as_deref(),
                            &shared_world.player_containers(character.id)?,
                            &mut open_content_windows,
                        )?;
                        continue;
                    }
                    let equipment = shared_world.player_equipment(character.id)?;
                    let containers = shared_world.player_containers(character.id)?;
                    let Some(container) = containers.container(container_id) else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-unknown-container-source",
                        );
                        continue;
                    };
                    if container.has_parent {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-nested-container-source",
                        );
                        continue;
                    }
                    let Some(item) = container.items.item(item_index) else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-unknown-container-source-item",
                        );
                        continue;
                    };
                    if item.count < u16::from(count)
                        || catalog
                            .presentation(item.server_id)
                            .map(|entry| entry.client_thing_id)
                            != Some(source_client_thing_id)
                    {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-invalid-container-item-identity-or-source-count",
                        );
                        continue;
                    }
                    let requested_count = u16::from(count);
                    if requested_count < item.count {
                        if equipment
                            .item(target_slot)
                            .is_some_and(|destination| destination.server_id != item.server_id)
                        {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-nonmatching-equipment-stack-merge-destination",
                            );
                            continue;
                        }
                        shared_world.move_container_stack_to_equipment(
                            character.id,
                            container_id,
                            item_index,
                            target_slot,
                            requested_count,
                        )?;
                        let next_equipment = shared_world.player_equipment(character.id)?;
                        let next_containers = shared_world.player_containers(character.id)?;
                        database.replace_player_inventory(
                            character.id,
                            &next_equipment,
                            &next_containers,
                        )?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=throw-item outcome=top-level-container-stack-to-equipment-merge container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                                container_id,
                                item_index,
                                target_slot.code(),
                                source_client_thing_id,
                                count
                            ),
                        );
                        continue;
                    }
                    if equipment
                        .item(target_slot)
                        .is_some_and(|destination| destination.server_id == item.server_id)
                    {
                        shared_world.move_container_stack_to_equipment(
                            character.id,
                            container_id,
                            item_index,
                            target_slot,
                            requested_count,
                        )?;
                        let next_equipment = shared_world.player_equipment(character.id)?;
                        let next_containers = shared_world.player_containers(character.id)?;
                        database.replace_player_inventory(
                            character.id,
                            &next_equipment,
                            &next_containers,
                        )?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=throw-item outcome=top-level-container-stack-to-equipment-merge container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                                container_id,
                                item_index,
                                target_slot.code(),
                                source_client_thing_id,
                                count
                            ),
                        );
                        continue;
                    }
                    if equipment.item(target_slot).is_some() {
                        if requested_count == item.count {
                            shared_world.swap_container_item_with_equipment(
                                character.id,
                                container_id,
                                item_index,
                                target_slot,
                            )?;
                            let next_equipment = shared_world.player_equipment(character.id)?;
                            let next_containers = shared_world.player_containers(character.id)?;
                            database.replace_player_inventory(
                                character.id,
                                &next_equipment,
                                &next_containers,
                            )?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=throw-item outcome=top-level-container-to-occupied-equipment-swap container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                                    container_id,
                                    item_index,
                                    target_slot.code(),
                                    source_client_thing_id,
                                    count
                                ),
                            );
                            continue;
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-partial-or-unverified-occupied-equipment-target",
                        );
                        continue;
                    }
                    if !native_legacy_slot_types_allow_equipment_slot(
                        config.item_slot_types_by_server_id.as_deref(),
                        item.server_id,
                        target_slot,
                    ) {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-container-item-slot-type-mismatch",
                        );
                        continue;
                    }
                    shared_world.move_container_item_to_equipment(
                        character.id,
                        container_id,
                        item_index,
                        target_slot,
                    )?;
                    let next_equipment = shared_world.player_equipment(character.id)?;
                    let next_containers = shared_world.player_containers(character.id)?;
                    database.replace_player_inventory(
                        character.id,
                        &next_equipment,
                        &next_containers,
                    )?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=throw-item outcome=top-level-container-to-equipment container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                            container_id,
                            item_index,
                            target_slot.code(),
                            source_client_thing_id,
                            count
                        ),
                    );
                    continue;
                }
                let Some(source_slot) = source_slot else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=throw-item outcome=deferred-non-equipment-source-position",
                    );
                    continue;
                };
                let equipment = shared_world.player_equipment(character.id)?;
                let Some(item) = equipment.item(source_slot).cloned() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=throw-item outcome=deferred-empty-source-slot",
                    );
                    continue;
                };
                if item.count < u16::from(count)
                    || catalog
                        .presentation(item.server_id)
                        .map(|entry| entry.client_thing_id)
                        != Some(source_client_thing_id)
                {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=throw-item outcome=deferred-invalid-item-identity-or-source-count",
                    );
                    continue;
                }
                match (target_slot, target_container_id) {
                    (Some(target_slot), None) => {
                        if source_slot == target_slot {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-same-equipment-slot-target",
                            );
                            continue;
                        }
                        if equipment.item(target_slot).is_some() {
                            if u16::from(count) != item.count {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "action=throw-item outcome=deferred-partial-occupied-equipment-target",
                                );
                                continue;
                            }
                            shared_world.swap_equipment_items(
                                character.id,
                                source_slot,
                                target_slot,
                            )?;
                            let next_equipment = shared_world.player_equipment(character.id)?;
                            database.replace_player_equipment(character.id, &next_equipment)?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=throw-item outcome=occupied-equipment-slot-swap source-slot={} target-slot={} client-thing-id={} count={}",
                                    source_slot.code(),
                                    target_slot.code(),
                                    source_client_thing_id,
                                    count
                                ),
                            );
                            continue;
                        }
                        let mut next_equipment = equipment;
                        next_equipment.unequip(source_slot);
                        next_equipment.equip(target_slot, item);
                        database.replace_player_equipment(character.id, &next_equipment)?;
                        shared_world.replace_player_equipment(character.id, next_equipment)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=throw-item outcome=equipment-slot-transfer source-slot={} target-slot={} client-thing-id={} count={}",
                                source_slot.code(),
                                target_slot.code(),
                                source_client_thing_id,
                                count
                            ),
                        );
                    }
                    (None, Some(container_id)) => {
                        let containers = shared_world.player_containers(character.id)?;
                        let Some(container) = containers.container(container_id) else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-unknown-container-target",
                            );
                            continue;
                        };
                        if container.has_parent {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-nested-container-target",
                            );
                            continue;
                        }
                        let requested_count = u16::from(count);
                        let destination_index = usize::from(target_position.z);
                        if container
                            .items
                            .item(destination_index)
                            .is_some_and(|destination| destination.server_id == item.server_id)
                        {
                            shared_world.move_equipment_stack_to_container(
                                character.id,
                                source_slot,
                                container_id,
                                requested_count,
                            )?;
                            let next_equipment = shared_world.player_equipment(character.id)?;
                            let next_containers = shared_world.player_containers(character.id)?;
                            database.replace_player_inventory(
                                character.id,
                                &next_equipment,
                                &next_containers,
                            )?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=throw-item outcome=equipment-stack-to-top-level-container-merge source-slot={} container-id={} destination-index={} client-thing-id={} count={}",
                                    source_slot.code(),
                                    container_id,
                                    destination_index,
                                    source_client_thing_id,
                                    count
                                ),
                            );
                            continue;
                        }
                        if requested_count < item.count {
                            let outcome = if container.items.item(destination_index).is_some() {
                                "deferred-nonmatching-stack-merge-destination"
                            } else {
                                "deferred-missing-stack-merge-destination"
                            };
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!("action=throw-item outcome={outcome}"),
                            );
                            continue;
                        }
                        if container.items.item(destination_index).is_some() {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=throw-item outcome=deferred-nonmatching-full-stack-merge-destination",
                            );
                            continue;
                        }
                        shared_world.move_equipment_item_to_container(
                            character.id,
                            source_slot,
                            container_id,
                        )?;
                        let next_equipment = shared_world.player_equipment(character.id)?;
                        let next_containers = shared_world.player_containers(character.id)?;
                        database.replace_player_inventory(
                            character.id,
                            &next_equipment,
                            &next_containers,
                        )?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=throw-item outcome=equipment-to-top-level-container source-slot={} container-id={} client-thing-id={} count={}",
                                source_slot.code(),
                                container_id,
                                source_client_thing_id,
                                count
                            ),
                        );
                    }
                    _ => {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=throw-item outcome=deferred-unsupported-target-position",
                        );
                    }
                }
            }
            NativeOtClientGameAction::ChangeFightModes(request) => {
                let mode = match request.mode {
                    NativeOtClientFightMode::Attack => PlayerFightMode::Attack,
                    NativeOtClientFightMode::Balanced => PlayerFightMode::Balanced,
                    NativeOtClientFightMode::Defense => PlayerFightMode::Defense,
                };
                let changed = shared_world.replace_player_fight_mode_state(
                    character.id,
                    PlayerFightModeState {
                        mode,
                        chase: request.chase,
                        secure: request.secure,
                    },
                )?;
                if changed {
                    let player_modes =
                        encode_native_otclient_player_modes(&config.client_profile, request)
                            .map_err(HostError::Protocol)?;
                    write_frame(stream, &player_modes)?;
                }
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=change-fight-modes outcome=applied changed={changed} delivery={}",
                        if changed { "player-modes" } else { "unchanged" }
                    ),
                );
            }
            NativeOtClientGameAction::CloseContainer(container_id) => {
                closed_container_ids.insert(container_id);
                open_corpse_windows.remove(&container_id);
                open_content_windows.remove(&container_id);
                let close =
                    encode_native_otclient_close_container(&config.client_profile, container_id)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &close)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=close-container outcome=session-view-closed container-id={container_id}"
                    ),
                );
            }
            NativeOtClientGameAction::UpArrowContainer(container_id) => {
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=up-arrow-container outcome=deferred-no-supported-parent container-id={container_id}"
                    ),
                );
            }
            NativeOtClientGameAction::UpdateContainer(container_id) => {
                let containers = shared_world.player_containers(character.id)?;
                let frame = containers
                    .container(container_id)
                    .map(|container| {
                        native_classic_container_frame(
                            &config.client_profile,
                            config.item_presentation_catalog.as_deref(),
                            container,
                        )
                    })
                    .transpose()
                    .map_err(HostError::Protocol)?
                    .flatten();
                let refreshed = frame.is_some();
                if let Some(frame) = frame {
                    closed_container_ids.remove(&container_id);
                    write_frame(stream, &frame)?;
                }
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=update-container outcome={} container-id={container_id}",
                        if refreshed {
                            "session-view-refreshed"
                        } else {
                            "deferred-unavailable-or-unmapped"
                        }
                    ),
                );
            }
            NativeOtClientGameAction::UseItem {
                position,
                client_thing_id,
                stack_position,
                index,
            } => {
                // Nested content window: using an item inside an owned-container window that
                // itself holds content presents those contents as a child window. Items without
                // contents fall through to the consumable handler below.
                if position.x == 0xffff && position.y & 0x40 != 0 {
                    let parent_container_id = (position.y & 0x0f) as u8;
                    let item_index = usize::from(position.z);
                    let containers = shared_world.player_containers(character.id)?;
                    if let Some(item) = containers
                        .container(parent_container_id)
                        .and_then(|container| container.items.item(item_index))
                        .filter(|item| !item.contents().is_empty())
                        .cloned()
                    {
                        let mut busy: BTreeSet<u8> = containers
                            .iter()
                            .map(|(_, container)| container.container_id)
                            .collect();
                        busy.extend(open_corpse_windows.keys().copied());
                        busy.extend(open_content_windows.keys().copied());
                        if let Some(window_id) = (0..=15u8).find(|id| !busy.contains(id)) {
                            if let Some(frame) = native_nested_content_window_frame(
                                &config.client_profile,
                                config.item_presentation_catalog.as_deref(),
                                window_id,
                                parent_container_id,
                                &item,
                            )
                            .map_err(HostError::Protocol)?
                            {
                                write_frame(stream, &frame)?;
                                open_content_windows
                                    .insert(window_id, (parent_container_id, item_index));
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=use-item outcome=content-window-opened parent={} item-index={item_index} window-id={window_id} contents={}",
                                        parent_container_id,
                                        item.contents().len()
                                    ),
                                );
                            }
                        }
                        // A content-bearing item is a container-open action, never a consumable:
                        // stop here so the consumable handler does not also process it.
                        continue;
                    }
                    // No contents on this item: fall through to consumable handling.
                }

                let source_is_own_inventory = position.x == 0xffff;
                // Owned-inventory consumable use runs before map-item routing: classic clients
                // address own equipment with x=0xFFFF and a plain slot code in y, and own
                // container items with the container flag plus the child index in z.
                if source_is_own_inventory {
                    if observed_dead {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=use-item outcome=deferred-consume-while-dead",
                        );
                        continue;
                    }
                    let consumable_target: Option<ConsumableSource> =
                        if position.x == 0xffff && position.y & 0x40 == 0 {
                            EquipmentSlot::from_code(position.y as u8).and_then(|slot| {
                                let equipment = shared_world.player_equipment(character.id).ok()?;
                                let item = equipment.item(slot)?;
                                Some(ConsumableSource {
                                    server_id: item.server_id,
                                    slot: Some(slot),
                                    container_ref: None,
                                })
                            })
                        } else if position.x == 0xffff && position.y & 0x40 != 0 {
                            let container_id = (position.y & 0x0f) as u8;
                            let child_index = usize::from(position.z);
                            shared_world
                                .player_containers(character.id)
                                .ok()
                                .and_then(|containers| containers.container(container_id).cloned())
                                .and_then(|container| container.items.item(child_index).cloned())
                                .map(|item| ConsumableSource {
                                    server_id: item.server_id,
                                    slot: None,
                                    container_ref: Some((container_id, child_index)),
                                })
                        } else {
                            None
                        };
                    let Some(ConsumableSource {
                        server_id: consumable_server_id,
                        slot,
                        container_ref,
                    }) = consumable_target
                    else {
                        continue;
                    };
                    let _ = client_thing_id;
                    let Some(&effect) = config
                        .consumable_effects
                        .as_deref()
                        .and_then(|effects| effects.get(&consumable_server_id))
                    else {
                        continue;
                    };
                    let (heal, mana_restore) = (effect.health, effect.mana);
                    // Classic fed state (plan v49 slice 16): eating while a food window is
                    // active answers "You are full." and leaves the item untouched.
                    if effect.regeneration_seconds > 0 {
                        let granted = shared_world
                            .lock()?
                            .grant_player_food_window(character.id, effect.regeneration_seconds)
                            .map_err(HostError::Core)?;
                        if !granted {
                            let full_notice = encode_native_otclient_status_message(
                                &config.client_profile,
                                "You are full.",
                            )
                            .map_err(HostError::Protocol)?;
                            write_frame(stream, &full_notice)?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=use-item outcome=too-full server-id={consumable_server_id}"
                                ),
                            );
                            continue;
                        }
                    }
                    let mut vitals = shared_world.player_vitals(character.id)?;
                    if heal > 0 {
                        vitals.health = vitals.health.saturating_add(heal).min(vitals.max_health);
                    }
                    if mana_restore > 0 {
                        vitals.mana = vitals
                            .mana
                            .saturating_add(mana_restore)
                            .min(vitals.max_mana);
                    }
                    // Consume one unit from the resolved inventory location.
                    match (&slot, &container_ref) {
                        (Some(slot), _) => {
                            let mut equipment = shared_world.player_equipment(character.id)?;
                            if let Some(item) = equipment.item(*slot).cloned() {
                                if item.count > 1 {
                                    let mut remaining = item;
                                    remaining.count -= 1;
                                    equipment.equip(*slot, remaining);
                                } else {
                                    equipment.unequip(*slot);
                                }
                            }
                            shared_world
                                .replace_player_equipment(character.id, equipment.clone())?;
                            database
                                .replace_player_equipment(character.id, &equipment)
                                .map_err(HostError::Persistence)?;
                        }
                        (_, Some((container_id, child_index))) => {
                            let mut containers = shared_world.player_containers(character.id)?;
                            let mut container = match containers.remove(*container_id) {
                                Some(container) => container,
                                None => continue,
                            };
                            if !container.items.consume_item_unit(*child_index) {
                                continue;
                            }
                            containers.insert(container).map_err(HostError::Core)?;
                            database
                                .replace_player_containers(character.id, &containers)
                                .map_err(HostError::Persistence)?;
                        }
                        _ => {}
                    }
                    shared_world
                        .lock()?
                        .update_player_vitals(character.id, vitals)
                        .map_err(HostError::Core)?;
                    shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
                    database
                        .update_player_vitals(
                            character.id,
                            PersistedPlayerVitals {
                                health: vitals.health,
                                max_health: vitals.max_health,
                                mana: vitals.mana,
                                max_mana: vitals.max_mana,
                                capacity: vitals.capacity,
                                magic_level: vitals.magic_level,
                            },
                        )
                        .map_err(HostError::Persistence)?;
                    let self_native_id = native_player_id(character.id)?;
                    let health_update = encode_native_otclient_creature_health(
                        &config.client_profile,
                        self_native_id,
                        vitals.health,
                        vitals.max_health,
                    )
                    .map_err(HostError::Protocol)?;
                    write_frame(stream, &health_update)?;
                    observed_vitals_epoch = shared_world.vitals_epoch();
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=use-item outcome=consumed server-id={} heal={} mana={} health={} mana={}",
                            consumable_server_id,
                            heal,
                            mana_restore,
                            vitals.health,
                            vitals.mana,
                        ),
                    );
                    continue;
                }
                // Backpack-in-hand: using the equipped backpack item opens the lowest owned
                // top-level container as a client window. FE links one backpack to one
                // container by convention until full nesting lands.
                if position.x == 0xffff
                    && position.y & 0x40 == 0
                    && EquipmentSlot::from_code(position.y as u8) == Some(EquipmentSlot::Backpack)
                {
                    if observed_dead {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=use-item outcome=deferred-backpack-while-dead",
                        );
                        continue;
                    }
                    let equipped_backpack = shared_world
                        .player_equipment(character.id)
                        .ok()
                        .and_then(|equipment| equipment.item(EquipmentSlot::Backpack).cloned());
                    if equipped_backpack.is_none() {
                        continue;
                    }
                    let containers = shared_world.player_containers(character.id)?;
                    let open_container = containers
                        .iter()
                        .find(|(_, container)| !container.has_parent)
                        .map(|(id, _)| id);
                    let Some(container_id) = open_container else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=use-item outcome=deferred-backpack-no-container",
                        );
                        continue;
                    };
                    if closed_container_ids.contains(&container_id) {
                        closed_container_ids.remove(&container_id);
                    }
                    if let Some(container) = containers.container(container_id) {
                        if let Some(frame) = native_classic_container_frame(
                            &config.client_profile,
                            config.item_presentation_catalog.as_deref(),
                            container,
                        )
                        .map_err(HostError::Protocol)?
                        {
                            write_frame(stream, &frame)?;
                            sent_container_windows.insert(
                                container_id,
                                native_rendered_container_window(
                                    &config.client_profile,
                                    config.item_presentation_catalog.as_deref(),
                                    container,
                                ),
                            );
                            observed_containers_epoch = shared_world.containers_epoch();
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=use-item outcome=backpack-window-opened container-id={container_id}"
                                ),
                            );
                        }
                    }
                    continue;
                }
                // Runtime-corpse opening runs first because identity comes from FE's own durable
                // registry rather than the operator presentation catalog.
                let corpse_attempt = map_owner.runtime_tile_item(
                    Position {
                        x: position.x,
                        y: position.y,
                        z: position.z,
                    },
                    usize::from(stack_position),
                )?;
                if let Some(corpse) = corpse_attempt {
                    let core_position = Position {
                        x: position.x,
                        y: position.y,
                        z: position.z,
                    };
                    if observed_dead {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=use-item outcome=deferred-corpse-use-while-dead",
                        );
                        continue;
                    }
                    if client_thing_id != corpse.server_id {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=use-item outcome=deferred-runtime-item-identity-mismatch",
                        );
                        continue;
                    }
                    let shared_snapshot = map_owner.render_snapshot()?;
                    let intent = PlayerItemUseIntent::new(
                        character.id,
                        core_position,
                        stack_position,
                        corpse.server_id,
                    )
                    .map_err(HostError::Core)?;
                    match shared_world.validate_player_item_use(shared_snapshot.as_ref(), intent) {
                        Ok(_) => {}
                        Err(HostError::Core(_)) => {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=use-item outcome=deferred-corpse-unreachable",
                            );
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                    let open_container_ids = shared_world
                        .player_containers(character.id)?
                        .iter()
                        .map(|(_, container)| container.container_id)
                        .collect::<BTreeSet<_>>();
                    match native_corpse_window_id(
                        &open_container_ids,
                        &open_corpse_windows.keys().copied().collect(),
                    ) {
                        Some(window_id) => {
                            match native_corpse_window_frame(
                                &config.client_profile,
                                config.item_presentation_catalog.as_deref(),
                                window_id,
                                &corpse,
                                config.item_name_by_server_id.as_deref(),
                            )
                            .map_err(HostError::Protocol)?
                            {
                                Some(frame) => {
                                    write_frame(stream, &frame)?;
                                    open_corpse_windows.insert(
                                        window_id,
                                        (core_position, usize::from(stack_position)),
                                    );
                                    native_diagnostic(
                                        config.extended_diagnostics,
                                        peer,
                                        &format!(
                                            "action=use-item outcome=corpse-window-opened server-id={} children={} window-id={window_id} index={index}",
                                            corpse.server_id,
                                            corpse.children.len(),
                                        ),
                                    );
                                }
                                None => native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "action=use-item outcome=deferred-corpse-window-unsupported",
                                ),
                            }
                        }
                        None => native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=use-item outcome=deferred-corpse-window-capacity",
                        ),
                    }
                    continue;
                }
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    position,
                    client_thing_id,
                    stack_position,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=use-item outcome=deferred-unmapped-or-ambiguous-client-thing-id client-thing-id={client_thing_id}"
                        ),
                    );
                    continue;
                };
                match shared_world.validate_player_item_use(world_map, intent) {
                    Ok(outcome) => {
                        if let Some(destination) = outcome.teleport_destination {
                            let teleported = activate_native_map_teleport_item(
                                stream,
                                &config.client_profile,
                                &snapshot,
                                &database,
                                shared_world,
                                character.id,
                                world_map,
                                &mut player_position,
                                facing,
                                destination,
                            )?;
                            if teleported {
                                active_click_walk = None;
                                observed_visibility_epoch = shared_world.visibility_epoch();
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=use-item outcome=teleported server-id={} destination={destination:?} index={index}",
                                        outcome.server_id,
                                    ),
                                );
                            } else {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=use-item outcome=deferred-teleport-destination-blocked server-id={} destination={destination:?} index={index}",
                                        outcome.server_id,
                                    ),
                                );
                            }
                        } else if let Some(text) =
                            native_validated_map_item_text(world_map, &outcome)
                        {
                            let text_window = encode_native_otclient_read_only_text_window(
                                &config.client_profile,
                                0,
                                client_thing_id,
                                text,
                            )
                            .map_err(HostError::Protocol)?;
                            write_frame(stream, &text_window)?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=use-item outcome=read-only-text-window server-id={} text-bytes={} index={index}",
                                    outcome.server_id,
                                    text.len(),
                                ),
                            );
                        } else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=use-item outcome=validated server-id={} count={} action-id={:?} unique-id={:?} text={} charges={:?} index={index}",
                                    outcome.server_id,
                                    outcome.count,
                                    outcome.action_id,
                                    outcome.unique_id,
                                    outcome.has_text,
                                    outcome.charges,
                                ),
                            );
                        }
                    }
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item outcome=deferred-invalid-server-owned-map-item",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::UseItemEx {
                source_position,
                source_client_thing_id,
                source_stack_position,
                target_position,
                target_client_thing_id,
                target_stack_position,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-ex outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_ex_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    (
                        source_position,
                        source_client_thing_id,
                        source_stack_position,
                    ),
                    (
                        target_position,
                        target_client_thing_id,
                        target_stack_position,
                    ),
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-ex outcome=deferred-unmapped-or-ambiguous-client-thing-id",
                    );
                    continue;
                };
                match shared_world.validate_player_item_use_ex(world_map, intent) {
                    Ok(outcome) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=use-item-ex outcome=validated source-server-id={} source-count={} target-server-id={} target-count={}",
                            outcome.source.server_id,
                            outcome.source.count,
                            outcome.target.server_id,
                            outcome.target.count,
                        ),
                    ),
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-ex outcome=deferred-invalid-server-owned-map-item",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::UseItemOnCreature {
                source_position,
                source_client_thing_id,
                source_stack_position,
                target_creature_id,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-on-creature outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_creature_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    source_position,
                    source_client_thing_id,
                    source_stack_position,
                    target_creature_id,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-on-creature outcome=deferred-unmapped-or-ambiguous-client-thing-id",
                    );
                    continue;
                };
                match shared_world.validate_player_item_use_creature(world_map, intent) {
                    Ok(outcome) => {
                        // Declarative weapon use against a creature: adjacent melee for sword/
                        // club/axe declarations and runes; ranged distance shots with ammo
                        // consumption plus a 0x85 missile record for declared distance weapons
                        // (plan v49 slices 9-10).
                        if let Some(catalog) = config.declarative_weapon_catalog.as_deref() {
                            if let Some(definition) =
                                catalog.get(outcome.source.server_id)
                            {
                                if let PlayerItemUseCreatureTargetOutcome::Player {
                                    player_id: target_id,
                                    ..
                                } = outcome.target
                                {
                                    let is_distance = definition.distance_range.is_some();
                                    // Target tile for slice-11 feedback records (missile,
                                    // hit effect, animated damage number).
                                    let target_position_feedback = shared_world
                                        .lock()
                                        .ok()
                                        .and_then(|world| {
                                            world
                                                .player(target_id)
                                                .map(|target| target.position)
                                        });
                                    let has_ammo = if is_distance {
                                        let has = shared_world
                                            .lock()
                                            .ok()
                                            .and_then(|world| {
                                                world.player_equipment(character.id).ok().map(
                                                    |equipment| {
                                                        equipment
                                                            .item(EquipmentSlot::Ammo)
                                                            .is_some()
                                                    },
                                                )
                                            })
                                            .unwrap_or(false);
                                        if !has {
                                            native_diagnostic(
                                                config.extended_diagnostics,
                                                peer,
                                                "action=distance-shot outcome=deferred-no-ammo",
                                            );
                                            continue;
                                        }
                                        true
                                    } else {
                                        true
                                    };
                                    if !has_ammo {
                                        continue;
                                    }
                                    let event = if is_distance {
                                        definition.distance_shot_event(character.id, target_id)
                                    } else {
                                        definition.adjacent_melee_event(character.id, target_id)
                                    };
                                    let event = match event {
                                        Ok(event) => event,
                                        Err(error) => {
                                            native_diagnostic(
                                                config.extended_diagnostics,
                                                peer,
                                                &format!("action=rune-hit outcome=invalid-event error={error}"),
                                            );
                                            continue;
                                        }
                                    };
                                    match shared_world.apply_player_combat_event_with_death(
                                        event,
                                        world_map,
                                    ) {
                                        Ok((combat_outcome, _, _)) => {
                                            if is_distance {
                                                let ammo_consumed = shared_world
                                                    .lock()
                                                    .ok()
                                                    .and_then(|mut world| {
                                                        world
                                                            .consume_player_equipment_item_unit(
                                                                character.id,
                                                                EquipmentSlot::Ammo,
                                                            )
                                                            .ok()
                                                    })
                                                    .unwrap_or(false);
                                                if let Some(shot_effect) = definition.shot_effect
                                                {
                                                    let target_position = shared_world
                                                        .lock()
                                                        .ok()
                                                        .and_then(|world| {
                                                            world
                                                                .player(target_id)
                                                                .map(|target| target.position)
                                                        });
                                                    if let Some(target_position) = target_position
                                                    {
                                                        let missile =
                                                            encode_native_otclient_distance_effect(
                                                                &config.client_profile,
                                                                native_position(player_position),
                                                                native_position(target_position),
                                                                shot_effect,
                                                            )
                                                            .map_err(HostError::Protocol)?;
                                                        write_frame(stream, &missile)?;
                                                    }                                                }
                                                native_diagnostic(
                                                    config.extended_diagnostics,
                                                    peer,
                                                    &format!(
                                                        "action=distance-shot item={} target={} damage={} defeated={} ammo-consumed={}",
                                                        outcome.source.server_id,
                                                        target_id,
                                                        combat_outcome.mitigated_damage,
                                                        combat_outcome.damage.defeated,
                                                        ammo_consumed
                                                    ),
                                                );
                                            } else {
                                                // Plan v49 slice 10: each fired rune consumes one
                                                // charge from its owned container stack.
                                                let charge_consumed = consume_declared_rune_charge(
                                                    shared_world,
                                                    &mut database,
                                                    character.id,
                                                    source_position,
                                                    source_stack_position,
                                                    &config.client_profile,
                                                    config.item_presentation_catalog.as_deref(),
                                                    &mut sent_container_windows,
                                                );
                                                native_diagnostic(
                                                    config.extended_diagnostics,
                                                    peer,
                                                    &format!(
                                                        "action=rune-hit item={} target={} damage={} defeated={} charge-consumed={}",
                                                        outcome.source.server_id,
                                                        target_id,
                                                        combat_outcome.mitigated_damage,
                                                        combat_outcome.damage.defeated,
                                                        charge_consumed
                                                    ),
                                                );
                                            }
                                            // Plan v49 slice 11 combat feedback: declared hit
                                            // effect, optional animated damage number, and the
                                            // attacker's white-skull award record.
                                            if let (Some(target_position), Some(hit_effect)) = (
                                                target_position_feedback,
                                                definition.hit_effect,
                                            ) {
                                                let effect_frame =
                                                    encode_native_otclient_magic_effect(
                                                        &config.client_profile,
                                                        native_position(target_position),
                                                        hit_effect,
                                                    )
                                                    .map_err(HostError::Protocol)?;
                                                write_frame(stream, &effect_frame)?;
                                            }
                                            if config.animated_damage_text_enabled
                                                && combat_outcome.damage.applied_damage > 0
                                            {
                                                if let Some(target_position) =
                                                    target_position_feedback
                                                {
                                                    let animated =
                                                        encode_native_otclient_animated_text(
                                                            &config.client_profile,
                                                            native_position(target_position),
                                                            180,
                                                            &combat_outcome
                                                                .damage
                                                                .applied_damage
                                                                .to_string(),
                                                        )
                                                        .map_err(HostError::Protocol)?;
                                                    write_frame(stream, &animated)?;
                                                }
                                            }
                                            if shared_world
                                                .lock()
                                                .map(|world| {
                                                    world.player_has_white_skull(character.id)
                                                })
                                                .unwrap_or(false)
                                                && !observed_white_skull_sent
                                            {
                                                observed_white_skull_sent = true;
                                                if let Ok(native_id) =
                                                    native_player_id(character.id)
                                                {
                                                    let skull = encode_native_otclient_creature_skull(
                                                        &config.client_profile,
                                                        native_id,
                                                        forgotten_protocol::NATIVE_OTCLIENT_SKULL_WHITE,
                                                    )
                                                    .map_err(HostError::Protocol)?;
                                                    write_frame(stream, &skull)?;
                                                }
                                            }
                                        }
                                        Err(HostError::Core(_)) => {}
                                        Err(error) => return Err(error),
                                    }
                                }
                            }
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=use-item-on-creature outcome=validated source-server-id={} source-count={} target={:?}",
                                outcome.source.server_id, outcome.source.count, outcome.target
                            ),
                        );
                    }
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-on-creature outcome=deferred-invalid-server-owned-item-or-creature",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::RotateItem {
                position,
                client_thing_id,
                stack_position,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=rotate-item outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    position,
                    client_thing_id,
                    stack_position,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=rotate-item outcome=deferred-unmapped-or-ambiguous-client-thing-id",
                    );
                    continue;
                };
                match shared_world.validate_player_item_use(world_map, intent) {
                    Ok(outcome) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=rotate-item outcome=validated server-id={} count={}",
                            outcome.server_id, outcome.count
                        ),
                    ),
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=rotate-item outcome=deferred-invalid-server-owned-map-item",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::RequestOutfit => {
                // A missing or misconfigured chooser range must degrade to a client-visible
                // rejection instead of tearing down the session.
                match encode_native_otclient_choose_outfit(
                    &config.client_profile,
                    player_outfit,
                    empty_world.outfit_first_look_type,
                    empty_world.outfit_last_look_type,
                ) {
                    Ok(outfit_window) => {
                        write_frame(stream, &outfit_window)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=choose-outfit opcode=0xc8 bytes={} look-type={}",
                                outfit_window.0.len(),
                                player_outfit.look_type
                            ),
                        );
                    }
                    Err(error) => {
                        let rejection = encode_native_otclient_failure_message(
                            &config.client_profile,
                            "The outfit window is not configured on this server.",
                        )
                        .map_err(HostError::Protocol)?;
                        write_frame(stream, &rejection)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!("action=request-outfit outcome=rejected reason={error}"),
                        );
                    }
                }
            }
            NativeOtClientGameAction::LeaveChannel(channel_id) => {
                let removed = open_public_channel_ids.remove(&channel_id);
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=leave-channel channel-id={channel_id} outcome=session-local-removed-{removed}"
                    ),
                );
            }
            NativeOtClientGameAction::JoinChannel(channel_id) => {
                let Some(channel) = native_configured_public_channel(
                    config.public_channel_catalog.as_deref(),
                    channel_id,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=join-channel outcome=deferred-unknown-or-unconfigured-channel",
                    );
                    continue;
                };
                open_public_channel_ids.insert(channel.id);
                let open_channel =
                    encode_native_otclient_open_public_channel(&config.client_profile, &channel)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &open_channel)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=open-public-channel opcode=0xac channel-id={} bytes={}",
                        channel.id,
                        open_channel.0.len(),
                    ),
                );
            }
            NativeOtClientGameAction::RequestChannels => {
                let mut entries =
                    native_classic_channel_list_entries(config.public_channel_catalog.as_deref());
                // Plan v49 slice 19: guild members see the reserved guild channel (0x00F1).
                let guild_context = native_guild_channel_context(&database, character.id);
                if let Some((channel, _)) = &guild_context {
                    entries.push(channel.clone());
                }
                let channels =
                    encode_native_otclient_channel_list(&config.client_profile, &entries)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &channels)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=channel-list opcode=0xab entries={} bytes={}",
                        entries.len(),
                        channels.0.len(),
                    ),
                );
            }
            NativeOtClientGameAction::RequestQuestLog => {
                // With an operator quest catalog, only started persisted quests appear, resolved
                // through catalog display names; without one the parser-shaped empty response
                // keeps prior behavior.
                let quest_entries = match config.quest_catalog.as_deref() {
                    Some(catalog) if !catalog.is_empty() => {
                        let mut entries = Vec::new();
                        for (quest_id, completed) in database
                            .player_quests(character.id)
                            .map_err(HostError::Persistence)?
                        {
                            if let Some(definition) = catalog.get(quest_id) {
                                let _ = completed;
                                entries.push((quest_id, definition.name.clone()));
                            }
                        }
                        entries
                    }
                    _ => Vec::new(),
                };
                let quest_log = if quest_entries.is_empty() {
                    encode_native_otclient_empty_quest_log(&config.client_profile)
                        .map_err(HostError::Protocol)?
                } else {
                    encode_native_otclient_quest_list(&config.client_profile, &quest_entries)
                        .map_err(HostError::Protocol)?
                };
                write_frame(stream, &quest_log)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=quest-log opcode=0xf0 entries={} bytes={}",
                        quest_entries.len(),
                        quest_log.0.len()
                    ),
                );
            }
            NativeOtClientGameAction::RequestQuestLine { quest_id } => {
                // The quest line window opens for started persisted quests with declared
                // missions; unknown or not-started quests receive an empty mission list.
                let missions = match config.quest_catalog.as_deref() {
                    Some(catalog) => {
                        let started = database
                            .player_quests(character.id)
                            .map_err(HostError::Persistence)?
                            .iter()
                            .any(|(started_id, _)| *started_id == quest_id);
                        match catalog.get(quest_id).filter(|_| started) {
                            Some(definition) => definition.missions.clone(),
                            None => Vec::new(),
                        }
                    }
                    None => Vec::new(),
                };
                let line_frame =
                    encode_native_otclient_quest_line(&config.client_profile, quest_id, &missions)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &line_frame)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=quest-line opcode=0xf1 quest-id={quest_id} missions={} bytes={}",
                        missions.len(),
                        line_frame.0.len()
                    ),
                );
            }
            NativeOtClientGameAction::ChangeOutfit(requested_outfit) => {
                let accepted = native_classic_outfit_is_allowed(
                    requested_outfit,
                    empty_world.outfit_first_look_type,
                    empty_world.outfit_last_look_type,
                );
                if accepted {
                    database.update_player_outfit(
                        character.id,
                        PlayerOutfit {
                            look_type: requested_outfit.look_type,
                            head: requested_outfit.head,
                            body: requested_outfit.body,
                            legs: requested_outfit.legs,
                            feet: requested_outfit.feet,
                        },
                    )?;
                    player_outfit = requested_outfit;
                    shared_world.update_player_outfit(character.id, player_outfit)?;
                    observed_visibility_epoch = shared_world.visibility_epoch();
                }
                // The applied-outfit echo must never fail the session; a rejected or
                // unencodable change degrades to a client-visible failure text.
                match encode_native_otclient_creature_outfit(
                    &config.client_profile,
                    snapshot.player_id,
                    player_outfit,
                ) {
                    Ok(applied_outfit) => {
                        write_frame(stream, &applied_outfit)?;
                    }
                    Err(error) => {
                        let rejection = encode_native_otclient_failure_message(
                            &config.client_profile,
                            "The selected outfit could not be applied on this server.",
                        )
                        .map_err(HostError::Protocol)?;
                        write_frame(stream, &rejection)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!("action=change-outfit outcome=echo-rejected reason={error}"),
                        );
                    }
                }
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=creature-outfit opcode=0x8e accepted={} look-type={}",
                        accepted, player_outfit.look_type
                    ),
                );
            }
            NativeOtClientGameAction::LookMap {
                position,
                thing_id,
                stack_position,
            } => {
                let equipment = shared_world.player_equipment(character.id)?;
                if let Some((slot, item)) = native_classic_equipment_look_item(
                    config.item_presentation_catalog.as_deref(),
                    &equipment,
                    position,
                    thing_id,
                    stack_position,
                ) {
                    let response = encode_native_otclient_look_message(
                        &config.client_profile,
                        &native_equipment_item_inspection_message(
                            slot,
                            &item,
                            config.item_name_by_server_id.as_deref(),
                            config.item_weight_by_server_id.as_deref(),
                            config.stackable_item_server_ids.as_deref(),
                        ),
                    )
                    .map_err(HostError::Protocol)?;
                    write_frame(stream, &response)?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=look-map outcome=equipment-slot-inspection slot={} item-id={}",
                            slot.code(),
                            item.server_id
                        ),
                    );
                    continue;
                }
                let containers = shared_world.player_containers(character.id)?;
                if let Some((container_id, item)) = native_classic_container_look_item(
                    config.item_presentation_catalog.as_deref(),
                    &containers,
                    &closed_container_ids,
                    position,
                    thing_id,
                    stack_position,
                ) {
                    let response = encode_native_otclient_look_message(
                        &config.client_profile,
                        &native_container_item_inspection_message(
                            container_id,
                            &item,
                            config.item_name_by_server_id.as_deref(),
                            config.item_weight_by_server_id.as_deref(),
                            config.stackable_item_server_ids.as_deref(),
                        ),
                    )
                    .map_err(HostError::Protocol)?;
                    write_frame(stream, &response)?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=look-map outcome=container-item-inspection container-id={} item-id={}",
                            container_id, item.server_id
                        ),
                    );
                    continue;
                }
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=look-map outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    position,
                    thing_id,
                    stack_position,
                ) else {
                    // Universal Look fallback: TFS always answers a look. Bare ground and
                    // unmapped decorations resolve through the imported item name when
                    // possible; raw numeric ids are never echoed (live-test regression A2).
                    let message = native_ground_look_message(
                        world_map,
                        Position {
                            x: position.x,
                            y: position.y,
                            z: position.z,
                        },
                        config.item_name_by_server_id.as_deref(),
                    );
                    let response =
                        encode_native_otclient_look_message(&config.client_profile, &message)
                            .map_err(HostError::Protocol)?;
                    write_frame(stream, &response)?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=look-map outcome=generic-fallback",
                    );
                    continue;
                };
                let item = match shared_world.validate_player_item_use(world_map, intent) {
                    Ok(item) => item,
                    Err(HostError::Core(_)) => {
                        // Tile exists but the item reference did not resolve (moved, out of
                        // range, or stale stackpos). Answer generically like TFS does.
                        let message = native_ground_look_message(
                            world_map,
                            Position {
                                x: position.x,
                                y: position.y,
                                z: position.z,
                            },
                            config.item_name_by_server_id.as_deref(),
                        );
                        let response =
                            encode_native_otclient_look_message(&config.client_profile, &message)
                                .map_err(HostError::Protocol)?;
                        write_frame(stream, &response)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=look-map outcome=generic-fallback-stale-reference",
                        );
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let message = native_map_item_inspection_message(
                    world_map,
                    &item,
                    config.item_name_by_server_id.as_deref(),
                    config.item_weight_by_server_id.as_deref(),
                    config.stackable_item_server_ids.as_deref(),
                );
                let response =
                    encode_native_otclient_look_message(&config.client_profile, &message)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &response)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=look-message opcode=0xb4 class=0x16 bytes={} action=look-map server-id={} count={}",
                        response.0.len(), item.server_id, item.count
                    ),
                );
            }
            NativeOtClientGameAction::LookCreature { creature_id } => {
                let Some(message) =
                    native_creature_inspection_message(shared_world, character.id, creature_id)?
                else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=look-creature outcome=deferred-unavailable-or-outside-viewport",
                    );
                    continue;
                };
                let response =
                    encode_native_otclient_look_message(&config.client_profile, &message)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &response)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=look-message opcode=0xb4 class=0x16 bytes={} action=look-creature native-id={creature_id}",
                        response.0.len()
                    ),
                );
            }
            NativeOtClientGameAction::IgnoredInteraction(opcode) => {
                if config.extended_diagnostics {
                    eprintln!("> Native OTCv8 compatibility action ignored opcode=0x{opcode:02x}");
                }
            }
            NativeOtClientGameAction::RequestTrade {
                position,
                client_thing_id,
                stack_position,
                target_creature_id,
            } => {
                handle_native_player_trade_request(
                    stream,
                    &config.client_profile,
                    shared_world,
                    character.id,
                    position,
                    client_thing_id,
                    stack_position,
                    target_creature_id,
                    config.item_presentation_catalog.as_deref(),
                    config.stackable_item_server_ids.as_deref(),
                )?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    "action=request-trade outcome=processed",
                );
            }
            NativeOtClientGameAction::AcceptTrade => {
                handle_native_trade_accept(
                    stream,
                    &config.client_profile,
                    shared_world,
                    &mut database,
                    character.id,
                    config.item_presentation_catalog.as_deref(),
                    config.stackable_item_server_ids.as_deref(),
                )?;
                native_diagnostic(config.extended_diagnostics, peer, "action=accept-trade");
            }
            NativeOtClientGameAction::RejectTrade => {
                handle_native_trade_reject(shared_world, character.id)?;
                native_diagnostic(config.extended_diagnostics, peer, "action=reject-trade");
            }
            NativeOtClientGameAction::NpcTradeClose => {
                native_diagnostic(config.extended_diagnostics, peer, "action=npc-trade-close");
            }
            NativeOtClientGameAction::NpcBuy {
                client_thing_id,
                subtype: _,
                amount,
                _ignore_capacity: _,
                _buy_with_backpack: _,
            } => {
                // Buy flows through the existing declarative shop keyword path by mapping the
                // client thing id back to a server item via the presentation catalog.
                let server_id = config
                    .item_presentation_catalog
                    .as_ref()
                    .and_then(|catalog| {
                        catalog.unique_server_id_for_client_thing_id(client_thing_id)
                    });
                let Some(server_id) = server_id else {
                    let failure = encode_native_otclient_failure_message(
                        &config.client_profile,
                        "You cannot buy this item.",
                    )
                    .map_err(HostError::Protocol)?;
                    write_frame(stream, &failure)?;
                    continue;
                };
                let message = handle_native_shop_keyword(
                    shared_world,
                    &mut database,
                    character.id,
                    &format!("buy {server_id} {amount}"),
                    config
                        .shop_catalog
                        .as_deref()
                        .unwrap_or(&DeclarativeShopCatalog::default()),
                )?;
                let reply_frame = encode_native_otclient_status_message(
                    &config.client_profile,
                    &message.unwrap_or_else(|| "Nothing to buy here.".into()),
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &reply_frame)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!("action=npc-buy item={server_id} amount={amount}"),
                );
            }
            NativeOtClientGameAction::NpcSell {
                client_thing_id,
                subtype: _,
                amount,
                _ignore_equipped: _,
            } => {
                let server_id = config
                    .item_presentation_catalog
                    .as_ref()
                    .and_then(|catalog| {
                        catalog.unique_server_id_for_client_thing_id(client_thing_id)
                    });
                let Some(server_id) = server_id else {
                    let failure = encode_native_otclient_failure_message(
                        &config.client_profile,
                        "You cannot sell this item.",
                    )
                    .map_err(HostError::Protocol)?;
                    write_frame(stream, &failure)?;
                    continue;
                };
                let message = handle_native_shop_keyword(
                    shared_world,
                    &mut database,
                    character.id,
                    &format!("sell {server_id} {amount}"),
                    config
                        .shop_catalog
                        .as_deref()
                        .unwrap_or(&DeclarativeShopCatalog::default()),
                )?;
                let reply_frame = encode_native_otclient_status_message(
                    &config.client_profile,
                    &message.unwrap_or_else(|| "Nothing to sell here.".into()),
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &reply_frame)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!("action=npc-sell item={server_id} amount={amount}"),
                );
            }
            NativeOtClientGameAction::SelectTarget(native_selected_id) => {
                let outcome = apply_native_player_interaction(
                    shared_world,
                    character.id,
                    native_selected_id,
                    NativePlayerInteractionKind::Target,
                    config.extended_diagnostics,
                )?;
                if native_selected_id == 0
                    || matches!(outcome, NativePlayerInteractionOutcome::Rejected)
                {
                    let clear_target = encode_native_otclient_clear_target(&config.client_profile)
                        .map_err(HostError::Protocol)?;
                    write_frame(stream, &clear_target)?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        if native_selected_id == 0 {
                            "outbound=clear-target opcode=0xa3 fields=none reason=explicit-target-clear"
                        } else {
                            "outbound=clear-target opcode=0xa3 fields=none reason=rejected-target"
                        },
                    );
                }
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
            NativeOtClientGameAction::PartyInvite(native_target_id) => {
                let Some(invitee_id) = native_player_id_to_character_id(native_target_id) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=party-invite outcome=rejected-invalid-native-player-id",
                    );
                    continue;
                };
                let outcome = shared_world.invite_to_party(character.id, invitee_id);
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    if outcome.is_ok() {
                        "action=party-invite outcome=authoritative-invitation-created"
                    } else {
                        "action=party-invite outcome=rejected-core-invariant"
                    },
                );
            }
            NativeOtClientGameAction::PartyJoin(native_target_id) => {
                let Some(leader_id) = native_player_id_to_character_id(native_target_id) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=party-join outcome=rejected-invalid-native-player-id",
                    );
                    continue;
                };
                let outcome = shared_world.accept_party_invitation(character.id, leader_id);
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    if outcome.is_ok() {
                        "action=party-join outcome=authoritative-membership-created"
                    } else {
                        "action=party-join outcome=rejected-core-invariant"
                    },
                );
            }
            NativeOtClientGameAction::PartyRevokeInvitation(native_target_id) => {
                let Some(invitee_id) = native_player_id_to_character_id(native_target_id) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=party-revoke-invitation outcome=rejected-invalid-native-player-id",
                    );
                    continue;
                };
                let outcome = shared_world.revoke_party_invitation(character.id, invitee_id);
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    if outcome.is_ok() {
                        "action=party-revoke-invitation outcome=authoritative-invitation-removed"
                    } else {
                        "action=party-revoke-invitation outcome=rejected-core-invariant"
                    },
                );
            }
            NativeOtClientGameAction::PartyPassLeadership(native_target_id) => {
                let Some(new_leader_id) = native_player_id_to_character_id(native_target_id) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=party-pass-leadership outcome=rejected-invalid-native-player-id",
                    );
                    continue;
                };
                let outcome = shared_world.transfer_party_leadership(character.id, new_leader_id);
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    if outcome.is_ok() {
                        "action=party-pass-leadership outcome=authoritative-leadership-transferred"
                    } else {
                        "action=party-pass-leadership outcome=rejected-core-invariant"
                    },
                );
            }
            NativeOtClientGameAction::PartyLeave => {
                let outcome = shared_world.leave_party(character.id);
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    if outcome.is_ok() {
                        "action=party-leave outcome=authoritative-membership-removed"
                    } else {
                        "action=party-leave outcome=rejected-core-invariant"
                    },
                );
            }
            NativeOtClientGameAction::PartySharedExperience(requested) => {
                let outcome = config.party_shared_experience_rules.map_or_else(
                    || {
                        Err(HostError::InvalidConfiguration(
                            "party shared experience is disabled by configuration".into(),
                        ))
                    },
                    |rules| {
                        shared_world.set_party_shared_experience_requested(
                            character.id,
                            requested,
                            rules,
                        )
                    },
                );
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    if outcome.is_ok() {
                        "action=party-shared-experience outcome=authoritative-request-updated"
                    } else {
                        "action=party-shared-experience outcome=rejected-disabled-or-core-invariant"
                    },
                );
            }
            NativeOtClientGameAction::CancelAttackAndFollow => {
                cancel_native_player_attack_and_follow(shared_world, character.id)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    "action=cancel-attack-and-follow outcome=authoritative-intents-cleared",
                );
            }
            NativeOtClientGameAction::Talk(request) => {
                // Bounded fixed-window flood control over every routed talk record. Suppressed
                // messages emit no client feedback and never reach the shared chat queue.
                let now = Instant::now();
                while talk_windows
                    .front()
                    .is_some_and(|sent| now.saturating_duration_since(*sent) >= CHAT_FLOOD_WINDOW)
                {
                    talk_windows.pop_front();
                }
                if talk_windows.len() >= CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=talk outcome=flood-suppressed",
                    );
                    continue;
                }
                talk_windows.push_back(now);
                // Plan v49 slice 17: muted accounts cannot route talk records. GMs are exempt
                // through their persisted tier so moderation stays possible while muted.
                let speaker_gm_level = database
                    .player_gm_level(character.id)
                    .map_err(HostError::Persistence)?;
                if speaker_gm_level == 0 {
                    if let Some(remaining) = database
                        .account_mute_remaining_seconds(account_id)
                        .map_err(HostError::Persistence)?
                    {
                        let muted_notice = encode_native_otclient_status_message(
                            &config.client_profile,
                            &format!("You are muted for {remaining} more seconds."),
                        )
                        .map_err(HostError::Protocol)?;
                        write_frame(stream, &muted_notice)?;
                        continue;
                    }
                }
                // Gamemaster talkactions ("/give", "/tp", "/spawn", ...) run before every other
                // keyword router and are only available to characters with a persisted GM tier.
                if request.mode == NATIVE_OTCLIENT_MESSAGE_SAY
                    && request.channel_id.is_none()
                    && request.recipient.is_none()
                {
                    let gm_level = database
                        .player_gm_level(character.id)
                        .map_err(HostError::Persistence)?;
                    if gm_level > 0 {
                        if let Some(reply) = handle_native_gm_talkaction(
                            shared_world,
                            &mut database,
                            character.id,
                            &request.message,
                            gm_level,
                            config.quest_catalog.as_deref(),
                        )? {
                            let reply_frame = encode_native_otclient_status_message(
                                &config.client_profile,
                                &reply,
                            )
                            .map_err(HostError::Protocol)?;
                            write_frame(stream, &reply_frame)?;
                            // GM talkactions mutate authoritative state (summons, teleports,
                            // deliveries), so this session resends its full viewport from live
                            // shared state instead of silently adopting the bumped visibility
                            // epoch â€” that swallow left summons invisible until relog
                            // (live-test regression A1). Other sessions refresh through their
                            // own epoch comparison.
                            shared_world.mark_visibility_changed();
                            let mut refreshed_snapshot = snapshot.clone();
                            refreshed_snapshot.player_position = native_position(player_position);
                            refreshed_snapshot.player_direction = facing.protocol_direction();
                            let refreshed_viewport = encode_shared_native_world_viewport(
                                &config.client_profile,
                                &refreshed_snapshot,
                                world_map.as_ref(),
                                shared_world,
                                character.id,
                            )?;
                            let refreshed_static_spawns = shared_world.active_static_spawns()?;
                            let refreshed_static_health_frames =
                                native_static_creature_health_frames(
                                    &config.client_profile,
                                    &refreshed_static_spawns,
                                )?;
                            write_frame(stream, &refreshed_viewport)?;
                            for frame in &refreshed_static_health_frames {
                                write_frame(stream, frame)?;
                            }
                            observed_visibility_epoch = shared_world.visibility_epoch();
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=talk outcome=gm-talkaction reply-bytes={}",
                                    reply.len()
                                ),
                            );
                            continue;
                        }
                    }
                }
                if request.mode == NATIVE_OTCLIENT_MESSAGE_SAY
                    && request.channel_id.is_none()
                    && request.recipient.is_none()
                {
                    // Spell invocation resolves either through the operator command or an exact
                    // declared Say keyword; both consume mana/cooldowns identically and may apply
                    // one bounded declared-damage hit to the caster's selected adjacent target.
                    let catalog = config.declarative_spell_catalog.as_deref();
                    let invoked_spell = match native_declarative_spell_command_id(&request.message)
                    {
                        Some(spell_id) => catalog.and_then(|catalog| {
                            catalog
                                .get(spell_id)
                                .map(|definition| (catalog, definition))
                        }),
                        None => catalog.and_then(|catalog| {
                            catalog
                                .by_words(&request.message)
                                .map(|definition| (catalog, definition))
                        }),
                    };
                    if let Some((catalog, definition)) = invoked_spell {
                        if config.progression_rules.is_none() {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=declarative-spell outcome=deferred-missing-catalog-or-progression-rules",
                            );
                            continue;
                        }
                        let Some(progression_rules) = config.progression_rules.as_deref() else {
                            continue;
                        };
                        match apply_and_persist_native_declarative_spell_cast(
                            &mut database,
                            shared_world,
                            character.id,
                            definition.spell_id,
                            catalog,
                            progression_rules,
                            config.magic_rate,
                        ) {
                            Ok((cast, magic)) => {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=declarative-spell outcome=accepted spell-id={} mana-spent={} remaining-mana={} awarded-magic-mana={} magic-level={} gained-levels={}",
                                        cast.spell_id,
                                        cast.mana_spent,
                                        cast.remaining_mana,
                                        u64::from(cast.mana_spent)
                                            .saturating_mul(u64::from(config.magic_rate)),
                                        magic.magic_level,
                                        magic.gained_levels,
                                    ),
                                );
                                // One bounded declared-damage hit on the selected living
                                // adjacent static target, sharing the per-player combat cooldown.
                                if let Some(damage) = definition.damage.filter(|_| !observed_dead) {
                                    let target_id = shared_world
                                        .player_interaction_intent(character.id)
                                        .ok()
                                        .and_then(|intent| intent.target_static_creature_id);
                                    if let Some(target_id) = target_id {
                                        match shared_world.apply_static_creature_melee_damage(
                                            character.id,
                                            target_id,
                                            damage,
                                        ) {
                                            Ok(outcome) if outcome.applied_damage > 0 => {
                                                persist_static_creature_runtime_to_open_database(
                                                    shared_world,
                                                    &mut database,
                                                )?;
                                                let health_update =
                                                    encode_native_otclient_creature_health(
                                                        &config.client_profile,
                                                        outcome.target_id,
                                                        u16::from(outcome.remaining_health_percent),
                                                        100,
                                                    )
                                                    .map_err(HostError::Protocol)?;
                                                write_frame(stream, &health_update)?;
                                                if outcome.deactivated {
                                                    for frame in
                                                        native_selected_player_death_target_frames(
                                                            &config.client_profile,
                                                            true,
                                                        )
                                                        .map_err(HostError::Protocol)?
                                                    {
                                                        write_frame(stream, &frame)?;
                                                    }
                                                }
                                                observed_visibility_epoch =
                                                    shared_world.visibility_epoch();
                                                native_diagnostic(
                                                    config.extended_diagnostics,
                                                    peer,
                                                    &format!(
                                                        "combat=declarative-spell-damage target={} damage={} health-percent={} deactivated={}",
                                                        outcome.target_id,
                                                        outcome.applied_damage,
                                                        outcome.remaining_health_percent,
                                                        outcome.deactivated,
                                                    ),
                                                );
                                            }
                                            Ok(_) => {}
                                            Err(HostError::Core(_)) => {}
                                            Err(error) => return Err(error),
                                        }
                                    }
                                }
                            }
                            Err(HostError::Core(
                                forgotten_core::CoreError::InsufficientMana { .. }
                                | forgotten_core::CoreError::SpellCooldownActive { .. }
                                | forgotten_core::CoreError::PlayerIsDead(_),
                            ))
                            | Err(HostError::InvalidConfiguration(_)) => {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "action=declarative-spell outcome=rejected-authoritative-state-or-catalog",
                                );
                            }
                            Err(error) => return Err(error),
                        }
                        continue;
                    }
                }
                // Bounded NPC banking keywords ("balance", "deposit all", "withdraw <n>") are
                // only handled for authenticated living players; unmatched messages fall through
                // to ordinary chat delivery.
                if request.mode == NATIVE_OTCLIENT_MESSAGE_SAY
                    && request.channel_id.is_none()
                    && request.recipient.is_none()
                    && !observed_dead
                {
                    if let Some(reply) = handle_native_bank_keyword(
                        shared_world,
                        &mut database,
                        character.id,
                        &request.message,
                    )? {
                        let reply_frame =
                            encode_native_otclient_status_message(&config.client_profile, &reply)
                                .map_err(HostError::Protocol)?;
                        write_frame(stream, &reply_frame)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "action=talk outcome=bank-keyword reply-bytes={}",
                                reply.len()
                            ),
                        );
                        continue;
                    }
                    // Bounded NPC shop keywords ("buy <id> <count>" / "sell <id> <count>") are
                    // only handled near an active NPC whose declared shop matches.
                    if let Some(shop_catalog) = config.shop_catalog.as_deref() {
                        if let Some(reply) = handle_native_shop_keyword(
                            shared_world,
                            &mut database,
                            character.id,
                            &request.message,
                            shop_catalog,
                        )? {
                            let reply_frame = encode_native_otclient_status_message(
                                &config.client_profile,
                                &reply,
                            )
                            .map_err(HostError::Protocol)?;
                            write_frame(stream, &reply_frame)?;
                            shared_world.mark_visibility_changed();
                            observed_visibility_epoch = shared_world.visibility_epoch();
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=talk outcome=shop-keyword reply-bytes={}",
                                    reply.len()
                                ),
                            );
                            continue;
                        }
                    }
                }
                let recipient_count = if request.mode == 5 {
                    let Some(recipient_name) = request.recipient.as_deref() else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=talk outcome=deferred-missing-private-recipient",
                        );
                        continue;
                    };
                    shared_world.send_private_chat(
                        character.id,
                        recipient_name,
                        &request.message,
                    )?
                } else if request.mode == 7 {
                    let Some(channel_id) = request.channel_id else {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=talk outcome=deferred-missing-channel-id",
                        );
                        continue;
                    };
                    if !open_public_channel_ids.contains(&channel_id)
                        || native_configured_public_channel(
                            config.public_channel_catalog.as_deref(),
                            channel_id,
                        )
                        .is_none()
                    {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=talk outcome=deferred-unopened-or-unconfigured-channel",
                        );
                        continue;
                    }
                    shared_world.broadcast_configured_public_channel_chat(
                        character.id,
                        channel_id,
                        &request.message,
                    )?
                } else if request.mode == 2 {
                    shared_world.broadcast_whisper_chat(character.id, &request.message)?
                } else if request.mode == 3 {
                    shared_world.broadcast_yell_chat(character.id, &request.message)?
                } else if request.channel_id == Some(NATIVE_GUILD_CHAT_CHANNEL_ID) {
                    // Guild chat: deliver to every online member of the sender's guild. The
                    // open-channel gate above does not apply because the guild channel is
                    // implicit membership from persisted rows, not a joined public channel.
                    let membership = database
                        .guild_membership(character.id)
                        .map_err(HostError::Persistence)?;
                    match membership {
                        Some(record) => {
                            let member_ids = database
                                .guild_member_ids(record.guild_id)
                                .map_err(HostError::Persistence)?;
                            shared_world.broadcast_guild_chat(
                                character.id,
                                &request.message,
                                &member_ids,
                            )?
                        }
                        None => {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "action=talk outcome=guild-chat-no-membership",
                            );
                            0
                        }
                    }
                } else if request.channel_id.is_some() || request.recipient.is_some() {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=talk outcome=deferred-unsupported-channel-mode",
                    );
                    continue;
                } else if request.mode == NATIVE_OTCLIENT_MESSAGE_SAY {
                    shared_world.broadcast_public_chat(character.id, &request.message)?
                } else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=talk outcome=deferred-unsupported-mode",
                    );
                    continue;
                };
                if config.extended_diagnostics {
                    eprintln!(
                        "> Native OTCv8 chat received mode={} bytes={} recipients={recipient_count}",
                        request.mode,
                        request.message.len()
                    );
                }
                drain_shared_public_chat(
                    stream,
                    &config.client_profile,
                    &chat_events,
                    &open_public_channel_ids,
                    config.extended_diagnostics,
                    peer,
                )?;
                if request.mode == NATIVE_OTCLIENT_MESSAGE_SAY
                    && request.channel_id.is_none()
                    && request.recipient.is_none()
                {
                    if let Some(catalog) = config.declarative_npc_dialogue_catalog.as_deref() {
                        if let Some((npc_id, npc_name, npc_position, text)) =
                            resolve_native_static_npc_dialogue(
                                shared_world,
                                character.id,
                                catalog,
                                &request.message,
                            )?
                        {
                            let record = encode_native_otclient_public_say(
                                &config.client_profile,
                                &npc_name,
                                npc_position,
                                &text,
                            )
                            .map_err(HostError::Protocol)?;
                            write_frame(stream, &record)?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "outbound=npc-dialogue opcode=0xaa mode=1 npc-id={} text-bytes={}",
                                    npc_id,
                                    text.len()
                                ),
                            );
                            // A greeting near a shop NPC also opens the classic shop windows
                            // (0x7A catalog + 0x7B player goods) when a declarative shop and
                            // presentation mapping exist for it.
                            if let Some(shop_catalog) = config.shop_catalog.as_deref() {
                                let _ = deliver_native_npc_shop_windows(
                                    stream,
                                    &config.client_profile,
                                    shared_world,
                                    &database,
                                    character.id,
                                    &npc_name,
                                    shop_catalog,
                                    config.item_presentation_catalog.as_deref(),
                                    config.stackable_item_server_ids.as_deref(),
                                    config.item_weight_by_server_id.as_deref(),
                                    config.item_name_by_server_id.as_deref(),
                                )?;
                            }
                        }
                    }
                }
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
                shared_world.update_player_facing(character.id, facing)?;
                observed_visibility_epoch = shared_world.visibility_epoch();
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
                    let equipment = shared_world.player_equipment(character.id)?;
                    let effective_speed = native_hasted_speed(
                        native_effective_player_speed(
                            snapshot.player_speed,
                            &equipment,
                            config.item_speed_bonus_by_server_id.as_deref(),
                        ),
                        shared_world.player_speed_bonus_percent(character.id),
                    );
                    let step_delay =
                        native_autowalk_step_delay(effective_speed, snapshot.server_beat);
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
                        let Some(direction) = task.queued_steps.pop_front() else {
                            continue;
                        };
                        if move_native_map_player(
                            stream,
                            &config.client_profile,
                            &snapshot,
                            &database,
                            shared_world,
                            character.id,
                            world_map.as_ref(),
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
                    world_map.as_ref(),
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
                    world_map.as_ref(),
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
