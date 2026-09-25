//! Chat and VIP presence fan-out on the shared world: public/whisper/yell/ranged broadcast,
//! configured-channel delivery, private messages, console broadcast, and VIP presence
//! events. Recipients are registered per connected session and receive bounded copies.

use super::*;
use forgotten_protocol::NativeOtClientTalkRequest;

/// Outcome of one authenticated private-chat delivery. `NotOnline` means no matching online
/// session owns that player name; callers surface the classic "not online" ack to the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateChatDelivery {
    Delivered,
    NotOnline,
}

impl SharedNativeWorld {
    pub(crate) fn register_public_chat_recipient(
        &self,
        player_id: u64,
        player_name: &str,
    ) -> Result<mpsc::Receiver<SharedPublicChatEvent>, HostError> {
        if player_name.is_empty() {
            return Err(HostError::InvalidConfiguration(
                "shared chat recipient name must not be empty".into(),
            ));
        }
        let (sender, receiver) = mpsc::sync_channel(NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY);
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        if recipients
            .values()
            .any(|recipient| recipient.player_name == player_name)
        {
            return Err(HostError::InvalidConfiguration(
                "shared chat recipient already registered for player name".into(),
            ));
        }
        if recipients.contains_key(&player_id) {
            return Err(HostError::InvalidConfiguration(
                "shared chat recipient already registered for player".into(),
            ));
        }
        recipients.insert(
            player_id,
            SharedChatRecipient {
                player_name: player_name.to_string(),
                sender,
            },
        );
        Ok(receiver)
    }

    pub(crate) fn unregister_public_chat_recipient(&self, player_id: u64) {
        if let Ok(mut recipients) = self.chat_recipients.lock() {
            recipients.remove(&player_id);
        }
    }

    /// Registers one active session to receive only presence changes for the persisted VIP target
    /// IDs delivered to that same session. The queue is deliberately bounded and nonblocking so a
    /// slow client cannot delay another player's lifecycle transition.
    pub(crate) fn register_vip_presence_recipient(
        &self,
        player_id: u64,
        watched_player_ids: BTreeSet<u32>,
    ) -> Result<mpsc::Receiver<SharedVipPresenceEvent>, HostError> {
        let (sender, receiver) = mpsc::sync_channel(NATIVE_OTCLIENT_SHARED_VIP_QUEUE_CAPACITY);
        let mut recipients = self
            .vip_presence_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        if recipients.contains_key(&player_id) {
            return Err(HostError::InvalidConfiguration(
                "VIP presence recipient already registered for player".into(),
            ));
        }
        recipients.insert(
            player_id,
            SharedVipPresenceRecipient {
                watched_player_ids,
                sender,
            },
        );
        Ok(receiver)
    }

    pub(crate) fn unregister_vip_presence_recipient(&self, player_id: u64) {
        if let Ok(mut recipients) = self.vip_presence_recipients.lock() {
            recipients.remove(&player_id);
        }
    }

    /// Fans one active player's classic-compatible presence change only to active sessions whose
    /// bounded persisted VIP list includes that exact target. There is no notification text,
    /// privacy policy, or persisted presence state in this delivery primitive.
    pub(crate) fn publish_vip_presence(
        &self,
        target_player_id: u64,
        online: bool,
    ) -> Result<usize, HostError> {
        let target_player_id = u32::try_from(target_player_id).map_err(|_| {
            HostError::InvalidConfiguration("VIP presence target does not fit classic ID".into())
        })?;
        if target_player_id == 0 {
            return Ok(0);
        }
        let event = SharedVipPresenceEvent {
            target_player_id,
            online,
        };
        let mut recipients = self
            .vip_presence_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        recipients.retain(|recipient_id, recipient| {
            if *recipient_id == u64::from(target_player_id)
                || !recipient.watched_player_ids.contains(&target_player_id)
            {
                return true;
            }
            match recipient.sender.try_send(event) {
                Ok(()) => {
                    delivered += 1;
                    true
                }
                Err(mpsc::TrySendError::Full(_)) => true,
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            }
        });
        Ok(delivered)
    }

