//! Classic native player-to-player trade flow: request validation and window delivery,
//! offer-item resolution, accept/cancel handling. Sessions live in the core world state;
//! this module owns the socket-facing sequence around them.

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpStream};

use forgotten_core::ItemInstance;

use super::npc_shop::{deliver_native_player_goods, handle_native_shop_keyword};
use super::{
    encode_native_otclient_counter_trade, encode_native_otclient_failure_message,
    encode_native_otclient_own_trade, encode_native_otclient_status_message, native_diagnostic,
    native_player_id_to_character_id, write_frame, DeclarativeShopCatalog, EngineDatabase,
    HostError, NativeItemPresentationCatalog, NativeOtClientPosition, NativeOtClientProfile,
    NativeOtClientTradeItem, SessionContext, SharedNativeWorld,
};
/// Handles a classic request-trade action: resolves the offered item from the sender's own
/// authoritative inventory, validates the target is an adjacent live player, opens the trade
/// session, and delivers both trade windows.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_native_player_trade_request(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    shared_world: &SharedNativeWorld,
    player_id: u64,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
    target_creature_id: u32,
    item_presentation_catalog: Option<&NativeItemPresentationCatalog>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
) -> Result<(), HostError> {
    // Resolve the target creature id to a connected character.
    let Some(target_character_id) = native_player_id_to_character_id(target_creature_id) else {
        let failure = encode_native_otclient_failure_message(
            profile,
            "Sorry, not possible. You are not close enough to trade.",
        )
        .map_err(HostError::Protocol)?;
        write_frame(stream, &failure)?;
        return Ok(());
    };
    let world = shared_world.lock()?;
    let Some(target_player) = world.player(target_character_id) else {
        drop(world);
        let failure = encode_native_otclient_failure_message(
            profile,
            "Sorry, not possible. You are not close enough to trade.",
        )
        .map_err(HostError::Protocol)?;
        write_frame(stream, &failure)?;
        return Ok(());
    };
    // Adjacency: same floor, within one tile (classic trade reach).
    let sender_position = world
        .player(player_id)
        .map(|player| player.position)
        .ok_or(forgotten_core::CoreError::UnknownPlayer(player_id))
        .map_err(HostError::Core)?;
    if target_player.position.z != sender_position.z
        || target_player.position.x.abs_diff(sender_position.x) > 1
        || target_player.position.y.abs_diff(sender_position.y) > 1
    {
        drop(world);
        let failure = encode_native_otclient_failure_message(
            profile,
            "Sorry, not possible. You are too far away to trade.",
        )
        .map_err(HostError::Protocol)?;
        write_frame(stream, &failure)?;
        return Ok(());
    }
    drop(world);

    // Resolve the offered item from the sender's own inventory: equipment slots use the
    // classic 0xFFFF position form; container windows map through open views. FE trusts its
    // own inventory state, never the client's echoed thing id.
    let resolved = resolve_native_trade_offer_item(
        shared_world,
        player_id,
        position,
        client_thing_id,
        stack_position,
    )?;
    let Some((container_id, item_index, _item)) = resolved else {
        let failure =
            encode_native_otclient_failure_message(profile, "You cannot trade that item.")
                .map_err(HostError::Protocol)?;
        write_frame(stream, &failure)?;
        return Ok(());
    };

    // Open the session (or reuse the live one when the counterparty re-offers).
    let existing = shared_world.lock()?.player_trade(player_id).cloned();
    match existing {
        None => {
            shared_world
                .lock()?
                .open_player_trade(player_id, target_character_id)
                .map_err(HostError::Core)?;
        }
        Some(session) => {
            if (session.initiator == player_id && session.counterparty != target_character_id)
                || (session.counterparty == player_id && session.initiator != target_character_id)
            {
                let failure =
                    encode_native_otclient_failure_message(profile, "You are already trading.")
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &failure)?;
                return Ok(());
            }
        }
    }
    shared_world
        .lock()?
        .stage_trade_item(
            player_id,
            forgotten_core::TradeItemReference {
                container_id,
                item_index,
            },
        )
        .map_err(HostError::Core)?;

    // Deliver the window records with current staged offers.
    deliver_native_trade_windows(
        stream,
        profile,
        shared_world,
        player_id,
        item_presentation_catalog,
        stackable_item_server_ids,
    )?;
    Ok(())
}

