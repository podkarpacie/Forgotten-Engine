//! Bounded sandboxed TFS talkaction dispatch and effect application. Splits a Say message
//! into a trigger word and argument, routes the word through the resource-capped callback
//! dispatcher, then validates and applies the returned intents against authoritative state.
//! Returned effects are neutral intents the caller must validate and apply; a script can never
//! reach filesystem, network, package/debug modules, or authoritative world state.

use super::*;
use forgotten_protocol::NativeOtClientTalkRequest;
use forgotten_scripting::{
    SandboxedLuaCallbackDispatchState, SandboxedLuaCallbackDispatcher, SandboxedLuaCallbackInput,
    SandboxedLuaEffect, SandboxedLuaPosition,
};

/// Dispatches one operator-registered talkaction word. Returns `Some(effects)` when the word is a
/// registered callback (the effect list may be empty if the script requested none or failed a
/// bound); returns `None` only for an unknown word so the caller can fall through to normal chat.
/// The optional authoritative subject position is forwarded so scripts can read a
/// `getThingPos`-style coordinate without any world access.
pub(crate) fn dispatch_native_lua_talkaction(
    dispatcher: &SandboxedLuaCallbackDispatcher,
    message: &str,
    player_id: u64,
    position: Option<SandboxedLuaPosition>,
) -> Option<Vec<SandboxedLuaEffect>> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (words, argument) = match trimmed.find(|c: char| c.is_whitespace()) {
        Some(index) => (&trimmed[..index], trimmed[index..].trim().to_owned()),
        None => (trimmed, String::new()),
    };
    let outcome = dispatcher.dispatch_effects(
        words,
        &SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: player_id,
            value: 0,
            argument,
            position,
        },
    );
    match outcome.state {
        SandboxedLuaCallbackDispatchState::CallbackNotFound => None,
        _ => Some(outcome.effects),
    }
}

