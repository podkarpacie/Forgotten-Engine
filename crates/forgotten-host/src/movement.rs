//! Native 740 map movement and walk pacing: cardinal/diagonal player steps with teleport
//! activation, click-walk task state, and speed-derived step delay helpers used by the
//! session loop and shared heartbeat paths.

use super::*;

/// Classic teleport pad arrival visual. `10` is the standard public-client magic-effect id for
/// teleportation (TFS `CONST_ME_TELEPORT`); FE emits it without asserting client-asset specifics.
const NATIVE_OTCLIENT_TELEPORT_EFFECT_ID: u8 = 10;

#[allow(clippy::too_many_arguments)]
pub(crate) fn move_native_map_player(
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
    // Operator freeze (plan v49 slice 18): frozen characters face the requested direction but
    // never step, identical to the classic blocked response.
    if database.player_frozen(character_id)? {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    }
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
    *facing = direction;
    shared_world.update_player_facing(character_id, *facing)?;
    if let Some(teleport_destination) =
        native_stepped_on_map_teleport_destination(world_map, destination)
    {
        if activate_native_map_teleport_item(
            stream,
            profile,
            snapshot,
            database,
            shared_world,
            character_id,
            world_map,
            player_position,
            *facing,
            teleport_destination,
        )? {
            return Ok(false);
        }
    }
    shared_world.mark_visibility_changed();
    database.update_player_position_and_facing(
        character_id,
        destination,
        facing.protocol_direction(),
    )?;
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

pub(crate) fn native_stepped_on_map_teleport_destination(
    world_map: &WorldMap,
    position: Position,
) -> Option<Position> {
    let mut destinations = world_map
        .tile_items(position)?
        .iter()
        .filter_map(|item| item.teleport_destination);
    let destination = destinations.next()?;
    destinations.next().is_none().then_some(destination)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn activate_native_map_teleport_item(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    database: &EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    world_map: &WorldMap,
    player_position: &mut Position,
    facing: NativeOtClientCardinalDirection,
    destination: Position,
) -> Result<bool, HostError> {
    const NATIVE_OTCLIENT_MAX_TELEPORT_HOPS: usize = 8;
    let final_destination = {
        let mut world = shared_world.lock()?;
        let mut current_destination = destination;
        let mut visited_destinations = BTreeSet::new();
        let mut final_destination = None;
        for _ in 0..NATIVE_OTCLIENT_MAX_TELEPORT_HOPS {
            if !visited_destinations.insert(current_destination)
                || !world_map.is_walkable(current_destination)
                || world.is_static_creature_occupied(current_destination)
                || world.is_player_occupied(current_destination)
            {
                break;
            }
            let (_, reached_destination) = world
                .teleport_player(character_id, current_destination)
                .map_err(HostError::Core)?;
            final_destination = Some(reached_destination);
            let Some(next_destination) =
                native_stepped_on_map_teleport_destination(world_map, reached_destination)
            else {
                break;
            };
            current_destination = next_destination;
        }
        final_destination
    };
    let Some(destination) = final_destination else {
        return Ok(false);
    };

    shared_world.mark_visibility_changed();
    database.update_player_position_and_facing(
        character_id,
        destination,
        facing.protocol_direction(),
    )?;
    let mut refreshed_snapshot = snapshot.clone();
    refreshed_snapshot.player_position = native_position(destination);
    refreshed_snapshot.player_direction = facing.protocol_direction();
    let teleport_effect = encode_native_otclient_magic_effect(
        profile,
        native_position(destination),
        NATIVE_OTCLIENT_TELEPORT_EFFECT_ID,
    )
    .map_err(HostError::Protocol)?;
    write_frame(stream, &teleport_effect)?;
    write_frame(
        stream,
        &encode_shared_native_world_viewport(
            profile,
            &refreshed_snapshot,
            world_map,
            shared_world,
            character_id,
        )?,
    )?;
    *player_position = destination;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn move_native_map_player_diagonal(
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
    // Operator freeze (plan v49 slice 18): frozen characters cancel the diagonal attempt.
    if database.player_frozen(character_id)? {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    }
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
    *facing = steps[1];
    shared_world.update_player_facing(character_id, *facing)?;
    if let Some(teleport_destination) =
        native_stepped_on_map_teleport_destination(world_map, destination)
    {
        if activate_native_map_teleport_item(
            stream,
            profile,
            snapshot,
            database,
            shared_world,
            character_id,
            world_map,
            player_position,
            *facing,
            teleport_destination,
        )? {
            return Ok(false);
        }
    }
    shared_world.mark_visibility_changed();
    database.update_player_position_and_facing(
        character_id,
        destination,
        facing.protocol_direction(),
    )?;
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

pub(crate) fn native_autowalk_step_delay(player_speed: u16, server_beat: u16) -> Duration {
    let speed = u64::from(player_speed).max(1);
    let server_beat = u64::from(server_beat).max(1);
    let interval_millis = (1000 * NATIVE_OTCLIENT_DEFAULT_GROUND_SPEED / speed)
        .max(server_beat)
        .min(NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY.as_millis() as u64);
    Duration::from_millis(interval_millis)
}

/// Classic-style effective walk speed: configured base speed plus the `speed` bonus of
/// equipped boots (BoH-style items). Bonuses add directly to the base; there is no stacking
/// across multiple slots because only the feet slot contributes.
pub fn native_effective_player_speed(
    base_speed: u16,
    equipment: &PlayerEquipment,
    speed_bonus_by_server_id: Option<&BTreeMap<u16, u16>>,
) -> u16 {
    let mut speed = base_speed;
    if let Some(bonuses) = speed_bonus_by_server_id {
        for slot in [EquipmentSlot::Feet] {
            if let Some(item) = equipment.item(slot) {
                if let Some(bonus) = bonuses.get(&item.server_id) {
                    speed = speed.saturating_add(*bonus);
                }
            }
        }
    }
    // Classic clients treat 0 or absurd speeds badly; keep a sane ceiling.
    speed.clamp(1, 2_000)
}

/// Applies the active haste condition (plan v49 slice 12): additive percent on the effective
/// speed, re-clamped so no bonus can push past the same sane ceiling.
pub fn native_hasted_speed(effective_speed: u16, speed_bonus_percent: u16) -> u16 {
    if speed_bonus_percent == 0 {
        return effective_speed;
    }
    let hasted = (u32::from(effective_speed) * (100 + u32::from(speed_bonus_percent))) / 100;
    u16::try_from(hasted).unwrap_or(2_000).clamp(1, 2_000)
}

/// Speed-derived walk delay for one voluntary step, shared by the click-walk
/// scheduler and every immediate step source (manual cardinal/diagonal moves
/// and single-step click paths). All voluntary steps advance the same
/// per-session cooldown, so input spam can never stack steps faster than the
/// configured pace no matter which frames carry it.
pub(crate) fn native_walk_step_delay(
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    shared_world: &SharedNativeWorld,
    config: &NativeOtClientHostConfig,
    character_id: u64,
) -> Result<Duration, HostError> {
    let equipment = shared_world.player_equipment(character_id)?;
    let effective_speed = native_hasted_speed(
        native_effective_player_speed(
            snapshot.player_speed,
            &equipment,
            config.item_speed_bonus_by_server_id.as_deref(),
        ),
        shared_world.player_speed_bonus_percent(character_id),
    );
    Ok(native_autowalk_step_delay(
        effective_speed,
        snapshot.server_beat,
    ))
}

/// Early-arrival tolerance for voluntary step inputs. Held-key repeats and client timers
/// quantize to their own grids, so a legitimately-paced input can land a few milliseconds
/// before the cooldown closes; dropping it would stall every step by a full repeat interval.
/// Inputs more than this early are still spam and gate silently.
pub(crate) const WALK_EARLY_TOLERANCE: Duration = Duration::from_millis(50);

/// Advances the shared walk cooldown after an executed displacement. Chained to the previous
/// deadline (never to arrival time), so the average pace stays exactly one delay per step no
/// matter how early within tolerance the input arrived. `double_cost` covers diagonal actions
/// and floor changes, matching the stock client's double animation for both; the two never
/// stack, mirroring the classic cost model.
pub(crate) fn advance_walk_cooldown(
    next_walk_at: &mut Instant,
    single_delay: Duration,
    double_cost: bool,
) {
    let delay = if double_cost {
        single_delay.saturating_mul(2)
    } else {
        single_delay
    };
    *next_walk_at = (*next_walk_at).max(Instant::now()) + delay;
}

/// Classic haste duration for spell-granted speed conditions (utani hur family).
pub(crate) const NATIVE_HASTE_DURATION_SECONDS: u16 = 25;

pub(crate) fn native_player_id(character_id: u64) -> Result<u32, HostError> {
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
pub(crate) enum NativePlayerInteractionKind {
    Target,
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativePlayerInteractionOutcome {
    Applied,
    Rejected,
}

/// A single server-owned native click-walk task. Client paths may replace its queued directions,
/// but never its next-step deadline. This mirrors the classic one-active-event behavior without
/// importing implementation code from another server.
///
/// `steps_executed` and `stop_grace_until` exist because the stock client answers every
/// cancel-walk frame with a Stop echo and a 500ms auto-walk retry: a Stop that arrives for an
/// older cancellation must not murder a task born after it. Only a Stop that lands after real
/// progress (or past the gesture grace) takes the task.
pub(crate) struct NativeActiveClickWalk {
    pub(crate) queued_steps: VecDeque<NativeOtClientCardinalDirection>,
    pub(crate) next_step_deadline: Instant,
    pub(crate) steps_executed: u32,
    pub(crate) stop_grace_until: Instant,
}

impl NativeActiveClickWalk {
    pub(crate) fn from_path(
        path: Vec<NativeOtClientAutoWalkDirection>,
        next_step_deadline: Instant,
    ) -> Self {
        Self {
            queued_steps: native_click_walk_steps(path),
            next_step_deadline,
            steps_executed: 0,
            stop_grace_until: Instant::now(),
        }
    }

    pub(crate) fn replace_path(
        &mut self,
        path: Vec<NativeOtClientAutoWalkDirection>,
        stop_grace_until: Instant,
    ) {
        self.queued_steps = native_click_walk_steps(path);
        self.steps_executed = 0;
        self.stop_grace_until = stop_grace_until;
    }
}

pub(crate) fn native_click_walk_steps(
    path: Vec<NativeOtClientAutoWalkDirection>,
) -> VecDeque<NativeOtClientCardinalDirection> {
    path.into_iter()
        .flat_map(|direction| direction.cardinal_steps().iter().copied())
        .collect()
}

/// The selected 740 map encoder renders an 18Ä‚â€”14 same-floor viewport centered at offset (8, 6),
/// which yields horizontal offsets -8 through +9 and vertical offsets -6 through +7. Creature
/// inspection is intentionally no broader than that already encoded viewport.
pub(crate) fn native_classic_viewport_contains(observer: Position, target: Position) -> bool {
    if observer.z != target.z {
        return false;
    }
    let horizontal_offset = i32::from(target.x) - i32::from(observer.x);
    let vertical_offset = i32::from(target.y) - i32::from(observer.y);
    (-8..=9).contains(&horizontal_offset) && (-6..=7).contains(&vertical_offset)
}

/// Applies one manual turn: cancels any click-walk, persists the new facing, and emits the
/// cancel-walk facing frame. Turns never move the player and never fail the session.
pub(crate) fn apply_native_turn_action(
    ctx: &mut SessionContext<'_>,
    direction: NativeOtClientCardinalDirection,
) -> Result<(), HostError> {
    let cancelled_click_walk = ctx.active_click_walk.take().is_some();
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "scheduler=click-walk-cancel reason=turn active={cancelled_click_walk} direction={direction:?}"
        ),
    );
    *ctx.facing = direction;
    ctx.shared_world
        .update_player_facing(ctx.character_id, *ctx.facing)?;
    *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
    write_frame(
        &mut *ctx.stream,
        &encode_native_otclient_game_cancel_walk_facing(
            &ctx.config.client_profile,
            ctx.facing.protocol_direction(),
        )
        .map_err(HostError::Protocol)?,
    )?;
    Ok(())
}

/// Applies one click-walk path: replaces any active walk, or creates a speed-derived scheduled
/// task when idle. A single-step path takes its step immediately only when the shared walk
/// cooldown is free; otherwise it queues behind the cooldown like any other path, so
/// spam-clicking adjacent tiles can never step faster than the configured pace.
pub(crate) fn apply_native_autowalk_action(
    ctx: &mut SessionContext<'_>,
    path: Vec<NativeOtClientAutoWalkDirection>,
    next_walk_at: &mut Instant,
) -> Result<(), HostError> {
    if let Some(task) = ctx.active_click_walk.as_mut() {
        let previous_steps = task.queued_steps.len();
        let replacement_steps = native_click_walk_steps(path.clone()).len();
        let step_delay =
            native_walk_step_delay(ctx.snapshot, ctx.shared_world, ctx.config, ctx.character_id)?;
        task.replace_path(path, Instant::now() + step_delay);
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "scheduler=click-walk-replace previous-steps={previous_steps} queued-steps={replacement_steps}"
            ),
        );
    } else {
        let step_delay =
            native_walk_step_delay(ctx.snapshot, ctx.shared_world, ctx.config, ctx.character_id)?;
        let now = Instant::now();
        let mut task =
            NativeActiveClickWalk::from_path(path, (*next_walk_at).max(now + step_delay));
        task.stop_grace_until = now + step_delay;
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "scheduler=click-walk-create queued-steps={} step-delay-ms={}",
                task.queued_steps.len(),
                step_delay.as_millis()
            ),
        );
        if task.queued_steps.is_empty() {
            return Ok(());
        }
        if task.queued_steps.len() == 1 && now + WALK_EARLY_TOLERANCE >= *next_walk_at {
            let Some(direction) = task.queued_steps.pop_front() else {
                return Ok(());
            };
            let pre_step_position = *ctx.player_position;
            if move_native_map_player(
                &mut *ctx.stream,
                &ctx.config.client_profile,
                ctx.snapshot,
                &*ctx.database,
                ctx.shared_world,
                ctx.character_id,
                ctx.world_map.as_ref(),
                &mut *ctx.player_position,
                &mut *ctx.facing,
                direction,
            )? {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "scheduler=click-walk-step direction={direction:?} outcome=moved position={},{},{}",
                        ctx.player_position.x, ctx.player_position.y, ctx.player_position.z
                    ),
                );
                *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
                advance_walk_cooldown(next_walk_at, step_delay, false);
                task.steps_executed = 1;
                *ctx.active_click_walk = Some(task);
            } else if *ctx.player_position != pre_step_position {
                // Stepped-on teleport: the mover reports false yet relocated the
                // player, so pace and resync exactly like a plain step, doubling
                // for a floor change like a diagonal.
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "scheduler=click-walk-step direction={direction:?} outcome=teleported position={},{},{}",
                        ctx.player_position.x, ctx.player_position.y, ctx.player_position.z
                    ),
                );
                *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
                let floor_changed = ctx.player_position.z != pre_step_position.z;
                advance_walk_cooldown(next_walk_at, step_delay, floor_changed);
            } else {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "scheduler=click-walk-step direction={direction:?} outcome=blocked position={},{},{}",
                        ctx.player_position.x, ctx.player_position.y, ctx.player_position.z
                    ),
                );
            }
        } else {
            *ctx.active_click_walk = Some(task);
        }
    }
    Ok(())
}