/// Resolves one trade offer reference from the sender's own authoritative inventory. Supports
/// the classic own-equipment position form and own-container windows; returns the container id,
/// item index, and item snapshot. Map positions are rejected for trade staging.
pub(crate) fn resolve_native_trade_offer_item(
    shared_world: &SharedNativeWorld,
    player_id: u64,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
) -> Result<Option<(u8, usize, ItemInstance)>, HostError> {
    let containers = shared_world.player_containers(player_id)?;
    if position.x == 0xffff {
        // Own-container window form: y bit 0x40 flags content windows per the classic layout.
        if position.y & 0x40 != 0 {
            let container_id = (position.y & 0x0f) as u8;
            let index = usize::from(position.z);
            if let Some(item) = containers
                .container(container_id)
                .and_then(|container| container.items.item(index))
                .cloned()
            {
                return Ok(Some((container_id, index, item)));
            }
            return Ok(None);
        }
        // Equipment slot form has no single container home; trades stage from containers only
        // in this slice, so equipment staging is deferred rather than guessed.
        return Ok(None);
    }
    // A client thing id over a map position is not owned inventory; ignore it.
    let _ = (client_thing_id, stack_position);
    Ok(None)
}

/// Delivers the own/counter trade window records for one side of a live trade using the
/// session's staged references mapped into client-visible item records.
pub(crate) fn deliver_native_trade_windows(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    shared_world: &SharedNativeWorld,
    viewer_id: u64,
    item_presentation_catalog: Option<&NativeItemPresentationCatalog>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
) -> Result<(), HostError> {
    let Some(session) = shared_world.lock()?.player_trade(viewer_id).cloned() else {
        return Ok(());
    };
    let snapshot_items = |owner: u64,
                          refs: &[forgotten_core::TradeItemReference]|
     -> Result<Vec<NativeOtClientTradeItem>, HostError> {
        let containers = shared_world.player_containers(owner)?;
        Ok(refs
            .iter()
            .filter_map(|reference| {
                let item = containers
                    .container(reference.container_id)?
                    .items
                    .item(reference.item_index)?;
                let client_thing_id = item_presentation_catalog
                    .and_then(|catalog| catalog.presentation(item.server_id))
                    .map(|presentation| presentation.client_thing_id)?;
                let count =
                    if stackable_item_server_ids.is_some_and(|ids| ids.contains(&item.server_id)) {
                        Some(u8::try_from(item.count).unwrap_or(u8::MAX))
                    } else {
                        None
                    };
                Some(NativeOtClientTradeItem {
                    client_thing_id,
                    count,
                })
            })
            .collect())
    };
    let (counterparty_id, counterparty_name, own_items, their_items) =
        if session.initiator == viewer_id {
            let name = shared_world
                .lock()?
                .player(session.counterparty)
                .map(|player| player.name.clone())
                .unwrap_or_default();
            (
                session.counterparty,
                name,
                snapshot_items(session.initiator, &session.initiator_items)?,
                snapshot_items(session.counterparty, &session.counterparty_items)?,
            )
        } else {
            let name = shared_world
                .lock()?
                .player(session.initiator)
                .map(|player| player.name.clone())
                .unwrap_or_default();
            (
                session.initiator,
                name,
                snapshot_items(session.counterparty, &session.counterparty_items)?,
                snapshot_items(session.initiator, &session.initiator_items)?,
            )
        };
    let _ = counterparty_id;
    let own_record = encode_native_otclient_own_trade(profile, &counterparty_name, &own_items)
        .map_err(HostError::Protocol)?;
    write_frame(stream, &own_record)?;
    let counter_record =
        encode_native_otclient_counter_trade(profile, &counterparty_name, &their_items)
            .map_err(HostError::Protocol)?;
    write_frame(stream, &counter_record)?;
    Ok(())
}

