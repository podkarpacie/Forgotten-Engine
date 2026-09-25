//! ThrowItem address decoding for the native 740 session loop.
//!
//! The `ThrowItem` matrix is the hardest remaining `session_loop.rs` block (hardest-first
//! order): source-by-target branches interleaved across ~1,300 lines that cannot split
//! verbatim. This module is step 1 of that restructure — the straight-line endpoint-address
//! preamble moved verbatim behind a pure, unit-tested classifier, with no behavior change.
//! Per-source handlers dispatching on `ThrowItemAddresses` follow in later increments.

use super::*;

/// Decoded ThrowItem endpoint addresses: each side is either an owned-equipment slot, an
/// owned-container window address, or (both `None`) a real ground tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemAddresses {
    pub source_slot: Option<EquipmentSlot>,
    pub source_container: Option<(u8, usize)>,
    pub target_slot: Option<EquipmentSlot>,
    pub target_container_id: Option<u8>,
}

/// Decodes ThrowItem source/target addresses verbatim from the session loop preamble.
/// Classic clients address owned endpoints with `x == 0xffff`: equipment slots carry the
/// slot code in `y` (no `0x40` flag, `z == 0`, stack position 0), container windows set
/// the `0x40` flag with the window identifier in the lower four bits of `y`.
pub(crate) fn decode_throw_item_addresses(
    source_position: NativeOtClientPosition,
    source_stack_position: u8,
    target_position: NativeOtClientPosition,
) -> ThrowItemAddresses {
    let source_slot = (source_position.x == 0xffff
        && source_position.y & 0x40 == 0
        && source_position.z == 0
        && source_stack_position == 0)
        .then(|| EquipmentSlot::from_code(source_position.y as u8))
        .flatten();
    let source_container = (source_position.x == 0xffff && source_position.y & 0x40 != 0)
        .then_some((
            (source_position.y & 0x0f) as u8,
            usize::from(source_position.z),
        ));
    let target_slot =
        (target_position.x == 0xffff && target_position.y & 0x40 == 0 && target_position.z == 0)
            .then(|| EquipmentSlot::from_code(target_position.y as u8))
            .flatten();
    // Classic clients address open container windows with the high container flag
    // in y and the window identifier in its lower four bits. For a whole item the
    // destination index remains a client-side drop location and FE appends to the
    // already-owned top-level container. A requested partial stack is narrower: it
    // must name an existing matching top-level container item at that exact index.
    let target_container_id = (target_position.x == 0xffff && target_position.y & 0x40 != 0)
        .then_some((target_position.y & 0x0f) as u8);
    ThrowItemAddresses {
        source_slot,
        source_container,
        target_slot,
        target_container_id,
    }
}

/// Owned-inventory ThrowItem request for a real ground tile: the client-asserted stack
/// identity plus the decoded source endpoints. The target position decides routing:
/// ground tiles are handled here, everything else falls through to the next router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemGroundDropRequest {
    pub target_position: NativeOtClientPosition,
    pub source_client_thing_id: u16,
    pub count: u8,
    pub source_slot: Option<EquipmentSlot>,
    pub source_container: Option<(u8, usize)>,
}

/// Mutable follow-state the ground-drop handler refreshes after an authoritative move:
/// the observer's mapped-equipment mirror plus the equipment/container epochs.
pub(crate) struct ThrowItemGroundDropFollow<'a> {
    pub observed_mapped_equipment: &'a mut BTreeMap<EquipmentSlot, NativeOtClientClassicItemRecord>,
    pub observed_equipment_epoch: &'a mut u64,
    pub observed_containers_epoch: &'a mut u64,
}

