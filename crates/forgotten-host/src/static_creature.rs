//! Static-creature movement/refresh and durability: externally selected steps and
//! policy application with native map refresh, reset, and runtime vitals/persistence
//! bridges used by the shared heartbeat and native session paths.

use super::*;

// Server-owner static-creature step/policy/reset commands. These are public host-runtime API
// exercised by socket regressions; the shared heartbeat drives the advance_* primitives, so
// these stay wired as the deterministic test-facing surface (staged conventions per
// docs/benchmarks/native-render-preparation-v7.4.44.md).
#[allow(dead_code)]
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
#[allow(dead_code)] // surfaced through socket regressions; heartbeat drives the advance_* primitives internally
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

/// Applies one explicitly requested target-directed creature step through the shared world and
/// refreshes the selected native session only after a real move. It creates no autonomous task,
/// protocol-specific target state, combat action, or pathfinding behavior.
#[allow(dead_code)] // surfaced through socket regressions; heartbeat drives the advance_* primitives internally
pub fn step_shared_native_static_creature_toward_target_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    shared_world: &SharedNativeWorld,
    viewer_player_id: u64,
    world_map: &WorldMap,
    creature_id: u32,
) -> Result<(StaticCreatureTargetStepOutcome, Option<Frame>), HostError> {
    let outcome = shared_world.step_static_creature_toward_target(creature_id, world_map)?;
    if !matches!(outcome, StaticCreatureTargetStepOutcome::Moved { .. }) {
        return Ok((outcome, None));
    }
    let frame = encode_shared_native_world_viewport(
        profile,
        snapshot,
        world_map,
        shared_world,
        viewer_player_id,
    )?;
    Ok((outcome, Some(frame)))
}

/// Reactivates inactive imported static entities at their validated spawn positions and emits a
/// native map refresh only when the active entity set changed. This is caller-triggered and adds
/// no timed respawn scheduler, AI, combat, drops, corpse, Lua, or action behavior.
#[allow(dead_code)] // surfaced through socket regressions; heartbeat drives the advance_* primitives internally
pub fn reset_native_static_creatures_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world: &mut WorldState,
    world_map: &WorldMap,
) -> Result<(StaticCreatureResetSummary, Option<Frame>), HostError> {
    let summary = world.reset_static_creatures();
    if summary.reactivated == 0 {
        return Ok((summary, None));
    }
    let active_static_spawns = world.active_static_spawn_collection();
    let frame = encode_native_otclient_map_viewport_with_static_spawns(
        profile,
        snapshot,
        world_map,
        Some(&active_static_spawns),
    )
    .map_err(HostError::Protocol)?;
    Ok((summary, Some(frame)))
}

/// Persists the post-heartbeat authoritative condition set. This must run even when a condition
/// has not damaged the player yet, because its elapsed interval remainder is part of deterministic
/// restart behavior; an empty set also removes schedules that expired during the heartbeat.
pub(crate) fn persist_runtime_player_conditions(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_id: u64,
) -> Result<(), HostError> {
    let conditions = shared_world.player_conditions(player_id)?;
    database
        .replace_player_conditions(player_id, &conditions)
        .map_err(HostError::Persistence)
}

/// Persists only the authoritative players actually changed by one static-target attack pass.
/// The caller provides a `BTreeSet`, making write order deterministic. Client combat effects,
/// death packets, loot, corpses, formulas, scripts, and general creature AI remain separate.
pub(crate) fn persist_static_target_attack_vitals(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_ids: &BTreeSet<u64>,
    death_loss_policy: DeathLossPolicy,
    progression_rules: Option<&BTreeMap<VocationId, PlayerProgressionRules>>,
) -> Result<(), HostError> {
    for &player_id in player_ids {
        let loss_persisted = if shared_world.player_respawn_state(player_id)?.dead {
            apply_configured_native_death_loss(
                database,
                shared_world,
                player_id,
                death_loss_policy,
                progression_rules,
            )?
        } else {
            false
        };
        if loss_persisted {
            continue;
        }
        let vitals = shared_world.player_vitals(player_id)?;
        let persisted_vitals = PersistedPlayerVitals {
            health: vitals.health,
            max_health: vitals.max_health,
            mana: vitals.mana,
            max_mana: vitals.max_mana,
            capacity: vitals.capacity,
            magic_level: vitals.magic_level,
        };
        let respawn_state = shared_world.player_respawn_state(player_id)?;
        if respawn_state.dead {
            database
                .update_player_vitals_and_respawn_state(player_id, persisted_vitals, respawn_state)
                .map_err(HostError::Persistence)?;
        } else {
            database
                .update_player_vitals(player_id, persisted_vitals)
                .map_err(HostError::Persistence)?;
        }
    }
    Ok(())
}
