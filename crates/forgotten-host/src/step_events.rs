//! Live sandboxed TFS step-event routing for successful player displacements. When a
//! step lands on (StepIn) or leaves (StepOut) a tile carrying an item id matching a
//! registered selector (singleton or range, first-match in document order), the match
//! dispatches through the resource-capped movement dispatcher and the returned intents
//! apply against authoritative subject state through the shared effect arms. Effect
//! application is intentionally NOT duplicated here: it reuses
//! `apply_native_action_effects` with a movement outcome tag. Equip and the remaining
//! movement families stay deferred; chained teleports (a step effect landing on
//! another scripted tile) deliberately do not re-trigger — one level only.

use super::*;
use forgotten_scripting::{
    SandboxedLuaCallbackDispatchState, SandboxedLuaCallbackInput, SandboxedLuaPosition,
};

/// Routes one arrival tile through registered StepIn scripts. Runs after any
/// successful player displacement (manual steps, click-walk scheduler steps, and
/// stepped-on teleports, all observed via position change); no-ops when no movement
/// dispatcher is configured or nothing on the tile matches. The move itself always
/// stands — scripts observe arrivals, they never veto them.
pub(crate) fn apply_native_step_in(
    ctx: &mut SessionContext<'_>,
    arrival: Position,
) -> Result<(), HostError> {
    dispatch_native_step_event(ctx, TfsMoveEventType::StepIn, arrival, "step")
}

/// Routes one departed tile through registered StepOut scripts. Runs alongside
/// StepIn after any successful displacement, before the arrival routing; same
/// no-op contract. The move itself always stands.
pub(crate) fn apply_native_step_out(
    ctx: &mut SessionContext<'_>,
    departed: Position,
) -> Result<(), HostError> {
    dispatch_native_step_event(ctx, TfsMoveEventType::StepOut, departed, "step-out")
}

/// Routes one equipped item through registered Equip scripts. Callers invoke this
/// after a successful move into an equipment slot with the slot's post-move item id;
/// Equip-slot string matching stays deferred (entries match on item id for now).
/// Deliberately mirrors the step-event routing shell instead of sharing it: the match
/// source (slot read vs tile scan) and position differ, and the codebase prefers an
/// explicit small duplicate over a clever parameter here (same call as the
/// talkaction/action effect-arms split). Effect arms are still shared.
pub(crate) fn fire_native_equip_event(
    ctx: &mut SessionContext<'_>,
    server_id: u16,
) -> Result<(), HostError> {
    let (Some(registry), Some(dispatcher)) = (
        ctx.config.movement_registry.as_deref(),
        ctx.config.movement_dispatcher.as_deref(),
    ) else {
        return Ok(());
    };
    let Some((callback_name, _)) =
        resolve_movement_callback(registry, TfsMoveEventType::Equip, server_id)
    else {
        return Ok(());
    };
    let subject_position = ctx.shared_world.player_position(ctx.character_id)?;
    let outcome = dispatcher.dispatch_api(
        &callback_name,
        &SandboxedLuaCallbackInput {
            event_kind: "movement".into(),
            subject_id: ctx.character_id,
            value: i64::from(server_id),
            argument: String::new(),
            position: Some(SandboxedLuaPosition {
                x: subject_position.x,
                y: subject_position.y,
                z: subject_position.z,
            }),
        },
    );
    match outcome.state {
        SandboxedLuaCallbackDispatchState::Completed => {
            apply_native_action_effects(
                ctx.config,
                &mut *ctx.stream,
                &mut *ctx.database,
                ctx.shared_world,
                ctx.character_id,
                ctx.peer,
                ctx.snapshot,
                *ctx.facing,
                &mut *ctx.player_position,
                &mut *ctx.observed_visibility_epoch,
                &mut *ctx.observed_vitals_epoch,
                ctx.world_map,
                outcome.effects,
                "movement=equip outcome=equip-applied",
            )?;
        }
        SandboxedLuaCallbackDispatchState::CallbackNotFound => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!("movement=equip outcome=equip-script-missing key={callback_name}"),
            );
        }
        _ => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "movement=equip outcome=equip-script-rejected key={callback_name} state={:?}",
                    outcome.state,
                ),
            );
        }
    }
    Ok(())
}