/// Drops an owned-inventory stack onto a real ground tile through the durable runtime
/// registry. Returns `Unhandled` when the target is not a ground tile; every
/// ground-target path consumes the record (`Handled`), including deferred diagnostics,
/// preserving the loop's terminal `continue`. The item-presentation catalog gate is
/// re-derived with the same diagnostic so the handler is total; it is unreachable when
/// called after the loop preamble, which gates first.
pub(crate) fn apply_native_throw_item_ground_drop(
    ctx: &mut SessionContext<'_>,
    map_owner: &SharedNativeMap,
    request: ThrowItemGroundDropRequest,
    closed_container_ids: &BTreeSet<u8>,
    open_content_windows: &mut BTreeMap<u8, (u8, usize)>,
    sent_container_windows: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
    follow: &mut ThrowItemGroundDropFollow<'_>,
) -> Result<SessionActionOutcome, HostError> {
    if request.target_position.x == 0xffff {
        return Ok(SessionActionOutcome::Unhandled);
    }
    let Some(catalog) = ctx.config.item_presentation_catalog.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-no-item-presentation-catalog",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let target_tile = Position {
        x: request.target_position.x,
        y: request.target_position.y,
        z: request.target_position.z,
    };
    let drop_source = if let Some(slot) = request.source_slot {
        Some(forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot))
    } else if let Some((container_id, item_index)) = request.source_container {
        if closed_container_ids.contains(&container_id) {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-closed-container-ground-drop",
            );
            return Ok(SessionActionOutcome::Handled);
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
    let Some(drop_source) = drop_source.filter(|_| !ctx.observed_dead) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unsupported-ground-drop-source",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    // Validate the requested stack identity before any authoritative mutation.
    let identity_ok = match &drop_source {
        forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot) => ctx
            .shared_world
            .player_equipment(ctx.character_id)
            .ok()
            .and_then(|equipment| equipment.item(*slot).cloned())
            .is_some_and(|item| {
                native_classic_item_record(Some(catalog), &item)
                    .is_some_and(|record| record.client_thing_id == request.source_client_thing_id)
            }),
        forgotten_core::PlayerGroundDropSource::ContainerItem {
            container_id,
            item_index,
        } => ctx
            .shared_world
            .player_containers(ctx.character_id)
            .ok()
            .and_then(|containers| containers.container(*container_id).cloned())
            .and_then(|container| container.items.item(*item_index).cloned())
            .is_some_and(|item| {
                native_classic_item_record(Some(catalog), &item)
                    .is_some_and(|record| record.client_thing_id == request.source_client_thing_id)
            }),
        forgotten_core::PlayerGroundDropSource::ContainerContent {
            container_id,
            item_index,
            content_index,
        } => ctx
            .shared_world
            .player_containers(ctx.character_id)
            .ok()
            .and_then(|containers| containers.container(*container_id).cloned())
            .and_then(|container| container.items.item(*item_index).cloned())
            .and_then(|item| item.contents().get(*content_index).cloned())
            .is_some_and(|item| {
                native_classic_item_record(Some(catalog), &item)
                    .is_some_and(|record| record.client_thing_id == request.source_client_thing_id)
            }),
    };
    if !identity_ok {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-ground-drop-identity-mismatch",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    match map_owner.move_player_stack_to_ground(
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        drop_source,
        target_tile,
        u16::from(request.count),
        ctx.config.item_weight_by_server_id.as_deref(),
    ) {
        Ok(Some(outcome)) => {
            if matches!(
                outcome.source,
                forgotten_core::PlayerGroundDropSource::ContainerContent { .. }
            ) {
                native_refresh_open_content_windows(
                    &mut *ctx.stream,
                    &ctx.config.client_profile,
                    ctx.config.item_presentation_catalog.as_deref(),
                    &ctx.shared_world.player_containers(ctx.character_id)?,
                    &mut *open_content_windows,
                )?;
            }
            if let forgotten_core::PlayerGroundDropSource::EquipmentSlot(_) = outcome.source {
                let equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
                let current_mapped_equipment =
                    native_classic_mapped_equipment(Some(catalog), &equipment);
                let equipment_updates = native_classic_equipment_delta_frames(
                    &ctx.config.client_profile,
                    &*follow.observed_mapped_equipment,
                    &current_mapped_equipment,
                )
                .map_err(HostError::Protocol)?;
                for frame in &equipment_updates {
                    write_frame(&mut *ctx.stream, frame)?;
                }
                *follow.observed_mapped_equipment = current_mapped_equipment;
                *follow.observed_equipment_epoch = ctx.shared_world.equipment_epoch();
                if outcome.source_remaining_count.is_none() {
                    fire_native_deequip_event(ctx, outcome.moved_item.server_id)?;
                }
            }
            if let forgotten_core::PlayerGroundDropSource::ContainerItem { container_id, .. } =
                outcome.source
            {
                if !closed_container_ids.contains(&container_id) {
                    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
                    if let Some(container) = containers.container(container_id) {
                        if let Some(frame) = native_classic_container_frame(
                            &ctx.config.client_profile,
                            Some(catalog),
                            container,
                        )
                        .map_err(HostError::Protocol)?
                        {
                            write_frame(&mut *ctx.stream, &frame)?;
                        }
                        sent_container_windows.insert(
                            container_id,
                            native_rendered_container_window(
                                &ctx.config.client_profile,
                                Some(catalog),
                                container,
                            ),
                        );
                    }
                }
                *follow.observed_containers_epoch = ctx.shared_world.containers_epoch();
            }
            let mut refreshed_snapshot = ctx.snapshot.clone();
            refreshed_snapshot.player_position = native_position(*ctx.player_position);
            refreshed_snapshot.player_direction = ctx.facing.protocol_direction();
            let map_snapshot = map_owner.render_snapshot()?;
            let refreshed_viewport = encode_shared_native_world_viewport(
                &ctx.config.client_profile,
                &refreshed_snapshot,
                map_snapshot.as_ref(),
                ctx.shared_world,
                ctx.character_id,
            )?;
            write_frame(&mut *ctx.stream, &refreshed_viewport)?;
            *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "action=throw-item outcome=inventory-to-ground target={},{},{} server-id={} count={} moved={} remaining={:?} map-revision={}",
                    target_tile.x,
                    target_tile.y,
                    target_tile.z,
                    outcome.moved_item.server_id,
                    request.source_client_thing_id,
                    outcome.moved_item.count,
                    outcome.source_remaining_count,
                    map_owner.revision(),
                ),
            );
        }
        Ok(None) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-ground-drop-rejected",
        ),
        Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-ground-drop-failed",
            );
        }
        Err(error) => return Err(error),
    }
    Ok(SessionActionOutcome::Handled)
}

/// Owned-inventory ThrowItem request for a durable-registry runtime ground stack: the
/// map source address plus the client-asserted stack identity and the owned
/// destination endpoints. Non-map sources and missing/foreign stacks fall through to
/// the map-source transfer paths below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemRuntimePickupRequest {
    pub source_position: NativeOtClientPosition,
    pub source_client_thing_id: u16,
    pub source_stack_position: u8,
    pub count: u8,
    pub target_slot: Option<EquipmentSlot>,
    pub target_container_id: Option<u8>,
}

/// Mutable follow-state the runtime-pickup handler refreshes after an authoritative
/// move: the observer's mapped-equipment mirror plus the equipment/container epochs.
pub(crate) struct ThrowItemRuntimePickupFollow<'a> {
    pub observed_mapped_equipment: &'a mut BTreeMap<EquipmentSlot, NativeOtClientClassicItemRecord>,
    pub observed_equipment_epoch: &'a mut u64,
    pub observed_containers_epoch: &'a mut u64,
}

/// Capacity gate for one incoming stack into owned equipment or containers: sums
/// carried equipment/container/shell weight plus the intake and refuses past the
/// vitals capacity with a player-facing message. Returns `None` when the intake fits
/// or when no weight catalog is configured (ungated behavior preserved); unmapped
/// items weigh zero per `carried_inventory_weight`, so only provably overweight
/// intakes refuse, never unknown ones.
fn check_native_carry_capacity(
    shared_world: &SharedNativeWorld,
    character_id: u64,
    incoming: &[(u16, u16)],
    weights: Option<&BTreeMap<u16, u32>>,
) -> Result<Option<String>, HostError> {
    let Some(weights) = weights else {
        return Ok(None);
    };
    let vitals = shared_world.player_vitals(character_id)?;
    let mut carried = forgotten_core::carried_inventory_weight(
        &shared_world.player_equipment(character_id)?,
        &shared_world.player_containers(character_id)?,
        weights,
    );
    for (server_id, count) in incoming {
        let unit = u64::from(weights.get(server_id).copied().unwrap_or(0));
        carried = carried.saturating_add(unit.saturating_mul(u64::from(*count)));
    }
    if carried > u64::from(vitals.capacity) {
        return Ok(Some("You cannot carry that item.".into()));
    }
    Ok(None)
}

/// Refuses one overweight intake with a player-visible status message plus the
/// extended-diagnostics trace. Callers return `Handled`: the record is consumed.
/// Emits nothing and returns false when the gate passes.
fn refuse_overweight_intake(
    ctx: &mut SessionContext<'_>,
    incoming: &[(u16, u16)],
) -> Result<bool, HostError> {
    let Some(refusal) = check_native_carry_capacity(
        ctx.shared_world,
        ctx.character_id,
        incoming,
        ctx.config.item_weight_by_server_id.as_deref(),
    )?
    else {
        return Ok(false);
    };
    let refusal_frame = encode_native_otclient_status_message(&ctx.config.client_profile, &refusal)
        .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &refusal_frame)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        "action=throw-item outcome=carry-capacity-refused",
    );
    Ok(true)
}

