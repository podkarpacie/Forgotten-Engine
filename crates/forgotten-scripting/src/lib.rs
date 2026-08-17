//! Transparent TFS Lua compatibility inventory and non-executing dispatch boundary.
//!
//! This crate intentionally has no Lua runtime dependency. Its first dispatch interface accepts
//! only typed aggregate inventory metadata; it cannot receive a script path or source body and
//! always returns a deferred no-op outcome.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Implemented,
    Planned,
    Unsupported,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Planned => "planned",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiEntry {
    pub api: &'static str,
    pub capability: Capability,
    pub note: &'static str,
}

const MATRIX: &[ApiEntry] = &[
    ApiEntry {
        api: "Player:getLevel()",
        capability: Capability::Implemented,
        note: "Backed by forgotten-core Player level.",
    },
    ApiEntry {
        api: "Player:addExperience()",
        capability: Capability::Implemented,
        note: "Backed by forgotten-core progression.",
    },
    ApiEntry {
        api: "Player:getPosition()",
        capability: Capability::Implemented,
        note: "Backed by forgotten-core Position.",
    },
    ApiEntry {
        api: "Game.createItem()",
        capability: Capability::Planned,
        note: "Item-domain implementation required.",
    },
    ApiEntry {
        api: "Game.createMonster()",
        capability: Capability::Planned,
        note: "Creature spawning implementation required.",
    },
    ApiEntry {
        api: "addEvent()",
        capability: Capability::Planned,
        note: "Scheduler contract required.",
    },
    ApiEntry {
        api: "stopEvent()",
        capability: Capability::Planned,
        note: "Scheduler contract required.",
    },
];

pub fn compatibility_matrix() -> &'static [ApiEntry] {
    MATRIX
}

pub fn find_api(name: &str) -> Option<ApiEntry> {
    MATRIX.iter().copied().find(|entry| entry.api == name)
}

/// A typed TFS registry family that may later produce a sandboxed script event. This enum holds
/// no path, script name, source code, or operator-owned content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredScriptEventKind {
    Action,
    CreatureScript,
    Event,
    GlobalEvent,
    Movement,
    Spell,
    TalkAction,
    Weapon,
    Monster,
    Npc,
}

impl DeferredScriptEventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::CreatureScript => "creaturescript",
            Self::Event => "event",
            Self::GlobalEvent => "globalevent",
            Self::Movement => "movement",
            Self::Spell => "spell",
            Self::TalkAction => "talkaction",
            Self::Weapon => "weapon",
            Self::Monster => "monster",
            Self::Npc => "npc",
        }
    }
}

/// Safe aggregate input created from the TFS content audit. Counts communicate readiness without
/// exposing script references or granting authority to access their local files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredScriptEvent {
    pub kind: DeferredScriptEventKind,
    pub reference_count: usize,
    pub missing_reference_count: usize,
    pub unsafe_reference_count: usize,
}

/// The only result available from the initial dispatch boundary. A later sandbox must introduce a
/// distinct, explicitly reviewed execution result rather than changing this no-op contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredScriptDispatchState {
    DeferredNoop,
}

impl DeferredScriptDispatchState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeferredNoop => "deferred-noop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredScriptDispatchOutcome {
    pub event: DeferredScriptEvent,
    pub state: DeferredScriptDispatchState,
}

/// Boundary for future sandboxed dispatch. Implementations receive metadata only in this first
/// stage; no script path or body is available to execute.
pub trait ScriptEventDispatcher {
    fn dispatch(&self, event: DeferredScriptEvent) -> DeferredScriptDispatchOutcome;
}

/// The only currently supported dispatcher. It records that an audited registry category was
/// considered, but it performs no file I/O, no parsing, no process spawning, and no Lua execution.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopDeferredScriptExecutor;

impl ScriptEventDispatcher for NoopDeferredScriptExecutor {
    fn dispatch(&self, event: DeferredScriptEvent) -> DeferredScriptDispatchOutcome {
        DeferredScriptDispatchOutcome {
            event,
            state: DeferredScriptDispatchState::DeferredNoop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_claims_unknown_api_as_supported() {
        assert_eq!(find_api("doCreatureSay()"), None);
        assert_eq!(
            find_api("Player:getLevel()").unwrap().capability,
            Capability::Implemented
        );
    }

    #[test]
    fn no_op_dispatcher_preserves_only_aggregate_audit_metadata() {
        let event = DeferredScriptEvent {
            kind: DeferredScriptEventKind::TalkAction,
            reference_count: 3,
            missing_reference_count: 1,
            unsafe_reference_count: 2,
        };
        let outcome = NoopDeferredScriptExecutor.dispatch(event);
        assert_eq!(outcome.event, event);
        assert_eq!(outcome.event.kind.label(), "talkaction");
        assert_eq!(outcome.state, DeferredScriptDispatchState::DeferredNoop);
        assert_eq!(outcome.state.as_str(), "deferred-noop");
    }
}
