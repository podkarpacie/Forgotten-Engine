//! Live sandboxed TFS action routing for validated map-item uses. When a used item carries
//! an action or unique id matching a registered singleton selector (or any item id with a
//! registered item selector), the match dispatches through the resource-capped action
//! dispatcher before generic handling; matched records are consumed so default behavior does
//! not also run. Effect application mirrors the talkaction intents (subject-relative validated
//! intents applied against authoritative state) with action-flavored diagnostics; a future
//! dedup pass can unify the two once both paths have live coverage.

use super::*;
use forgotten_config::resolve_action_callback;
use forgotten_scripting::{
    SandboxedLuaCallbackDispatchState, SandboxedLuaCallbackDispatcher, SandboxedLuaCallbackInput,
    SandboxedLuaEffect, SandboxedLuaPosition,
};

/// Routes one validated map-item use through registered action scripts. Returns `true` when a
/// registry entry matched (effects applied or script error consumed with a diagnostic, generic
/// handling skipped); returns `false` when nothing matched so generic handling proceeds.
/// Resolution is first-match in registry document order across singleton and range selectors
/// via [`resolve_action_callback`] — the same resolver the CLI `dispatch-action` verb proves,
/// so live routing and the CLI can never disagree on precedence.
///
/// Takes explicit session pieces rather than the shared context because callers hold a live
/// shared borrow (the derived world map) that conflicts with a whole-context mutable borrow;
/// all borrows here are field-disjoint or shared-compatible by construction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_native_action_use(
    config: &NativeOtClientHostConfig,
    stream: &mut TcpStream,
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    peer: SocketAddr,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    facing: NativeOtClientCardinalDirection,
    player_position: &mut Position,
    observed_visibility_epoch: &mut u64,
    observed_vitals_epoch: &mut u64,
    world_map: &WorldMap,
    dispatcher: &SandboxedLuaCallbackDispatcher,
    server_id: u16,
    action_id: Option<u16>,
    unique_id: Option<u16>,
) -> Result<bool, HostError> {
    let subject_position = shared_world.player_position(character_id)?;
    let input = SandboxedLuaCallbackInput {
        event_kind: "action".into(),
        subject_id: character_id,
        value: i64::from(server_id),
        argument: String::new(),
        position: Some(SandboxedLuaPosition {
            x: subject_position.x,
            y: subject_position.y,
            z: subject_position.z,
        }),
        storage: database.player_storage_snapshot(character_id)?,
    };
    let Some(registry) = config.action_registry.as_deref() else {
        return Ok(false);
    };
    let Some((key, _)) = resolve_action_callback(registry, server_id, action_id, unique_id) else {
        return Ok(false);
    };
    let outcome = dispatcher.dispatch_api(&key, &input);
    if let Some(event) = script_budget_event(
        "action",
        &key,
        &outcome.state,
        outcome.instruction_checks,
        dispatcher.limits().max_instructions,
    ) {
        native_diagnostic(config.extended_diagnostics, peer, &event);
    }
    match outcome.state {
        SandboxedLuaCallbackDispatchState::Completed => {
            apply_native_action_effects(
                config,
                stream,
                database,
                shared_world,
                character_id,
                peer,
                snapshot,
                facing,
                player_position,
                observed_visibility_epoch,
                observed_vitals_epoch,
                world_map,
                outcome.effects,
                "action=use outcome=action-applied",
            )?;
            Ok(true)
        }
        // The builder registers every registry entry, so a resolved match always has a
        // callback; a miss means registry/dispatcher skew, consumed loudly rather than
        // silently falling through to generic handling.
        SandboxedLuaCallbackDispatchState::CallbackNotFound => {
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                &format!("action=use outcome=action-script-missing key={key}"),
            );
            Ok(true)
        }
        _ => {
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                &format!(
                    "action=use outcome=action-script-rejected key={key} state={:?}",
                    outcome.state,
                ),
            );
            Ok(true)
        }
    }
}