/// Picks up a durable-registry runtime ground stack into owned equipment or an owned
/// container. Returns `Unhandled` when the source is not a map tile or no live runtime
/// stack sits at the addressed index (including while dead), so the map-source
/// transfer paths still run; every live-stack path consumes the record (`Handled`),
/// including deferred diagnostics, preserving the loop's terminal `continue`. The
/// item-presentation catalog gate is re-derived with the same diagnostic so the
/// handler is total; it is unreachable when called after the loop preamble.
pub(crate) fn apply_native_throw_item_runtime_pickup(
    ctx: &mut SessionContext<'_>,
    map_owner: &SharedNativeMap,
    request: ThrowItemRuntimePickupRequest,
    closed_container_ids: &BTreeSet<u8>,
    sent_container_windows: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
    follow: &mut ThrowItemRuntimePickupFollow<'_>,
) -> Result<SessionActionOutcome, HostError> {
    if request.source_position.x == 0xffff {
        return Ok(SessionActionOutcome::Unhandled);
    }
    let Some(catalog) = ctx.config.item_presentation_catalog.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-no-item-presentation-catalog",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let core_source_position = Position {
        x: request.source_position.x,
        y: request.source_position.y,
        z: request.source_position.z,
    };
    // Runtime ground items (dropped stacks) are picked up from the durable
    // registry; imported source items keep their own transfer paths below.
    let runtime_pickup = map_owner.runtime_tile_item(
        core_source_position,
        usize::from(request.source_stack_position),
    )?;
    let Some(runtime_item) = runtime_pickup.filter(|_| !ctx.observed_dead) else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let identity_ok = request.source_client_thing_id == runtime_item.server_id
        || catalog
            .presentation(runtime_item.server_id)
            .is_some_and(|presentation| {
                presentation.client_thing_id == request.source_client_thing_id
            });
    if !identity_ok {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-runtime-pickup-identity-mismatch",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    if refuse_overweight_intake(ctx, &[(runtime_item.server_id, u16::from(request.count))])? {
        return Ok(SessionActionOutcome::Handled);
    }
    let destination = if let Some(slot) = request.target_slot {
        Some(forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot))
    } else if let Some(container_id) = request.target_container_id {
        if closed_container_ids.contains(&container_id) {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-closed-container-pickup-target",
            );
            return Ok(SessionActionOutcome::Handled);
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
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unsupported-runtime-pickup-target",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    match map_owner.move_runtime_item_to_inventory(
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        core_source_position,
        usize::from(request.source_stack_position),
        None,
        u16::from(request.count),
        destination,
        ctx.config.item_weight_by_server_id.as_deref(),
    ) {
        Ok(Some(outcome)) => {
            if let forgotten_core::PlayerGroundDropSource::EquipmentSlot(_) = outcome.source {
                let equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
                let current_mapped_equipment =
                    native_classic_mapped_equipment(Some(catalog), &equipment);
                let equipment_updates = native_classic_equipment_delta_frames(
                    &ctx.config.client_profile,
                    &*follow.observed_mapped_equipment,
                    &current_mapped_equipment,
                )
                .map_err(HostError::Protocol)?;
                for frame in &equipment_updates {
                    write_frame(&mut *ctx.stream, frame)?;
                }
                *follow.observed_mapped_equipment = current_mapped_equipment;
                *follow.observed_equipment_epoch = ctx.shared_world.equipment_epoch();
            }
            if let forgotten_core::PlayerGroundDropSource::ContainerItem { container_id, .. } =
                outcome.source
            {
                if !closed_container_ids.contains(&container_id) {
                    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
                    if let Some(container) = containers.container(container_id) {
                        if let Some(frame) = native_classic_container_frame(
                            &ctx.config.client_profile,
                            Some(catalog),
                            container,
                        )
                        .map_err(HostError::Protocol)?
                        {
                            write_frame(&mut *ctx.stream, &frame)?;
                        }
                        sent_container_windows.insert(
                            container_id,
                            native_rendered_container_window(
                                &ctx.config.client_profile,
                                Some(catalog),
                                container,
                            ),
                        );
                    }
                }
                *follow.observed_containers_epoch = ctx.shared_world.containers_epoch();
            }
            let mut refreshed_snapshot = ctx.snapshot.clone();
            refreshed_snapshot.player_position = native_position(*ctx.player_position);
            refreshed_snapshot.player_direction = ctx.facing.protocol_direction();
            let map_snapshot = map_owner.render_snapshot()?;
            let refreshed_viewport = encode_shared_native_world_viewport(
                &ctx.config.client_profile,
                &refreshed_snapshot,
                map_snapshot.as_ref(),
                ctx.shared_world,
                ctx.character_id,
            )?;
            write_frame(&mut *ctx.stream, &refreshed_viewport)?;
            *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "action=throw-item outcome=runtime-ground-pickup source={},{},{} server-id={} count={} moved={} remaining={:?} index={}",
                    core_source_position.x,
                    core_source_position.y,
                    core_source_position.z,
                    runtime_item.server_id,
                    request.source_client_thing_id,
                    outcome.moved_item.count,
                    outcome.source_remaining_count,
                    request.source_stack_position,
                ),
            );
        }
        Ok(None) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-runtime-pickup-rejected",
        ),
        Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-runtime-pickup-failed",
            );
        }
        Err(error) => return Err(error),
    }
    Ok(SessionActionOutcome::Handled)
}

/// Owned-inventory ThrowItem request for an imported map source item: the map source
/// address plus the client-asserted stack identity and the owned destination
/// endpoints. Non-map sources fall through to the owned-source routers below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemMapSourceRequest {
    pub source_position: NativeOtClientPosition,
    pub source_client_thing_id: u16,
    pub source_stack_position: u8,
    pub count: u8,
    pub target_slot: Option<EquipmentSlot>,
    pub target_container_id: Option<u8>,
}

/// Mutable follow-state the map-source handler refreshes after an authoritative move:
/// the observer's mapped-equipment mirror plus the equipment/container epochs.
pub(crate) struct ThrowItemMapSourceFollow<'a> {
    pub observed_mapped_equipment: &'a mut BTreeMap<EquipmentSlot, NativeOtClientClassicItemRecord>,
    pub observed_equipment_epoch: &'a mut u64,
    pub observed_containers_epoch: &'a mut u64,
}

