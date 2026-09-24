//! Transparent TFS Lua compatibility inventory and non-executing dispatch boundary.
//!
//! This crate intentionally has no Lua runtime dependency. Its first dispatch interface accepts
//! only typed aggregate inventory metadata; it cannot receive a script path or source body and
//! always returns a deferred no-op outcome.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Table, Value, Variadic};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex, RwLock,
};

pub const MAX_SANDBOXED_LUA_SOURCE_BYTES: usize = 4 * 1024;
pub const MAX_SANDBOXED_LUA_MEMORY_BYTES: usize = 64 * 1024;
pub const MAX_SANDBOXED_LUA_INSTRUCTIONS: u32 = 10_000;
pub const MAX_SANDBOXED_LUA_CALLBACKS: usize = 64;
pub const MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES: usize = 64;
pub const MAX_SANDBOXED_LUA_CALLBACK_EVENT_KIND_BYTES: usize = 64;
pub const MAX_SANDBOXED_LUA_CALLBACK_ARGUMENT_BYTES: usize = 255;
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

/// An authoritative subject position the host may attach to one callback invocation so scripts
/// can read a `getThingPos`-style coordinate without any world access. It crosses as a fifth
/// `{ x, y, z }` argument (or nil when the caller has no game position); callbacks written for
/// fewer arguments simply ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxedLuaPosition {
    pub x: u16,
    pub y: u16,
    pub z: u8,
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
    /// Optional bounded string payload (for example a talkaction argument after the trigger).
    /// The callback receives it as a fourth argument; three-parameter callbacks simply ignore it.
    pub argument: String,
    /// Optional authoritative subject position, received as a fifth `{ x, y, z }` argument.
    pub position: Option<SandboxedLuaPosition>,
    /// Durable script storage snapshot for the dispatch subject, hydrated by the host
    /// before dispatch. `getPlayerStorageValue` reads this map (absent keys answer
    /// `-1`); it never touches the live database, and writes cross back as intents.
    /// Bounded by the persistence-layer per-player entry cap.
    pub storage: BTreeMap<i64, i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxedLuaCallbackInputError {
    InvalidEventKind,
    SubjectIdOutOfRange,
    ArgumentTooLong,
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
        if self.argument.len() > MAX_SANDBOXED_LUA_CALLBACK_ARGUMENT_BYTES {
            return Err(SandboxedLuaCallbackInputError::ArgumentTooLong);
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

/// Bounded number of typed effects one callback may request in a single dispatch.
pub const MAX_SANDBOXED_LUA_EFFECTS: usize = 8;
pub const MAX_SANDBOXED_LUA_EFFECT_TEXT_BYTES: usize = 255;
/// Maximum unit count one give-item effect may request. A request above this is rejected outright
/// rather than admitted to the host's container-staging loop.
pub const MAX_SANDBOXED_LUA_EFFECT_ITEM_COUNT: u16 = 100;

/// One bounded, side-effect-free intent a sandbox callback may return. It carries no authority:
/// the host must validate and apply it against authoritative state, so a script can never mutate
/// the world or the process directly.
#[derive(Debug, Clone, PartialEq)]
pub enum SandboxedLuaEffect {
    Say(String),
    Teleport {
        x: u16,
        y: u16,
        z: u8,
    },
    /// Capped health/mana restore applied to the dispatch subject. Values are u16 and the host
    /// clamps to the subject's current maximums; both zero is rejected as a no-op intent.
    Heal {
        health: u16,
        mana: u16,
    },
    /// Bounded unit delivery of one server item id to the dispatch subject's owned containers.
    GiveItem {
        id: u16,
        count: u16,
    },
    /// Bounded unit removal of one server item id from the dispatch subject's owned equipment
    /// and containers. The host applies it atomically and reports insufficiency instead.
    RemoveItem {
        id: u16,
        count: u16,
    },
    /// A client-visible tile effect at an explicit coordinate. The host emits it to the dispatch
    /// subject's session only; broadcast to spectators stays deferred.
    MagicEffect {
        x: u16,
        y: u16,
        z: u8,
        kind: u8,
    },
    /// Durable script storage write for the dispatch subject. The host persists it;
    /// absent-key reads answer `-1` through `getPlayerStorageValue`, matching TFS.
    SetStorage {
        key: i64,
        value: i64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxedLuaEffectDispatchOutcome {
    pub state: SandboxedLuaCallbackDispatchState,
    pub effects: Vec<SandboxedLuaEffect>,
    pub instruction_checks: u32,
}

/// Reads one explicit callback-function chunk from a canonical operator-owned script root,
/// returning the canonical root plus the file body. Shared by registration and hot-reload so a
/// reload can never accept a file registration would reject.
fn read_callback_file_source(
    script_root: &Path,
    relative_path: &Path,
) -> Result<(PathBuf, String), SandboxedLuaCallbackFileRegistrationError> {
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
    Ok((canonical_root, source))
}

/// The per-name outcome of [`SandboxedLuaCallbackDispatcher::reload_file_callbacks`]: every
/// reloaded name serves its fresh source, every failed name keeps its previous source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptFileReloadReport {
    pub reloaded: Vec<String>,
    pub failed: Vec<ScriptFileReloadFailure>,
}

/// One file-backed callback whose reload failed validation; the previous source keeps serving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFileReloadFailure {
    pub name: String,
    pub error: String,
}

impl ScriptFileReloadReport {
    /// Bounded one-line operator summary: names are length-capped at registration and the
    /// callback count is capped, so this line cannot grow without bound.
    pub fn summary(&self) -> String {
        let mut summary = format!(
            "reloaded={} failed={}",
            self.reloaded.len(),
            self.failed.len()
        );
        if !self.reloaded.is_empty() {
            summary.push_str(&format!(" reloaded_names=[{}]", self.reloaded.join(",")));
        }
        for failure in &self.failed {
            summary.push_str(&format!(
                " failed_names=[{}:{}]",
                failure.name, failure.error
            ));
        }
        summary
    }
}

/// A bounded trusted-source callback registry. Registration is explicit and in-memory: it does
/// not discover files, load TFS registries, preserve global Lua state, or resolve modules. Every
/// dispatch creates a new VM and expects the source to evaluate to a function accepting exactly
/// `(event_kind, subject_id, value)` primitive arguments.
///
/// The callback and file-source maps live behind reader-writer locks so a running host can
/// hot-reload file-backed callbacks in place: every session shares the same dispatcher object
/// through `Arc` clones, so a reload is visible to live sessions without reconnects. Dispatch
/// clones the source under a read lock and evaluates outside it, so a reload write waits only
/// for in-flight lookups, never for script execution. Cloning a dispatcher still snapshots
/// independent maps, exactly like the previous plain-`BTreeMap` behavior.
#[derive(Debug)]
pub struct SandboxedLuaCallbackDispatcher {
    limits: SandboxedLuaLimits,
    callbacks: Arc<RwLock<BTreeMap<String, String>>>,
    file_sources: Arc<RwLock<BTreeMap<String, ScriptFileSource>>>,
}

/// A remembered file-backed callback registration: the canonical script root plus the declared
/// relative path, re-read verbatim on every hot-reload.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScriptFileSource {
    root: PathBuf,
    relative: PathBuf,
}

impl Clone for SandboxedLuaCallbackDispatcher {
    fn clone(&self) -> Self {
        Self {
            limits: self.limits,
            callbacks: Arc::new(RwLock::new(self.read_callbacks().clone())),
            file_sources: Arc::new(RwLock::new(self.read_file_sources().clone())),
        }
    }
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
            callbacks: Arc::new(RwLock::new(BTreeMap::new())),
            file_sources: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    fn read_callbacks(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, String>> {
        self.callbacks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn read_file_sources(
        &self,
    ) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, ScriptFileSource>> {
        self.file_sources
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_callbacks(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<String, String>> {
        self.callbacks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub const fn limits(&self) -> SandboxedLuaLimits {
        self.limits
    }

    pub fn len(&self) -> usize {
        self.read_callbacks().len()
    }

    pub fn is_empty(&self) -> bool {
        self.read_callbacks().is_empty()
    }

    /// Registers one operator-provided callback source. The source must be a Lua chunk that
    /// evaluates to a function, for example: `return function(kind, id, value) return value end`.
    /// Source execution is deferred until dispatch and takes place in a fresh restricted VM.
    /// Takes `&mut self` deliberately: registration is build-time only; live updates go
    /// through [`Self::reload_file_callbacks`] on the shared dispatcher instead.
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
        let mut callbacks = self.write_callbacks();
        if callbacks.contains_key(&name) {
            return Err(SandboxedLuaCallbackRegistrationError::DuplicateName(name));
        }
        if callbacks.len() >= MAX_SANDBOXED_LUA_CALLBACKS {
            return Err(SandboxedLuaCallbackRegistrationError::CallbackLimit(
                MAX_SANDBOXED_LUA_CALLBACKS,
            ));
        }
        callbacks.insert(name, source);
        Ok(())
    }

    /// Loads one explicit callback-function chunk from a canonical operator-owned script root.
    /// The path must contain only normal relative components and resolve to a regular UTF-8 file
    /// inside that root. Ordinary TFS script registries, module imports, filesystem access from
    /// Lua, and legacy callback APIs are intentionally not enabled by this loader. The file
    /// source is remembered so [`Self::reload_file_callbacks`] can re-read it later.
    pub fn register_callback_file(
        &mut self,
        name: impl Into<String>,
        script_root: &Path,
        relative_path: &Path,
    ) -> Result<(), SandboxedLuaCallbackFileRegistrationError> {
        let (canonical_root, source) = read_callback_file_source(script_root, relative_path)?;
        let name = name.into();
        self.register_callback(name.clone(), source)
            .map_err(SandboxedLuaCallbackFileRegistrationError::Registration)?;
        self.file_sources
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                name,
                ScriptFileSource {
                    root: canonical_root,
                    relative: relative_path.to_path_buf(),
                },
            );
        Ok(())
    }

    /// Re-reads every file-backed callback from disk and swaps the loaded sources in place. A
    /// file that fails validation keeps serving its previous source; the failure is reported,
    /// never thrown — one broken script must not unload the working set. Inline-registered
    /// callbacks have no file source and are never touched.
    pub fn reload_file_callbacks(&self) -> ScriptFileReloadReport {
        let sources: Vec<(String, ScriptFileSource)> = self
            .read_file_sources()
            .iter()
            .map(|(name, source)| (name.clone(), source.clone()))
            .collect();
        let mut report = ScriptFileReloadReport::default();
        for (name, source) in sources {
            match read_callback_file_source(&source.root, &source.relative) {
                Ok((_, body)) => {
                    if body.len() > self.limits.max_source_bytes {
                        report.failed.push(ScriptFileReloadFailure {
                            name,
                            error: format!(
                                "{:?}",
                                SandboxedLuaCallbackFileRegistrationError::Registration(
                                    SandboxedLuaCallbackRegistrationError::SourceRejected
                                )
                            ),
                        });
                        continue;
                    }
                    self.write_callbacks().insert(name.clone(), body);
                    report.reloaded.push(name);
                }
                Err(error) => report.failed.push(ScriptFileReloadFailure {
                    name,
                    error: format!("{error:?}"),
                }),
            }
        }
        report.reloaded.sort();
        report
            .failed
            .sort_by(|left, right| left.name.cmp(&right.name));
        report
    }

    /// Invokes one registered callback in a new no-standard-library VM. Callback state cannot
    /// carry across invocations, and only the explicitly supplied primitive input plus an
    /// optional subject position cross the sandbox boundary.
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
        let Some(source) = self.read_callbacks().get(callback_name).cloned() else {
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
            let subject_id = i64::try_from(input.subject_id).map_err(|_| {
                mlua::Error::RuntimeError("callback subject ID out of signed integer range".into())
            })?;
            let position = sandboxed_lua_position_value(&lua, input.position)?;
            callback.call::<_, Value>((
                input.event_kind.as_str(),
                subject_id,
                input.value,
                input.argument.as_str(),
                position,
            ))
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

    /// Invokes one registered callback and extracts a bounded list of typed effects (say text,
    /// teleport) from an array-of-effect-table return. The same fresh-VM, memory, instruction,
    /// and primitive-only boundaries as `dispatch` apply; returned effects are intents that the
    /// caller must validate and apply, never direct world mutation.
    pub fn dispatch_effects(
        &self,
        callback_name: &str,
        input: &SandboxedLuaCallbackInput,
    ) -> SandboxedLuaEffectDispatchOutcome {
        if input.validate().is_err() {
            return SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::InputRejected,
                effects: Vec::new(),
                instruction_checks: 0,
            };
        }
        let Some(source) = self.read_callbacks().get(callback_name).cloned() else {
            return SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::CallbackNotFound,
                effects: Vec::new(),
                instruction_checks: 0,
            };
        };
        if source.len() > self.limits.max_source_bytes {
            return SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::SourceRejected,
                effects: Vec::new(),
                instruction_checks: 0,
            };
        }
        let lua = match Lua::new_with(StdLib::NONE, LuaOptions::default()) {
            Ok(lua) => lua,
            Err(_) => return rejected_effect_outcome(0),
        };
        if lua.set_memory_limit(self.limits.max_memory_bytes).is_err() {
            return rejected_effect_outcome(0);
        }
        if install_sandboxed_tfs_compatibility_globals(&lua).is_err() {
            return rejected_effect_outcome(0);
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
                mlua::Error::RuntimeError("callback subject ID out of signed integer range".into())
            })?;
            let position = sandboxed_lua_position_value(&lua, input.position)?;
            callback.call::<_, Value>((
                input.event_kind.as_str(),
                subject_id,
                input.value,
                input.argument.as_str(),
                position,
            ))
        });
        let instruction_checks = instruction_checks.load(Ordering::Relaxed);
        let instruction_limit_reached = instruction_checks > self.limits.max_instructions;
        match result {
            Ok(value) => match sandboxed_lua_effects(value) {
                Some(effects) => SandboxedLuaEffectDispatchOutcome {
                    state: SandboxedLuaCallbackDispatchState::Completed,
                    effects,
                    instruction_checks,
                },
                None => SandboxedLuaEffectDispatchOutcome {
                    state: SandboxedLuaCallbackDispatchState::UnsupportedValue,
                    effects: Vec::new(),
                    instruction_checks,
                },
            },
            Err(_) if instruction_limit_reached => SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::InstructionLimitReached,
                effects: Vec::new(),
                instruction_checks,
            },
            Err(_) => rejected_effect_outcome(instruction_checks),
        }
    }

    /// Invokes one registered callback with TFS-shaped bound host functions (`doCreatureSay`,
    /// `doPlayerAddItem`, `doTeleportThing`, `doPlayerAddHealth`, `doPlayerAddMana`,
    /// `doPlayerRemoveItem`, `doSendMagicEffect`, `getThingPos`,
    /// `getPlayerStorageValue`, `setPlayerStorageValue`) installed. Scripts CALL these
    /// functions and may additionally return an effect table; call-recorded intents and the
    /// returned table are unioned (capped) so return-table scripts keep working unchanged under
    /// api routing. Each call validates its arguments; over-budget or invalid calls, malformed
    /// return tables, and over-cap unions fail the whole dispatch. The same fresh-VM, memory,
    /// instruction, and primitive-only boundaries as `dispatch_effects` apply. No world
    /// mutation, I/O, or host state is reachable from Lua; the functions are pure validated
    /// intent recorders against a per-dispatch budget.
    pub fn dispatch_api(
        &self,
        callback_name: &str,
        input: &SandboxedLuaCallbackInput,
    ) -> SandboxedLuaEffectDispatchOutcome {
        if input.validate().is_err() {
            return SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::InputRejected,
                effects: Vec::new(),
                instruction_checks: 0,
            };
        }
        let Some(source) = self.read_callbacks().get(callback_name).cloned() else {
            return SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::CallbackNotFound,
                effects: Vec::new(),
                instruction_checks: 0,
            };
        };
        if source.len() > self.limits.max_source_bytes {
            return SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::SourceRejected,
                effects: Vec::new(),
                instruction_checks: 0,
            };
        }
        let lua = match Lua::new_with(StdLib::NONE, LuaOptions::default()) {
            Ok(lua) => lua,
            Err(_) => return rejected_effect_outcome(0),
        };
        if lua.set_memory_limit(self.limits.max_memory_bytes).is_err() {
            return rejected_effect_outcome(0);
        }
        if install_sandboxed_tfs_compatibility_globals(&lua).is_err() {
            return rejected_effect_outcome(0);
        }
        let intents = Arc::new(Mutex::new(Vec::new()));
        if install_sandboxed_host_api(
            &lua,
            &intents,
            input.position,
            input.subject_id,
            input.storage.clone(),
        )
        .is_err()
        {
            return rejected_effect_outcome(0);
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
                mlua::Error::RuntimeError("callback subject ID out of signed integer range".into())
            })?;
            let position = sandboxed_lua_position_value(&lua, input.position)?;
            callback.call::<_, Value>((
                input.event_kind.as_str(),
                subject_id,
                input.value,
                input.argument.as_str(),
                position,
            ))
        });
        let instruction_checks = instruction_checks.load(Ordering::Relaxed);
        let instruction_limit_reached = instruction_checks > self.limits.max_instructions;
        match result {
            Ok(value) => {
                let mut effects = intents
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .clone();
                // Union call-recorded intents with a returned effect table so old
                // return-table scripts keep working unchanged under api routing. A
                // non-table return (nil, boolean, number, text) is ignored, letting
                // call-style scripts end with an explicit return; a malformed table
                // or an over-budget union rejects the whole dispatch.
                if matches!(value, Value::Table(_)) {
                    match sandboxed_lua_effects(value) {
                        Some(returned) => effects.extend(returned),
                        None => {
                            return SandboxedLuaEffectDispatchOutcome {
                                state: SandboxedLuaCallbackDispatchState::UnsupportedValue,
                                effects: Vec::new(),
                                instruction_checks,
                            };
                        }
                    }
                }
                if effects.len() > MAX_SANDBOXED_LUA_EFFECTS {
                    return SandboxedLuaEffectDispatchOutcome {
                        state: SandboxedLuaCallbackDispatchState::UnsupportedValue,
                        effects: Vec::new(),
                        instruction_checks,
                    };
                }
                SandboxedLuaEffectDispatchOutcome {
                    state: SandboxedLuaCallbackDispatchState::Completed,
                    effects,
                    instruction_checks,
                }
            }
            Err(_) if instruction_limit_reached => SandboxedLuaEffectDispatchOutcome {
                state: SandboxedLuaCallbackDispatchState::InstructionLimitReached,
                effects: Vec::new(),
                instruction_checks,
            },
            Err(_) => rejected_effect_outcome(instruction_checks),
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

fn rejected_effect_outcome(instruction_checks: u32) -> SandboxedLuaEffectDispatchOutcome {
    SandboxedLuaEffectDispatchOutcome {
        state: SandboxedLuaCallbackDispatchState::RuntimeRejected,
        effects: Vec::new(),
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

/// Builds the fifth callback argument from an optional authoritative subject position: a
/// `{ x, y, z }` table, or nil when the caller has no game position.
fn sandboxed_lua_position_value(
    lua: &Lua,
    position: Option<SandboxedLuaPosition>,
) -> Result<Value<'_>, mlua::Error> {
    let Some(position) = position else {
        return Ok(Value::Nil);
    };
    let table = lua.create_table()?;
    table.set("x", position.x)?;
    table.set("y", position.y)?;
    table.set("z", position.z)?;
    Ok(Value::Table(table))
}

/// Installs the bound TFS-shaped host API (`doCreatureSay`, `doPlayerAddItem`,
/// `doTeleportThing`, `doPlayerAddHealth`, `doPlayerAddMana`, `doPlayerRemoveItem`,
/// `doSendMagicEffect`, `getThingPos`, `getPlayerStorageValue`,
/// `setPlayerStorageValue`). Each function validates its arguments and records one
/// bounded `SandboxedLuaEffect` into the per-dispatch queue, except the readers
/// (`getThingPos`, `getPlayerStorageValue`), which answer from dispatch input;
/// over-budget or invalid calls fail the whole dispatch rather than partially
/// recording. Nothing here touches world state, files, or the network. Storage
/// functions are subject-locked: a mismatched player id fails closed.
fn install_sandboxed_host_api(
    lua: &Lua,
    intents: &Arc<Mutex<Vec<SandboxedLuaEffect>>>,
    position: Option<SandboxedLuaPosition>,
    subject_id: u64,
    storage: BTreeMap<i64, i64>,
) -> Result<(), mlua::Error> {
    let say_intents = Arc::clone(intents);
    let do_creature_say = lua.create_function(move |_, text: String| {
        if text.is_empty()
            || text.len() > MAX_SANDBOXED_LUA_EFFECT_TEXT_BYTES
            || text.chars().any(char::is_control)
        {
            return Err(mlua::Error::RuntimeError(
                "invalid doCreatureSay text".into(),
            ));
        }
        record_sandboxed_intent(&say_intents, SandboxedLuaEffect::Say(text))
    })?;
    let add_item_intents = Arc::clone(intents);
    let do_player_add_item = lua.create_function(move |_, (id, count): (u16, u16)| {
        if id == 0 || count == 0 || count > MAX_SANDBOXED_LUA_EFFECT_ITEM_COUNT {
            return Err(mlua::Error::RuntimeError(
                "invalid doPlayerAddItem id or count".into(),
            ));
        }
        record_sandboxed_intent(
            &add_item_intents,
            SandboxedLuaEffect::GiveItem { id, count },
        )
    })?;
    let teleport_intents = Arc::clone(intents);
    let do_teleport_thing = lua.create_function(move |_, (x, y, z): (u16, u16, u8)| {
        record_sandboxed_intent(&teleport_intents, SandboxedLuaEffect::Teleport { x, y, z })
    })?;
    let health_intents = Arc::clone(intents);
    let do_player_add_health = lua.create_function(move |_, amount: u16| {
        if amount == 0 {
            return Err(mlua::Error::RuntimeError(
                "invalid doPlayerAddHealth amount".into(),
            ));
        }
        record_sandboxed_intent(
            &health_intents,
            SandboxedLuaEffect::Heal {
                health: amount,
                mana: 0,
            },
        )
    })?;
    let mana_intents = Arc::clone(intents);
    let do_player_add_mana = lua.create_function(move |_, amount: u16| {
        if amount == 0 {
            return Err(mlua::Error::RuntimeError(
                "invalid doPlayerAddMana amount".into(),
            ));
        }
        record_sandboxed_intent(
            &mana_intents,
            SandboxedLuaEffect::Heal {
                health: 0,
                mana: amount,
            },
        )
    })?;
    let remove_item_intents = Arc::clone(intents);
    let do_player_remove_item = lua.create_function(move |_, (id, count): (u16, u16)| {
        if id == 0 || count == 0 || count > MAX_SANDBOXED_LUA_EFFECT_ITEM_COUNT {
            return Err(mlua::Error::RuntimeError(
                "invalid doPlayerRemoveItem id or count".into(),
            ));
        }
        record_sandboxed_intent(
            &remove_item_intents,
            SandboxedLuaEffect::RemoveItem { id, count },
        )
    })?;
    let magic_intents = Arc::clone(intents);
    let do_send_magic_effect =
        lua.create_function(move |_, (x, y, z, kind): (u16, u16, u8, u8)| {
            if kind == 0 {
                return Err(mlua::Error::RuntimeError(
                    "invalid doSendMagicEffect kind".into(),
                ));
            }
            record_sandboxed_intent(
                &magic_intents,
                SandboxedLuaEffect::MagicEffect { x, y, z, kind },
            )
        })?;
    let get_thing_pos =
        lua.create_function(move |lua, (): ()| sandboxed_lua_position_value(lua, position))?;
    let do_get_storage_value = lua.create_function(move |_, (cid, key): (u64, i64)| {
        if cid != subject_id {
            return Err(mlua::Error::RuntimeError("foreign storage subject".into()));
        }
        Ok(storage.get(&key).copied().unwrap_or(-1))
    })?;
    let set_storage_intents = Arc::clone(intents);
    let do_set_storage_value =
        lua.create_function(move |_, (cid, key, value): (u64, i64, i64)| {
            if cid != subject_id {
                return Err(mlua::Error::RuntimeError("foreign storage subject".into()));
            }
            record_sandboxed_intent(
                &set_storage_intents,
                SandboxedLuaEffect::SetStorage { key, value },
            )
        })?;
    lua.globals().set("doCreatureSay", do_creature_say)?;
    lua.globals().set("doPlayerAddItem", do_player_add_item)?;
    lua.globals().set("doTeleportThing", do_teleport_thing)?;
    lua.globals()
        .set("doPlayerAddHealth", do_player_add_health)?;
    lua.globals().set("doPlayerAddMana", do_player_add_mana)?;
    lua.globals()
        .set("doPlayerRemoveItem", do_player_remove_item)?;
    lua.globals()
        .set("doSendMagicEffect", do_send_magic_effect)?;
    lua.globals().set("getThingPos", get_thing_pos)?;
    lua.globals()
        .set("getPlayerStorageValue", do_get_storage_value)?;
    lua.globals()
        .set("setPlayerStorageValue", do_set_storage_value)?;
    Ok(())
}

/// Records one validated intent against the per-dispatch budget. Over-budget calls fail
/// closed so a runaway script cannot queue unbounded work.
fn record_sandboxed_intent(
    intents: &Arc<Mutex<Vec<SandboxedLuaEffect>>>,
    effect: SandboxedLuaEffect,
) -> Result<bool, mlua::Error> {
    let mut intents = intents.lock().unwrap_or_else(|poison| poison.into_inner());
    if intents.len() >= MAX_SANDBOXED_LUA_EFFECTS {
        return Err(mlua::Error::RuntimeError(
            "sandbox effect budget exhausted".into(),
        ));
    }
    intents.push(effect);
    Ok(true)
}

/// Extracts a bounded list of typed effects from an array-of-effect-table return:
/// `{ { say = "text" }, { teleport = { x = 1, y = 2, z = 7 } }, { heal = { health = 10, mana = 0 } },
/// { give_item = { id = 2160, count = 1 } }, { remove_item = { id = 2160, count = 1 } },
/// { magic_effect = { x = 1, y = 2, z = 7, kind = 10 } },
/// { set_storage = { key = 1000, value = 3 } } }`. Any non-table value, a non-sequence
/// element, an effect entry with no recognized field, an oversize/control text, an out-of-range
/// coordinate, a zero heal/give-item/remove-item, a zero magic-effect kind, or more than the
/// bounded effect count rejects the whole return.
fn sandboxed_lua_effects(value: Value) -> Option<Vec<SandboxedLuaEffect>> {
    let Value::Table(table) = value else {
        return None;
    };
    let mut effects = Vec::new();
    for entry in table.sequence_values::<Value>() {
        let entry = entry.ok()?;
        if effects.len() >= MAX_SANDBOXED_LUA_EFFECTS {
            return None;
        }
        let Value::Table(effect) = entry else {
            return None;
        };
        let mut produced = false;
        let say: Option<String> = match effect.get("say") {
            Ok(value) => value,
            Err(_) => return None,
        };
        if let Some(say) = say {
            if say.is_empty()
                || say.len() > MAX_SANDBOXED_LUA_EFFECT_TEXT_BYTES
                || say.chars().any(char::is_control)
            {
                return None;
            }
            effects.push(SandboxedLuaEffect::Say(say));
            produced = true;
        }
        let teleport: Option<Table> = match effect.get("teleport") {
            Ok(value) => value,
            Err(_) => return None,
        };
        if let Some(position) = teleport {
            effects.push(parse_teleport_effect(position)?);
            produced = true;
        }
        let heal: Option<Table> = match effect.get("heal") {
            Ok(value) => value,
            Err(_) => return None,
        };
        if let Some(amounts) = heal {
            effects.push(parse_heal_effect(amounts)?);
            produced = true;
        }
        let give_item: Option<Table> = match effect.get("give_item") {
            Ok(value) => value,
            Err(_) => return None,
        };
        if let Some(item) = give_item {
            effects.push(parse_give_item_effect(item)?);
            produced = true;
        }
        let remove_item: Option<Table> = match effect.get("remove_item") {
            Ok(value) => value,
            Err(_) => return None,
        };
        if let Some(item) = remove_item {
            effects.push(parse_remove_item_effect(item)?);
            produced = true;
        }
        let magic_effect: Option<Table> = match effect.get("magic_effect") {
            Ok(value) => value,
            Err(_) => return None,
        };
        if let Some(effect_table) = magic_effect {
            effects.push(parse_magic_effect(effect_table)?);
            produced = true;
        }
        let set_storage: Option<Table> = match effect.get("set_storage") {
            Ok(value) => value,
            Err(_) => return None,
        };
        if let Some(storage) = set_storage {
            effects.push(parse_set_storage_effect(storage)?);
            produced = true;
        }
        if !produced {
            return None;
        }
    }
    Some(effects)
}

fn parse_teleport_effect(position: Table) -> Option<SandboxedLuaEffect> {
    let x: u16 = position.get("x").ok()?;
    let y: u16 = position.get("y").ok()?;
    let z: u8 = position.get("z").ok()?;
    Some(SandboxedLuaEffect::Teleport { x, y, z })
}

fn parse_heal_effect(amounts: Table) -> Option<SandboxedLuaEffect> {
    let health: u16 = amounts.get("health").ok()?;
    let mana: u16 = amounts.get("mana").ok()?;
    if health == 0 && mana == 0 {
        return None;
    }
    Some(SandboxedLuaEffect::Heal { health, mana })
}

fn parse_give_item_effect(item: Table) -> Option<SandboxedLuaEffect> {
    let id: u16 = item.get("id").ok()?;
    let count: u16 = item.get("count").ok()?;
    if id == 0 || count == 0 || count > MAX_SANDBOXED_LUA_EFFECT_ITEM_COUNT {
        return None;
    }
    Some(SandboxedLuaEffect::GiveItem { id, count })
}

fn parse_remove_item_effect(item: Table) -> Option<SandboxedLuaEffect> {
    let id: u16 = item.get("id").ok()?;
    let count: u16 = item.get("count").ok()?;
    if id == 0 || count == 0 || count > MAX_SANDBOXED_LUA_EFFECT_ITEM_COUNT {
        return None;
    }
    Some(SandboxedLuaEffect::RemoveItem { id, count })
}

fn parse_magic_effect(effect_table: Table) -> Option<SandboxedLuaEffect> {
    let x: u16 = effect_table.get("x").ok()?;
    let y: u16 = effect_table.get("y").ok()?;
    let z: u8 = effect_table.get("z").ok()?;
    let kind: u8 = effect_table.get("kind").ok()?;
    if kind == 0 {
        return None;
    }
    Some(SandboxedLuaEffect::MagicEffect { x, y, z, kind })
}

fn parse_set_storage_effect(storage_table: Table) -> Option<SandboxedLuaEffect> {
    let key: i64 = storage_table.get("key").ok()?;
    let value: i64 = storage_table.get("value").ok()?;
    Some(SandboxedLuaEffect::SetStorage { key, value })
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
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
    fn callback_dispatcher_passes_a_bounded_string_argument_and_rejects_oversized_ones() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "echo-arg",
                "return function(kind, id, value, argument) if kind == 'talkaction' then return argument end return value end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: "100 100".into(),
            position: None,
            storage: BTreeMap::new(),
        };
        assert_eq!(
            dispatcher.dispatch("echo-arg", &input).value,
            Some(SandboxedLuaValue::Text("100 100".into()))
        );

        // A three-parameter callback ignores the fourth argument, preserving backward access.
        dispatcher
            .register_callback("legacy", "return function(kind, _, value) return value end")
            .unwrap();
        let legacy = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 1,
            value: 7,
            argument: "ignored".into(),
            position: None,
            storage: BTreeMap::new(),
        };
        assert_eq!(
            dispatcher.dispatch("legacy", &legacy).value,
            Some(SandboxedLuaValue::Integer(7))
        );

        assert_eq!(
            dispatcher
                .dispatch(
                    "echo-arg",
                    &SandboxedLuaCallbackInput {
                        event_kind: "talkaction".into(),
                        subject_id: 1,
                        value: 0,
                        argument: "x".repeat(MAX_SANDBOXED_LUA_CALLBACK_ARGUMENT_BYTES + 1),
                        position: None,
                        storage: BTreeMap::new(),
                    },
                )
                .state,
            SandboxedLuaCallbackDispatchState::InputRejected
        );
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
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
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
                        argument: String::new(),
                        position: None,
                        storage: BTreeMap::new(),
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
                        argument: String::new(),
                        position: None,
                        storage: BTreeMap::new(),
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
                        argument: String::new(),
                        position: None,
                        storage: BTreeMap::new(),
                    }
                )
                .state,
            SandboxedLuaCallbackDispatchState::InputRejected
        );
    }

    #[test]
    fn callback_dispatcher_extracts_bounded_effect_intents() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "go",
                "return function() return { { say = 'Teleported!' }, { teleport = { x = 100, y = 100, z = 7 } } } end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
        };
        let outcome = dispatcher.dispatch_effects("go", &input);
        assert_eq!(outcome.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            outcome.effects,
            vec![
                SandboxedLuaEffect::Say("Teleported!".into()),
                SandboxedLuaEffect::Teleport {
                    x: 100,
                    y: 100,
                    z: 7
                },
            ]
        );

        dispatcher
            .register_callback("bad", "return function() return 'not-a-table' end")
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_effects("bad", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );

        dispatcher
            .register_callback(
                "bad-coord",
                "return function() return { { teleport = { x = 1, y = 2, z = 999 } } } end",
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_effects("bad-coord", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );
    }

    #[test]
    fn callback_dispatcher_extracts_heal_and_give_item_effect_intents() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "recover",
                "return function() return { { heal = { health = 25, mana = 0 } }, { give_item = { id = 2160, count = 2 } } } end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
        };
        let outcome = dispatcher.dispatch_effects("recover", &input);
        assert_eq!(outcome.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            outcome.effects,
            vec![
                SandboxedLuaEffect::Heal {
                    health: 25,
                    mana: 0
                },
                SandboxedLuaEffect::GiveItem { id: 2160, count: 2 },
            ]
        );

        dispatcher
            .register_callback(
                "zero-heal",
                "return function() return { { heal = { health = 0, mana = 0 } } } end",
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_effects("zero-heal", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );

        dispatcher
            .register_callback(
                "oversized-give",
                format!(
                    "return function() return {{ {{ give_item = {{ id = 2160, count = {} }} }} }} end",
                    MAX_SANDBOXED_LUA_EFFECT_ITEM_COUNT + 1
                ),
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_effects("oversized-give", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );

        dispatcher
            .register_callback(
                "zero-give",
                "return function() return { { give_item = { id = 2160, count = 0 } } } end",
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_effects("zero-give", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );
    }

    #[test]
    fn callback_dispatcher_extracts_remove_item_and_magic_effect_intents() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "consume",
                "return function() return { { remove_item = { id = 2160, count = 3 } }, { magic_effect = { x = 10, y = 20, z = 7, kind = 10 } } } end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
        };
        let outcome = dispatcher.dispatch_effects("consume", &input);
        assert_eq!(outcome.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            outcome.effects,
            vec![
                SandboxedLuaEffect::RemoveItem { id: 2160, count: 3 },
                SandboxedLuaEffect::MagicEffect {
                    x: 10,
                    y: 20,
                    z: 7,
                    kind: 10
                },
            ]
        );

        dispatcher
            .register_callback(
                "zero-kind",
                "return function() return { { magic_effect = { x = 1, y = 2, z = 7, kind = 0 } } } end",
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_effects("zero-kind", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );

        dispatcher
            .register_callback(
                "zero-remove",
                "return function() return { { remove_item = { id = 2160, count = 0 } } } end",
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_effects("zero-remove", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
        );
    }

    #[test]
    fn callback_dispatcher_passes_subject_position_as_an_optional_fifth_argument() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "where",
                "return function(_, _, _, _, position) if position == nil then return 'none' end return position.x + position.y + position.z end",
            )
            .unwrap();
        let positioned = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: String::new(),
            position: Some(SandboxedLuaPosition {
                x: 100,
                y: 200,
                z: 7,
            }),
            storage: BTreeMap::new(),
        };
        assert_eq!(
            dispatcher.dispatch("where", &positioned).value,
            Some(SandboxedLuaValue::Integer(307))
        );
        let unpositioned = SandboxedLuaCallbackInput {
            position: None,
            storage: BTreeMap::new(),
            ..positioned.clone()
        };
        // A callback written for fewer arguments ignores the trailing position.
        dispatcher
            .register_callback("legacy", "return function(kind, _, value) return value end")
            .unwrap();
        let legacy = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 1,
            value: 7,
            argument: "ignored".into(),
            position: Some(SandboxedLuaPosition { x: 1, y: 2, z: 3 }),
            storage: BTreeMap::new(),
        };
        assert_eq!(
            dispatcher.dispatch("legacy", &legacy).value,
            Some(SandboxedLuaValue::Integer(7))
        );
        assert_eq!(
            dispatcher.dispatch("where", &unpositioned).value,
            Some(SandboxedLuaValue::Text("none".into()))
        );
    }

    #[test]
    fn callback_dispatcher_records_bound_host_calls_as_intents() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "greet",
                "return function(_, _, _, _, position) doCreatureSay('Hello ' .. position.x) doPlayerAddItem(2160, 2) end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: String::new(),
            position: Some(SandboxedLuaPosition {
                x: 100,
                y: 200,
                z: 7,
            }),
            storage: BTreeMap::new(),
        };
        let outcome = dispatcher.dispatch_api("greet", &input);
        assert_eq!(outcome.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            outcome.effects,
            vec![
                SandboxedLuaEffect::Say("Hello 100".into()),
                SandboxedLuaEffect::GiveItem { id: 2160, count: 2 },
            ]
        );

        dispatcher
            .register_callback(
                "locate",
                "return function() local pos = getThingPos() return pos == nil end",
            )
            .unwrap();
        let unpositioned = SandboxedLuaCallbackInput {
            position: None,
            storage: BTreeMap::new(),
            ..input.clone()
        };
        // getThingPos answers nil without a subject position; the boolean return is ignored.
        assert_eq!(
            dispatcher.dispatch_api("locate", &unpositioned).state,
            SandboxedLuaCallbackDispatchState::Completed
        );
        assert!(dispatcher
            .dispatch_api("locate", &unpositioned)
            .effects
            .is_empty());

        dispatcher
            .register_callback("bad-call", "return function() doPlayerAddItem(0, 1) end")
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_api("bad-call", &input).state,
            SandboxedLuaCallbackDispatchState::RuntimeRejected
        );
        assert!(dispatcher
            .dispatch_api("bad-call", &input)
            .effects
            .is_empty());

        assert_eq!(
            dispatcher.dispatch_api("missing", &input).state,
            SandboxedLuaCallbackDispatchState::CallbackNotFound
        );
    }

    #[test]
    fn callback_dispatcher_records_teleport_heal_remove_and_magic_calls() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "buff",
                "return function() doTeleportThing(10, 20, 7) doPlayerAddHealth(25) doPlayerAddMana(10) doPlayerRemoveItem(2160, 1) doSendMagicEffect(10, 20, 7, 10) end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
        };
        let outcome = dispatcher.dispatch_api("buff", &input);
        assert_eq!(outcome.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            outcome.effects,
            vec![
                SandboxedLuaEffect::Teleport { x: 10, y: 20, z: 7 },
                SandboxedLuaEffect::Heal {
                    health: 25,
                    mana: 0
                },
                SandboxedLuaEffect::Heal {
                    health: 0,
                    mana: 10
                },
                SandboxedLuaEffect::RemoveItem { id: 2160, count: 1 },
                SandboxedLuaEffect::MagicEffect {
                    x: 10,
                    y: 20,
                    z: 7,
                    kind: 10
                },
            ]
        );

        dispatcher
            .register_callback("zero-heal", "return function() doPlayerAddHealth(0) end")
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_api("zero-heal", &input).state,
            SandboxedLuaCallbackDispatchState::RuntimeRejected
        );

        dispatcher
            .register_callback(
                "zero-kind",
                "return function() doSendMagicEffect(1, 2, 7, 0) end",
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_api("zero-kind", &input).state,
            SandboxedLuaCallbackDispatchState::RuntimeRejected
        );
    }

    #[test]
    fn callback_dispatcher_answers_storage_reads_and_records_writes() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "read",
                "return function(_, cid) return getPlayerStorageValue(cid, 1000) end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "write",
                "return function(_, cid) setPlayerStorageValue(cid, 1000, 3) end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "foreign",
                "return function() return getPlayerStorageValue(77, 1000) end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 7,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::from([(1000, 3)]),
        };
        // A hit returns the stored value; the callback return itself is ignored.
        let hit = dispatcher.dispatch_api("read", &input);
        assert_eq!(hit.state, SandboxedLuaCallbackDispatchState::Completed);
        assert!(hit.effects.is_empty());
        // A write records a single SetStorage intent against the budget.
        let written = dispatcher.dispatch_api("write", &input);
        assert_eq!(written.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            written.effects,
            vec![SandboxedLuaEffect::SetStorage {
                key: 1000,
                value: 3
            }]
        );
        // A foreign subject id fails the dispatch closed.
        assert_eq!(
            dispatcher.dispatch_api("foreign", &input).state,
            SandboxedLuaCallbackDispatchState::RuntimeRejected
        );
    }

    #[test]
    fn callback_dispatcher_misses_storage_as_minus_one_and_parses_set_tables() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "miss",
                "return function(_, cid) if getPlayerStorageValue(cid, 999) == -1 then doCreatureSay('unset') end end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "table",
                "return function() return { { set_storage = { key = 1000, value = 3 } } } end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 7,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
        };
        let miss = dispatcher.dispatch_api("miss", &input);
        assert_eq!(miss.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(miss.effects, vec![SandboxedLuaEffect::Say("unset".into())]);
        let table = dispatcher.dispatch_api("table", &input);
        assert_eq!(table.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            table.effects,
            vec![SandboxedLuaEffect::SetStorage {
                key: 1000,
                value: 3
            }]
        );
    }

    #[test]
    fn callback_dispatcher_unions_calls_with_returned_effect_tables() {
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback(
                "mixed",
                "return function() doCreatureSay('called') return { { teleport = { x = 1, y = 2, z = 7 } } } end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "legacy-table",
                "return function() return { { say = 'still works' } } end",
            )
            .unwrap();
        dispatcher
            .register_callback(
                "explicit-return",
                "return function() doCreatureSay('hi') return true end",
            )
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "talkaction".into(),
            subject_id: 9,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
        };
        let mixed = dispatcher.dispatch_api("mixed", &input);
        assert_eq!(mixed.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            mixed.effects,
            vec![
                SandboxedLuaEffect::Say("called".into()),
                SandboxedLuaEffect::Teleport { x: 1, y: 2, z: 7 },
            ]
        );
        // A pure return-table script is unaffected by api routing.
        let legacy = dispatcher.dispatch_api("legacy-table", &input);
        assert_eq!(legacy.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(
            legacy.effects,
            vec![SandboxedLuaEffect::Say("still works".into())]
        );
        // A trailing non-table return after calls is ignored, not rejected.
        let explicit = dispatcher.dispatch_api("explicit-return", &input);
        assert_eq!(explicit.state, SandboxedLuaCallbackDispatchState::Completed);
        assert_eq!(explicit.effects, vec![SandboxedLuaEffect::Say("hi".into())]);

        // Malformed return tables still reject even when calls succeeded.
        dispatcher
            .register_callback(
                "bad-table",
                "return function() doCreatureSay('hi') return { { nope = 1 } } end",
            )
            .unwrap();
        assert_eq!(
            dispatcher.dispatch_api("bad-table", &input).state,
            SandboxedLuaCallbackDispatchState::UnsupportedValue
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
                        argument: String::new(),
                        position: None,
                        storage: BTreeMap::new(),
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
    fn callback_dispatcher_reloads_edited_files_and_keeps_unreadable_ones() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("forgotten-engine-script-reload-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("live.lua"), "return function() return 1 end").unwrap();
        let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
        dispatcher
            .register_callback_file("live", &root, Path::new("live.lua"))
            .unwrap();
        dispatcher
            .register_callback("inline", "return function() return 0 end")
            .unwrap();
        let input = SandboxedLuaCallbackInput {
            event_kind: "reload".into(),
            subject_id: 7,
            value: 0,
            argument: String::new(),
            position: None,
            storage: BTreeMap::new(),
        };
        assert_eq!(
            dispatcher.dispatch("live", &input).value,
            Some(SandboxedLuaValue::Integer(1))
        );
        // An edited file is picked up; the inline callback is never touched.
        fs::write(root.join("live.lua"), "return function() return 2 end").unwrap();
        let report = dispatcher.reload_file_callbacks();
        assert_eq!(report.reloaded, vec!["live".to_string()]);
        assert!(report.failed.is_empty());
        assert!(report.summary().contains("reloaded=1"));
        assert_eq!(
            dispatcher.dispatch("live", &input).value,
            Some(SandboxedLuaValue::Integer(2))
        );
        assert_eq!(
            dispatcher.dispatch("inline", &input).value,
            Some(SandboxedLuaValue::Integer(0))
        );
        // An unreadable file keeps serving its previous source and is reported.
        fs::remove_file(root.join("live.lua")).unwrap();
        let report = dispatcher.reload_file_callbacks();
        assert!(report.reloaded.is_empty());
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].name, "live");
        assert!(report.summary().contains("failed=1"));
        assert_eq!(
            dispatcher.dispatch("live", &input).value,
            Some(SandboxedLuaValue::Integer(2))
        );
        // Clones snapshot independent maps: a twin reloads the same files alone.
        fs::write(root.join("live.lua"), "return function() return 3 end").unwrap();
        let twin = dispatcher.clone();
        let twin_report = twin.reload_file_callbacks();
        assert_eq!(twin_report.reloaded, vec!["live".to_string()]);
        assert_eq!(
            twin.dispatch("live", &input).value,
            Some(SandboxedLuaValue::Integer(3))
        );
        assert_eq!(
            dispatcher.dispatch("live", &input).value,
            Some(SandboxedLuaValue::Integer(2))
        );
        fs::remove_dir_all(root).unwrap();
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
