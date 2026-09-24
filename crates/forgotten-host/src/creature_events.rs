//! Live sandboxed TFS creature-event routing for session lifecycle. Login scripts run
//! once per session right after bootstrap, before the first loop pass; every `login`
//! entry dispatches in registry order and the returned intents union (capped at the
//! shared per-dispatch effect bound) before applying against authoritative subject
//! state through the shared effect arms. One failing script never blocks the others
//! or the login itself — fail-open per script, logged. Logout, kill, and the
//! remaining creature families stay deferred.

use super::*;
use forgotten_scripting::{
    SandboxedLuaCallbackDispatchState, SandboxedLuaCallbackInput, SandboxedLuaPosition,
    MAX_SANDBOXED_LUA_EFFECTS,
};

/// Routes all registered login scripts for a fresh session. No-ops when no creature
/// dispatcher is configured or nothing registers `login`. Effect application reuses
/// the shared arms with a login outcome tag.
pub(crate) fn apply_native_login_scripts(ctx: &mut SessionContext<'_>) -> Result<(), HostError> {
    let (Some(registry), Some(dispatcher)) = (
        ctx.config.creature_registry.as_deref(),
        ctx.config.creature_dispatcher.as_deref(),
    ) else {
        return Ok(());
    };
    let mut effects = Vec::new();
    for (index, entry) in resolve_creature_entries(registry, "login") {
        let callback_name = creature_callback_name(index);
        let outcome = dispatcher.dispatch_api(
            &callback_name,
            &SandboxedLuaCallbackInput {
                event_kind: "creaturescript".into(),
                subject_id: ctx.character_id,
                value: 0,
                argument: String::new(),
                position: Some(SandboxedLuaPosition {
                    x: ctx.player_position.x,
                    y: ctx.player_position.y,
                    z: ctx.player_position.z,
                }),
            },
        );
        match outcome.state {
            SandboxedLuaCallbackDispatchState::Completed => {
                effects.extend(outcome.effects);
                if effects.len() >= MAX_SANDBOXED_LUA_EFFECTS {
                    effects.truncate(MAX_SANDBOXED_LUA_EFFECTS);
                    break;
                }
            }
            _ => {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "creature=login outcome=login-script-rejected name={} state={:?}",
                        entry.name, outcome.state,
                    ),
                );
            }
        }
    }
    if effects.is_empty() {
        return Ok(());
    }
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
        effects,
        "creature=login outcome=login-applied",
    )?;
    Ok(())
}
