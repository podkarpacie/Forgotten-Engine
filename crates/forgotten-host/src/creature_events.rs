//! Live sandboxed TFS creature-event routing for session lifecycle and kills. Login
//! scripts run once per session right after bootstrap; static-creature kills route
//! after rewards settle. Every matching entry dispatches in registry order and the
//! returned intents union (capped at the shared per-dispatch effect bound) before
//! applying against authoritative subject state through the shared effect arms. One
//! failing script never blocks the others — fail-open per script, logged. Logout,
//! PvP kills, and the remaining creature families stay deferred.

use super::*;
use forgotten_scripting::{
    SandboxedLuaCallbackDispatchState, SandboxedLuaCallbackInput, SandboxedLuaEffect,
    SandboxedLuaPosition, MAX_SANDBOXED_LUA_EFFECTS,
};

/// Routes all registered login scripts for a fresh session. No-ops when no creature
/// dispatcher is configured or nothing registers `login`. Effect application reuses
/// the shared arms with a login outcome tag.
pub(crate) fn apply_native_login_scripts(ctx: &mut SessionContext<'_>) -> Result<(), HostError> {
    let position = *ctx.player_position;
    let effects = dispatch_creature_event_effects(
        ctx,
        "login",
        ctx.character_id,
        0,
        String::new(),
        position,
    )?;
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

/// Routes one static-creature kill through registered `kill` scripts. The killer is
/// the dispatch subject; the victim id and name cross as value and argument, the
/// killer position as the subject-relative read. Same union, cap, and fail-open
/// contract as login. PvP kills stay deferred.
pub(crate) fn fire_native_kill_event(
    ctx: &mut SessionContext<'_>,
    victim_id: u32,
    victim_name: &str,
) -> Result<(), HostError> {
    let position = *ctx.player_position;
    let effects = dispatch_creature_event_effects(
        ctx,
        "kill",
        ctx.character_id,
        i64::from(victim_id),
        victim_name.to_owned(),
        position,
    )?;
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
        "creature=kill outcome=kill-applied",
    )?;
    Ok(())
}

/// Shared creature-event resolve-dispatch-union core: runs every entry of one event
/// type in registry order, concatenating Completed intents up to the shared
/// per-dispatch effect bound. Failed scripts log and yield nothing; the union never
/// fails closed because of one bad script.
fn dispatch_creature_event_effects(
    ctx: &mut SessionContext<'_>,
    event_type: &str,
    subject_id: u64,
    value: i64,
    argument: String,
    position: Position,
) -> Result<Vec<SandboxedLuaEffect>, HostError> {
    let (Some(registry), Some(dispatcher)) = (
        ctx.config.creature_registry.as_deref(),
        ctx.config.creature_dispatcher.as_deref(),
    ) else {
        return Ok(Vec::new());
    };
    let mut effects = Vec::new();
    for (index, entry) in resolve_creature_entries(registry, event_type) {
        let callback_name = creature_callback_name(index);
        let outcome = dispatcher.dispatch_api(
            &callback_name,
            &SandboxedLuaCallbackInput {
                event_kind: "creaturescript".into(),
                subject_id,
                value,
                argument: argument.clone(),
                position: Some(SandboxedLuaPosition {
                    x: position.x,
                    y: position.y,
                    z: position.z,
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
                        "creature={event_type} outcome=script-rejected name={} state={:?}",
                        entry.name, outcome.state,
                    ),
                );
            }
        }
    }
    Ok(effects)
}
