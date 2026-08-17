//! Transparent TFS Lua compatibility inventory and non-executing dispatch boundary.
//!
//! This crate intentionally has no Lua runtime dependency. Its first dispatch interface accepts
//! only typed aggregate inventory metadata; it cannot receive a script path or source body and
//! always returns a deferred no-op outcome.

use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Value};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

pub const MAX_SANDBOXED_LUA_SOURCE_BYTES: usize = 4 * 1024;
pub const MAX_SANDBOXED_LUA_MEMORY_BYTES: usize = 64 * 1024;
pub const MAX_SANDBOXED_LUA_INSTRUCTIONS: u32 = 10_000;
pub const MAX_SANDBOXED_LUA_CALLBACKS: usize = 64;
pub const MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES: usize = 64;
const INSTRUCTION_LIMIT_MARKER: &str = "forgotten-engine-sandbox-instruction-limit";

/// Explicit limits for one side-effect-free Lua expression evaluation. The executor creates a
/// fresh VM per call with no standard libraries, so it offers no file, network, process, package,
/// debug, or host API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxedLuaLimits {
    pub max_source_bytes: usize,
    pub max_memory_bytes: usize,
    pub max_instructions: u32,
}

impl Default for SandboxedLuaLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: MAX_SANDBOXED_LUA_SOURCE_BYTES,
            max_memory_bytes: MAX_SANDBOXED_LUA_MEMORY_BYTES,
            max_instructions: MAX_SANDBOXED_LUA_INSTRUCTIONS,
        }
    }
}

