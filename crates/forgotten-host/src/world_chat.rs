//! Chat and VIP presence fan-out on the shared world: public/whisper/yell/ranged broadcast,
//! configured-channel delivery, private messages, console broadcast, and VIP presence
//! events. Recipients are registered per connected session and receive bounded copies.

use super::*;

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
            return Ok(0);
        };
        match recipient.try_send(event) {
            Ok(()) => Ok(1),
            Err(mpsc::TrySendError::Full(_)) => Ok(0),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                recipients.remove(&recipient_id);
                Ok(0)
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