/// Handles one side's accept: flips acceptance, delivers refreshed windows, and executes +
/// persists the swap once both sides accepted. Both clients receive the closed-window signal.
pub(crate) fn handle_native_trade_accept(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
    player_id: u64,
    item_presentation_catalog: Option<&NativeItemPresentationCatalog>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
) -> Result<(), HostError> {
    let both_accepted = shared_world
        .lock()?
        .accept_player_trade(player_id)
        .map_err(HostError::Core)?;
    deliver_native_trade_windows(
        stream,
        profile,
        shared_world,
        player_id,
        item_presentation_catalog,
        stackable_item_server_ids,
    )?;
    if !both_accepted {
        return Ok(());
    }
    let execution = shared_world
        .lock()?
        .execute_player_trade(player_id)
        .map_err(HostError::Core)?;
    // Persist both inventories atomically enough for this slice: each replace is one SQLite
    // transaction; a failure on the second leaves the first persisted (logged loudly).
    let initiator_containers = shared_world.player_containers(execution.initiator)?;
    if let Err(error) =
        database.replace_player_containers(execution.initiator, &initiator_containers)
    {
        eprintln!(
            "> trade persistence failed for player {}: {error}",
            execution.initiator
        );
    }
    let counterparty_containers = shared_world.player_containers(execution.counterparty)?;
    if let Err(error) =
        database.replace_player_containers(execution.counterparty, &counterparty_containers)
    {
        eprintln!(
            "> trade persistence failed for player {}: {error}",
            execution.counterparty
        );
    }
    shared_world.signal_trade_closed(execution.initiator, execution.counterparty)?;
    shared_world.mark_visibility_changed();
    Ok(())
}
/// Handles one side's reject/cancel: closes the session and signals the other side's window.
pub(crate) fn handle_native_trade_reject(
    shared_world: &SharedNativeWorld,
    player_id: u64,
) -> Result<(), HostError> {
    shared_world.cancel_player_trade_and_signal(player_id)?;
    Ok(())
}

/// Applies one player-trade request: validates the offered item and target, then delivers the
/// trade window. Failures travel via `Err(HostError)`; the record is always consumed.
pub(crate) fn apply_native_request_trade_action(
    ctx: &mut SessionContext<'_>,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
    target_creature_id: u32,
) -> Result<(), HostError> {
    handle_native_player_trade_request(
        &mut *ctx.stream,
        &ctx.config.client_profile,
        ctx.shared_world,
        ctx.character_id,
        position,
        client_thing_id,
        stack_position,
        target_creature_id,
        ctx.config.item_presentation_catalog.as_deref(),
        ctx.config.stackable_item_server_ids.as_deref(),
    )?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        "action=request-trade outcome=processed",
    );
    Ok(())
}

/// Applies one trade acceptance: swaps the staged offers and persists both sides.
pub(crate) fn apply_native_accept_trade_action(
    ctx: &mut SessionContext<'_>,
) -> Result<(), HostError> {
    handle_native_trade_accept(
        &mut *ctx.stream,
        &ctx.config.client_profile,
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        ctx.config.item_presentation_catalog.as_deref(),
        ctx.config.stackable_item_server_ids.as_deref(),
    )?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        "action=accept-trade",
    );
    Ok(())
}

/// Applies one trade rejection: clears the caller's trade intent and signals the window shut.
/// Takes direct parameters (three session items) rather than the shared context, which would
/// be pure overhead for this narrow handler.
pub(crate) fn apply_native_reject_trade_action(
    shared_world: &SharedNativeWorld,
    character_id: u64,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    handle_native_trade_reject(shared_world, character_id)?;
    native_diagnostic(extended_diagnostics, peer, "action=reject-trade");
    Ok(())
}

/// Records one NPC trade-window closure. Purely diagnostic; no state changes.
pub(crate) fn apply_native_npc_trade_close_action(
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    native_diagnostic(extended_diagnostics, peer, "action=npc-trade-close");
    Ok(())
}

