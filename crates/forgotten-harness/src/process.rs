//! Spawns the real `forgotten-engine` CLI as a subprocess for every step -
//! `init`, `account create`, `player create`, `run` - rather than linking
//! against `forgotten-engine-cli` as a library. This keeps the harness a
//! true black-box test: it provisions and drives a world exactly the way an
//! operator's shell session would, over the same CLI surface and the same
//! wire protocol a real client would use. Nothing here reaches into engine
//! internals.
//!
//! Port handling, explained because it isn't obvious from the CLI alone:
//! `forgotten-engine run` has no `--port` flag. It binds whatever
//! `otclientLoginPort` / `otclientGamePort` are set to in the world's
//! `config.lua`, which `init` seeds to the fixed defaults 7174/7175 - the
//! exact fixed ports the task spec warned against, since they collide with
//! an operator's already-running world. There is also no port-0 ephemeral
//! bind support in `run` to read a port back from. So this module: probes
//! two free ports by binding `127.0.0.1:0` and reading back what the OS
//! assigned, closes those probe sockets immediately, then rewrites
//! `config.lua` before calling `run`. That leaves a small, standard
//! probe-then-bind race (another process could take the port in between) -
//! acceptable for a test harness, not for anything that has to be airtight.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::HarnessError;

/// Path to the real, built CLI binary.
///
/// `CARGO_BIN_EXE_forgotten-engine` is deliberately NOT used: that variable is
/// only defined for integration tests of a package that has a *binary target*,
/// and `forgotten-engine-cli` cannot be declared as a dependency here at all
/// because it is bin-only (no lib target), so cargo silently drops it and the
/// variable never materialises. Resolving relative to this test executable's
/// own location sidesteps both problems: an integration-test binary lives in
/// `<target>/debug/deps/`, and the CLI is built into `<target>/debug/`.
fn cli_binary_path() -> Result<PathBuf, HarnessError> {
    // An explicit override wins, for running against a release build or a
    // binary produced by another toolchain.
    if let Some(explicit) = std::env::var_os("FE_ENGINE_BIN") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
    }
    let executable = std::env::current_exe()?;
    // <target>/debug/deps/<test-bin> -> <target>/debug/forgotten-engine[.exe]
    let directory = executable.parent().and_then(Path::parent).ok_or_else(|| {
        HarnessError::Io(std::io::Error::other(format!(
            "cannot locate the target directory from test executable {}",
            executable.display()
        )))
    })?;
    let name = if cfg!(windows) {
        "forgotten-engine.exe"
    } else {
        "forgotten-engine"
    };
    let candidate = directory.join(name);
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(HarnessError::Io(std::io::Error::other(format!(
        "forgotten-engine binary not found at {} - build the workspace first \
         (cargo build --workspace), or point FE_ENGINE_BIN at it",
        candidate.display()
    ))))
}

/// A directory under `std::env::temp_dir()` unique to this run. No
/// `tempfile` dependency, per review: a process-id + nanosecond-timestamp
/// suffix is enough collision resistance for a test harness, and it keeps
/// this crate off the dependency tree that produced the earlier MSRV
/// confusion entirely.
fn fresh_temp_dir(label: &str) -> PathBuf {
    let unique = format!(
        "{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    std::env::temp_dir().join(unique)
}

/// Binds `127.0.0.1:0`, reads back the OS-assigned port, and immediately
/// releases it. See the module doc comment for the race this accepts.
fn probe_free_port() -> Result<u16, HarnessError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn run_cli(args: &[&str]) -> Result<String, HarnessError> {
    let mut command = Command::new(cli_binary_path()?);
    command.args(args);
    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HarnessError::Io(std::io::Error::other(format!(
            "forgotten-engine {args:?} failed: status={:?} stdout={stdout} stderr={stderr}",
            output.status.code()
        ))));
    }
    Ok(stdout)
}

/// A provisioned, running world under test. Kills the child process on
/// drop so a failed assertion never leaks a server bound to a real port.
pub struct RunningWorld {
    pub directory: PathBuf,
    pub login_addr: SocketAddr,
    pub game_addr: SocketAddr,
    pub account_id: u32,
    pub protocol_version: u16,
    child: Child,
}

