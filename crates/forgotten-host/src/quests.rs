//! Session quest-log delivery: the empty/parser-shaped log and per-quest mission lines
//! resolved through the operator quest catalog against persisted started/completed state.
//! Delivery is read-only; quest completion itself lives in the shop/quest runtime.

use super::*;

/// Delivers the quest log: started persisted quests resolved through catalog display names,
/// or the parser-shaped empty response without a catalog or entries. Never fails the session.
pub(crate) fn apply_native_request_quest_log_action(
    ctx: &mut SessionContext<'_>,
) -> Result<(), HostError> {
    // With an operator quest catalog, only started persisted quests appear, resolved
    // through catalog display names; without one the parser-shaped empty response
    // keeps prior behavior.
    let quest_entries = match ctx.config.quest_catalog.as_deref() {
        Some(catalog) if !catalog.is_empty() => {
            let mut entries = Vec::new();
            for (quest_id, completed) in ctx
                .database
                .player_quests(ctx.character_id)
                .map_err(HostError::Persistence)?
            {
                if let Some(definition) = catalog.get(quest_id) {
                    let _ = completed;
                    entries.push((quest_id, definition.name.clone()));
                }
            }
            entries
        }
        _ => Vec::new(),
    };
    let quest_log = if quest_entries.is_empty() {
        encode_native_otclient_empty_quest_log(&ctx.config.client_profile)
            .map_err(HostError::Protocol)?
    } else {
        encode_native_otclient_quest_list(&ctx.config.client_profile, &quest_entries)
            .map_err(HostError::Protocol)?
    };
    write_frame(&mut *ctx.stream, &quest_log)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "outbound=quest-log opcode=0xf0 entries={} bytes={}",
            quest_entries.len(),
            quest_log.0.len()
        ),
    );
    Ok(())
}

/// Delivers one quest's mission lines for started persisted quests with declared missions;
/// unknown or not-started quests receive an empty mission list. Never fails the session.
pub(crate) fn apply_native_request_quest_line_action(
    ctx: &mut SessionContext<'_>,
    quest_id: u16,
) -> Result<(), HostError> {
    // The quest line window opens for started persisted quests with declared
    // missions; unknown or not-started quests receive an empty mission list.
    let missions = match ctx.config.quest_catalog.as_deref() {
        Some(catalog) => {
            let started = ctx
                .database
                .player_quests(ctx.character_id)
                .map_err(HostError::Persistence)?
                .iter()
                .any(|(started_id, _)| *started_id == quest_id);
            match catalog.get(quest_id).filter(|_| started) {
                Some(definition) => definition.missions.clone(),
                None => Vec::new(),
            }
        }
        None => Vec::new(),
    };
    let line_frame =
        encode_native_otclient_quest_line(&ctx.config.client_profile, quest_id, &missions)
            .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &line_frame)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "outbound=quest-line opcode=0xf1 quest-id={quest_id} missions={} bytes={}",
            missions.len(),
            line_frame.0.len()
        ),
    );
    Ok(())
}
