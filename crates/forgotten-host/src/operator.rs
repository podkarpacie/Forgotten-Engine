//! Localhost-only live operator bridge for Forgotten Engine.
//!
//! While the native game listener runs, this module accepts one JSON-line request per TCP
//! connection on a loopback control port, applies the requested authoritative mutation to the
//! shared world (and/or SQLite), and answers with one JSON line. It exists so operators and the
//! Forgotten Cloud console can act on a *running* world instead of only editing the database
//! behind a stopped server.
//!
//! Security model: the listener binds explicitly to `127.0.0.1` and rejects every non-loopback
//! peer before parsing input. Requests are bounded; responses never include credentials or raw
//! packet data.

use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use forgotten_persistence::EngineDatabase;

use crate::{HostError, SharedNativeWorld};

/// Maximum accepted request line size. Operator commands are tiny; anything larger is rejected.
const MAX_REQUEST_LINE_BYTES: usize = 4_096;
/// One request per connection keeps parsing stateless and prevents pipelined flooding.
const ACCEPT_TIMEOUT_MILLIS: u64 = 200;

/// A parsed operator request.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum OperatorRequest {
    /// Announce a message to every connected player through the GM-broadcast talk record.
    Broadcast {
        #[serde(default)]
        message: String,
    },
    /// Grant, set, or revoke a gamemaster tier for one character by name.
    Gm {
        #[serde(default)]
        player: String,
        /// `online` resolves only connected characters; offline reads SQLite directly.
        #[serde(default)]
        scope: String,
        /// Target gamemaster tier 0-3 (0 revokes).
        #[serde(default)]
        level: u8,
    },
    /// Create and insert items into a character's first backpack.
    Give {
        #[serde(default)]
        player: String,
        #[serde(default)]
        scope: String,
        /// Authoritative server item ID from the operator's items catalog.
        #[serde(default)]
        item_id: u16,
        #[serde(default = "default_count")]
        count: u16,
    },
    /// Teleport one character to another character's current position.
    Teleport {
        #[serde(default)]
        from: String,
        #[serde(default)]
        to: String,
        #[serde(default)]
        scope: String,
    },
    /// Summon an entity (by installed creature name) in front of a character.
    Spawn {
        #[serde(default)]
        entity: String,
        /// Optional character whose facing position anchors the summon; defaults to any
        /// connected player when omitted.
        #[serde(default)]
        player: String,
    },
    /// Disconnect one character's active session.
    Kick {
        #[serde(default)]
        player: String,
    },
    /// Report live world counters for console display.
    Status,
}

fn default_count() -> u16 {
    1
}

/// A serialized success/failure answer. `ok=false` carries a human-readable `error`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OperatorResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handled: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl OperatorResponse {
    fn success(handled: &str, detail: Option<String>) -> Self {
        Self {
            ok: true,
            handled: Some(handled.to_string()),
            detail,
            error: None,
        }
    }

    fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            handled: None,
            detail: None,
            error: Some(error.into()),
        }
    }
}

/// Dependencies the bridge needs from the running host. All handles stay shared with the game
/// listener so mutations are visible to sessions immediately.
pub struct OperatorBridgeConfig {
    pub shared_world: SharedNativeWorld,
    pub database_path: PathBuf,
}

struct BridgeRuntime {
    config: OperatorBridgeConfig,
    shutdown: Arc<AtomicBool>,
}

/// Starts the loopback operator bridge. Returns the bound port plus a shutdown handle.
pub fn start_operator_bridge(
    config: OperatorBridgeConfig,
) -> Result<(u16, Arc<AtomicBool>), HostError> {
    let listener =
        TcpListener::bind((IpAddr::from(Ipv4Addr::LOCALHOST), 0)).map_err(HostError::Io)?;
    listener.set_nonblocking(true).map_err(HostError::Io)?;
    let port = listener.local_addr().map_err(HostError::Io)?.port();
    let shutdown = Arc::new(AtomicBool::new(false));
    let runtime = Arc::new(BridgeRuntime {
        config,
        shutdown: Arc::clone(&shutdown),
    });
    thread::spawn(move || serve_operator_bridge(listener, runtime));
    Ok((port, shutdown))
}

fn serve_operator_bridge(listener: TcpListener, runtime: Arc<BridgeRuntime>) {
    while !runtime.shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = handle_operator_connection(&mut stream, &runtime);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(ACCEPT_TIMEOUT_MILLIS));
            }
            Err(_) => break,
        }
    }
}