/// Applies one manual cardinal step through the shared map mover, cancelling any click-walk and
/// refreshing visibility only on a real move. Steps arriving inside the shared walk cooldown
/// are a silent no-op: the task, facing, and wire stay untouched. Any frame here (even a
/// cancel) makes the stock client echo Stop and schedule a 500ms auto-walk retry, churning
/// the walk into a stuttering halt; doing nothing converges on the next free input instead.
/// Manual steps never fail the session.
pub(crate) fn apply_native_cardinal_move_action(
    ctx: &mut SessionContext<'_>,
    direction: NativeOtClientCardinalDirection,
    next_walk_at: &mut Instant,
) -> Result<(), HostError> {
    let step_delay =
        native_walk_step_delay(ctx.snapshot, ctx.shared_world, ctx.config, ctx.character_id)?;
    // Early tolerance: repeats and client timers quantize to their own grids, so a
    // legitimately-paced input may land a few milliseconds before the window closes.
    // Only deeper earliness is spam. The re-arm below chains to the deadline, so the
    // average pace stays exactly one delay per step.
    if Instant::now() + WALK_EARLY_TOLERANCE < *next_walk_at {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!("movement=cardinal direction={direction:?} outcome=exhausted-ignored"),
        );
        return Ok(());
    }
    let cancelled_click_walk = ctx.active_click_walk.take().is_some();
    let pre_step_position = *ctx.player_position;
    let moved = move_native_map_player(
        &mut *ctx.stream,
        &ctx.config.client_profile,
        ctx.snapshot,
        &*ctx.database,
        ctx.shared_world,
        ctx.character_id,
        ctx.world_map.as_ref(),
        &mut *ctx.player_position,
        &mut *ctx.facing,
        direction,
    )?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "movement=cardinal direction={direction:?} outcome={} position={},{},{} map-update={}",
            if moved { "moved" } else { "blocked" },
            ctx.player_position.x,
            ctx.player_position.y,
            ctx.player_position.z,
            if moved { "step" } else { "cancel-walk" }
        ),
    );
    if cancelled_click_walk {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "scheduler=click-walk-cancel reason=manual-cardinal active=true",
        );
    }
    // Displacement, not the mover's boolean, drives pacing and epoch sync: a stepped-on
    // teleport reports `moved == false` yet relocates the player, and leaving it unsynced
    // emits a phantom full-viewport refresh on the next heartbeat pass while leaving pad
    // chains unpaced. A floor-changing teleport costs double like a diagonal, never more.
    if *ctx.player_position != pre_step_position {
        *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
        let floor_changed = ctx.player_position.z != pre_step_position.z;
        advance_walk_cooldown(next_walk_at, step_delay, floor_changed);
    }
    Ok(())
}

