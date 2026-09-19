//! Owned UseItem routing past consumables: backpack-in-hand container windows, runtime
//! corpse opening from the durable registry, and map-item routing (teleport pads, read-only
//! text, validated generic use). Each handler consumes its record on match and falls through
//! otherwise, preserving the session loop's sequential routing order.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Opens a nested content window when the addressed item inside an owned-container
/// window itself holds content. Returns `Handled` when a content-bearing item consumes the
/// record (never a consumable afterwards); returns `Unhandled` for non-container positions
/// or content-less items so consumable handling still runs.
pub(crate) fn apply_native_nested_content_use_action(
    ctx: &mut SessionContext<'_>,
    position: NativeOtClientPosition,
    open_corpse_windows: &BTreeMap<u8, (Position, usize)>,
    open_content_windows: &mut BTreeMap<u8, (u8, usize)>,
) -> Result<SessionActionOutcome, HostError> {
    // Nested content window: using an item inside an owned-container window that
    // itself holds content presents those contents as a child window. Items without
    // contents fall through to the consumable handler below.
    if !(position.x == 0xffff && position.y & 0x40 != 0) {
        return Ok(SessionActionOutcome::Unhandled);
    }
    let parent_container_id = (position.y & 0x0f) as u8;
    let item_index = usize::from(position.z);
    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
    let Some(item) = containers
        .container(parent_container_id)
        .and_then(|container| container.items.item(item_index))
        .filter(|item| !item.contents().is_empty())
        .cloned()
    else {
        // No contents on this item: fall through to consumable handling.
        return Ok(SessionActionOutcome::Unhandled);
    };
    let mut busy: BTreeSet<u8> = containers
        .iter()
        .map(|(_, container)| container.container_id)
        .collect();
    busy.extend(open_corpse_windows.keys().copied());
    busy.extend(open_content_windows.keys().copied());
    if let Some(window_id) = (0..=15u8).find(|id| !busy.contains(id)) {
        if let Some(frame) = native_nested_content_window_frame(
            &ctx.config.client_profile,
            ctx.config.item_presentation_catalog.as_deref(),
            window_id,
            parent_container_id,
            &item,
        )
        .map_err(HostError::Protocol)?
        {
            write_frame(&mut *ctx.stream, &frame)?;
            open_content_windows.insert(window_id, (parent_container_id, item_index));
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
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
    Ok(SessionActionOutcome::Handled)
}

/// Opens the lowest owned top-level container when the equipped backpack itself is used.
/// Returns `Handled` for backpack positions (with or without effect), `Unhandled` otherwise
/// so corpse and map routing still run.
pub(crate) fn apply_native_backpack_use_action(
    ctx: &mut SessionContext<'_>,
    position: NativeOtClientPosition,
    closed_container_ids: &mut BTreeSet<u8>,
    sent_container_windows: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
    observed_containers_epoch: &mut u64,
) -> Result<SessionActionOutcome, HostError> {
    if !(position.x == 0xffff
        && position.y & 0x40 == 0
        && EquipmentSlot::from_code(position.y as u8) == Some(EquipmentSlot::Backpack))
    {
        return Ok(SessionActionOutcome::Unhandled);
    }
    if ctx.observed_dead {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-backpack-while-dead",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let equipped_backpack = ctx
        .shared_world
        .player_equipment(ctx.character_id)
        .ok()
        .and_then(|equipment| equipment.item(EquipmentSlot::Backpack).cloned());
    if equipped_backpack.is_none() {
        return Ok(SessionActionOutcome::Handled);
    }
    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
    let open_container = containers
        .iter()
        .find(|(_, container)| !container.has_parent)
        .map(|(id, _)| id);
    let Some(container_id) = open_container else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-backpack-no-container",
        );
        return Ok(SessionActionOutcome::Handled);
    };
    if closed_container_ids.contains(&container_id) {
        closed_container_ids.remove(&container_id);
    }
    if let Some(container) = containers.container(container_id) {
        if let Some(frame) = native_classic_container_frame(
            &ctx.config.client_profile,
            ctx.config.item_presentation_catalog.as_deref(),
            container,
        )
        .map_err(HostError::Protocol)?
        {
            write_frame(&mut *ctx.stream, &frame)?;
            sent_container_windows.insert(
                container_id,
                native_rendered_container_window(
                    &ctx.config.client_profile,
                    ctx.config.item_presentation_catalog.as_deref(),
                    container,
                ),
            );
            *observed_containers_epoch = ctx.shared_world.containers_epoch();
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "action=use-item outcome=backpack-window-opened container-id={container_id}"
                ),
            );
        }
    }
    Ok(SessionActionOutcome::Handled)
}

