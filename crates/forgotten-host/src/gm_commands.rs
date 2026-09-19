//! Classic GM talkaction handler: bounded operator verbs (/spawn, /give, /tp, /kick, /gm,
//! /broadcast, /heal, /playerinfo, /goto, /tome and friends) executed against the shared world
//! and durable state.

use forgotten_config::QuestCatalog;
use forgotten_persistence::PersistenceError;
use forgotten_protocol::NativeOtClientTalkRequest;

use super::{
    complete_native_player_quest, encode_native_otclient_status_message,
    encode_shared_native_world_viewport, give_items_to_player, native_diagnostic, native_position,
    native_static_creature_health_frames, write_frame, EngineDatabase, HostError,
    SessionActionOutcome, SessionContext, SharedNativeWorld, NATIVE_OTCLIENT_MESSAGE_SAY,
};

pub(crate) fn handle_native_gm_talkaction(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
    player_id: u64,
    message: &str,
    _gm_level: u8,
    quest_catalog: Option<&QuestCatalog>,
) -> Result<Option<String>, HostError> {
    let trimmed = message.trim();
    if !trimmed.starts_with('/') {
        return Ok(None);
    }
    let mut parts = trimmed[1..].split_whitespace();
    let verb = parts.next().unwrap_or("").to_ascii_lowercase();
    match verb.as_str() {
        "spawn" => {
            let entity = parts.next().unwrap_or("");
            if entity.is_empty() {
                return Ok(Some("Usage: /spawn <entity>".into()));
            }
            match shared_world.spawn_dynamic_entity_in_front_of_player(player_id, entity) {
                Ok(spawned_id) => Ok(Some(format!(
                    "Summoned {entity} ahead (creature {spawned_id})."
                ))),
                Err(HostError::Core(forgotten_core::CoreError::UnknownEntityName(_))) => {
                    Ok(Some(format!(
                        "Unknown entity `{entity}`; only imported creature names can be summoned."
                    )))
                }
                Err(_) => Ok(Some("The target tile is blocked.".into())),
            }
        }
        "give" => {
            let target_name = parts.next().unwrap_or("");
            let item_id: u16 = match parts.next().and_then(|value| value.parse().ok()) {
                Some(value) => value,
                None => return Ok(Some("Usage: /give <player> <item-id> [count]".into())),
            };
            let count: u16 = parts
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
            if item_id == 0 || count == 0 || count > 100 {
                return Ok(Some(
                    "Item id must be nonzero and count between 1 and 100.".into(),
                ));
            }
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            give_items_to_player(shared_world, database, target_id, item_id, u64::from(count))
        }
        "tp" => {
            let from_name = parts.next().unwrap_or("");
            let to_name = parts.next().unwrap_or("");
            if to_name.is_empty() {
                return Ok(Some(
                    "Usage: /tp <player> <player>   ('me' = yourself)".into(),
                ));
            }
            let resolve = |name: &str| -> Result<Option<u64>, HostError> {
                if name.eq_ignore_ascii_case("me") {
                    return Ok(Some(player_id));
                }
                database
                    .player_id_by_name(name)
                    .map_err(HostError::Persistence)
            };
            let Some(from_target) = resolve(from_name)? else {
                return Ok(Some(format!("Player `{from_name}` does not exist.")));
            };
            let Some(to_target) = resolve(to_name)? else {
                return Ok(Some(format!("Player `{to_name}` does not exist.")));
            };
            if !shared_world.has_player(from_target)? {
                return Ok(Some(format!("`{from_name}` must be online to teleport.")));
            }
            let destination = if shared_world.has_player(to_target)? {
                shared_world.player_position(to_target)?
            } else {
                database
                    .player_by_id(to_target)
                    .map_err(HostError::Persistence)?
                    .position
            };
            match shared_world.teleport_player_for_operator(from_target, destination) {
                Ok(()) => Ok(Some(format!(
                    "Teleported {} to {} {} {}.",
                    from_name, destination.x, destination.y, destination.z
                ))),
                Err(_) => Ok(Some("That destination is blocked.".into())),
            }
        }
        "god" | "invisible" => {
            // Toggles on the speaking GM only (classic semantics). Runtime-only: relogs reset.
            let enabling = !match verb.as_str() {
                "god" => shared_world
                    .lock()?
                    .player_is_in_god_mode(player_id),
                _ => shared_world.lock()?.player_is_invisible(player_id),
            };
            let applied = {
                let mut world = shared_world.lock()?;
                if verb == "god" {
                    world.set_player_god_mode(player_id, enabling)
                } else {
                    world.set_player_invisible(player_id, enabling)
                }
                .map_err(HostError::Core)?
            };
            Ok(Some(format!(
                "You are now {}{}.",
                if applied { "" } else { "no longer " },
                if verb == "god" { "invulnerable (god mode)" } else { "invisible" }
            )))
        }
        "item" => {
            // /item <id> [count]: quick self-delivery. For giving to other players use /give.
            let item_id: u16 = match parts.next().and_then(|value| value.parse().ok()) {
                Some(value) => value,
                None => return Ok(Some("Usage: /item <item-id> [count]".into())),
            };
            let count: u16 = parts
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
            if item_id == 0 || count == 0 || count > 100 {
                return Ok(Some(
                    "Item id must be nonzero and count between 1 and 100.".into(),
                ));
            }
            give_items_to_player(shared_world, database, player_id, item_id, u64::from(count))
        }
        "freeze" | "unfreeze" => {
            let freezing = verb == "freeze";
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            database
                .set_player_frozen(target_id, freezing)
                .map_err(HostError::Persistence)?;
            Ok(Some(if freezing {
                format!("Froze {target_name} in place.")
            } else {
                format!("Unfroze {target_name}.")
            }))
        }
        "down" | "up" => {
            let current = shared_world.player_position(player_id)?;
            let destination_z = if verb == "down" {
                current.z.saturating_add(1).min(15)
            } else {
                current.z.saturating_sub(1)
            };
            let destination = forgotten_core::Position {
                x: current.x,
                y: current.y,
                z: destination_z,
            };
            match shared_world.teleport_player_for_operator(player_id, destination) {
                Ok(()) => Ok(Some(format!(
                    "Moved {} to floor {}.",
                    if verb == "down" { "down" } else { "up" },
                    destination.z
                ))),
                Err(_) => Ok(Some("That level is blocked here.".into())),
            }
        }
        "completequest" => {
            // /completequest <player> <quest-id>: flips the persisted completion flag and
            // grants the catalog-declared rewards into the player's starter backpack.
            let target_name = parts.next().unwrap_or("");
            let quest_id: u16 = match parts.next().and_then(|value| value.parse().ok()) {
                Some(value) => value,
                None => {
                    return Ok(Some(
                        "Usage: /completequest <player> <quest-id>".into(),
                    ))
                }
            };
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            match complete_native_player_quest(
                shared_world,
                database,
                target_id,
                quest_id,
                quest_catalog,
            )? {
                Some(rewards) => {
                    let reward_text = rewards
                        .iter()
                        .map(|(item_id, count)| format!("{count}x item {item_id}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    Ok(Some(format!(
                        "{target_name} completed quest {quest_id}; granted {reward_text}."
                    )))
                }
                None => Ok(Some(format!(
                    "`{target_name}` is not online or quest {quest_id} is unknown/already completed."
                ))),
            }
        }
        "kick" => {
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            match shared_world.request_kick(target_id) {
                Ok(true) => Ok(Some(format!("Kicked {target_name}."))),
                Ok(false) => Ok(Some(format!("`{target_name}` is not online."))),
                Err(_) => Ok(Some("Kick failed; try again.".into())),
            }
        }
        "ban" => {
            // /ban <player> [days] [reason words...]: days 0 (default) means permanent.
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            let account_id = database
                .account_id_by_player_id(target_id)
                .map_err(HostError::Persistence)?
                .ok_or(HostError::Persistence(PersistenceError::UnknownPlayer(target_id)))?;
            // Days are optional; a non-numeric word starts the reason so operators can write
            // `/ban Name Botting in depot` without quoting.
            let rest: Vec<String> = parts.map(str::to_owned).collect();
            let (days, reason) = match rest.first().and_then(|value| value.parse::<u64>().ok()) {
                Some(days) => (days, rest[1..].join(" ")),
                None => (0_u64, rest.join(" ")),
            };
            let reason = if reason.is_empty() {
                "Unspecified misconduct".to_owned()
            } else {
                reason
            };
            database
                .record_account_ban(
                    account_id,
                    &reason,
                    (days > 0).then(|| days.saturating_mul(86_400)),
                )
                .map_err(HostError::Persistence)?;
            let _ = shared_world.request_kick(target_id);
            Ok(Some(if days > 0 {
                format!("Banned {target_name} for {days} day(s): {reason}")
            } else {
                format!("Permanently banned {target_name}: {reason}")
            }))
        }
        "unban" => {
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            let account_id = database
                .account_id_by_player_id(target_id)
                .map_err(HostError::Persistence)?
                .ok_or(HostError::Persistence(PersistenceError::UnknownPlayer(target_id)))?;
            let removed = database
                .clear_account_bans(u64::from(account_id))
                .map_err(HostError::Persistence)?;
            Ok(Some(format!(
                "Lifted {removed} ban(s) from {target_name}."
            )))
        }
        "mute" => {
            // /mute <player> [minutes]: default 5 minutes, bounded to 30 days.
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            let account_id = database
                .account_id_by_player_id(target_id)
                .map_err(HostError::Persistence)?
                .ok_or(HostError::Persistence(PersistenceError::UnknownPlayer(target_id)))?;
            let minutes: u64 = parts
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(5);
            database
                .record_account_mute(account_id, minutes.saturating_mul(60))
                .map_err(HostError::Persistence)?;
            Ok(Some(format!("Muted {target_name} for {minutes} minute(s).")))
        }
        "unmute" => {
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            let account_id = database
                .account_id_by_player_id(target_id)
                .map_err(HostError::Persistence)?
                .ok_or(HostError::Persistence(PersistenceError::UnknownPlayer(target_id)))?;
            let _ = database.account_mute_remaining_seconds(u64::from(account_id))?; // prunes lapsed rows
            let cleared = database
                .clear_account_mute(u64::from(account_id))
                .map_err(HostError::Persistence)?;
            Ok(Some(if cleared == 1 {
                format!("Unmuted {target_name}.")
            } else {
                format!("{target_name} was not muted.")
            }))
        }
        "gm" => {
            // /gm <online|offline> <player> [level] promotes another character.
            let scope = parts.next().unwrap_or("").to_ascii_lowercase();
            let target_name = parts.next().unwrap_or("");
            let level: u8 = parts
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
            if scope != "online" && scope != "offline" {
                return Ok(Some(
                    "Usage: /gm <online|offline> <player> [level 0-3]".into(),
                ));
            }
            if level > 3 {
                return Ok(Some("GM levels run 0-3.".into()));
            }
            let Some(target_id) = database
                .player_id_by_name(target_name)
                .map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            if scope == "online" && !shared_world.has_player(target_id)? {
                return Ok(Some(format!("`{target_name}` is not online.")));
            }
            database
                .update_player_gm_level(target_id, level)
                .map_err(HostError::Persistence)?;
            Ok(Some(format!(
                "Set gamemaster level {level} for {target_name}."
            )))
        }
        "broadcast" => {
            let body: String = parts.collect::<Vec<_>>().join(" ");
            if body.is_empty() {
                return Ok(Some("Usage: /broadcast <message>".into()));
            }
            match shared_world.broadcast_console_message("Console", &body) {
                Ok(delivered) => Ok(Some(format!("Broadcast queued for {delivered} players."))),
                Err(_) => Ok(Some("Broadcast failed; try again.".into())),
            }
        }
        "heal" => match shared_world.restore_player_vitals(player_id) {
            Ok(true) => Ok(Some("You feel better. Vitals restored.".into())),
            _ => Ok(Some("Healing failed; target is not online.".into())),
        },
        "playerinfo" => {
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) =
                database.player_id_by_name(target_name).map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            let character =
                database.player_by_id(target_id).map_err(HostError::Persistence)?;
            let gm_tier = database.player_gm_level(target_id).map_err(HostError::Persistence)?;
            let online_state = if shared_world.has_player(target_id)? {
                "online"
            } else {
                "offline"
            };
            let frags = shared_world.lock()?.player_frag_count(target_id);
            let skull = if shared_world.lock()?.player_has_white_skull(target_id) {
                "white-skull"
            } else {
                "none"
            };
            Ok(Some(format!(
                "{}: level {} pos {},{},{} {} gm-tier {gm_tier} frags {frags} skull {skull}",
                character.name,
                character.level,
                character.position.x,
                character.position.y,
                character.position.z,
                online_state,
            )))
        }
        "goto" => {
            // /goto <player> teleports the executing GM to the target's position.
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) =
                database.player_id_by_name(target_name).map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            let destination = if shared_world.has_player(target_id)? {
                shared_world.player_position(target_id)?
            } else {
                database
                    .player_by_id(target_id)
                    .map_err(HostError::Persistence)?
                    .position
            };
            match shared_world.teleport_player_for_operator(player_id, destination) {
                Ok(()) => Ok(Some(format!(
                    "Teleported to {} at {},{},{}.",
                    target_name, destination.x, destination.y, destination.z
                ))),
                Err(_) => Ok(Some("Destination blocked.".into())),
            }
        }
        "summon" | "tome" => {
            // /tome <player> pulls the target player to the GM's position.
            let target_name = parts.next().unwrap_or("");
            let Some(target_id) =
                database.player_id_by_name(target_name).map_err(HostError::Persistence)?
            else {
                return Ok(Some(format!("Player `{target_name}` does not exist.")));
            };
            let destination = shared_world.player_position(player_id)?;
            if !shared_world.has_player(target_id)? {
                return Ok(Some(format!("`{target_name}` must be online to be summoned.")));
            }
            match shared_world.teleport_player_for_operator(target_id, destination) {
                Ok(()) => Ok(Some(format!("Summoned {target_name} to you."))),
                Err(_) => Ok(Some("Your tile area is blocked.".into())),
            }
        }
        _ => Ok(Some(format!(
            "Unknown GM command `/{verb}`; available: spawn, give, tp, kick, gm, broadcast, heal, playerinfo, goto, tome."
        ))),
    }
}

