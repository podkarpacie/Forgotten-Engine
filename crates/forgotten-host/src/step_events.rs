//! Live sandboxed TFS StepIn routing for successful player displacements. When a step
//! lands on a tile carrying an item id matching a registered StepIn selector (singleton
//! or range, first-match in document order), the match dispatches through the
//! resource-capped movement dispatcher and the returned intents apply against
//! authoritative subject state through the shared effect arms. Effect application is
//! intentionally NOT duplicated here: it reuses `apply_native_action_effects` with a
//! movement outcome tag. StepOut, Equip, and the remaining movement families stay
//! deferred; chained teleports (a StepIn effect landing on another scripted tile)
//! deliberately do not re-trigger — one level only.

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
    let (Some(registry), Some(dispatcher)) = (
        ctx.config.movement_registry.as_deref(),
        ctx.config.movement_dispatcher.as_deref(),
    ) else {
        return Ok(());
    };
    let Some(tile_items) = ctx.world_map.tile_items(arrival) else {
        return Ok(());
    };
    let Some((matched_id, callback_name)) = tile_items.iter().find_map(|item| {
        resolve_movement_callback(registry, TfsMoveEventType::StepIn, item.server_id)
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
                x: arrival.x,
                y: arrival.y,
                z: arrival.z,
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
                &format!("movement=step outcome=step-script-missing key={callback_name}"),
            );
        }
        _ => {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "movement=step outcome=step-script-rejected key={callback_name} state={:?}",
                    outcome.state,
                ),
            );
        }
    }
    Ok(())
}