    pub(crate) fn broadcast_public_chat(
        &self,
        sender_id: u64,
        message: &str,
    ) -> Result<usize, HostError> {
        self.broadcast_chat(sender_id, message, None)
    }

    pub(crate) fn broadcast_whisper_chat(
        &self,
        sender_id: u64,
        message: &str,
    ) -> Result<usize, HostError> {
        self.broadcast_ranged_chat(sender_id, message, NATIVE_CLASSIC_WHISPER_RANGE_TILES)
    }

    pub(crate) fn broadcast_yell_chat(
        &self,
        sender_id: u64,
        message: &str,
    ) -> Result<usize, HostError> {
        self.broadcast_ranged_chat(sender_id, message, NATIVE_CLASSIC_YELL_RANGE_TILES)
    }

    /// Delivers one whisper or yell to same-floor recipients within the audited classic range of
    /// the authoritative speaker position. The speaker always receives their own message. The
    /// listener-position snapshot is captured under one short lock so no world lock is taken
    /// while the chat-recipient mutex is held.
    pub(crate) fn broadcast_ranged_chat(
        &self,
        sender_id: u64,
        message: &str,
        range_tiles: u16,
    ) -> Result<usize, HostError> {
        let (sender, listener_positions) = {
            let world = self.lock()?;
            let sender = world
                .player(sender_id)
                .cloned()
                .ok_or(forgotten_core::CoreError::UnknownPlayer(sender_id))
                .map_err(HostError::Core)?;
            (sender, world.player_positions())
        };
        let body = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if body.is_empty() {
            return Ok(0);
        }
        let talk_mode = if range_tiles <= NATIVE_CLASSIC_WHISPER_RANGE_TILES {
            NATIVE_OTCLIENT_MESSAGE_WHISPER
        } else {
            NATIVE_OTCLIENT_MESSAGE_YELL
        };
        let event = SharedPublicChatEvent {
            speaker_name: sender.name.clone(),
            speaker_position: native_position(sender.position),
            channel_id: None,
            private: false,
            talk_mode,
            text: truncate_native_chat_text(&body),
        };
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        recipients.retain(|recipient_id, recipient| {
            let within_range = *recipient_id == sender_id
                || listener_positions
                    .get(recipient_id)
                    .is_some_and(|listener| {
                        listener.z == sender.position.z
                            && listener.x.abs_diff(sender.position.x) <= range_tiles
                            && listener.y.abs_diff(sender.position.y) <= range_tiles
                    });
            if !within_range {
                return true;
            }
            match recipient.sender.try_send(event.clone()) {
                Ok(()) => {
                    delivered += 1;
                    true
                }
                Err(mpsc::TrySendError::Full(_)) => true,
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            }
        });
        Ok(delivered)
    }

    pub(crate) fn broadcast_configured_public_channel_chat(
        &self,
        sender_id: u64,
        channel_id: u16,
        message: &str,
    ) -> Result<usize, HostError> {
        self.broadcast_chat(sender_id, message, Some(channel_id))
    }