fn handle_operator_connection(
    stream: &mut TcpStream,
    runtime: &BridgeRuntime,
) -> Result<(), std::io::Error> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.len() > MAX_REQUEST_LINE_BYTES {
        return write_response(stream, OperatorResponse::failure("request too large"));
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return write_response(stream, OperatorResponse::failure("empty request"));
    }
    let response = match serde_json::from_str::<OperatorRequest>(trimmed) {
        Ok(request) => apply_operator_request(runtime, request),
        Err(error) => OperatorResponse::failure(format!(
            "invalid request: {error}; expected {{\"op\": \"broadcast\"|\"gm\"|\"give\"|\"tp\"|\"spawn\"|\"kick\"|\"status\", ...}}"
        )),
    };
    write_response(stream, response)
}

fn write_response(
    stream: &mut TcpStream,
    response: OperatorResponse,
) -> Result<(), std::io::Error> {
    let mut encoded = serde_json::to_string(&response).unwrap_or_else(|_| {
        "{\"ok\":false,\"error\":\"response serialization failed\"}".to_string()
    });
    encoded.push('\n');
    stream.write_all(encoded.as_bytes())?;
    stream.flush()
}

/// Applies one validated operator request against the live shared world and database.
pub fn apply_operator_request(
    runtime: &BridgeRuntime,
    request: OperatorRequest,
) -> OperatorResponse {
    match request {
        OperatorRequest::Status => match collect_status(&runtime.config) {
            Ok(detail) => OperatorResponse::success("status", Some(detail)),
            Err(error) => OperatorResponse::failure(format!("status failed: {error}")),
        },
        OperatorRequest::Broadcast { message } => {
            let body: String = message.split_whitespace().collect::<Vec<_>>().join(" ");
            if body.is_empty() {
                return OperatorResponse::failure("broadcast message is required");
            }
            let truncated = crate::truncate_native_chat_text(&body);
            match runtime
                .config
                .shared_world
                .broadcast_console_message("Console", &truncated)
            {
                Ok(delivered) => {
                    OperatorResponse::success("broadcast", Some(format!("recipients={delivered}")))
                }
                Err(error) => OperatorResponse::failure(format!("broadcast failed: {error}")),
            }
        }
        OperatorRequest::Gm {
            player,
            scope,
            level,
        } => apply_gm(runtime, &player, &scope, level),
        OperatorRequest::Give {
            player,
            scope,
            item_id,
            count,
        } => apply_give(runtime, &player, &scope, item_id, count),
        OperatorRequest::Teleport { from, to, scope } => {
            apply_teleport(runtime, &from, &to, &scope)
        }
        OperatorRequest::Spawn { entity, player } => apply_spawn(runtime, &entity, &player),
        OperatorRequest::Kick { player } => apply_kick(runtime, &player),
    }
}

fn open_database(path: &Path) -> Result<EngineDatabase, String> {
    EngineDatabase::open(path).map_err(|error| format!("database unavailable: {error}"))
}

fn resolve_player_id(runtime: &BridgeRuntime, name: &str, scope: &str) -> Result<u64, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("player name is required".into());
    }
    let online_first = scope != "offline";
    if online_first {
        if let Some(player_id) = runtime
            .config
            .shared_world
            .online_player_id_by_name(trimmed)
            .map_err(|error| error.to_string())?
        {
            return Ok(player_id);
        }
        if scope == "online" {
            return Err(format!("player `{trimmed}` is not online"));
        }
    }
    let database = open_database(&runtime.config.database_path)?;
    database
        .player_id_by_name(trimmed)
        .map_err(|error| format!("lookup failed: {error}"))?
        .ok_or_else(|| format!("player `{trimmed}` does not exist"))
}

fn apply_gm(runtime: &BridgeRuntime, player: &str, scope: &str, level: u8) -> OperatorResponse {
    if level > 3 {
        return OperatorResponse::failure("gm levels run 0-3");
    }
    let player_id = match resolve_player_id(runtime, player, scope) {
        Ok(id) => id,
        Err(error) => return OperatorResponse::failure(error),
    };
    let database = match open_database(&runtime.config.database_path) {
        Ok(database) => database,
        Err(error) => return OperatorResponse::failure(error),
    };
    if let Err(error) = database.update_player_gm_level(player_id, level) {
        return OperatorResponse::failure(format!("gm update failed: {error}"));
    }
    if let Err(error) = runtime
        .config
        .shared_world
        .update_player_gm_level(player_id, level)
    {
        return OperatorResponse::failure(format!("live update failed: {error}"));
    }
    OperatorResponse::success("gm", Some(format!("player-id={player_id} level={level}")))
}