/// Applies one gamemaster talkaction for a Say record. Only characters with a persisted GM tier
/// may trigger verbs; a handled verb replies, resends the full viewport from live shared state
/// (so summons stay visible without relog), and consumes the record. Returns
/// `SessionActionOutcome::Handled` on a handled verb, `Unhandled` for non-Say records,
/// non-GM speakers, or unrecognized verbs.
pub(crate) fn apply_native_gm_talkaction_talk(
    ctx: &mut SessionContext<'_>,
    request: &NativeOtClientTalkRequest,
) -> Result<SessionActionOutcome, HostError> {
    if !(request.mode == NATIVE_OTCLIENT_MESSAGE_SAY
        && request.channel_id.is_none()
        && request.recipient.is_none())
    {
        return Ok(SessionActionOutcome::Unhandled);
    }
    let gm_level = ctx
        .database
        .player_gm_level(ctx.character_id)
        .map_err(HostError::Persistence)?;
    if gm_level == 0 {
        return Ok(SessionActionOutcome::Unhandled);
    }
    let Some(reply) = handle_native_gm_talkaction(
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        &request.message,
        gm_level,
        ctx.config.quest_catalog.as_deref(),
    )?
    else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let reply_frame = encode_native_otclient_status_message(&ctx.config.client_profile, &reply)
        .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &reply_frame)?;
    // GM talkactions mutate authoritative state (summons, teleports,
    // deliveries), so this session resends its full viewport from live
    // shared state instead of silently adopting the bumped visibility
    // epoch — that swallow left summons invisible until relog
    // (live-test regression A1). Other sessions refresh through their
    // own epoch comparison.
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
    let refreshed_static_health_frames =
        native_static_creature_health_frames(&ctx.config.client_profile, &refreshed_static_spawns)?;
    write_frame(&mut *ctx.stream, &refreshed_viewport)?;
    for frame in &refreshed_static_health_frames {
        write_frame(&mut *ctx.stream, frame)?;
    }
    *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "action=talk outcome=gm-talkaction reply-bytes={}",
            reply.len()
        ),
    );
    Ok(SessionActionOutcome::Handled)
}
