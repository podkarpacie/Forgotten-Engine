//! Shared per-session handler context for extracted `session_loop.rs` action blocks.
//!
//! The session loop is a single large function whose locals (stream, database, authoritative
//! handles, snapshot, facing, position, refresh epochs) every action block needs. Rather than
//! threading a dozen parameters — or one near-identical context struct per handler — extracted
//! handlers take `&mut SessionContext` (universal plumbing) plus only their handler-specific
//! inputs (request, dispatcher, catalogs). This is the converged shape decided at Bug #5
//! increment 2; new handlers reuse it instead of defining their own.

use super::*;

/// Universal borrowed session plumbing shared by every extracted action handler. All fields are
/// either `Copy` values or references; constructing it is cheap, so call sites build one per
/// handler call rather than holding borrows across unrelated code.
pub(crate) struct SessionContext<'a> {
    pub stream: &'a mut TcpStream,
    pub peer: SocketAddr,
    pub character_id: u64,
    pub database: &'a mut EngineDatabase,
    pub shared_world: &'a SharedNativeWorld,
    pub config: &'a NativeOtClientHostConfig,
    pub world_map: &'a Arc<WorldMap>,
    pub snapshot: &'a NativeOtClientEmptyWorldSnapshot,
    pub facing: NativeOtClientCardinalDirection,
    pub player_position: &'a mut Position,
    pub observed_dead: bool,
    pub observed_visibility_epoch: &'a mut u64,
    pub observed_vitals_epoch: &'a mut u64,
}

/// Control-flow outcome of one extracted handler. `Handled` means the record was consumed (the
/// caller must `continue` to the next session action); `Unhandled` falls through to the next
/// handler. Session-ending failures travel via `Err(HostError)`, never this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionActionOutcome {
    Handled,
    Unhandled,
}