    pub(crate) fn send_private_chat(
        &self,
        sender_id: u64,
        recipient_name: &str,
        message: &str,
    ) -> Result<PrivateChatDelivery, HostError> {
        let sender = self
            .lock()?
            .player(sender_id)
            .cloned()
            .ok_or(forgotten_core::CoreError::UnknownPlayer(sender_id))
            .map_err(HostError::Core)?;
        let body = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if body.is_empty() {
            return Ok(PrivateChatDelivery::NotOnline);
        }
        let event = SharedPublicChatEvent {
            speaker_name: sender.name,
            speaker_position: native_position(sender.position),
            channel_id: None,
            private: true,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
            text: truncate_native_chat_text(&body),
        };
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let Some((recipient_id, recipient)) = recipients
            .iter()
            .find(|(_, recipient)| recipient.player_name == recipient_name)
            .map(|(player_id, recipient)| (*player_id, recipient.sender.clone()))
        else {
            return Ok(PrivateChatDelivery::NotOnline);
        };
        match recipient.try_send(event) {
            Ok(()) => Ok(PrivateChatDelivery::Delivered),
            Err(mpsc::TrySendError::Full(_)) => Ok(PrivateChatDelivery::NotOnline),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                recipients.remove(&recipient_id);
                Ok(PrivateChatDelivery::NotOnline)
            }
        }
    }

    pub(crate) fn broadcast_chat(
        &self,
        sender_id: u64,
        message: &str,
        channel_id: Option<u16>,
    ) -> Result<usize, HostError> {
        let sender = self
            .lock()?
            .player(sender_id)
            .cloned()
            .ok_or(forgotten_core::CoreError::UnknownPlayer(sender_id))
            .map_err(HostError::Core)?;
        let body = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if body.is_empty() {
            return Ok(0);
        }
        let event = SharedPublicChatEvent {
            speaker_name: sender.name,
            speaker_position: native_position(sender.position),
            channel_id,
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
            text: truncate_native_chat_text(&body),
        };
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        recipients.retain(
            |_, recipient| match recipient.sender.try_send(event.clone()) {
                Ok(()) => {
                    delivered += 1;
                    true
                }
                Err(mpsc::TrySendError::Full(_)) => true,
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            },
        );
        Ok(delivered)
    }

    /// Delivers one console-originated GM broadcast to every connected session through the
    /// classic mode-9 talk record. Returns the number of queued recipients.
    pub fn broadcast_console_message(
        &self,
        speaker_name: &str,
        message: &str,
    ) -> Result<usize, HostError> {
        let body = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if body.is_empty() {
            return Ok(0);
        }
        let event = SharedPublicChatEvent {
            speaker_name: speaker_name.to_string(),
            speaker_position: NativeOtClientPosition { x: 0, y: 0, z: 0 },
            channel_id: None,
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_GM_BROADCAST,
            text: truncate_native_chat_text(&body),
        };
        self.fan_out_chat_event(event)
    }

    /// Queues one prebuilt chat event for every live recipient.
    pub(crate) fn fan_out_chat_event(
        &self,
        event: SharedPublicChatEvent,
    ) -> Result<usize, HostError> {
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        recipients.retain(
            |_, recipient| match recipient.sender.try_send(event.clone()) {
                Ok(()) => {
                    delivered += 1;
                    true
                }
                Err(mpsc::TrySendError::Full(_)) => true,
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            },
        );
        Ok(delivered)
    }
}

/// Routes one Talk record through the keyword chain (gamemaster verbs, operator Lua
/// words, declarative spell invocations, bank keywords, shop keywords) into chat
/// delivery, which always terminates the chain. Each router reports
/// `Handled`/`Unhandled`; the first handler wins. Exactly four parameters per the
/// handler rule: the session context, the record, and the two chat-drain handles.
/// Flood/mute gating stays in the session loop ahead of this call.
pub(crate) fn apply_native_talk_action(
    ctx: &mut SessionContext<'_>,
    request: &NativeOtClientTalkRequest,
    chat_events: &mpsc::Receiver<SharedPublicChatEvent>,
    open_public_channel_ids: &BTreeSet<u16>,
) -> Result<(), HostError> {
    // Gamemaster talkactions run before every other keyword router; see
    // gm_commands.rs. A handled verb consumes the record.
    if apply_native_gm_talkaction_talk(ctx, request)? == SessionActionOutcome::Handled {
        return Ok(());
    }
    // Operator-registered Lua talkactions dispatch through the resource-capped
    // sandbox; dispatch and effect application live in talkactions.rs. A handled word
    // consumes the record; anything else falls through to ordinary routing.
    // The config reference is copied out first so the dispatcher borrow never
    // overlaps the mutable context borrow below.
    let config = ctx.config;
    if let Some(dispatcher) = config.talkaction_dispatcher.as_ref() {
        if apply_native_lua_talkaction(ctx, request, dispatcher)? == SessionActionOutcome::Handled {
            return Ok(());
        }
    }
    // Spell invocation resolves through the operator command or an exact
    // declared Say keyword; see native_combat.rs. A handled invocation consumes
    // the record; anything else falls through to ordinary routing.
    if apply_native_declarative_spell_talk(ctx, request)? == SessionActionOutcome::Handled {
        return Ok(());
    }
    // Bounded NPC banking and shop keywords; see bank.rs and npc_shop.rs.
    // A handled keyword consumes the record; anything else falls through.
    if apply_native_bank_keyword_talk(ctx, request)? == SessionActionOutcome::Handled {
        return Ok(());
    }
    if apply_native_shop_keyword_talk(ctx, request)? == SessionActionOutcome::Handled {
        return Ok(());
    }
    // Chat delivery (private/channel/whisper/yell/guild/public), inbound
    // drain, and NPC dialogue; see below. Always ends Talk handling.
    apply_native_chat_routing(ctx, request, chat_events, open_public_channel_ids)
}