/// Moves an imported map source item into an owned container or owned equipment.
/// Returns `Unhandled` when the source is not a map tile; every map-source path
/// consumes the record (`Handled`), including deferred diagnostics, preserving the
/// loop's terminal `continue`s. The item-presentation catalog gate is re-derived with
/// the same diagnostic so the handler is total; it is unreachable when called after
/// the loop preamble.
pub(crate) fn apply_native_throw_item_map_source(
    ctx: &mut SessionContext<'_>,
    map_owner: &SharedNativeMap,
    request: ThrowItemMapSourceRequest,
    closed_container_ids: &BTreeSet<u8>,
    sent_container_windows: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
    follow: &mut ThrowItemMapSourceFollow<'_>,
) -> Result<SessionActionOutcome, HostError> {
    if request.source_position.x == 0xffff {
        return Ok(SessionActionOutcome::Unhandled);
    }
    let Some(catalog) = ctx.config.item_presentation_catalog.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-no-item-presentation-catalog",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let Some(intent) = native_map_item_use_intent(
        Some(catalog),
        ctx.character_id,
        request.source_position,
        request.source_client_thing_id,
        request.source_stack_position,
    ) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unmapped-or-ambiguous-map-source-item",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let map_snapshot = map_owner.render_snapshot()?;
    let source = match ctx
        .shared_world
        .validate_player_item_use(&map_snapshot, intent)
    {
        Ok(source) => source,
        Err(HostError::Core(_)) => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-invalid-server-owned-map-source",
            );
            return Ok(SessionActionOutcome::Handled);
        }
        Err(error) => return Err(error),
    };
    if refuse_overweight_intake(ctx, &[(source.server_id, u16::from(request.count))])? {
        return Ok(SessionActionOutcome::Handled);
    }
    let source_position = Position {
        x: request.source_position.x,
        y: request.source_position.y,
        z: request.source_position.z,
    };
    if let Some(container_id) = request.target_container_id {
        if closed_container_ids.contains(&container_id) {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-closed-map-source-container-target",
            );
            return Ok(SessionActionOutcome::Handled);
        }
        let transfer = match map_owner.move_source_item_stack_to_top_level_container(
            ctx.shared_world,
            &mut *ctx.database,
            ctx.character_id,
            source_position,
            usize::from(request.source_stack_position),
            u16::from(request.count),
            container_id,
        ) {
            Ok(transfer) => transfer,
            Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=throw-item outcome=deferred-map-source-container-transfer-rejected",
                );
                return Ok(SessionActionOutcome::Handled);
            }
            Err(error) => return Err(error),
        };
        let containers = ctx.shared_world.player_containers(ctx.character_id)?;
        let Some(container) = containers.container(container_id) else {
            return Err(HostError::InvalidConfiguration(
                "published map-source container transfer lost its container".into(),
            ));
        };
        let Some(container_frame) =
            native_classic_container_frame(&ctx.config.client_profile, Some(catalog), container)
                .map_err(HostError::Protocol)?
        else {
            return Err(HostError::InvalidConfiguration(
                "published map-source container transfer is not client-mapped".into(),
            ));
        };
        write_frame(&mut *ctx.stream, &container_frame)?;
        sent_container_windows.insert(
            container_id,
            native_rendered_container_window(&ctx.config.client_profile, Some(catalog), container),
        );
        *follow.observed_containers_epoch = ctx.shared_world.containers_epoch();
        let mut refreshed_snapshot = ctx.snapshot.clone();
        refreshed_snapshot.player_position = native_position(*ctx.player_position);
        refreshed_snapshot.player_direction = ctx.facing.protocol_direction();
        let map_snapshot = map_owner.render_snapshot()?;
        let refreshed_viewport = encode_shared_native_world_viewport(
            &ctx.config.client_profile,
            &refreshed_snapshot,
            map_snapshot.as_ref(),
            ctx.shared_world,
            ctx.character_id,
        )?;
        write_frame(&mut *ctx.stream, &refreshed_viewport)?;
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=throw-item outcome=map-source-to-top-level-container source={:?} container-id={} client-thing-id={} count={} source-index={} map-revision={} container-refresh-bytes={} map-refresh-bytes={}",
                transfer.source_identity.position,
                container_id,
                request.source_client_thing_id,
                request.count,
                transfer.source_identity.item_index,
                transfer.map_revision,
                container_frame.0.len(),
                refreshed_viewport.0.len(),
            ),
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let Some(target_slot) = request.target_slot else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unsupported-map-source-target",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if !native_legacy_slot_types_allow_equipment_slot(
        ctx.config.item_slot_types_by_server_id.as_deref(),
        source.server_id,
        target_slot,
    ) {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-map-source-slot-type-mismatch",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let transfer = match map_owner.move_source_item_stack_to_equipment(
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        source_position,
        usize::from(request.source_stack_position),
        u16::from(request.count),
        target_slot,
    ) {
        Ok(transfer) => transfer,
        Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-map-source-transfer-rejected",
            );
            return Ok(SessionActionOutcome::Handled);
        }
        Err(error) => return Err(error),
    };
    let equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
    let current_mapped_equipment = native_classic_mapped_equipment(Some(catalog), &equipment);
    let equipment_updates = native_classic_equipment_delta_frames(
        &ctx.config.client_profile,
        &*follow.observed_mapped_equipment,
        &current_mapped_equipment,
    )
    .map_err(HostError::Protocol)?;
    for frame in &equipment_updates {
        write_frame(&mut *ctx.stream, frame)?;
    }
    *follow.observed_mapped_equipment = current_mapped_equipment;
    *follow.observed_equipment_epoch = ctx.shared_world.equipment_epoch();
    let mut refreshed_snapshot = ctx.snapshot.clone();
    refreshed_snapshot.player_position = native_position(*ctx.player_position);
    refreshed_snapshot.player_direction = ctx.facing.protocol_direction();
    let map_snapshot = map_owner.render_snapshot()?;
    let refreshed_viewport = encode_shared_native_world_viewport(
        &ctx.config.client_profile,
        &refreshed_snapshot,
        map_snapshot.as_ref(),
        ctx.shared_world,
        ctx.character_id,
    )?;
    write_frame(&mut *ctx.stream, &refreshed_viewport)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "action=throw-item outcome=map-source-to-equipment source={:?} target-slot={} client-thing-id={} count={} source-index={} map-revision={} equipment-records={} map-refresh-bytes={}",
            transfer.source_identity.position,
            target_slot.code(),
            request.source_client_thing_id,
            request.count,
            transfer.source_identity.item_index,
            transfer.map_revision,
            equipment_updates.len(),
            refreshed_viewport.0.len(),
        ),
    );
    Ok(SessionActionOutcome::Handled)
}

/// Owned-inventory ThrowItem request for an open corpse window: the window id plus the
/// client-asserted loot-child index and the owned destination endpoints. Sources
/// without an open corpse window fall through to the owned-container routers below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemCorpseTakeRequest {
    pub container_id: u8,
    pub source_stack_position: u8,
    pub count: u8,
    pub target_slot: Option<EquipmentSlot>,
    pub target_container_id: Option<u8>,
}