impl SandboxedLuaLimits {
    pub fn new(
        max_source_bytes: usize,
        max_memory_bytes: usize,
        max_instructions: u32,
    ) -> Result<Self, SandboxedLuaLimitError> {
        if max_source_bytes == 0 || max_source_bytes > MAX_SANDBOXED_LUA_SOURCE_BYTES {
            return Err(SandboxedLuaLimitError::InvalidSourceLimit(max_source_bytes));
        }
        if max_memory_bytes == 0 || max_memory_bytes > MAX_SANDBOXED_LUA_MEMORY_BYTES {
            return Err(SandboxedLuaLimitError::InvalidMemoryLimit(max_memory_bytes));
        }
        if max_instructions == 0 || max_instructions > MAX_SANDBOXED_LUA_INSTRUCTIONS {
            return Err(SandboxedLuaLimitError::InvalidInstructionLimit(
                max_instructions,
            ));
        }
        Ok(Self {
            max_source_bytes,
            max_memory_bytes,
            max_instructions,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxedLuaLimitError {
    InvalidSourceLimit(usize),
    InvalidMemoryLimit(usize),
    InvalidInstructionLimit(u32),
}

/// Values intentionally permitted across the sandbox boundary. Tables, functions, threads,
/// userdata, and arbitrary binary strings are rejected rather than being converted implicitly.
#[derive(Debug, Clone, PartialEq)]
pub enum SandboxedLuaValue {
    Nil,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxedLuaExecutionState {
    Completed,
    SourceRejected,
    InstructionLimitReached,
    RuntimeRejected,
    UnsupportedValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxedLuaOutcome {
    pub state: SandboxedLuaExecutionState,
    pub value: Option<SandboxedLuaValue>,
    pub instruction_checks: u32,
}

/// Executes one Lua expression within a fresh resource-capped VM. The source is wrapped as a
/// `return` expression, so normal statement scripts cannot be evaluated through this API. This is
/// not a TFS Lua runner and is deliberately separate from `ScriptEventDispatcher`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SandboxedLuaExecutor {
    limits: SandboxedLuaLimits,
}

impl SandboxedLuaExecutor {
    pub const fn new(limits: SandboxedLuaLimits) -> Self {
        Self { limits }
    }

    pub const fn limits(self) -> SandboxedLuaLimits {
        self.limits
    }

    pub fn execute_expression(self, source: &str) -> SandboxedLuaOutcome {
        if source.len() > self.limits.max_source_bytes {
            return SandboxedLuaOutcome {
                state: SandboxedLuaExecutionState::SourceRejected,
                value: None,
                instruction_checks: 0,
            };
        }
        let lua = match Lua::new_with(StdLib::NONE, LuaOptions::default()) {
            Ok(lua) => lua,
            Err(_) => return rejected_runtime_outcome(0),
        };
        if lua.set_memory_limit(self.limits.max_memory_bytes).is_err() {
            return rejected_runtime_outcome(0);
        }
        let instruction_checks = Arc::new(AtomicU32::new(0));
        let hook_checks = Arc::clone(&instruction_checks);
        let instruction_limit = self.limits.max_instructions;
        lua.set_hook(
            HookTriggers {
                every_nth_instruction: Some(1),
                ..HookTriggers::default()
            },
            move |_, _| {
                if hook_checks.fetch_add(1, Ordering::Relaxed) >= instruction_limit {
                    Err(mlua::Error::RuntimeError(INSTRUCTION_LIMIT_MARKER.into()))
                } else {
                    Ok(())
                }
            },
        );
        let wrapped = format!("return ({source})");
        let result = lua.load(&wrapped).eval::<Value>();
        let instruction_checks = instruction_checks.load(Ordering::Relaxed);
        let instruction_limit_reached = instruction_checks > self.limits.max_instructions;
        match result {
            Ok(value) => match sandboxed_lua_value(value) {
                Some(value) => SandboxedLuaOutcome {
                    state: SandboxedLuaExecutionState::Completed,
                    value: Some(value),
                    instruction_checks,
                },
                None => SandboxedLuaOutcome {
                    state: SandboxedLuaExecutionState::UnsupportedValue,
                    value: None,
                    instruction_checks,
                },
            },
            Err(_) if instruction_limit_reached => SandboxedLuaOutcome {
                state: SandboxedLuaExecutionState::InstructionLimitReached,
                value: None,
                instruction_checks,
            },
            Err(_) => rejected_runtime_outcome(instruction_checks),
        }
    }
}

/// Typed primitive arguments admitted to one explicitly registered callback. The dispatcher does
/// not expose world state, host objects, Lua tables, paths, files, network access, modules, or
/// mutable server APIs. The event kind is an operator-chosen label, not a claimed TFS callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxedLuaCallbackInput {
    pub event_kind: String,
    pub subject_id: u64,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxedLuaCallbackRegistrationError {
    InvalidName,
    DuplicateName(String),
    CallbackLimit(usize),
    SourceRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxedLuaCallbackDispatchState {
    Completed,
    CallbackNotFound,
    SourceRejected,
    InstructionLimitReached,
    RuntimeRejected,
    UnsupportedValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxedLuaCallbackDispatchOutcome {
    pub state: SandboxedLuaCallbackDispatchState,
    pub value: Option<SandboxedLuaValue>,
    pub instruction_checks: u32,
}

/// A bounded trusted-source callback registry. Registration is explicit and in-memory: it does
/// not discover files, load TFS registries, preserve global Lua state, or resolve modules. Every
/// dispatch creates a new VM and expects the source to evaluate to a function accepting exactly
/// `(event_kind, subject_id, value)` primitive arguments.
#[derive(Debug, Clone)]
pub struct SandboxedLuaCallbackDispatcher {
    limits: SandboxedLuaLimits,
    callbacks: BTreeMap<String, String>,
}

impl Default for SandboxedLuaCallbackDispatcher {
    fn default() -> Self {
        Self::new(SandboxedLuaLimits::default())
    }
}

impl SandboxedLuaCallbackDispatcher {
    pub fn new(limits: SandboxedLuaLimits) -> Self {
        Self {
            limits,
            callbacks: BTreeMap::new(),
        }
    }

    pub const fn limits(&self) -> SandboxedLuaLimits {
        self.limits
    }

    pub fn len(&self) -> usize {
        self.callbacks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.callbacks.is_empty()
    }

    /// Registers one operator-provided callback source. The source must be a Lua chunk that
    /// evaluates to a function, for example: `return function(kind, id, value) return value end`.
    /// Source execution is deferred until dispatch and takes place in a fresh restricted VM.
    pub fn register_callback(
        &mut self,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<(), SandboxedLuaCallbackRegistrationError> {
        let name = name.into();
        let source = source.into();
        if name.trim().is_empty() || name.len() > MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES {
            return Err(SandboxedLuaCallbackRegistrationError::InvalidName);
        }
        if source.len() > self.limits.max_source_bytes {
            return Err(SandboxedLuaCallbackRegistrationError::SourceRejected);
        }
        if self.callbacks.contains_key(&name) {
            return Err(SandboxedLuaCallbackRegistrationError::DuplicateName(name));
        }
        if self.callbacks.len() >= MAX_SANDBOXED_LUA_CALLBACKS {
            return Err(SandboxedLuaCallbackRegistrationError::CallbackLimit(
                MAX_SANDBOXED_LUA_CALLBACKS,
            ));
        }
        self.callbacks.insert(name, source);
        Ok(())
    }

    /// Invokes one registered callback in a new no-standard-library VM. Callback state cannot
    /// carry across invocations, and only the explicitly supplied primitive input crosses the
    /// sandbox boundary.
    pub fn dispatch(
        &self,
        callback_name: &str,
        input: &SandboxedLuaCallbackInput,
    ) -> SandboxedLuaCallbackDispatchOutcome {
        let Some(source) = self.callbacks.get(callback_name) else {
            return SandboxedLuaCallbackDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::CallbackNotFound,
                value: None,
                instruction_checks: 0,
            };
        };
        if source.len() > self.limits.max_source_bytes {
            return SandboxedLuaCallbackDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::SourceRejected,
                value: None,
                instruction_checks: 0,
            };
        }
        let lua = match Lua::new_with(StdLib::NONE, LuaOptions::default()) {
            Ok(lua) => lua,
            Err(_) => return rejected_callback_outcome(0),
        };
        if lua.set_memory_limit(self.limits.max_memory_bytes).is_err() {
            return rejected_callback_outcome(0);
        }
        let instruction_checks = Arc::new(AtomicU32::new(0));
        let hook_checks = Arc::clone(&instruction_checks);
        let instruction_limit = self.limits.max_instructions;
        lua.set_hook(
            HookTriggers {
                every_nth_instruction: Some(1),
                ..HookTriggers::default()
            },
            move |_, _| {
                if hook_checks.fetch_add(1, Ordering::Relaxed) >= instruction_limit {
                    Err(mlua::Error::RuntimeError(INSTRUCTION_LIMIT_MARKER.into()))
                } else {
                    Ok(())
                }
            },
        );
        let result = lua.load(source).eval::<Function>().and_then(|callback| {
            let subject_id = i64::try_from(input.subject_id).map_err(|_| {
                mlua::Error::RuntimeError("subject ID exceeds signed Lua integer range".into())
            })?;
            callback.call::<_, Value>((input.event_kind.as_str(), subject_id, input.value))
        });
        let instruction_checks = instruction_checks.load(Ordering::Relaxed);
        let instruction_limit_reached = instruction_checks > self.limits.max_instructions;
        match result {
            Ok(value) => match sandboxed_lua_value(value) {
                Some(value) => SandboxedLuaCallbackDispatchOutcome {
                    state: SandboxedLuaCallbackDispatchState::Completed,
                    value: Some(value),
                    instruction_checks,
                },
                None => SandboxedLuaCallbackDispatchOutcome {
                    state: SandboxedLuaCallbackDispatchState::UnsupportedValue,
                    value: None,
                    instruction_checks,
                },
            },
            Err(_) if instruction_limit_reached => SandboxedLuaCallbackDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::InstructionLimitReached,
                value: None,
                instruction_checks,
            },
            Err(_) => rejected_callback_outcome(instruction_checks),
        }
    }
}

fn rejected_callback_outcome(instruction_checks: u32) -> SandboxedLuaCallbackDispatchOutcome {
    SandboxedLuaCallbackDispatchOutcome {
        state: SandboxedLuaCallbackDispatchState::RuntimeRejected,
        value: None,
        instruction_checks,
    }
}

fn rejected_runtime_outcome(instruction_checks: u32) -> SandboxedLuaOutcome {
    SandboxedLuaOutcome {
        state: SandboxedLuaExecutionState::RuntimeRejected,
        value: None,
        instruction_checks,
    }
}

fn sandboxed_lua_value(value: Value) -> Option<SandboxedLuaValue> {
    match value {
        Value::Nil => Some(SandboxedLuaValue::Nil),
        Value::Boolean(value) => Some(SandboxedLuaValue::Boolean(value)),
        Value::Integer(value) => Some(SandboxedLuaValue::Integer(value)),
        Value::Number(value) => Some(SandboxedLuaValue::Number(value)),
        Value::String(value) => value
            .to_str()
            .ok()
            .map(|value| SandboxedLuaValue::Text(value.to_owned())),
        Value::LightUserData(_)
        | Value::Table(_)
        | Value::Function(_)
        | Value::Thread(_)
        | Value::UserData(_)
        | Value::Error(_) => None,
    }
}

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

    #[test]
    fn callback_dispatcher_runs_registered_primitive_callbacks_in_fresh_sandboxes() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "award",
                "return function(kind, subject_id, value) if kind == 'award' and subject_id == 7 then return value + 1 end return false end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "fresh-state",
                "return function(_, _, _) counter = (counter or 0) + 1 return counter end",
            )
            .unwrap();
        assert_eq!(dispatcher.len(), 2);
        assert!(!dispatcher.is_empty());

        let input = SandboxedLuaCallbackInput {
            event_kind: "award".into(),
            subject_id: 7,
            value: 41,
        };
        let outcome = dispatcher.dispatch("award", &input);
        assert_eq!(outcome.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(outcome.value, Some(SandboxedLuaValue::Integer(42)));
        assert!(outcome.instruction_checks > 0);

        let first = dispatcher.dispatch("fresh-state", &input);
        let second = dispatcher.dispatch("fresh-state", &input);
        assert_eq!(first.value, Some(SandboxedLuaValue::Integer(1)));
        assert_eq!(second.value, Some(SandboxedLuaValue::Integer(1)));

        let missing = dispatcher.dispatch("missing", &input);
        assert_eq!(
            missing.state,
            SandboxedLuaCallbackDispatchState::CallbackNotFound
        );
        assert_eq!(missing.value, None);
        assert_eq!(missing.instruction_checks, 0);
    }

    #[test]
    fn callback_dispatcher_enforces_registration_and_execution_boundaries() {
        let limits = SandboxedLuaLimits::new(96, MAX_SANDBOXED_LUA_MEMORY_BYTES, 32).unwrap();
        let mut dispatcher = SandboxedLuaCallbackDispatcher::new(limits);
        assert_eq!(
            dispatcher.register_callback("", "return function() return true end"),
            Err(SandboxedLuaCallbackRegistrationError::InvalidName)
        );
        assert_eq!(
            dispatcher.register_callback("too-long", "x".repeat(97)),
            Err(SandboxedLuaCallbackRegistrationError::SourceRejected)
        );
        dispatcher
            .register_callback("typed", "return function() return {} end")
            .unwrap();
        assert_eq!(
            dispatcher.register_callback("typed", "return function() return true end"),
            Err(SandboxedLuaCallbackRegistrationError::DuplicateName(
                "typed".into()
            ))
        );
        dispatcher
            .register_callback("limit", "return function() while true do end end")
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "test".into(),
            subject_id: 1,
            value: 0,
        };
        assert_eq!(
            dispatcher.dispatch("typed", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );
        assert_eq!(
            dispatcher.dispatch("limit", &input).state,
            SandboxedLuaCallbackDispatchState::InstructionLimitReached
        );
        assert_eq!(
            dispatcher
                .dispatch(
                    "typed",
                    &SandboxedLuaCallbackInput {
                        event_kind: "test".into(),
                        subject_id: u64::MAX,
                        value: 0,
                    }
                )
                .state,
            SandboxedLuaCallbackDispatchState::RuntimeRejected
        );
    }

    #[test]
    fn sandboxed_executor_returns_only_typed_primitive_expression_values() {
        let executor = SandboxedLuaExecutor::default();
        let arithmetic = executor.execute_expression("1 + 2 * 3");
        assert_eq!(arithmetic.state, SandboxedLuaExecutionState::Completed);
        assert_eq!(arithmetic.value, Some(SandboxedLuaValue::Integer(7)));
        assert!(arithmetic.instruction_checks > 0);

        let text = executor.execute_expression("'fe' .. '-sandbox'");
        assert_eq!(text.state, SandboxedLuaExecutionState::Completed);
        assert_eq!(
            text.value,
            Some(SandboxedLuaValue::Text("fe-sandbox".into()))
        );
        assert!(text.instruction_checks > 0);

        let io = executor.execute_expression("io");
        assert_eq!(io.state, SandboxedLuaExecutionState::Completed);
        assert_eq!(io.value, Some(SandboxedLuaValue::Nil));
        assert!(io.instruction_checks > 0);
    }

    #[test]
    fn sandboxed_executor_enforces_source_instruction_and_value_boundaries() {
        let limits = SandboxedLuaLimits::new(64, MAX_SANDBOXED_LUA_MEMORY_BYTES, 32).unwrap();
        let executor = SandboxedLuaExecutor::new(limits);
        assert_eq!(
            executor.execute_expression("x".repeat(65).as_str()).state,
            SandboxedLuaExecutionState::SourceRejected
        );
        assert_eq!(
            executor.execute_expression("{}").state,
            SandboxedLuaExecutionState::UnsupportedValue
        );
        assert_eq!(
            executor
                .execute_expression("(function() while true do end end)()")
                .state,
            SandboxedLuaExecutionState::InstructionLimitReached
        );
        assert_eq!(
            SandboxedLuaLimits::new(0, 1, 1),
            Err(SandboxedLuaLimitError::InvalidSourceLimit(0))
        );
    }
}