/// Opens one runtime corpse window from the durable registry. Returns `Handled` when the
/// addressed tile holds a corpse (with or without effect), `Unhandled` when it does not so
/// map-item routing still runs.
pub(crate) fn apply_native_corpse_use_action(
    ctx: &mut SessionContext<'_>,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
    index: u8,
    map_owner: &SharedNativeMap,
    open_corpse_windows: &mut BTreeMap<u8, (Position, usize)>,
) -> Result<SessionActionOutcome, HostError> {
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
    let Some(corpse) = corpse_attempt else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let core_position = Position {
        x: position.x,
        y: position.y,
        z: position.z,
    };
    if ctx.observed_dead {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-corpse-use-while-dead",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    if client_thing_id != corpse.server_id {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-runtime-item-identity-mismatch",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let shared_snapshot = map_owner.render_snapshot()?;
    let intent = PlayerItemUseIntent::new(
        ctx.character_id,
        core_position,
        stack_position,
        corpse.server_id,
    )
    .map_err(HostError::Core)?;
    match ctx
        .shared_world
        .validate_player_item_use(shared_snapshot.as_ref(), intent)
    {
        Ok(_) => {}
        Err(HostError::Core(_)) => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=use-item outcome=deferred-corpse-unreachable",
            );
            return Ok(SessionActionOutcome::Handled);
        }
        Err(error) => return Err(error),
    }
    let open_container_ids = ctx
        .shared_world
        .player_containers(ctx.character_id)?
        .iter()
        .map(|(_, container)| container.container_id)
        .collect::<BTreeSet<_>>();
    match native_corpse_window_id(
        &open_container_ids,
        &open_corpse_windows.keys().copied().collect(),
    ) {
        Some(window_id) => {
            match native_corpse_window_frame(
                &ctx.config.client_profile,
                ctx.config.item_presentation_catalog.as_deref(),
                window_id,
                &corpse,
                ctx.config.item_name_by_server_id.as_deref(),
            )
            .map_err(HostError::Protocol)?
            {
                Some(frame) => {
                    write_frame(&mut *ctx.stream, &frame)?;
                    open_corpse_windows
                        .insert(window_id, (core_position, usize::from(stack_position)));
                    native_diagnostic(
                        ctx.config.extended_diagnostics,
                        ctx.peer,
                        &format!(
                            "action=use-item outcome=corpse-window-opened server-id={} children={} window-id={window_id} index={index}",
                            corpse.server_id,
                            corpse.children.len(),
                        ),
                    );
                }
                None => native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=use-item outcome=deferred-corpse-window-unsupported",
                ),
            }
        }
        None => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-corpse-window-capacity",
        ),
    }
    Ok(SessionActionOutcome::Handled)
}

/// Routes one map-item use: teleport pads, read-only text windows, or validated generic use.
/// Terminal for UseItem processing; every path ends the record.
pub(crate) fn apply_native_map_item_use_action(
    ctx: &mut SessionContext<'_>,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
    index: u8,
) -> Result<(), HostError> {
    let Some(world_map) = ctx.config.world_map.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-no-world-map",
        );
        return Ok(());
    };
    let Some(intent) = native_map_item_use_intent(
        ctx.config.item_presentation_catalog.as_deref(),
        ctx.character_id,
        position,
        client_thing_id,
        stack_position,
    ) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=use-item outcome=deferred-unmapped-or-ambiguous-client-thing-id client-thing-id={client_thing_id}"
            ),
        );
        return Ok(());
    };
    match ctx.shared_world.validate_player_item_use(world_map, intent) {
        Ok(outcome) => {
            if let Some(destination) = outcome.teleport_destination {
                let teleported = activate_native_map_teleport_item(
                    &mut *ctx.stream,
                    &ctx.config.client_profile,
                    ctx.snapshot,
                    &*ctx.database,
                    ctx.shared_world,
                    ctx.character_id,
                    world_map,
                    &mut *ctx.player_position,
                    *ctx.facing,
                    destination,
                )?;
                if teleported {
                    *ctx.active_click_walk = None;
                    *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
                    native_diagnostic(
                        ctx.config.extended_diagnostics,
                        ctx.peer,
                        &format!(
                            "action=use-item outcome=teleported server-id={} destination={destination:?} index={index}",
                            outcome.server_id,
                        ),
                    );
                } else {
                    native_diagnostic(
                        ctx.config.extended_diagnostics,
                        ctx.peer,
                        &format!(
                            "action=use-item outcome=deferred-teleport-destination-blocked server-id={} destination={destination:?} index={index}",
                            outcome.server_id,
                        ),
                    );
                }
            } else if let Some(text) = native_validated_map_item_text(world_map, &outcome) {
                let text_window = encode_native_otclient_read_only_text_window(
                    &ctx.config.client_profile,
                    0,
                    client_thing_id,
                    text,
                )
                .map_err(HostError::Protocol)?;
                write_frame(&mut *ctx.stream, &text_window)?;
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "action=use-item outcome=read-only-text-window server-id={} text-bytes={} index={index}",
                        outcome.server_id,
                        text.len(),
                    ),
                );
            } else {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
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
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-invalid-server-owned-map-item",
        ),
        Err(error) => return Err(error),
    }
    Ok(())
}