/// Session window maps the corpse-take handler reads and prunes: open corpse windows
/// plus the nested-content windows that share their id space.
pub(crate) struct ThrowItemCorpseWindows<'a> {
    pub open_corpse_windows: &'a mut BTreeMap<u8, (Position, usize)>,
    pub open_content_windows: &'a mut BTreeMap<u8, (u8, usize)>,
}

/// Mutable follow-state the corpse-take handler refreshes after an authoritative move:
/// the observer's mapped-equipment mirror plus the equipment/container epochs.
pub(crate) struct ThrowItemCorpseTakeFollow<'a> {
    pub observed_mapped_equipment: &'a mut BTreeMap<EquipmentSlot, NativeOtClientClassicItemRecord>,
    pub observed_equipment_epoch: &'a mut u64,
    pub observed_containers_epoch: &'a mut u64,
}

/// Takes loot from an open corpse window into owned equipment or an owned container.
/// Returns `Unhandled` when the source window has no open corpse (so the
/// owned-container routers below still run); every corpse path consumes the record
/// (`Handled`), including deferred diagnostics, preserving the loop's terminal
/// `continue`. The item-presentation catalog gate is re-derived with the same
/// diagnostic so the handler is total; it is unreachable when called after the loop
/// preamble.
pub(crate) fn apply_native_throw_item_corpse_take(
    ctx: &mut SessionContext<'_>,
    map_owner: &SharedNativeMap,
    request: ThrowItemCorpseTakeRequest,
    windows: &mut ThrowItemCorpseWindows<'_>,
    closed_container_ids: &BTreeSet<u8>,
    sent_container_windows: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
    follow: &mut ThrowItemCorpseTakeFollow<'_>,
) -> Result<SessionActionOutcome, HostError> {
    let Some(catalog) = ctx.config.item_presentation_catalog.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-no-item-presentation-catalog",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    // Open corpse windows are session-local views over durable runtime registry
    // items; taking loot routes through the registry composite instead of
    // player-owned container storage.
    let Some((corpse_position, corpse_item_index)) = windows
        .open_corpse_windows
        .get(&request.container_id)
        .copied()
    else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let destination = if let Some(slot) = request.target_slot {
        Some(forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot))
    } else if let Some(target_container) = request.target_container_id {
        if closed_container_ids.contains(&target_container)
            || target_container == request.container_id
        {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-closed-corpse-take-target",
            );
            return Ok(SessionActionOutcome::Handled);
        }
        Some(forgotten_core::PlayerGroundDropSource::ContainerItem {
            container_id: target_container,
            item_index: 0,
        })
    } else {
        None
    };
    let Some(destination) = destination.filter(|_| !ctx.observed_dead) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unsupported-corpse-take-target",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if let Some(corpse_item) = map_owner.runtime_tile_item(corpse_position, corpse_item_index)? {
        if refuse_overweight_intake(ctx, &[(corpse_item.server_id, u16::from(request.count))])? {
            return Ok(SessionActionOutcome::Handled);
        }
    }
    match map_owner.move_runtime_item_to_inventory(
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        corpse_position,
        corpse_item_index,
        Some(usize::from(request.source_stack_position)),
        u16::from(request.count),
        destination,
        ctx.config.item_weight_by_server_id.as_deref(),
    ) {
        Ok(Some(outcome)) => {
            // Re-send the refreshed corpse window so remaining loot stays accurate.
            if let Some(runtime_corpse) =
                map_owner.runtime_tile_item(corpse_position, corpse_item_index)?
            {
                if let Some(frame) = native_corpse_window_frame(
                    &ctx.config.client_profile,
                    Some(catalog),
                    request.container_id,
                    &runtime_corpse,
                    ctx.config.item_name_by_server_id.as_deref(),
                )
                .map_err(HostError::Protocol)?
                {
                    write_frame(&mut *ctx.stream, &frame)?;
                }
            } else {
                windows.open_corpse_windows.remove(&request.container_id);
                windows.open_content_windows.remove(&request.container_id);
            }
            if let forgotten_core::PlayerGroundDropSource::EquipmentSlot(_) = outcome.source {
                let equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
                let current_mapped_equipment =
                    native_classic_mapped_equipment(Some(catalog), &equipment);
                let equipment_updates = native_classic_equipment_delta_frames(
                    &ctx.config.client_profile,
                    &*follow.observed_mapped_equipment,
                    &current_mapped_equipment,
                )
                .map_err(HostError::Protocol)?;
                for frame in &equipment_updates {
                    write_frame(&mut *ctx.stream, frame)?;
                }
                *follow.observed_mapped_equipment = current_mapped_equipment;
                *follow.observed_equipment_epoch = ctx.shared_world.equipment_epoch();
            }
            if let forgotten_core::PlayerGroundDropSource::ContainerItem {
                container_id: target_container,
                ..
            } = outcome.source
            {
                if !closed_container_ids.contains(&target_container) {
                    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
                    if let Some(container) = containers.container(target_container) {
                        if let Some(frame) = native_classic_container_frame(
                            &ctx.config.client_profile,
                            Some(catalog),
                            container,
                        )
                        .map_err(HostError::Protocol)?
                        {
                            write_frame(&mut *ctx.stream, &frame)?;
                        }
                        sent_container_windows.insert(
                            target_container,
                            native_rendered_container_window(
                                &ctx.config.client_profile,
                                Some(catalog),
                                container,
                            ),
                        );
                    }
                }
                *follow.observed_containers_epoch = ctx.shared_world.containers_epoch();
            }
            let mut refreshed_snapshot = ctx.snapshot.clone();
            refreshed_snapshot.player_position = native_position(*ctx.player_position);
            refreshed_snapshot.player_direction = ctx.facing.protocol_direction();
            let map_snapshot = map_owner.render_snapshot()?;
            let refreshed_viewport = encode_shared_native_world_viewport(
                &ctx.config.client_profile,
                &refreshed_snapshot,
                map_snapshot.as_ref(),
                ctx.shared_world,
                ctx.character_id,
            )?;
            write_frame(&mut *ctx.stream, &refreshed_viewport)?;
            *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "action=throw-item outcome=corpse-loot-taken window-id={} child-index={} server-id={} moved={} remaining={:?}",
                    request.container_id,
                    request.source_stack_position,
                    outcome.moved_item.server_id,
                    outcome.moved_item.count,
                    outcome.source_remaining_count,
                ),
            );
        }
        Ok(None) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-corpse-take-rejected",
        ),
        Err(HostError::Core(_) | HostError::InvalidConfiguration(_)) => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-corpse-take-failed",
            );
        }
        Err(error) => return Err(error),
    }
    Ok(SessionActionOutcome::Handled)
}

