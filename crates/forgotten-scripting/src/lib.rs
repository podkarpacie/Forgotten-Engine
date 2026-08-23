//! Transparent TFS Lua compatibility inventory and non-executing dispatch boundary.
//!
//! This crate intentionally has no Lua runtime dependency. Its first dispatch interface accepts
//! only typed aggregate inventory metadata; it cannot receive a script path or source body and
//! always returns a deferred no-op outcome.

use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Value, Variadic};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

pub const MAX_SANDBOXED_LUA_SOURCE_BYTES: usize = 4 * 1024;
pub const MAX_SANDBOXED_LUA_MEMORY_BYTES: usize = 64 * 1024;
pub const MAX_SANDBOXED_LUA_INSTRUCTIONS: u32 = 10_000;
pub const MAX_SANDBOXED_LUA_CALLBACKS: usize = 64;
pub const MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES: usize = 64;
pub const MAX_SANDBOXED_LUA_CALLBACK_EVENT_KIND_BYTES: usize = 64;
pub const MAX_SANDBOXED_LUA_TABLE_CREATE_ARRAY_CAPACITY: usize = 256;
pub const MAX_SANDBOXED_LUA_TABLE_CREATE_RECORD_CAPACITY: usize = 256;
pub const MAX_SANDBOXED_LUA_MATH_ARGUMENTS: usize = 256;
pub const MAX_SANDBOXED_LUA_STRING_BYTES: usize = 1024;
const INSTRUCTION_LIMIT_MARKER: &str = "forgotten-engine-sandbox-instruction-limit";