/// Re-sends the player-goods record (0x7B) after a completed shop trade so the open
/// trade window stops showing stale gold and stock. `None` (fallthrough or usage
/// error) sends nothing; a missing shop or presentation mapping skips silently.
fn refresh_player_goods_after_trade(
    ctx: &mut SessionContext<'_>,
    traded_npc_name: Option<String>,
) -> Result<(), HostError> {
    let Some(npc_name) = traded_npc_name else {
        return Ok(());
    };
    let Some(shop) = ctx
        .config
        .shop_catalog
        .as_deref()
        .and_then(|catalog| catalog.by_npc_name(&npc_name))
    else {
        return Ok(());
    };
    let _ = deliver_native_player_goods(
        &mut *ctx.stream,
        &ctx.config.client_profile,
        ctx.shared_world,
        &*ctx.database,
        ctx.character_id,
        shop,
        ctx.config.item_presentation_catalog.as_deref(),
    )?;
    Ok(())
}

/// Applies one NPC buy: maps the client thing id back to a server item, then routes through
/// the declarative shop keyword path. An unmapped id answers with a failure frame.
pub(crate) fn apply_native_npc_buy_action(
    ctx: &mut SessionContext<'_>,
    client_thing_id: u16,
    amount: u8,
) -> Result<(), HostError> {
    // Buy flows through the existing declarative shop keyword path by mapping the
    // client thing id back to a server item via the presentation catalog.
    let server_id = ctx
        .config
        .item_presentation_catalog
        .as_ref()
        .and_then(|catalog| catalog.unique_server_id_for_client_thing_id(client_thing_id));
    let Some(server_id) = server_id else {
        let failure = encode_native_otclient_failure_message(
            &ctx.config.client_profile,
            "You cannot buy this item.",
        )
        .map_err(HostError::Protocol)?;
        write_frame(&mut *ctx.stream, &failure)?;
        return Ok(());
    };
    let message = handle_native_shop_keyword(
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        &format!("buy {server_id} {amount}"),
        ctx.config
            .shop_catalog
            .as_deref()
            .unwrap_or(&DeclarativeShopCatalog::default()),
        ctx.config.item_weight_by_server_id.as_deref(),
    )?;
    let (reply, traded_npc_name) = match message {
        Some(outcome) => (outcome.reply, outcome.traded_npc_name),
        None => ("Nothing to buy here.".into(), None),
    };
    let reply_frame = encode_native_otclient_status_message(&ctx.config.client_profile, &reply)
        .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &reply_frame)?;
    refresh_player_goods_after_trade(ctx, traded_npc_name)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!("action=npc-buy item={server_id} amount={amount}"),
    );
    Ok(())
}

/// Applies one NPC sell: mirrors the buy path through the declarative shop keyword path.
pub(crate) fn apply_native_npc_sell_action(
    ctx: &mut SessionContext<'_>,
    client_thing_id: u16,
    amount: u8,
) -> Result<(), HostError> {
    let server_id = ctx
        .config
        .item_presentation_catalog
        .as_ref()
        .and_then(|catalog| catalog.unique_server_id_for_client_thing_id(client_thing_id));
    let Some(server_id) = server_id else {
        let failure = encode_native_otclient_failure_message(
            &ctx.config.client_profile,
            "You cannot sell this item.",
        )
        .map_err(HostError::Protocol)?;
        write_frame(&mut *ctx.stream, &failure)?;
        return Ok(());
    };
    let message = handle_native_shop_keyword(
        ctx.shared_world,
        &mut *ctx.database,
        ctx.character_id,
        &format!("sell {server_id} {amount}"),
        ctx.config
            .shop_catalog
            .as_deref()
            .unwrap_or(&DeclarativeShopCatalog::default()),
        ctx.config.item_weight_by_server_id.as_deref(),
    )?;
    let (reply, traded_npc_name) = match message {
        Some(outcome) => (outcome.reply, outcome.traded_npc_name),
        None => ("Nothing to sell here.".into(), None),
    };
    let reply_frame = encode_native_otclient_status_message(&ctx.config.client_profile, &reply)
        .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &reply_frame)?;
    refresh_player_goods_after_trade(ctx, traded_npc_name)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!("action=npc-sell item={server_id} amount={amount}"),
    );
    Ok(())
}