impl Drop for RunningWorld {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Full milestone-1 provisioning: init -> patch config for ephemeral,
/// native-enabled ports -> account create -> player create -> run, polling
/// the login port until it accepts a raw TCP connection rather than
/// sleeping a fixed guess.
///
/// `protocol_version` should be 740 or 760 - see the note already on record
/// in `config.lua`'s own template: "740 (legacy; an unmodified OTCv8 at 740 discards all
/// chat/look text) and 760 (recommended)". Milestone 1 does not assert on
/// chat, so 740 is fine here; a later milestone touching Talk (Task A step
/// 7) should very likely default to 760 instead, given that documented
/// limitation - flagging this now rather than letting it be rediscovered.
pub fn provision_and_run(
    profile: &str,
    protocol_version: u16,
    account_name: &str,
    password: &str,
    character_name: &str,
) -> Result<RunningWorld, HarnessError> {
    let directory = fresh_temp_dir("forgotten-harness-world");
    let directory_str = directory.to_string_lossy().into_owned();

    run_cli(&["init", &directory_str, profile])?;

    let login_port = probe_free_port()?;
    let game_port = probe_free_port()?;
    patch_config_for_harness(&directory, login_port, game_port, protocol_version)?;

    let account_output = run_cli(&["account", "create", &directory_str, account_name, password])?;
    let account_id = parse_prefixed_u32(&account_output, "native-account-id=")?;

    run_cli(&[
        "player",
        "create",
        &directory_str,
        &account_id.to_string(),
        character_name,
    ])?;

    let child = Command::new(cli_binary_path()?)
        .args(["run", &directory_str])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let login_addr: SocketAddr = ("127.0.0.1", login_port)
        .to_socket_addrs_first()
        .ok_or(HarnessError::EmptyFrame)?; // see note on EmptyFrame reuse
                                           // below; a dedicated variant is
                                           // owed here in a follow-up pass.
    wait_for_port(login_addr, Duration::from_secs(10))?;

    Ok(RunningWorld {
        directory,
        login_addr,
        game_addr: ("127.0.0.1", game_port)
            .to_socket_addrs_first()
            .ok_or(HarnessError::EmptyFrame)?,
        account_id,
        protocol_version,
        child,
    })
}

/// Rewrites the four keys `init` seeds that the harness needs to differ
/// from the operator-facing defaults. Uses targeted line replacement against
/// the exact default lines `init` writes (see `forgotten-engine-cli`'s own
/// embedded template) rather than a general key=value parser, since this
/// crate has no dependency on whatever config-parsing crate (if any)
/// `forgotten-config` uses internally, and adding one was explicitly ruled
/// out. If `init`'s template ever changes these exact default lines, this
/// will fail loudly (the expected line won't be found) rather than silently
/// writing a no-op - which is the right failure mode for a harness.
fn patch_config_for_harness(
    directory: &Path,
    login_port: u16,
    game_port: u16,
    protocol_version: u16,
) -> Result<(), HarnessError> {
    let config_path = directory.join("config.lua");
    let original = std::fs::read_to_string(&config_path)?;
    let patched = original
        .replace(
            "otclientNativeEnabled = false",
            "otclientNativeEnabled = true",
        )
        .replace(
            "otclientLoginPort = 7174",
            &format!("otclientLoginPort = {login_port}"),
        )
        .replace(
            "otclientGamePort = 7175",
            &format!("otclientGamePort = {game_port}"),
        )
        .replace(
            "otclientProtocolVersion = 0",
            &format!("otclientProtocolVersion = {protocol_version}"),
        )
        .replace(
            "advertisedOtClientV8GamePort = 7175",
            &format!("advertisedOtClientV8GamePort = {game_port}"),
        )
        // `init` also ships the native empty-world diagnostic fixture disabled
        // with a zero look type. Without these the game port answers character
        // selection with "native map initialization is not enabled for this
        // selected client profile". A zero look type additionally renders the
        // player invisible (effect 13) - see the known-issue about accidental
        // invisibility - so a real look type is set here too. 128 is the
        // classic knight look, and 102 the classic grass ground tile, matching
        // the values the operator's working debug sandbox uses.
        .replace(
            "otclientNativeEmptyWorldEnabled = false",
            "otclientNativeEmptyWorldEnabled = true",
        )
        .replace(
            "otclientEmptyWorldGroundThingId = 0",
            "otclientEmptyWorldGroundThingId = 102",
        )
        .replace("otclientPlayerLookType = 0", "otclientPlayerLookType = 128")
        // A nonzero look type also requires a coherent outfit range: the config
        // validator rejects `look_type != 0` unless first <= look <= last, and
        // init ships all three at 0. 128..131 is the classic knight outfit
        // chooser, again matching the operator's working debug sandbox.
        .replace(
            "otclientOutfitFirstLookType = 0",
            "otclientOutfitFirstLookType = 128",
        )
        .replace(
            "otclientOutfitLastLookType = 0",
            "otclientOutfitLastLookType = 131",
        );
    // Every replacement above is load-bearing. If init's template ever changes,
    // fail loudly instead of silently running against a world with the native
    // handshake off or the ports still at their operator defaults.
    for expected in [
        "otclientNativeEnabled = true",
        "otclientNativeEmptyWorldEnabled = true",
        "otclientEmptyWorldGroundThingId = 102",
        "otclientPlayerLookType = 128",
        "otclientOutfitFirstLookType = 128",
        "otclientOutfitLastLookType = 131",
    ] {
        if !patched.contains(expected) {
            return Err(HarnessError::Io(std::io::Error::other(format!(
                "config.lua is missing `{expected}` after patching - init's \
                 template may have changed; update patch_config_for_harness \
                 to match rather than silently continuing"
            ))));
        }
    }
    if !patched.contains(&format!("otclientLoginPort = {login_port}"))
        || !patched.contains(&format!("otclientGamePort = {game_port}"))
    {
        return Err(HarnessError::Io(std::io::Error::other(format!(
            "config.lua still does not carry the harness ports \
             (login={login_port} game={game_port}) - init's template may have \
             changed; update patch_config_for_harness to match"
        ))));
    }
    std::fs::write(&config_path, patched)?;
    Ok(())
}

/// Pulls `prefix<N>` out of CLI stdout. The token is searched for *anywhere* in
/// the line, not only at the start: `account create` emits a single line shaped
/// like `created local account name=... native-account-id=1`, so a
/// `strip_prefix` on the trimmed line never matches.
fn parse_prefixed_u32(output: &str, prefix: &str) -> Result<u32, HarnessError> {
    for line in output.lines() {
        let Some(start) = line.find(prefix) else {
            continue;
        };
        let value = line[start + prefix.len()..].trim_start();
        // Take the leading run of digits, then stop at the first non-digit, so
        // trailing ` name=...` or end-of-line text is tolerated.
        let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(parsed) = digits.parse::<u32>() {
            return Ok(parsed);
        }
    }
    Err(HarnessError::Io(std::io::Error::other(format!(
        "CLI output did not contain a `{prefix}<u32>` token: {output:?}"
    ))))
}

fn wait_for_port(addr: SocketAddr, timeout: Duration) -> Result<(), HarnessError> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(last_error
        .map(HarnessError::Io)
        .unwrap_or(HarnessError::EmptyFrame))
}

/// Small local helper: `(&str, u16)` doesn't implement `ToSocketAddrs` in a
/// way that avoids a DNS-resolution code path for `"127.0.0.1"` specifically
/// being just a formality; spelled out as its own function so the two call
/// sites above stay readable.
trait FirstSocketAddr {
    fn to_socket_addrs_first(&self) -> Option<SocketAddr>;
}
impl FirstSocketAddr for (&str, u16) {
    fn to_socket_addrs_first(&self) -> Option<SocketAddr> {
        use std::net::ToSocketAddrs;
        (self.0, self.1).to_socket_addrs().ok()?.next()
    }
}

// Reading the child's stdout/stderr (e.g. --debug trace lines, matching how
// the walking-cadence work earlier this session diagnosed itself) is real,
// wanted scope for a later milestone. Deliberately not stubbed out here:
// a half-written, uncompiled stub is worse than no stub, given this whole
// crate is being handed over without a compile check on this end.