/// Owned-inventory ThrowItem request for a container source moving into an owned
/// container: the source window address plus the client-asserted stack identity and
/// the target window id. Non-container targets fall through to the equipment-target
/// router below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemContainerToContainerRequest {
    pub container_id: u8,
    pub item_index: usize,
    pub target_container_id: Option<u8>,
    pub source_client_thing_id: u16,
    pub count: u8,
}

/// Moves an owned-container stack into an owned container through the atomic
/// inventory boundary. Returns `Unhandled` when the target is not a container;
/// every container-target path consumes the record (`Handled`), including deferred
/// diagnostics, preserving the loop's terminal `continue`s. The item-presentation
/// catalog gate is re-derived with the same diagnostic so the handler is total; it
/// is unreachable when called after the loop preamble.
pub(crate) fn apply_native_throw_item_container_to_container(
    ctx: &mut SessionContext<'_>,
    request: ThrowItemContainerToContainerRequest,
    closed_container_ids: &BTreeSet<u8>,
    open_content_windows: &mut BTreeMap<u8, (u8, usize)>,
) -> Result<SessionActionOutcome, HostError> {
    let Some(target_container_id) = request.target_container_id else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let Some(catalog) = ctx.config.item_presentation_catalog.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-no-item-presentation-catalog",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    // Nested content window source: translate the ephemeral window address
    // and move the whole content item into the target owned container.
    if let Some(&(parent_container_id, parent_item_index)) =
        open_content_windows.get(&request.container_id)
    {
        ctx.shared_world.move_content_item_to_container(
            ctx.character_id,
            parent_container_id,
            parent_item_index,
            request.item_index,
            target_container_id,
        )?;
        let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
        let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
        ctx.database.replace_player_inventory(
            ctx.character_id,
            &next_equipment,
            &next_containers,
        )?;
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=throw-item outcome=content-item-to-container parent-container-id={} parent-item-index={} content-index={} target-container-id={} client-thing-id={}",
                parent_container_id,
                parent_item_index,
                request.item_index,
                target_container_id,
                request.source_client_thing_id
            ),
        );
        native_refresh_open_content_windows(
            &mut *ctx.stream,
            &ctx.config.client_profile,
            ctx.config.item_presentation_catalog.as_deref(),
            &ctx.shared_world.player_containers(ctx.character_id)?,
            &mut *open_content_windows,
        )?;
        return Ok(SessionActionOutcome::Handled);
    }
    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
    let Some(source_container) = containers.container(request.container_id) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unknown-container-source",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let Some(target_container) = containers.container(target_container_id) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unknown-container-target",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if closed_container_ids.contains(&request.container_id)
        || closed_container_ids.contains(&target_container_id)
        || request.container_id == target_container_id
        || source_container.has_parent
        || target_container.has_parent
    {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-invalid-container-to-container-boundary",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let Some(item) = source_container.items.item(request.item_index) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unknown-container-source-item",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if item.count < u16::from(request.count)
        || catalog
            .presentation(item.server_id)
            .map(|entry| entry.client_thing_id)
            != Some(request.source_client_thing_id)
    {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-invalid-container-item-identity-or-source-count",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    ctx.shared_world.move_container_stack_to_container(
        ctx.character_id,
        request.container_id,
        request.item_index,
        target_container_id,
        u16::from(request.count),
    )?;
    let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
    ctx.database
        .replace_player_containers(ctx.character_id, &next_containers)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "action=throw-item outcome=top-level-container-to-container-stack source-container-id={} item-index={} target-container-id={} client-thing-id={} count={}",
            request.container_id,
            request.item_index,
            target_container_id,
            request.source_client_thing_id,
            request.count
        ),
    );
    Ok(SessionActionOutcome::Handled)
}

/// Owned-inventory ThrowItem request for a container source moving into owned
/// equipment: the source window address plus the client-asserted stack identity and
/// the target slot. Non-equipment targets fall through to the equipment-source
/// router below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemContainerToEquipmentRequest {
    pub container_id: u8,
    pub item_index: usize,
    pub target_slot: Option<EquipmentSlot>,
    pub source_client_thing_id: u16,
    pub count: u8,
}