fn apply_give(
    runtime: &BridgeRuntime,
    player: &str,
    scope: &str,
    item_id: u16,
    count: u16,
) -> OperatorResponse {
    if item_id == 0 {
        return OperatorResponse::failure("item id must be nonzero");
    }
    if count == 0 {
        return OperatorResponse::failure("count must be positive");
    }
    let player_id = match resolve_player_id(runtime, player, scope) {
        Ok(id) => id,
        Err(error) => return OperatorResponse::failure(error),
    };
    let is_online = runtime
        .config
        .shared_world
        .has_player(player_id)
        .unwrap_or(false);
    let mut database = match open_database(&runtime.config.database_path) {
        Ok(database) => database,
        Err(error) => return OperatorResponse::failure(error),
    };
    let containers = if is_online {
        match runtime.config.shared_world.player_containers(player_id) {
            Ok(containers) => containers,
            Err(error) => {
                return OperatorResponse::failure(format!("containers read failed: {error}"))
            }
        }
    } else {
        match database.player_containers(player_id) {
            Ok(containers) => containers,
            Err(error) => {
                return OperatorResponse::failure(format!("containers read failed: {error}"))
            }
        }
    };
    let (staged, unplaced) =
        crate::insert_units_into_containers(containers.clone(), item_id, u64::from(count));
    if unplaced > 0 {
        return OperatorResponse::failure(format!(
            "target has no container space for {unplaced} of {count}"
        ));
    }
    if let Err(error) = database.replace_player_containers(player_id, &staged) {
        return OperatorResponse::failure(format!("persistence failed: {error}"));
    }
    if is_online {
        if let Err(error) = runtime
            .config
            .shared_world
            .replace_player_containers(player_id, staged)
        {
            return OperatorResponse::failure(format!("live update failed: {error}"));
        }
        runtime
            .config
            .shared_world
            .vitals_epoch
            .fetch_add(1, Ordering::SeqCst);
    }
    let _ = &mut database;
    OperatorResponse::success(
        "give",
        Some(format!(
            "item={item_id} count={count} player-id={player_id}"
        )),
    )
}

fn apply_teleport(runtime: &BridgeRuntime, from: &str, to: &str, scope: &str) -> OperatorResponse {
    let from_id = match resolve_player_id(runtime, from, scope) {
        Ok(id) => id,
        Err(error) => return OperatorResponse::failure(error),
    };
    let to_id = match resolve_player_id(runtime, to, scope) {
        Ok(id) => id,
        Err(error) => return OperatorResponse::failure(error),
    };
    if from_id == to_id {
        return OperatorResponse::failure("cannot teleport a player to themself");
    }
    let destination = if runtime
        .config
        .shared_world
        .has_player(to_id)
        .unwrap_or(false)
    {
        match runtime.config.shared_world.player_position(to_id) {
            Ok(position) => position,
            Err(error) => {
                return OperatorResponse::failure(format!("position read failed: {error}"))
            }
        }
    } else {
        let database = match open_database(&runtime.config.database_path) {
            Ok(database) => database,
            Err(error) => return OperatorResponse::failure(error),
        };
        match database.player_by_id(to_id) {
            Ok(player) => player.position,
            Err(error) => return OperatorResponse::failure(format!("lookup failed: {error}")),
        }
    };
    if !runtime
        .config
        .shared_world
        .has_player(from_id)
        .unwrap_or(false)
    {
        // Offline mover: persist the new spawn tile.
        let database = match open_database(&runtime.config.database_path) {
            Ok(database) => database,
            Err(error) => return OperatorResponse::failure(error),
        };
        if let Err(error) = database.update_player_position(from_id, destination) {
            return OperatorResponse::failure(format!("persist failed: {error}"));
        }
        return OperatorResponse::success(
            "tp",
            Some(format!(
                "player-id={from_id} x={} y={} z={} (offline)",
                destination.x, destination.y, destination.z
            )),
        );
    }
    match runtime
        .config
        .shared_world
        .teleport_player_for_operator(from_id, destination)
    {
        Ok(()) => OperatorResponse::success(
            "tp",
            Some(format!(
                "player-id={from_id} x={} y={} z={}",
                destination.x, destination.y, destination.z
            )),
        ),
        Err(error) => OperatorResponse::failure(format!("teleport failed: {error}")),
    }
}

