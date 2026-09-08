//! Bounded sandboxed TFS talkaction dispatch. Splits a Say message into a trigger word and
//! argument, then routes the word through the resource-capped callback dispatcher. Only a
//! non-empty text return produces a reply; a script can never reach filesystem, network,
//! package/debug modules, or authoritative world state.

use forgotten_scripting::{
    SandboxedLuaCallbackDispatcher, SandboxedLuaCallbackInput, SandboxedLuaValue,
};

/// Dispatches one operator-registered talkaction word. Returns the script's reply text only when
/// the word is registered and the sandbox returns a non-empty string; an unknown word, rejected
/// input, instruction/memory exhaustion, a runtime error, or a non-text return all yield `None`.
pub(crate) fn dispatch_native_lua_talkaction(
    dispatcher: &SandboxedLuaCallbackDispatcher,
    message: &str,
    player_id: u64,
) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (words, argument) = match trimmed.find(|c: char| c.is_whitespace()) {
        Some(index) => (&trimmed[..index], trimmed[index..].trim().to_owned()),
        None => (trimmed, String::new()),
    };
    let outcome = dispatcher.dispatch(
        words,
        &SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: player_id,
            value: 0,
            argument,
        },
    );
    match outcome.value {
        Some(SandboxedLuaValue::Text(text)) if !text.trim().is_empty() => Some(text),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn talkaction_returns_text_only_for_registered_words() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "/echo",
                "return function(kind, _, _, argument) return argument end",
            )
            .unwrap();
        dispatcher
            .register_callback("/silent", "return function() return 42 end")
            .unwrap();

        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/echo 100 100", 7),
            Some("100 100".to_owned())
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/echo", 7),
            None // empty text reply is suppressed
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/silent", 7),
            None
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/missing", 7),
            None
        );
        assert_eq!(dispatch_native_lua_talkaction(&dispatcher, "", 7), None);
    }

    #[test]
    fn talkaction_splits_first_word_from_the_argument() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "/word",
                "return function(kind, _, _, argument) return '[' .. argument .. ']' end",
            )
            .unwrap();
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/word a b c", 1),
            Some("[a b c]".to_owned())
        );
        assert_eq!(
            dispatch_native_lua_talkaction(&dispatcher, "/word", 1),
            Some("[]".to_owned()) // no whitespace -> empty argument, still a non-empty reply
        );
    }
}