/// Routes one unequipped item through registered DeEquip scripts. Callers invoke
/// this when an equipment slot stops holding an item to the outside world (full
/// moves out, vacating merges, ground drops that empty the slot, container swaps
/// displacing the occupant); partial moves leaving a remainder stay silent, as do
/// internal equipment-to-equipment transfers. Equip-slot string matching stays
/// deferred like Equip. Same documented duplicate shell as Equip.
pub(crate) fn fire_native_deequip_event(
    ctx: &mut SessionContext<'_>,
    server_id: u16,
) -> Result<(), HostError> {
    let (Some(registry), Some(dispatcher)) = (
        ctx.config.movement_registry.as_deref(),
        ctx.config.movement_dispatcher.as_deref(),
    ) else {
        return Ok(());
    };
    let Some((callback_name, _)) =
        resolve_movement_callback(registry, TfsMoveEventType::DeEquip, server_id)
    else {
        return Ok(());
    };
    let subject_position = ctx.shared_world.player_position(ctx.character_id)?;
    let outcome = dispatcher.dispatch_api(
        &callback_name,
        &SandboxedLuaCallbackInput {
            event_kind: "movement".into(),
            subject_id: ctx.character_id,
            value: i64::from(server_id),
            argument: String::new(),
            position: Some(SandboxedLuaPosition {
                x: subject_position.x,
                y: subject_position.y,
                z: subject_position.z,
            }),
        },
    );
    match outcome.state {
        SandboxedLuaCallbackDispatchState::Completed => {
            apply_native_action_effects(
                ctx.config,
                &mut *ctx.stream,
                &mut *ctx.database,
                ctx.shared_world,
                ctx.character_id,
                ctx.peer,
                ctx.snapshot,
                *ctx.facing,
                &mut *ctx.player_position,
                &mut *ctx.observed_visibility_epoch,
                &mut *ctx.observed_vitals_epoch,
                ctx.world_map,
                outcome.effects,
                "movement=deequip outcome=deequip-applied",
            )?;
        }
        SandboxedLuaCallbackDispatchState::CallbackNotFound => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!("movement=deequip outcome=deequip-script-missing key={callback_name}"),
            );
        }
        _ => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "movement=deequip outcome=deequip-script-rejected key={callback_name} state={:?}",
                    outcome.state,
                ),
            );
        }
    }
    Ok(())
}

/// Shared StepIn/StepOut resolve-dispatch-apply core. `kind_tag` feeds the outcome
/// diagnostics (`step` vs `step-out`); the effect arms keep their shared tag.
fn dispatch_native_step_event(
    ctx: &mut SessionContext<'_>,
    movement_type: TfsMoveEventType,
    tile: Position,
    kind_tag: &'static str,
) -> Result<(), HostError> {
    let (Some(registry), Some(dispatcher)) = (
        ctx.config.movement_registry.as_deref(),
        ctx.config.movement_dispatcher.as_deref(),
    ) else {
        return Ok(());
    };
    let Some(tile_items) = ctx.world_map.tile_items(tile) else {
        return Ok(());
    };
    let Some((matched_id, callback_name)) = tile_items.iter().find_map(|item| {
        resolve_movement_callback(registry, movement_type, item.server_id)
            .map(|(callback_name, _)| (item.server_id, callback_name))
    }) else {
        return Ok(());
    };
    let outcome = dispatcher.dispatch_api(
        &callback_name,
        &SandboxedLuaCallbackInput {
            event_kind: "movement".into(),
            subject_id: ctx.character_id,
            value: i64::from(matched_id),
            argument: String::new(),
            position: Some(SandboxedLuaPosition {
                x: tile.x,
                y: tile.y,
                z: tile.z,
            }),
        },
    );
    match outcome.state {
        SandboxedLuaCallbackDispatchState::Completed => {
            apply_native_action_effects(
                ctx.config,
                &mut *ctx.stream,
                &mut *ctx.database,
                ctx.shared_world,
                ctx.character_id,
                ctx.peer,
                ctx.snapshot,
                *ctx.facing,
                &mut *ctx.player_position,
                &mut *ctx.observed_visibility_epoch,
                &mut *ctx.observed_vitals_epoch,
                ctx.world_map,
                outcome.effects,
                "movement=step outcome=step-applied",
            )?;
        }
        // The builder registers every registry entry, so a resolved match always has
        // a callback; a miss means registry/dispatcher skew, logged, move stands.
        SandboxedLuaCallbackDispatchState::CallbackNotFound => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!("movement={kind_tag} outcome=step-script-missing key={callback_name}"),
            );
        }
        _ => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "movement={kind_tag} outcome=step-script-rejected key={callback_name} state={:?}",
                    outcome.state,
                ),
            );
        }
    }
    Ok(())
}