/// Explicit limits for one side-effect-free Lua expression evaluation. The executor creates a
/// fresh VM per call with no standard libraries. The installed compatibility surface is limited to
/// VM-local, capacity-capped `table.create` and `table.pack`, deterministic `math.abs`,
/// `math.ceil`, `math.floor`, `math.min`, and `math.max`, and ASCII-only bounded `string.len`,
/// `string.lower`, `string.upper`, `string.reverse`, and `string.sub`; the sandbox offers no
/// file, network, process, package, debug, random-state, time, or mutable host API surface.
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
        if install_sandboxed_tfs_compatibility_globals(&lua).is_err() {
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
/// mutable server APIs. The event kind is a nonempty operator-chosen label bounded to 64 bytes,
/// not a claimed TFS callback. Subject IDs must fit the signed Lua integer range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxedLuaCallbackInput {
    pub event_kind: String,
    pub subject_id: u64,
    pub value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxedLuaCallbackInputError {
    InvalidEventKind,
    SubjectIdOutOfRange,
}

impl SandboxedLuaCallbackInput {
    fn validate(&self) -> Result<(), SandboxedLuaCallbackInputError> {
        if self.event_kind.trim().is_empty()
            || self.event_kind.len() > MAX_SANDBOXED_LUA_CALLBACK_EVENT_KIND_BYTES
        {
            return Err(SandboxedLuaCallbackInputError::InvalidEventKind);
        }
        if self.subject_id > i64::MAX as u64 {
            return Err(SandboxedLuaCallbackInputError::SubjectIdOutOfRange);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxedLuaCallbackRegistrationError {
    InvalidName,
    DuplicateName(String),
    CallbackLimit(usize),
    SourceRejected,
}

/// Bounded file-loading failures for explicit callback-function chunks. This loader is not a TFS
/// script runtime: it rejects traversal, resolves both root and candidate canonically, and then
/// delegates only the source bytes to the existing callback registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxedLuaCallbackFileRegistrationError {
    InvalidRelativePath,
    SourceReadFailed,
    SourceOutsideRoot,
    SourceNotRegularFile,
    Registration(SandboxedLuaCallbackRegistrationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxedLuaCallbackDispatchState {
    Completed,
    CallbackNotFound,
    InputRejected,
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

    /// Loads one explicit callback-function chunk from a canonical operator-owned script root.
    /// The path must contain only normal relative components and resolve to a regular UTF-8 file
    /// inside that root. Ordinary TFS script registries, module imports, filesystem access from
    /// Lua, and legacy callback APIs are intentionally not enabled by this loader.
    pub fn register_callback_file(
        &mut self,
        name: impl Into<String>,
        script_root: &Path,
        relative_path: &Path,
    ) -> Result<(), SandboxedLuaCallbackFileRegistrationError> {
        if !relative_path.is_relative()
            || !relative_path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
        {
            return Err(SandboxedLuaCallbackFileRegistrationError::InvalidRelativePath);
        }
        let canonical_root = fs::canonicalize(script_root)
            .map_err(|_| SandboxedLuaCallbackFileRegistrationError::SourceReadFailed)?;
        let canonical_source = fs::canonicalize(script_root.join(relative_path))
            .map_err(|_| SandboxedLuaCallbackFileRegistrationError::SourceReadFailed)?;
        if !canonical_source.starts_with(&canonical_root) {
            return Err(SandboxedLuaCallbackFileRegistrationError::SourceOutsideRoot);
        }
        let metadata = fs::metadata(&canonical_source)
            .map_err(|_| SandboxedLuaCallbackFileRegistrationError::SourceReadFailed)?;
        if !metadata.is_file() {
            return Err(SandboxedLuaCallbackFileRegistrationError::SourceNotRegularFile);
        }
        let source = fs::read_to_string(canonical_source)
            .map_err(|_| SandboxedLuaCallbackFileRegistrationError::SourceReadFailed)?;
        self.register_callback(name, source)
            .map_err(SandboxedLuaCallbackFileRegistrationError::Registration)
    }

    /// Invokes one registered callback in a new no-standard-library VM. Callback state cannot
    /// carry across invocations, and only the explicitly supplied primitive input crosses the
    /// sandbox boundary.
    pub fn dispatch(
        &self,
        callback_name: &str,
        input: &SandboxedLuaCallbackInput,
    ) -> SandboxedLuaCallbackDispatchOutcome {
        if input.validate().is_err() {
            return SandboxedLuaCallbackDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::InputRejected,
                value: None,
                instruction_checks: 0,
            };
        }
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
        if install_sandboxed_tfs_compatibility_globals(&lua).is_err() {
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
            let subject_id = i64::try_from(input.subject_id)
                .expect("validated callback subject ID fits signed Lua integer range");
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

fn install_sandboxed_tfs_compatibility_globals(lua: &Lua) -> Result<(), mlua::Error> {
    let create_table =
        lua.create_function(|lua, (array_capacity, record_capacity): (i64, i64)| {
            let array_capacity = usize::try_from(array_capacity)
                .ok()
                .filter(|capacity| *capacity <= MAX_SANDBOXED_LUA_TABLE_CREATE_ARRAY_CAPACITY)
                .ok_or_else(|| {
                    mlua::Error::RuntimeError("invalid sandbox table array capacity".into())
                })?;
            let record_capacity = usize::try_from(record_capacity)
                .ok()
                .filter(|capacity| *capacity <= MAX_SANDBOXED_LUA_TABLE_CREATE_RECORD_CAPACITY)
                .ok_or_else(|| {
                    mlua::Error::RuntimeError("invalid sandbox table record capacity".into())
                })?;
            lua.create_table_with_capacity(array_capacity, record_capacity)
        })?;
    let pack_table = lua.create_function(|lua, values: Variadic<Value>| {
        if values.len() > MAX_SANDBOXED_LUA_TABLE_CREATE_ARRAY_CAPACITY {
            return Err(mlua::Error::RuntimeError(
                "sandbox table.pack argument count exceeds the configured limit".into(),
            ));
        }
        let argument_count = values.len();
        let table = lua.create_table_with_capacity(argument_count, 1)?;
        for (index, value) in values.into_iter().enumerate() {
            table.raw_set(index + 1, value)?;
        }
        table.set("n", i64::try_from(argument_count).unwrap_or(i64::MAX))?;
        Ok(table)
    })?;
    let table = lua.create_table()?;
    table.set("create", create_table)?;
    table.set("pack", pack_table)?;
    lua.globals().set("table", table)?;

    let math_abs = lua.create_function(|_, value: f64| Ok(value.abs()))?;
    let math_ceil = lua.create_function(|_, value: f64| Ok(value.ceil()))?;
    let math_floor = lua.create_function(|_, value: f64| Ok(value.floor()))?;
    let math_min = lua.create_function(|_, values: Variadic<f64>| {
        if values.len() > MAX_SANDBOXED_LUA_MATH_ARGUMENTS {
            return Err(mlua::Error::RuntimeError(
                "sandbox math.min argument count exceeds the configured limit".into(),
            ));
        }
        let mut values = values.into_iter();
        let first = values.next().ok_or_else(|| {
            mlua::Error::RuntimeError("sandbox math.min requires an argument".into())
        })?;
        values.try_fold(first, |minimum, value| Ok(minimum.min(value)))
    })?;
    let math_max = lua.create_function(|_, values: Variadic<f64>| {
        if values.len() > MAX_SANDBOXED_LUA_MATH_ARGUMENTS {
            return Err(mlua::Error::RuntimeError(
                "sandbox math.max argument count exceeds the configured limit".into(),
            ));
        }
        let mut values = values.into_iter();
        let first = values.next().ok_or_else(|| {
            mlua::Error::RuntimeError("sandbox math.max requires an argument".into())
        })?;
        values.try_fold(first, |maximum, value| Ok(maximum.max(value)))
    })?;
    let math = lua.create_table()?;
    math.set("abs", math_abs)?;
    math.set("ceil", math_ceil)?;
    math.set("floor", math_floor)?;
    math.set("min", math_min)?;
    math.set("max", math_max)?;
    lua.globals().set("math", math)?;

    let string_len = lua.create_function(|_, value: String| {
        let value = bounded_sandboxed_ascii_string(value)?;
        Ok(i64::try_from(value.len()).unwrap_or(i64::MAX))
    })?;
    let string_lower = lua.create_function(|_, value: String| {
        let value = bounded_sandboxed_ascii_string(value)?;
        Ok(value.to_ascii_lowercase())
    })?;
    let string_upper = lua.create_function(|_, value: String| {
        let value = bounded_sandboxed_ascii_string(value)?;
        Ok(value.to_ascii_uppercase())
    })?;
    let string_reverse = lua.create_function(|_, value: String| {
        let value = bounded_sandboxed_ascii_string(value)?;
        Ok(value.chars().rev().collect::<String>())
    })?;
    let string_sub =
        lua.create_function(|_, (value, start, end): (String, i64, Option<i64>)| {
            let value = bounded_sandboxed_ascii_string(value)?;
            let length = i64::try_from(value.len()).unwrap_or(i64::MAX);
            let start = normalized_sandboxed_lua_string_index(start, length).max(1);
            let end = normalized_sandboxed_lua_string_index(end.unwrap_or(-1), length).min(length);
            if start > end || start > length || end < 1 {
                return Ok(String::new());
            }
            Ok(value[(start - 1) as usize..end as usize].to_owned())
        })?;
    let string = lua.create_table()?;
    string.set("len", string_len)?;
    string.set("lower", string_lower)?;
    string.set("upper", string_upper)?;
    string.set("reverse", string_reverse)?;
    string.set("sub", string_sub)?;
    lua.globals().set("string", string)
}

fn bounded_sandboxed_ascii_string(value: String) -> Result<String, mlua::Error> {
    if value.len() > MAX_SANDBOXED_LUA_STRING_BYTES {
        return Err(mlua::Error::RuntimeError(
            "sandbox string exceeds the configured byte limit".into(),
        ));
    }
    if !value.is_ascii() {
        return Err(mlua::Error::RuntimeError(
            "sandbox string helpers accept ASCII only".into(),
        ));
    }
    Ok(value)
}

fn normalized_sandboxed_lua_string_index(index: i64, length: i64) -> i64 {
    if index >= 0 {
        index
    } else {
        length.saturating_add(index).saturating_add(1)
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
        dispatcher
            .register_callback(
                "table-create",
                "return function(_, _, value) local t = table.create(1, 1) t[1] = value return t[1] end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "string-helpers",
                "return function(_, _, value) if string.upper(string.reverse('fe')) == 'EF' and string.sub('abcd', 2, -2) == 'bc' then return value end return 0 end",
            )
            .unwrap();
        assert_eq!(dispatcher.len(), 4);
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

        let table_create = dispatcher.dispatch("table-create", &input);
        assert_eq!(
            table_create.value,
            Some(SandboxedLuaValue::Integer(input.value))
        );

        let string_helpers = dispatcher.dispatch("string-helpers", &input);
        assert_eq!(
            string_helpers.value,
            Some(SandboxedLuaValue::Integer(input.value))
        );

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
            SandboxedLuaCallbackDispatchState::InputRejected
        );
        assert_eq!(
            dispatcher
                .dispatch(
                    "typed",
                    &SandboxedLuaCallbackInput {
                        event_kind: " ".into(),
                        subject_id: 1,
                        value: 0,
                    }
                )
                .state,
            SandboxedLuaCallbackDispatchState::InputRejected
        );
        assert_eq!(
            dispatcher
                .dispatch(
                    "typed",
                    &SandboxedLuaCallbackInput {
                        event_kind: "a".repeat(MAX_SANDBOXED_LUA_CALLBACK_EVENT_KIND_BYTES + 1),
                        subject_id: 1,
                        value: 0,
                    }
                )
                .state,
            SandboxedLuaCallbackDispatchState::InputRejected
        );
    }

    #[test]
    fn callback_dispatcher_loads_only_bounded_callback_files_under_its_root() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("forgotten-engine-script-root-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("award.lua"),
            "return function(kind, _, value) if kind == 'award' then return value + 1 end return false end",
        )
        .unwrap();
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback_file("award", &root, Path::new("award.lua"))
            .unwrap();
        assert_eq!(
            dispatcher
                .dispatch(
                    "award",
                    &SandboxedLuaCallbackInput {
                        event_kind: "award".into(),
                        subject_id: 7,
                        value: 41,
                    },
                )
                .value,
            Some(SandboxedLuaValue::Integer(42))
        );
        assert_eq!(
            dispatcher.register_callback_file("outside", &root, Path::new("../outside.lua")),
            Err(SandboxedLuaCallbackFileRegistrationError::InvalidRelativePath)
        );
        let escaped_source = root.with_extension("escaped.lua");
        fs::write(
            &escaped_source,
            "return function(_, _, value) return value end",
        )
        .unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&escaped_source, root.join("escape.lua")).unwrap();
            assert_eq!(
                dispatcher.register_callback_file("escape", &root, Path::new("escape.lua")),
                Err(SandboxedLuaCallbackFileRegistrationError::SourceOutsideRoot)
            );
        }

        let constrained_limits =
            SandboxedLuaLimits::new(64, MAX_SANDBOXED_LUA_MEMORY_BYTES, 32).unwrap();
        fs::write(root.join("oversized.lua"), "x".repeat(65)).unwrap();
        let mut constrained = SandboxedLuaCallbackDispatcher::new(constrained_limits);
        assert_eq!(
            constrained.register_callback_file("oversized", &root, Path::new("oversized.lua")),
            Err(SandboxedLuaCallbackFileRegistrationError::Registration(
                SandboxedLuaCallbackRegistrationError::SourceRejected
            ))
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(escaped_source).unwrap();
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

        let table_create = executor.execute_expression(
            "(function() local t = table.create(2, 1); t[1] = 42; t.answer = 7; return t[1] + t.answer end)()",
        );
        assert_eq!(table_create.state, SandboxedLuaExecutionState::Completed);
        assert_eq!(table_create.value, Some(SandboxedLuaValue::Integer(49)));

        let table_pack = executor.execute_expression(
            "(function() local t = table.pack(4, 'fe', nil, true); return t.n == 4 and t[1] == 4 and t[2] == 'fe' and t[3] == nil and t[4] == true end)()",
        );
        assert_eq!(table_pack.state, SandboxedLuaExecutionState::Completed);
        assert_eq!(table_pack.value, Some(SandboxedLuaValue::Boolean(true)));

        let math = executor.execute_expression(
            "math.abs(-3.5) == 3.5 and math.ceil(2.1) == 3 and math.floor(2.9) == 2 and math.min(4, -2, 7) == -2 and math.max(4, -2, 7) == 7",
        );
        assert_eq!(math.state, SandboxedLuaExecutionState::Completed);
        assert_eq!(math.value, Some(SandboxedLuaValue::Boolean(true)));

        let string = executor.execute_expression(
            "string.len('Abc1') == 4 and string.lower('Abc1') == 'abc1' and string.upper('Abc1') == 'ABC1' and string.reverse('Abc1') == '1cbA' and string.sub('Abc1', 2, -2) == 'bc' and string.sub('Abc1', -2) == 'c1' and string.sub('Abc1', 8) == ''",
        );
        assert_eq!(string.state, SandboxedLuaExecutionState::Completed);
        assert_eq!(string.value, Some(SandboxedLuaValue::Boolean(true)));

        assert_eq!(
            executor.execute_expression("math.min()").state,
            SandboxedLuaExecutionState::RuntimeRejected
        );
        assert_eq!(
            executor
                .execute_expression(&format!(
                    "math.max({})",
                    vec!["1"; MAX_SANDBOXED_LUA_MATH_ARGUMENTS + 1].join(",")
                ))
                .state,
            SandboxedLuaExecutionState::RuntimeRejected
        );
        assert_eq!(
            executor.execute_expression("math.random").value,
            Some(SandboxedLuaValue::Nil)
        );
        assert_eq!(
            executor
                .execute_expression(&format!(
                    "string.lower('{}')",
                    "a".repeat(MAX_SANDBOXED_LUA_STRING_BYTES + 1)
                ))
                .state,
            SandboxedLuaExecutionState::RuntimeRejected
        );
        assert_eq!(
            executor.execute_expression("string.upper('ą')").state,
            SandboxedLuaExecutionState::RuntimeRejected
        );
        assert_eq!(
            executor.execute_expression("string.match").value,
            Some(SandboxedLuaValue::Nil)
        );

        let oversized_pack = executor.execute_expression(&format!(
            "table.pack({})",
            vec!["1"; MAX_SANDBOXED_LUA_TABLE_CREATE_ARRAY_CAPACITY + 1].join(",")
        ));
        assert_eq!(
            oversized_pack.state,
            SandboxedLuaExecutionState::RuntimeRejected
        );

        let oversized_table = executor.execute_expression("table.create(257, 0)");
        assert_eq!(
            oversized_table.state,
            SandboxedLuaExecutionState::RuntimeRejected
        );

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