/// Applies script intents against authoritative subject state. Subject-relative validated
/// intents only; teleport resends the viewport, every mutation persists before frames.
/// Shared by action routing and movement step routing; the trailing `outcome_tag`
/// keeps each caller's diagnostics honest (`action=use ...` vs `movement=step ...`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_native_action_effects(
    config: &NativeOtClientHostConfig,
    stream: &mut TcpStream,
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    peer: SocketAddr,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    facing: NativeOtClientCardinalDirection,
    player_position: &mut Position,
    observed_visibility_epoch: &mut u64,
    observed_vitals_epoch: &mut u64,
    world_map: &WorldMap,
    effects: Vec<SandboxedLuaEffect>,
    outcome_tag: &'static str,
) -> Result<(), HostError> {
    let mut teleported = false;
    for effect in effects {
        match effect {
            SandboxedLuaEffect::Say(text) => {
                let reply_frame =
                    encode_native_otclient_status_message(&config.client_profile, &text)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &reply_frame)?;
            }
            SandboxedLuaEffect::Teleport { x, y, z } => {
                let destination = Position { x, y, z };
                if shared_world
                    .teleport_player_for_operator(character_id, destination)
                    .is_ok()
                {
                    *player_position = destination;
                    teleported = true;
                } else {
                    let reply_frame = encode_native_otclient_status_message(
                        &config.client_profile,
                        "That destination is blocked.",
                    )
                    .map_err(HostError::Protocol)?;
                    write_frame(stream, &reply_frame)?;
                }
            }
            SandboxedLuaEffect::Heal { health, mana } => {
                let mut vitals = shared_world.player_vitals(character_id)?;
                if health > 0 {
                    vitals.health = vitals.health.saturating_add(health).min(vitals.max_health);
                }
                if mana > 0 {
                    vitals.mana = vitals.mana.saturating_add(mana).min(vitals.max_mana);
                }
                shared_world
                    .lock()?
                    .update_player_vitals(character_id, vitals)
                    .map_err(HostError::Core)?;
                shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
                database.update_player_vitals(
                    character_id,
                    PersistedPlayerVitals {
                        health: vitals.health,
                        max_health: vitals.max_health,
                        mana: vitals.mana,
                        max_mana: vitals.max_mana,
                        capacity: vitals.capacity,
                        magic_level: vitals.magic_level,
                    },
                )?;
                let self_native_id = native_player_id(character_id)?;
                let health_update = encode_native_otclient_creature_health(
                    &config.client_profile,
                    self_native_id,
                    vitals.health,
                    vitals.max_health,
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &health_update)?;
                *observed_vitals_epoch = shared_world.vitals_epoch();
            }
            SandboxedLuaEffect::GiveItem { id, count } => {
                if let Some(message) = give_items_to_player(
                    shared_world,
                    database,
                    character_id,
                    id,
                    u64::from(count),
                )? {
                    let reply_frame =
                        encode_native_otclient_status_message(&config.client_profile, &message)
                            .map_err(HostError::Protocol)?;
                    write_frame(stream, &reply_frame)?;
                }
            }
            SandboxedLuaEffect::RemoveItem { id, count } => {
                if let Some(message) = remove_items_from_player(
                    shared_world,
                    database,
                    character_id,
                    id,
                    u64::from(count),
                )? {
                    let reply_frame =
                        encode_native_otclient_status_message(&config.client_profile, &message)
                            .map_err(HostError::Protocol)?;
                    write_frame(stream, &reply_frame)?;
                }
            }
            SandboxedLuaEffect::MagicEffect { x, y, z, kind } => {
                let effect_frame = encode_native_otclient_magic_effect(
                    &config.client_profile,
                    native_position(Position { x, y, z }),
                    kind,
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &effect_frame)?;
            }
            SandboxedLuaEffect::SetStorage { key, value } => {
                database.set_player_storage_value(character_id, key, value)?;
            }
        }
    }
    if teleported {
        shared_world.mark_visibility_changed();
        let mut refreshed_snapshot = snapshot.clone();
        refreshed_snapshot.player_position = native_position(*player_position);
        refreshed_snapshot.player_direction = facing.protocol_direction();
        let refreshed_viewport = encode_shared_native_world_viewport(
            &config.client_profile,
            &refreshed_snapshot,
            world_map,
            shared_world,
            character_id,
        )?;
        let refreshed_static_spawns = shared_world.active_static_spawns()?;
        let refreshed_static_health_frames =
            native_static_creature_health_frames(&config.client_profile, &refreshed_static_spawns)?;
        write_frame(stream, &refreshed_viewport)?;
        for frame in &refreshed_static_health_frames {
            write_frame(stream, frame)?;
        }
        *observed_visibility_epoch = shared_world.visibility_epoch();
    }
    native_diagnostic(config.extended_diagnostics, peer, outcome_tag);
    Ok(())
}