fn apply_spawn(runtime: &BridgeRuntime, entity: &str, player: &str) -> OperatorResponse {
    if entity.trim().is_empty() {
        return OperatorResponse::failure("entity name is required");
    }
    let anchor = if player.trim().is_empty() {
        match runtime.config.shared_world.any_online_player_anchor() {
            Ok(anchor) => anchor,
            Err(error) => {
                return OperatorResponse::failure(format!("anchor lookup failed: {error}"))
            }
        }
    } else {
        let player_id = match resolve_player_id(runtime, player, "online") {
            Ok(id) => id,
            Err(error) => return OperatorResponse::failure(error),
        };
        match runtime
            .config
            .shared_world
            .player_position_and_facing(player_id)
        {
            Ok(anchor) => anchor,
            Err(error) => {
                return OperatorResponse::failure(format!("anchor lookup failed: {error}"))
            }
        }
    };
    let Some((position, direction_byte)) = anchor else {
        return OperatorResponse::failure("no connected player to anchor the summon");
    };
    match runtime.config.shared_world.spawn_dynamic_entity_in_front(
        entity.trim(),
        position,
        direction_byte,
    ) {
        Ok(spawned_id) => OperatorResponse::success(
            "spawn",
            Some(format!("creature-id={spawned_id} entity={}", entity.trim())),
        ),
        Err(error) => OperatorResponse::failure(format!("spawn failed: {error}")),
    }
}

fn apply_kick(runtime: &BridgeRuntime, player: &str) -> OperatorResponse {
    let player_id = match resolve_player_id(runtime, player, "online") {
        Ok(id) => id,
        Err(error) => return OperatorResponse::failure(error),
    };
    match runtime.config.shared_world.request_kick(player_id) {
        Ok(true) => OperatorResponse::success("kick", Some(format!("player-id={player_id}"))),
        Ok(false) => OperatorResponse::failure(format!("player `{}` is not online", player.trim())),
        Err(error) => OperatorResponse::failure(format!("kick failed: {error}")),
    }
}

fn collect_status(config: &OperatorBridgeConfig) -> Result<String, String> {
    let online = config
        .shared_world
        .online_players_counter()
        .load(Ordering::SeqCst);
    let revision = config
        .shared_world
        .world_revision()
        .map_err(|error| error.to_string())?;
    Ok(format!("players-online={online} world-revision={revision}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_deserialization_matches_wire_shapes() {
        let broadcast: OperatorRequest =
            serde_json::from_str(r#"{"op":"broadcast","message":"hello world"}"#).unwrap();
        assert_eq!(
            broadcast,
            OperatorRequest::Broadcast {
                message: "hello world".into()
            }
        );
        let gm: OperatorRequest =
            serde_json::from_str(r#"{"op":"gm","player":"Bob","scope":"offline","level":2}"#)
                .unwrap();
        assert_eq!(
            gm,
            OperatorRequest::Gm {
                player: "Bob".into(),
                scope: "offline".into(),
                level: 2,
            }
        );
        let status: OperatorRequest = serde_json::from_str(r#"{"op":"status"}"#).unwrap();
        assert_eq!(status, OperatorRequest::Status);
        let give: OperatorRequest =
            serde_json::from_str(r#"{"op":"give","player":"Bob","item_id":3031,"count":10}"#)
                .unwrap();
        assert_eq!(
            give,
            OperatorRequest::Give {
                player: "Bob".into(),
                scope: "".into(),
                item_id: 3031,
                count: 10,
            }
        );
    }

    #[test]
    fn response_serialization_omits_empty_fields() {
        let success = OperatorResponse::success("status", None);
        let encoded = serde_json::to_string(&success).unwrap();
        assert_eq!(encoded, r#"{"ok":true,"handled":"status"}"#);
        let failure = OperatorResponse::failure("nope");
        let encoded = serde_json::to_string(&failure).unwrap();
        assert_eq!(encoded, r#"{"ok":false,"error":"nope"}"#);
    }
}
