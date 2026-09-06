//! Native 740 map movement and walk pacing: cardinal/diagonal player steps with teleport
//! activation, click-walk task state, and speed-derived step delay helpers used by the
//! session loop and shared heartbeat paths.

use super::*;

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
pub(crate) struct NativeActiveClickWalk {
    pub(crate) queued_steps: VecDeque<NativeOtClientCardinalDirection>,
    pub(crate) next_step_deadline: Instant,
}

impl NativeActiveClickWalk {
    pub(crate) fn from_path(
        path: Vec<NativeOtClientAutoWalkDirection>,
        next_step_deadline: Instant,
    ) -> Self {
        Self {
            queued_steps: native_click_walk_steps(path),
            next_step_deadline,
        }
    }

    pub(crate) fn replace_path(&mut self, path: Vec<NativeOtClientAutoWalkDirection>) {
        self.queued_steps = native_click_walk_steps(path);
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