/// Applies one manual diagonal step through the shared diagonal mover. Same cancel, diagnostic,
/// visibility, and cooldown contract as the cardinal step: exhausted inputs are a silent
/// no-op so the stock client's cancel echo can never churn the walk.
pub(crate) fn apply_native_diagonal_move_action(
    ctx: &mut SessionContext<'_>,
    direction: NativeOtClientAutoWalkDirection,
    next_walk_at: &mut Instant,
) -> Result<(), HostError> {
    // Diagonal actions always cost double (the client's own ×2 animation), whether the
    // displacement is a plain double step or a teleport; floor changes never stack a
    // third multiple on top.
    let step_delay =
        native_walk_step_delay(ctx.snapshot, ctx.shared_world, ctx.config, ctx.character_id)?;
    if Instant::now() + WALK_EARLY_TOLERANCE < *next_walk_at {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!("movement=diagonal direction={direction:?} outcome=exhausted-ignored"),
        );
        return Ok(());
    }
    let cancelled_click_walk = ctx.active_click_walk.take().is_some();
    let pre_step_position = *ctx.player_position;
    let moved = move_native_map_player_diagonal(
        &mut *ctx.stream,
        &ctx.config.client_profile,
        ctx.snapshot,
        &*ctx.database,
        ctx.shared_world,
        ctx.character_id,
        ctx.world_map.as_ref(),
        &mut *ctx.player_position,
        &mut *ctx.facing,
        direction,
    )?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "movement=diagonal direction={direction:?} outcome={} position={},{},{} map-update={}",
            if moved { "moved" } else { "blocked" },
            ctx.player_position.x,
            ctx.player_position.y,
            ctx.player_position.z,
            if moved { "double-step" } else { "cancel-walk" }
        ),
    );
    if cancelled_click_walk {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "scheduler=click-walk-cancel reason=manual-diagonal active=true",
        );
    }
    // Same displacement rule as the cardinal step: teleports relocate with
    // `moved == false` and must still pace pad chains and resync the epoch.
    if *ctx.player_position != pre_step_position {
        *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
        advance_walk_cooldown(next_walk_at, step_delay, true);
    }
    Ok(())
}