/// Applies one operator-registered Lua talkaction for a Say record. Returns
/// `SessionActionOutcome::Handled` when the word was handled (the caller must `continue` to the
/// next session action); returns `Unhandled` for a non-Say record or an unknown word so the
/// caller falls through to ordinary routing. Effect application is identical to the former
/// inline session-loop block: bounded typed effects are validated and applied against
/// authoritative state, teleports resend the viewport, and every mutation persists before any
/// client frame is emitted.
pub(crate) fn apply_native_lua_talkaction(
    ctx: &mut SessionContext<'_>,
    request: &NativeOtClientTalkRequest,
    dispatcher: &SandboxedLuaCallbackDispatcher,
) -> Result<SessionActionOutcome, HostError> {
    if !(request.mode == NATIVE_OTCLIENT_MESSAGE_SAY
        && request.channel_id.is_none()
        && request.recipient.is_none())
    {
        return Ok(SessionActionOutcome::Unhandled);
    }
    let subject_position = ctx.shared_world.player_position(ctx.character_id)?;
    let subject_position = Some(SandboxedLuaPosition {
        x: subject_position.x,
        y: subject_position.y,
        z: subject_position.z,
    });
    let Some(effects) = dispatch_native_lua_talkaction(
        dispatcher,
        &request.message,
        ctx.character_id,
        subject_position,
    ) else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let mut teleported = false;
    for effect in effects {
        match effect {
            SandboxedLuaEffect::Say(text) => {
                let reply_frame =
                    encode_native_otclient_status_message(&ctx.config.client_profile, &text)
                        .map_err(HostError::Protocol)?;
                write_frame(&mut *ctx.stream, &reply_frame)?;
            }
            SandboxedLuaEffect::Teleport { x, y, z } => {
                let destination = Position { x, y, z };
                if ctx
                    .shared_world
                    .teleport_player_for_operator(ctx.character_id, destination)
                    .is_ok()
                {
                    *ctx.player_position = destination;
                    teleported = true;
                } else {
                    let reply_frame = encode_native_otclient_status_message(
                        &ctx.config.client_profile,
                        "That destination is blocked.",
                    )
                    .map_err(HostError::Protocol)?;
                    write_frame(&mut *ctx.stream, &reply_frame)?;
                }
            }
            SandboxedLuaEffect::Heal { health, mana } => {
                let mut vitals = ctx.shared_world.player_vitals(ctx.character_id)?;
                if health > 0 {
                    vitals.health = vitals.health.saturating_add(health).min(vitals.max_health);
                }
                if mana > 0 {
                    vitals.mana = vitals.mana.saturating_add(mana).min(vitals.max_mana);
                }
                ctx.shared_world
                    .lock()?
                    .update_player_vitals(ctx.character_id, vitals)
                    .map_err(HostError::Core)?;
                ctx.shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
                ctx.database.update_player_vitals(
                    ctx.character_id,
                    PersistedPlayerVitals {
                        health: vitals.health,
                        max_health: vitals.max_health,
                        mana: vitals.mana,
                        max_mana: vitals.max_mana,
                        capacity: vitals.capacity,
                        magic_level: vitals.magic_level,
                    },
                )?;
                let self_native_id = native_player_id(ctx.character_id)?;
                let health_update = encode_native_otclient_creature_health(
                    &ctx.config.client_profile,
                    self_native_id,
                    vitals.health,
                    vitals.max_health,
                )
                .map_err(HostError::Protocol)?;
                write_frame(&mut *ctx.stream, &health_update)?;
                *ctx.observed_vitals_epoch = ctx.shared_world.vitals_epoch();
            }
            SandboxedLuaEffect::GiveItem { id, count } => {
                if let Some(message) = give_items_to_player(
                    ctx.shared_world,
                    &mut *ctx.database,
                    ctx.character_id,
                    id,
                    u64::from(count),
                )? {
                    let reply_frame =
                        encode_native_otclient_status_message(&ctx.config.client_profile, &message)
                            .map_err(HostError::Protocol)?;
                    write_frame(&mut *ctx.stream, &reply_frame)?;
                }
            }
            SandboxedLuaEffect::RemoveItem { id, count } => {
                if let Some(message) = remove_items_from_player(
                    ctx.shared_world,
                    &mut *ctx.database,
                    ctx.character_id,
                    id,
                    u64::from(count),
                )? {
                    let reply_frame =
                        encode_native_otclient_status_message(&ctx.config.client_profile, &message)
                            .map_err(HostError::Protocol)?;
                    write_frame(&mut *ctx.stream, &reply_frame)?;
                }
            }
            SandboxedLuaEffect::MagicEffect { x, y, z, kind } => {
                let effect_frame = encode_native_otclient_magic_effect(
                    &ctx.config.client_profile,
                    native_position(Position { x, y, z }),
                    kind,
                )
                .map_err(HostError::Protocol)?;
                write_frame(&mut *ctx.stream, &effect_frame)?;
            }
        }
    }
    if teleported {
        ctx.shared_world.mark_visibility_changed();
        let mut refreshed_snapshot = ctx.snapshot.clone();
        refreshed_snapshot.player_position = native_position(*ctx.player_position);
        refreshed_snapshot.player_direction = ctx.facing.protocol_direction();
        let refreshed_viewport = encode_shared_native_world_viewport(
            &ctx.config.client_profile,
            &refreshed_snapshot,
            ctx.world_map.as_ref(),
            ctx.shared_world,
            ctx.character_id,
        )?;
        let refreshed_static_spawns = ctx.shared_world.active_static_spawns()?;
        let refreshed_static_health_frames = native_static_creature_health_frames(
            &ctx.config.client_profile,
            &refreshed_static_spawns,
        )?;
        write_frame(&mut *ctx.stream, &refreshed_viewport)?;
        for frame in &refreshed_static_health_frames {
            write_frame(&mut *ctx.stream, frame)?;
        }
        *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
    }
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        "action=talk outcome=lua-talkaction",
    );
    Ok(SessionActionOutcome::Handled)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn talkaction_returns_effects_only_for_registered_words() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "/echo",
                "return function(kind, _, _, argument) return { { say = argument } } end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "/goto",
                "return function() return { { teleport = { x = 1, y = 2, z = 7 } } } end",
            )
            .unwrap();
        dispatcher
            .register_callback("/silent", "return function() return {} end")
            .unwrap();

        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/echo 100 100", 7, None),
            Some(vec![SandboxedLuaEffect::Say("100 100".into())])
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/goto", 7, None),
            Some(vec![SandboxedLuaEffect::Teleport { x: 1, y: 2, z: 7 }])
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/silent", 7, None),
            Some(vec![])
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/missing", 7, None),
            None
        );
        // A registered word whose argument is over the bound is still handled, with no effects.
        let long = "x".repeat(256);
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, &format!("/silent {long}"), 7, None),
            Some(vec![])
        );
    }
}