/// Moves an owned-container stack into owned equipment through the atomic inventory
/// boundary (partial-stack merges, full-stack merges, occupied-slot swaps, empty-slot
/// moves, plus nested-content sources). Every path consumes the record (`Handled`),
/// including deferred diagnostics, preserving the loop's terminal `continue`s; the
/// handler runs after the container-to-container router, so a non-equipment target
/// here is a terminal deferred record, never fall-through. The item-presentation
/// catalog gate is re-derived with the same diagnostic so the handler is total; it
/// is unreachable when called after the loop preamble.
pub(crate) fn apply_native_throw_item_container_to_equipment(
    ctx: &mut SessionContext<'_>,
    request: ThrowItemContainerToEquipmentRequest,
    open_content_windows: &mut BTreeMap<u8, (u8, usize)>,
) -> Result<SessionActionOutcome, HostError> {
    let Some(target_slot) = request.target_slot else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unsupported-container-source-target",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let Some(catalog) = ctx.config.item_presentation_catalog.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-no-item-presentation-catalog",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    // Nested content window source: translate the ephemeral window address and
    // move the whole content item into an empty equipment slot.
    if let Some(&(parent_container_id, parent_item_index)) =
        open_content_windows.get(&request.container_id)
    {
        ctx.shared_world.move_content_item_to_equipment(
            ctx.character_id,
            parent_container_id,
            parent_item_index,
            request.item_index,
            target_slot,
        )?;
        let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
        let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
        ctx.database.replace_player_inventory(
            ctx.character_id,
            &next_equipment,
            &next_containers,
        )?;
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=throw-item outcome=content-item-to-equipment parent-container-id={} parent-item-index={} content-index={} target-slot={} client-thing-id={}",
                parent_container_id,
                parent_item_index,
                request.item_index,
                target_slot.code(),
                request.source_client_thing_id
            ),
        );
        native_refresh_open_content_windows(
            &mut *ctx.stream,
            &ctx.config.client_profile,
            ctx.config.item_presentation_catalog.as_deref(),
            &ctx.shared_world.player_containers(ctx.character_id)?,
            &mut *open_content_windows,
        )?;
        let equipped_id = ctx
            .shared_world
            .player_equipment(ctx.character_id)?
            .item(target_slot)
            .map(|item| item.server_id);
        if let Some(server_id) = equipped_id {
            fire_native_equip_event(ctx, server_id)?;
        }
        return Ok(SessionActionOutcome::Handled);
    }
    let equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
    let Some(container) = containers.container(request.container_id) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unknown-container-source",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if container.has_parent {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-nested-container-source",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let Some(item) = container.items.item(request.item_index) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-unknown-container-source-item",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if item.count < u16::from(request.count)
        || catalog
            .presentation(item.server_id)
            .map(|entry| entry.client_thing_id)
            != Some(request.source_client_thing_id)
    {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-invalid-container-item-identity-or-source-count",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let requested_count = u16::from(request.count);
    if requested_count < item.count {
        if equipment
            .item(target_slot)
            .is_some_and(|destination| destination.server_id != item.server_id)
        {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-nonmatching-equipment-stack-merge-destination",
            );
            return Ok(SessionActionOutcome::Handled);
        }
        ctx.shared_world.move_container_stack_to_equipment(
            ctx.character_id,
            request.container_id,
            request.item_index,
            target_slot,
            requested_count,
        )?;
        let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
        let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
        ctx.database.replace_player_inventory(
            ctx.character_id,
            &next_equipment,
            &next_containers,
        )?;
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=throw-item outcome=top-level-container-stack-to-equipment-merge container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                request.container_id,
                request.item_index,
                target_slot.code(),
                request.source_client_thing_id,
                request.count
            ),
        );
        let equipped_id = ctx
            .shared_world
            .player_equipment(ctx.character_id)?
            .item(target_slot)
            .map(|item| item.server_id);
        if let Some(server_id) = equipped_id {
            fire_native_equip_event(ctx, server_id)?;
        }
        return Ok(SessionActionOutcome::Handled);
    }
    if equipment
        .item(target_slot)
        .is_some_and(|destination| destination.server_id == item.server_id)
    {
        ctx.shared_world.move_container_stack_to_equipment(
            ctx.character_id,
            request.container_id,
            request.item_index,
            target_slot,
            requested_count,
        )?;
        let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
        let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
        ctx.database.replace_player_inventory(
            ctx.character_id,
            &next_equipment,
            &next_containers,
        )?;
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=throw-item outcome=top-level-container-stack-to-equipment-merge container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                request.container_id,
                request.item_index,
                target_slot.code(),
                request.source_client_thing_id,
                request.count
            ),
        );
        let equipped_id = ctx
            .shared_world
            .player_equipment(ctx.character_id)?
            .item(target_slot)
            .map(|item| item.server_id);
        if let Some(server_id) = equipped_id {
            fire_native_equip_event(ctx, server_id)?;
        }
        return Ok(SessionActionOutcome::Handled);
    }
    if equipment.item(target_slot).is_some() {
        if requested_count == item.count {
            ctx.shared_world.swap_container_item_with_equipment(
                ctx.character_id,
                request.container_id,
                request.item_index,
                target_slot,
            )?;
            let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
            let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
            ctx.database.replace_player_inventory(
                ctx.character_id,
                &next_equipment,
                &next_containers,
            )?;
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "action=throw-item outcome=top-level-container-to-occupied-equipment-swap container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                    request.container_id,
                    request.item_index,
                    target_slot.code(),
                    request.source_client_thing_id,
                    request.count
                ),
            );
            let equipped_id = ctx
                .shared_world
                .player_equipment(ctx.character_id)?
                .item(target_slot)
                .map(|item| item.server_id);
            if let Some(server_id) = equipped_id {
                fire_native_equip_event(ctx, server_id)?;
            }
            if let Some(displaced_id) = equipment
                .item(target_slot)
                .map(|previous| previous.server_id)
            {
                fire_native_deequip_event(ctx, displaced_id)?;
            }
            return Ok(SessionActionOutcome::Handled);
        }
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-partial-or-unverified-occupied-equipment-target",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    if !native_legacy_slot_types_allow_equipment_slot(
        ctx.config.item_slot_types_by_server_id.as_deref(),
        item.server_id,
        target_slot,
    ) {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-container-item-slot-type-mismatch",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    ctx.shared_world.move_container_item_to_equipment(
        ctx.character_id,
        request.container_id,
        request.item_index,
        target_slot,
    )?;
    let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
    let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
    ctx.database
        .replace_player_inventory(ctx.character_id, &next_equipment, &next_containers)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
                "action=throw-item outcome=top-level-container-to-equipment container-id={} item-index={} target-slot={} client-thing-id={} count={}",
                request.container_id,
                request.item_index,
                target_slot.code(),
                request.source_client_thing_id,
                request.count
            ),
        );
    let equipped_id = ctx
        .shared_world
        .player_equipment(ctx.character_id)?
        .item(target_slot)
        .map(|item| item.server_id);
    if let Some(server_id) = equipped_id {
        fire_native_equip_event(ctx, server_id)?;
    }
    Ok(SessionActionOutcome::Handled)
}

/// Owned-inventory ThrowItem request for an equipment source: the source slot plus
/// the client-asserted stack identity and the owned destination endpoints. This is
/// the fallthrough tail: any record reaching it consumes here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemEquipmentSourceRequest {
    pub source_slot: Option<EquipmentSlot>,
    pub target_slot: Option<EquipmentSlot>,
    pub target_container_id: Option<u8>,
    pub target_position: NativeOtClientPosition,
    pub source_client_thing_id: u16,
    pub count: u8,
}

