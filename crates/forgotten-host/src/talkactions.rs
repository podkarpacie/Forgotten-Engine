//! Bounded sandboxed TFS talkaction dispatch. Splits a Say message into a trigger word and
//! argument, then routes the word through the resource-capped callback dispatcher. Returned
//! effects are neutral intents the caller must validate and apply; a script can never reach
//! filesystem, network, package/debug modules, or authoritative world state.

use forgotten_scripting::{
    SandboxedLuaCallbackDispatchState, SandboxedLuaCallbackDispatcher, SandboxedLuaCallbackInput,
    SandboxedLuaEffect,
};

/// Dispatches one operator-registered talkaction word. Returns `Some(effects)` when the word is a
/// registered callback (the effect list may be empty if the script requested none or failed a
/// bound); returns `None` only for an unknown word so the caller can fall through to normal chat.
pub(crate) fn dispatch_native_lua_talkaction(
    dispatcher: &SandboxedLuaCallbackDispatcher,
    message: &str,
    player_id: u64,
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
        },
    );
    match outcome.state {
        SandboxedLuaCallbackDispatchState::CallbackNotFound => None,
        _ => Some(outcome.effects),
    }
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
            dispatch_native_lua_talkaction(&dispatcher, "/echo 100 100", 7),
            Some(vec![SandboxedLuaEffect::Say("100 100".into())])
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/goto", 7),
            Some(vec![SandboxedLuaEffect::Teleport { x: 1, y: 2, z: 7 }])
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/silent", 7),
            Some(vec![])
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/missing", 7),
            None
        );
        // A registered word whose argument is over the bound is still handled, with no effects.
        let long = "x".repeat(256);
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, &format!("/silent {long}"), 7),
            Some(vec![])
        );
    }
}