/// Routes one Talk record to private, channel, whisper, yell, guild, or public chat, drains
/// pending inbound chat, and answers Say records with NPC dialogue plus shop windows. Deferred
/// modes/channels emit a diagnostic and consume the record without effect; every path ends Talk
/// processing, so this returns `Result<(), _>` rather than a handled-flag like the keyword
/// routers that precede it.
pub(crate) fn apply_native_chat_routing(
    ctx: &mut SessionContext<'_>,
    request: &NativeOtClientTalkRequest,
    chat_events: &mpsc::Receiver<SharedPublicChatEvent>,
    open_public_channel_ids: &BTreeSet<u16>,
) -> Result<(), HostError> {
    let recipient_count = if request.mode == 5 {
        let Some(recipient_name) = request.recipient.as_deref() else {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=talk outcome=deferred-missing-private-recipient",
            );
            return Ok(());
        };
        match ctx.shared_world.send_private_chat(
            ctx.character_id,
            recipient_name,
            &request.message,
        )? {
            // Sender ack (plan 1.5): when the recipient has no online session, the
            // sender immediately hears the classic "not online" status line.
            PrivateChatDelivery::NotOnline => {
                let not_online = encode_native_otclient_status_message(
                    &ctx.config.client_profile,
                    &format!("A player called '{recipient_name}' is not online."),
                )
                .map_err(HostError::Protocol)?;
                write_frame(&mut *ctx.stream, &not_online)?;
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=talk outcome=private-recipient-not-online",
                );
                0
            }
            PrivateChatDelivery::Delivered => 1,
        }
    } else if request.mode == 7 {
        let Some(channel_id) = request.channel_id else {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=talk outcome=deferred-missing-channel-id",
            );
            return Ok(());
        };
        if !open_public_channel_ids.contains(&channel_id)
            || native_configured_public_channel(
                ctx.config.public_channel_catalog.as_deref(),
                channel_id,
            )
            .is_none()
        {
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=talk outcome=deferred-unopened-or-unconfigured-channel",
            );
            return Ok(());
        }
        ctx.shared_world.broadcast_configured_public_channel_chat(
            ctx.character_id,
            channel_id,
            &request.message,
        )?
    } else if request.mode == 2 {
        ctx.shared_world
            .broadcast_whisper_chat(ctx.character_id, &request.message)?
    } else if request.mode == 3 {
        ctx.shared_world
            .broadcast_yell_chat(ctx.character_id, &request.message)?
    } else if request.channel_id == Some(NATIVE_GUILD_CHAT_CHANNEL_ID) {
        // Guild chat: deliver to every online member of the sender's guild. The
        // open-channel gate above does not apply because the guild channel is
        // implicit membership from persisted rows, not a joined public channel.
        let membership = ctx
            .database
            .guild_membership(ctx.character_id)
            .map_err(HostError::Persistence)?;
        match membership {
            Some(record) => {
                let member_ids = ctx
                    .database
                    .guild_member_ids(record.guild_id)
                    .map_err(HostError::Persistence)?;
                ctx.shared_world.broadcast_guild_chat(
                    ctx.character_id,
                    &request.message,
                    &member_ids,
                )?
            }
            None => {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=talk outcome=guild-chat-no-membership",
                );
                0
            }
        }
    } else if request.channel_id.is_some() || request.recipient.is_some() {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=talk outcome=deferred-unsupported-channel-mode",
        );
        return Ok(());
    } else if request.mode == NATIVE_OTCLIENT_MESSAGE_SAY {
        ctx.shared_world
            .broadcast_public_chat(ctx.character_id, &request.message)?
    } else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=talk outcome=deferred-unsupported-mode",
        );
        return Ok(());
    };
    if ctx.config.extended_diagnostics {
        eprintln!(
            "> Native OTCv8 chat received mode={} bytes={} recipients={recipient_count}",
            request.mode,
            request.message.len()
        );
    }
    drain_shared_public_chat(
        &mut *ctx.stream,
        &ctx.config.client_profile,
        chat_events,
        open_public_channel_ids,
        ctx.config.extended_diagnostics,
        ctx.peer,
    )?;
    if request.mode == NATIVE_OTCLIENT_MESSAGE_SAY
        && request.channel_id.is_none()
        && request.recipient.is_none()
    {
        if let Some(catalog) = ctx.config.declarative_npc_dialogue_catalog.as_deref() {
            if let Some((npc_id, npc_name, npc_position, text)) =
                resolve_native_static_npc_dialogue(
                    ctx.shared_world,
                    ctx.character_id,
                    catalog,
                    &request.message,
                )?
            {
                let record = encode_native_otclient_public_say(
                    &ctx.config.client_profile,
                    &npc_name,
                    npc_position,
                    &text,
                )
                .map_err(HostError::Protocol)?;
                write_frame(&mut *ctx.stream, &record)?;
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    &format!(
                        "outbound=npc-dialogue opcode=0xaa mode=1 npc-id={} text-bytes={}",
                        npc_id,
                        text.len()
                    ),
                );
                // A greeting near a shop NPC also opens the classic shop windows
                // (0x7A catalog + 0x7B player goods) when a declarative shop and
                // presentation mapping exist for it.
                if let Some(shop_catalog) = ctx.config.shop_catalog.as_deref() {
                    let _ = deliver_native_npc_shop_windows(
                        &mut *ctx.stream,
                        &ctx.config.client_profile,
                        ctx.shared_world,
                        &*ctx.database,
                        ctx.character_id,
                        &npc_name,
                        shop_catalog,
                        ctx.config.item_presentation_catalog.as_deref(),
                        ctx.config.stackable_item_server_ids.as_deref(),
                        ctx.config.item_weight_by_server_id.as_deref(),
                        ctx.config.item_name_by_server_id.as_deref(),
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Adds one account VIP entry, then delivers the classic no-additional-info entry frame with
/// fixed offline status. Rejected adds and out-of-range target ids emit diagnostics without
/// effect.
pub(crate) fn apply_native_add_vip_action(
    ctx: &mut SessionContext<'_>,
    account_id: u32,
    target_player_name: &str,
) -> Result<(), HostError> {
    let entry =
        match ctx
            .database
            .add_account_vip_entry(account_id, target_player_name, "", 0, false)
        {
            Ok(entry) => entry,
            Err(_) => {
                native_diagnostic(
                    ctx.config.extended_diagnostics,
                    ctx.peer,
                    "action=vip-add outcome=rejected",
                );
                return Ok(());
            }
        };
    let target_player_id = match u32::try_from(entry.target_player_id) {
        Ok(target_player_id) if target_player_id != 0 => target_player_id,
        _ => {
            let _ = ctx
                .database
                .remove_account_vip_entry(account_id, entry.target_player_id);
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                "action=vip-add outcome=deferred-target-id-out-of-classic-range",
            );
            return Ok(());
        }
    };
    write_frame(
        &mut *ctx.stream,
        &encode_native_otclient_classic_vip_entry(
            &ctx.config.client_profile,
            target_player_id,
            &entry.target_player_name,
            false,
        )
        .map_err(HostError::Protocol)?,
    )?;
    Ok(())
}

/// Removes one account VIP entry. Failures emit a diagnostic without effect. Narrow
/// enough for direct parameters.
pub(crate) fn apply_native_remove_vip_action(
    database: &mut EngineDatabase,
    account_id: u32,
    target_player_id: u32,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    if database
        .remove_account_vip_entry(account_id, u64::from(target_player_id))
        .is_err()
    {
        native_diagnostic(
            extended_diagnostics,
            peer,
            "action=vip-remove outcome=rejected",
        );
    }
    Ok(())
}

/// Edits one account VIP entry's description, icon, and notify flag. Failures emit a
/// diagnostic without effect.
pub(crate) fn apply_native_edit_vip_action(
    ctx: &mut SessionContext<'_>,
    account_id: u32,
    target_player_id: u32,
    description: &str,
    icon: u32,
    notify: bool,
) -> Result<(), HostError> {
    if ctx
        .database
        .edit_account_vip_entry(
            account_id,
            u64::from(target_player_id),
            description,
            icon,
            notify,
        )
        .is_err()
    {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=vip-edit outcome=rejected",
        );
    }
    Ok(())
}

/// Removes one channel id from the session-local open public-channel set. Pure session view
/// bookkeeping; no frames, no persistence.
pub(crate) fn apply_native_leave_channel_action(
    open_public_channel_ids: &mut BTreeSet<u16>,
    extended_diagnostics: bool,
    peer: SocketAddr,
    channel_id: u16,
) -> Result<(), HostError> {
    let removed = open_public_channel_ids.remove(&channel_id);
    native_diagnostic(
        extended_diagnostics,
        peer,
        &format!(
            "action=leave-channel channel-id={channel_id} outcome=session-local-removed-{removed}"
        ),
    );
    Ok(())
}

/// Opens one configured public channel for the session: validates against the operator catalog,
/// records membership locally, and emits the open-channel frame. Unknown channels emit a
/// diagnostic without effect.
pub(crate) fn apply_native_join_channel_action(
    ctx: &mut SessionContext<'_>,
    channel_id: u16,
    open_public_channel_ids: &mut BTreeSet<u16>,
) -> Result<(), HostError> {
    let Some(channel) =
        native_configured_public_channel(ctx.config.public_channel_catalog.as_deref(), channel_id)
    else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=join-channel outcome=deferred-unknown-or-unconfigured-channel",
        );
        return Ok(());
    };
    open_public_channel_ids.insert(channel.id);
    let open_channel =
        encode_native_otclient_open_public_channel(&ctx.config.client_profile, &channel)
            .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &open_channel)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "outbound=open-public-channel opcode=0xac channel-id={} bytes={}",
            channel.id,
            open_channel.0.len(),
        ),
    );
    Ok(())
}

/// Delivers the configured public-channel list, appending the reserved guild channel for guild
/// members. Never fails the session.
pub(crate) fn apply_native_request_channels_action(
    ctx: &mut SessionContext<'_>,
) -> Result<(), HostError> {
    let mut entries =
        native_classic_channel_list_entries(ctx.config.public_channel_catalog.as_deref());
    // Plan v49 slice 19: guild members see the reserved guild channel (0x00F1).
    let guild_context = native_guild_channel_context(&*ctx.database, ctx.character_id);
    if let Some((channel, _)) = &guild_context {
        entries.push(channel.clone());
    }
    let channels = encode_native_otclient_channel_list(&ctx.config.client_profile, &entries)
        .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &channels)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "outbound=channel-list opcode=0xab entries={} bytes={}",
            entries.len(),
            channels.0.len(),
        ),
    );
    Ok(())
}