/// Moves an owned-equipment stack into owned equipment (same-slot guard, swaps,
/// plain transfers) or into an owned container (stack merges, full moves). Every
/// path consumes the record (`Handled`), including deferred diagnostics, preserving
/// the loop's terminal `continue`s and the fallthrough arm ends. The
/// item-presentation catalog gate is re-derived with the same diagnostic so the
/// handler is total; it is unreachable when called after the loop preamble.
pub(crate) fn apply_native_throw_item_equipment_source(
    ctx: &mut SessionContext<'_>,
    request: ThrowItemEquipmentSourceRequest,
) -> Result<SessionActionOutcome, HostError> {
    let Some(source_slot) = request.source_slot else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-non-equipment-source-position",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let Some(catalog) = ctx.config.item_presentation_catalog.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-no-item-presentation-catalog",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    let equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
    let Some(item) = equipment.item(source_slot).cloned() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-empty-source-slot",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if item.count < u16::from(request.count)
        || catalog
            .presentation(item.server_id)
            .map(|entry| entry.client_thing_id)
            != Some(request.source_client_thing_id)
    {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=throw-item outcome=deferred-invalid-item-identity-or-source-count",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    match (request.target_slot, request.target_container_id) {
        (Some(target_slot), None) => {
            if source_slot == target_slot {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=throw-item outcome=deferred-same-equipment-slot-target",
                );
                return Ok(SessionActionOutcome::Handled);
            }
            if equipment.item(target_slot).is_some() {
                if u16::from(request.count) != item.count {
                    native_diagnostic(
                        ctx.config.extended_diagnostics,
                        ctx.peer,
                        "action=throw-item outcome=deferred-partial-occupied-equipment-target",
                    );
                    return Ok(SessionActionOutcome::Handled);
                }
                ctx.shared_world.swap_equipment_items(
                    ctx.character_id,
                    source_slot,
                    target_slot,
                )?;
                let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
                ctx.database
                    .replace_player_equipment(ctx.character_id, &next_equipment)?;
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "action=throw-item outcome=occupied-equipment-slot-swap source-slot={} target-slot={} client-thing-id={} count={}",
                        source_slot.code(),
                        target_slot.code(),
                        request.source_client_thing_id,
                        request.count
                    ),
                );
                return Ok(SessionActionOutcome::Handled);
            }
            let mut next_equipment = equipment;
            next_equipment.unequip(source_slot);
            next_equipment.equip(target_slot, item);
            ctx.database
                .replace_player_equipment(ctx.character_id, &next_equipment)?;
            ctx.shared_world
                .replace_player_equipment(ctx.character_id, next_equipment)?;
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "action=throw-item outcome=equipment-slot-transfer source-slot={} target-slot={} client-thing-id={} count={}",
                    source_slot.code(),
                    target_slot.code(),
                    request.source_client_thing_id,
                    request.count
                ),
            );
        }
        (None, Some(container_id)) => {
            let containers = ctx.shared_world.player_containers(ctx.character_id)?;
            let Some(container) = containers.container(container_id) else {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=throw-item outcome=deferred-unknown-container-target",
                );
                return Ok(SessionActionOutcome::Handled);
            };
            if container.has_parent {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=throw-item outcome=deferred-nested-container-target",
                );
                return Ok(SessionActionOutcome::Handled);
            }
            let requested_count = u16::from(request.count);
            let destination_index = usize::from(request.target_position.z);
            if container
                .items
                .item(destination_index)
                .is_some_and(|destination| destination.server_id == item.server_id)
            {
                ctx.shared_world.move_equipment_stack_to_container(
                    ctx.character_id,
                    source_slot,
                    container_id,
                    requested_count,
                )?;
                let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
                let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
                ctx.database.replace_player_inventory(
                    ctx.character_id,
                    &next_equipment,
                    &next_containers,
                )?;
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "action=throw-item outcome=equipment-stack-to-top-level-container-merge source-slot={} container-id={} destination-index={} client-thing-id={} count={}",
                        source_slot.code(),
                        container_id,
                        destination_index,
                        request.source_client_thing_id,
                        request.count
                    ),
                );
                let vacated = ctx
                    .shared_world
                    .player_equipment(ctx.character_id)?
                    .item(source_slot)
                    .map(|current| current.server_id)
                    != Some(item.server_id);
                if vacated {
                    fire_native_deequip_event(ctx, item.server_id)?;
                }
                return Ok(SessionActionOutcome::Handled);
            }
            if requested_count < item.count {
                let outcome = if container.items.item(destination_index).is_some() {
                    "deferred-nonmatching-stack-merge-destination"
                } else {
                    "deferred-missing-stack-merge-destination"
                };
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!("action=throw-item outcome={outcome}"),
                );
                return Ok(SessionActionOutcome::Handled);
            }
            if container.items.item(destination_index).is_some() {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=throw-item outcome=deferred-nonmatching-full-stack-merge-destination",
                );
                return Ok(SessionActionOutcome::Handled);
            }
            ctx.shared_world.move_equipment_item_to_container(
                ctx.character_id,
                source_slot,
                container_id,
            )?;
            let next_equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
            let next_containers = ctx.shared_world.player_containers(ctx.character_id)?;
            ctx.database.replace_player_inventory(
                ctx.character_id,
                &next_equipment,
                &next_containers,
            )?;
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                        "action=throw-item outcome=equipment-to-top-level-container source-slot={} container-id={} client-thing-id={} count={}",
                        source_slot.code(),
                        container_id,
                        request.source_client_thing_id,
                        request.count
                    ),
                );
            let vacated = ctx
                .shared_world
                .player_equipment(ctx.character_id)?
                .item(source_slot)
                .map(|current| current.server_id)
                != Some(item.server_id);
            if vacated {
                fire_native_deequip_event(ctx, item.server_id)?;
            }
        }
        _ => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=throw-item outcome=deferred-unsupported-target-position",
            );
        }
    }
    Ok(SessionActionOutcome::Handled)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn pos(x: u16, y: u16, z: u8) -> NativeOtClientPosition {
        NativeOtClientPosition { x, y, z }
    }

    #[test]
    fn decodes_equipment_source_and_target_slots() {
        let addresses = decode_throw_item_addresses(pos(0xffff, 5, 0), 0, pos(0xffff, 6, 0));
        assert_eq!(addresses.source_slot, Some(EquipmentSlot::RightHand));
        assert_eq!(addresses.source_container, None);
        assert_eq!(addresses.target_slot, Some(EquipmentSlot::LeftHand));
        assert_eq!(addresses.target_container_id, None);
    }

    #[test]
    fn rejects_equipment_source_with_nonzero_stack_position() {
        let addresses = decode_throw_item_addresses(pos(0xffff, 5, 0), 1, pos(0xffff, 6, 0));
        assert_eq!(addresses.source_slot, None);
        assert_eq!(addresses.source_container, None);
    }

    #[test]
    fn rejects_unknown_slot_codes_on_both_sides() {
        let addresses = decode_throw_item_addresses(pos(0xffff, 11, 0), 0, pos(0xffff, 0, 0));
        assert_eq!(addresses.source_slot, None);
        assert_eq!(addresses.target_slot, None);
    }

    #[test]
    fn decodes_container_window_addresses() {
        let addresses =
            decode_throw_item_addresses(pos(0xffff, 0x40 | 3, 2), 0, pos(0xffff, 0x40 | 7, 0));
        assert_eq!(addresses.source_slot, None);
        assert_eq!(addresses.source_container, Some((3, 2)));
        assert_eq!(addresses.target_slot, None);
        assert_eq!(addresses.target_container_id, Some(7));
    }

    #[test]
    fn treats_ground_tiles_as_neither_slot_nor_container() {
        let addresses = decode_throw_item_addresses(pos(100, 100, 7), 0, pos(101, 102, 7));
        assert_eq!(
            addresses,
            ThrowItemAddresses {
                source_slot: None,
                source_container: None,
                target_slot: None,
                target_container_id: None,
            }
        );
    }
}
