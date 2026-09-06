//! SharedNativeWorld: the authoritative shared-world facade over forgotten-core WorldState
//! with epoch counters, player/equipment/container/progression accessors, static-spawn
//! bootstrap and tick, operator teleport/summon helpers, and kick/trade signals.

use super::*;

impl SharedNativeWorld {
    pub fn from_static_spawns(
        static_spawns: Option<&FeTfsStaticSpawnCollection>,
    ) -> Result<Self, HostError> {
        let mut world = WorldState::default();
        if let Some(static_spawns) = static_spawns {
            world
                .install_static_creatures(static_spawns)
                .map_err(HostError::Core)?;
        }
        Ok(Self {
            world: Arc::new(Mutex::new(world)),
            player_outfits: Arc::new(Mutex::new(BTreeMap::new())),
            player_directions: Arc::new(Mutex::new(BTreeMap::new())),
            visibility_epoch: Arc::new(AtomicU64::new(0)),
            vitals_epoch: Arc::new(AtomicU64::new(0)),
            progression_epoch: Arc::new(AtomicU64::new(0)),
            equipment_epoch: Arc::new(AtomicU64::new(0)),
            containers_epoch: Arc::new(AtomicU64::new(0)),
            party_epoch: Arc::new(AtomicU64::new(0)),
            online_players: Arc::new(AtomicU64::new(0)),
            chat_recipients: Arc::new(Mutex::new(BTreeMap::new())),
            vip_presence_recipients: Arc::new(Mutex::new(BTreeMap::new())),
            pending_kicks: Arc::new(Mutex::new(BTreeMap::new())),
            pending_trades_closed: Arc::new(Mutex::new(BTreeSet::new())),
        })
    }

    pub fn advance_tick(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.advance_tick())
    }

    pub fn advance_ticks(&self, elapsed_seconds: u16) -> Result<u64, HostError> {
        Ok(self.lock()?.advance_ticks(elapsed_seconds))
    }

    pub fn reactivate_due_static_creatures(&self) -> Result<StaticCreatureResetSummary, HostError> {
        let summary = self.lock()?.reactivate_due_static_creatures();
        if summary.reactivated > 0 {
            self.mark_visibility_changed();
        }
        Ok(summary)
    }

    pub fn tick(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.tick())
    }

    /// Returns the generic authoritative world revision. Protocol paths continue to use their
    /// dedicated visibility and vitals epochs until a typed event stream is introduced.
    pub fn world_revision(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.revision())
    }

    pub fn visibility_epoch(&self) -> u64 {
        self.visibility_epoch.load(Ordering::SeqCst)
    }

    /// Returns the IDs of every registered player, sorted. Callers use this for bounded periodic
    /// snapshot flushes; it grants no mutation access.
    pub fn registered_player_ids(&self) -> Result<Vec<u64>, HostError> {
        Ok(self.lock()?.registered_player_ids())
    }

    pub fn vitals_epoch(&self) -> u64 {
        self.vitals_epoch.load(Ordering::SeqCst)
    }

    pub fn progression_epoch(&self) -> u64 {
        self.progression_epoch.load(Ordering::SeqCst)
    }

    pub fn equipment_epoch(&self) -> u64 {
        self.equipment_epoch.load(Ordering::SeqCst)
    }

    pub fn containers_epoch(&self) -> u64 {
        self.containers_epoch.load(Ordering::SeqCst)
    }

    pub fn party_epoch(&self) -> u64 {
        self.party_epoch.load(Ordering::SeqCst)
    }

    /// Live registered-player count for lock-free status/metrics consumers.
    pub fn online_players(&self) -> u32 {
        u32::try_from(self.online_players.load(Ordering::SeqCst)).unwrap_or(u32::MAX)
    }

    /// Cloneable counter handle shared with the status listener thread.
    pub fn online_players_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.online_players)
    }

    pub fn player_vitals(&self, player_id: u64) -> Result<PlayerVitals, HostError> {
        self.lock()?
            .player_vitals(player_id)
            .map_err(HostError::Core)
    }
    pub(crate) fn player_and_vitals(
        &self,
        player_id: u64,
    ) -> Result<(Player, PlayerVitals), HostError> {
        let world = self.lock()?;
        let player = world.player(player_id).cloned().ok_or(HostError::Core(
            forgotten_core::CoreError::UnknownPlayer(player_id),
        ))?;
        let vitals = world.player_vitals(player_id).map_err(HostError::Core)?;
        Ok((player, vitals))
    }

    pub fn player_progression(&self, player_id: u64) -> Result<PlayerProgression, HostError> {
        self.lock()?
            .player_progression(player_id)
            .map_err(HostError::Core)
    }

    pub fn replace_player_progression(
        &self,
        player_id: u64,
        progression: PlayerProgression,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .replace_player_progression(player_id, progression)
            .map_err(HostError::Core)?;
        if changed {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(changed)
    }

    pub fn player_progression_attempts(
        &self,
        player_id: u64,
    ) -> Result<PlayerProgressionAttempts, HostError> {
        self.lock()?
            .player_progression_attempts(player_id)
            .map_err(HostError::Core)
    }

    pub fn replace_player_progression_attempts(
        &self,
        player_id: u64,
        attempts: PlayerProgressionAttempts,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_progression_attempts(player_id, attempts)
            .map_err(HostError::Core)
    }

    pub fn replace_player_combat_defense(
        &self,
        player_id: u64,
        defense: PlayerCombatDefense,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_combat_defense(player_id, defense)
            .map_err(HostError::Core)
    }

    pub fn player_town(&self, player_id: u64) -> Result<u32, HostError> {
        self.lock()?.player_town(player_id).map_err(HostError::Core)
    }

    pub fn replace_player_town(&self, player_id: u64, town_id: u32) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_town(player_id, town_id)
            .map_err(HostError::Core)
    }

    pub fn player_respawn_state(
        &self,
        player_id: u64,
    ) -> Result<forgotten_core::PlayerRespawnState, HostError> {
        self.lock()?
            .player_respawn_state(player_id)
            .map_err(HostError::Core)
    }

    pub fn hydrate_player_respawn_state(
        &self,
        player_id: u64,
        state: PlayerRespawnState,
    ) -> Result<bool, HostError> {
        self.lock()?
            .hydrate_player_respawn_state(player_id, state)
            .map_err(HostError::Core)
    }

    pub fn apply_player_regeneration(
        &self,
        player_id: u64,
        rules: PlayerRegenerationRules,
        elapsed_seconds: u16,
    ) -> Result<PlayerRegenerationOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_player_regeneration(player_id, rules, elapsed_seconds)
            .map_err(HostError::Core)?;
        if outcome.health_gained > 0 || outcome.mana_gained > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    pub fn apply_player_conditions(
        &self,
        player_id: u64,
        elapsed_seconds: u16,
    ) -> Result<PlayerConditionOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_player_conditions(player_id, elapsed_seconds)
            .map_err(HostError::Core)?;
        if outcome.applied_damage > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    /// Advances bounded conditions and enters authoritative death state only when the resulting
    /// damage is lethal at a validated assigned town. Client death effects and packet delivery
    /// remain separate host responsibilities.
    pub fn apply_player_conditions_with_death(
        &self,
        player_id: u64,
        world_map: &WorldMap,
        elapsed_seconds: u16,
    ) -> Result<
        (
            PlayerConditionOutcome,
            PlayerVitals,
            Option<forgotten_core::PlayerRespawnState>,
        ),
        HostError,
    > {
        let mut world = self.lock()?;
        let town_id = world.player_town(player_id).map_err(HostError::Core)?;
        let (outcome, death_state) = world
            .apply_player_conditions_with_death(player_id, town_id, world_map, elapsed_seconds)
            .map_err(HostError::Core)?;
        let vitals = world.player_vitals(player_id).map_err(HostError::Core)?;
        if outcome.applied_damage > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok((outcome, vitals, death_state))
    }

    pub fn award_player_experience(
        &self,
        player_id: u64,
        raw_experience: u64,
        policy: &ExperienceAwardPolicy,
    ) -> Result<PlayerExperienceAwardOutcome, HostError> {
        let outcome = self
            .lock()?
            .award_player_experience(player_id, raw_experience, policy)
            .map_err(HostError::Core)?;
        if outcome.awarded_experience > 0 {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        if outcome.gained_levels > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    /// Additive wrapper for configuration-selected vocation gains. The existing no-gain method
    /// remains available for callers that do not yet hydrate a legacy vocation registry.
    pub fn award_player_experience_with_vocation_gains(
        &self,
        player_id: u64,
        raw_experience: u64,
        policy: &ExperienceAwardPolicy,
        gains: VocationLevelUpGains,
    ) -> Result<PlayerExperienceAwardOutcome, HostError> {
        let outcome = self
            .lock()?
            .award_player_experience_with_vocation_gains(player_id, raw_experience, policy, gains)
            .map_err(HostError::Core)?;
        if outcome.awarded_experience > 0 {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        if outcome.gained_levels > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    pub fn apply_player_skill_tries(
        &self,
        player_id: u64,
        skill: PlayerSkill,
        awarded_tries: u64,
        rules: PlayerProgressionRules,
    ) -> Result<PlayerSkillTryOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_player_skill_tries(player_id, skill, awarded_tries, rules)
            .map_err(HostError::Core)?;
        if awarded_tries > 0 {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    pub fn player_equipment(&self, player_id: u64) -> Result<PlayerEquipment, HostError> {
        self.lock()?
            .player_equipment(player_id)
            .cloned()
            .map_err(HostError::Core)
    }

    pub fn replace_player_equipment(
        &self,
        player_id: u64,
        equipment: PlayerEquipment,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .replace_player_equipment(player_id, equipment)
            .map_err(HostError::Core)?;
        if changed {
            self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(changed)
    }

    pub fn player_containers(&self, player_id: u64) -> Result<PlayerContainers, HostError> {
        self.lock()?
            .player_containers(player_id)
            .cloned()
            .map_err(HostError::Core)
    }

    pub fn replace_player_containers(
        &self,
        player_id: u64,
        containers: PlayerContainers,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .replace_player_containers(player_id, containers)
            .map_err(HostError::Core)?;
        if changed {
            self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(changed)
    }

    /// Active haste modifier percent for one player (0 when none) â€” plan v49 slice 12.
    pub fn player_speed_bonus_percent(&self, player_id: u64) -> u16 {
        self.lock()
            .map_or(0, |world| world.player_speed_bonus_percent(player_id))
    }

    /// Deterministic loot-split recipient order for one killer (plan v49 slice 14): the party
    /// leader first, then members in ascending id order; the killer is included wherever they
    /// sit in that order. Empty output means the killer has no party and nothing is distributed.
    /// Unknown players yield empty output too â€” the corpse path must never fail on a vanished
    /// killer, so this helper fails open instead of propagating the unknown-player error.
    pub fn party_loot_split_targets(&self, player_id: u64) -> Result<Vec<u64>, HostError> {
        let world = self.lock()?;
        let Ok(Some(leader)) = world
            .player_party_leader(player_id)
            .map_err(HostError::Core)
        else {
            return Ok(Vec::new());
        };
        let mut targets = vec![leader];
        if let Ok(members) = world.player_party_members(leader) {
            targets.extend(members);
        }
        Ok(targets)
    }

    pub fn replace_player_conditions(
        &self,
        player_id: u64,
        conditions: BTreeMap<PlayerConditionKind, PlayerCondition>,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_conditions(player_id, conditions)
            .map_err(HostError::Core)
    }

    /// Captures all world-owned data needed for native map rendering under one short lock. The
    /// returned snapshot contains owned values so protocol encoding and socket writes can proceed
    /// concurrently without retaining the authoritative-world mutex.
    pub(crate) fn native_render_snapshot(
        &self,
        observer_id: u64,
        look_type: u8,
        speed: u16,
    ) -> Result<NativeWorldRenderSnapshot, HostError> {
        let (static_spawns, player_snapshots, invisible_ids) = {
            let world = self.lock()?;
            (
                world.active_static_spawn_collection(),
                world.player_render_snapshots(),
                world.invisible_player_ids(),
            )
        };
        let invisible_ids: std::collections::BTreeSet<u64> = invisible_ids.into_iter().collect();
        let player_outfits = self
            .player_outfits
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let player_directions = self
            .player_directions
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let visible_players = player_snapshots
            .into_iter()
            // /invisible players are hidden from every other player's viewport.
            .filter(|player| player.id != observer_id && !invisible_ids.contains(&player.id))
            .map(|player| {
                Ok(NativeOtClientVisiblePlayer {
                    player_id: native_player_id(player.id)?,
                    name: player.name,
                    position: native_position(player.position),
                    health_percent: player.health_percent,
                    outfit: player_outfits.get(&player.id).copied().unwrap_or(
                        NativeOtClientClassicOutfit {
                            look_type,
                            head: 0,
                            body: 0,
                            legs: 0,
                            feet: 0,
                        },
                    ),
                    direction: player_directions
                        .get(&player.id)
                        .copied()
                        .unwrap_or(NativeOtClientCardinalDirection::South.protocol_direction()),
                    speed,
                })
            })
            .collect::<Result<Vec<_>, HostError>>()?;
        Ok(NativeWorldRenderSnapshot {
            static_spawns,
            visible_players,
        })
    }

    /// Resolves one connected player ID by exact case-insensitive name.
    pub fn online_player_id_by_name(&self, name: &str) -> Result<Option<u64>, HostError> {
        let lowered = name.trim().to_lowercase();
        if lowered.is_empty() {
            return Ok(None);
        }
        let snapshots = self.lock()?.player_render_snapshots();
        Ok(snapshots
            .into_iter()
            .find(|player| player.name.to_lowercase() == lowered)
            .map(|player| player.id))
    }

    /// True when the player is currently registered in the shared world.
    pub fn has_player(&self, player_id: u64) -> Result<bool, HostError> {
        Ok(self
            .lock()?
            .player_render_snapshots()
            .iter()
            .any(|player| player.id == player_id))
    }

    /// Reads one connected player's authoritative position.
    pub fn player_position(&self, player_id: u64) -> Result<forgotten_core::Position, HostError> {
        self.lock()?
            .player_render_snapshots()
            .into_iter()
            .find(|player| player.id == player_id)
            .map(|player| player.position)
            .ok_or(forgotten_core::CoreError::UnknownPlayer(player_id))
            .map_err(HostError::Core)
    }

    /// Reads one connected player's position and facing direction byte for summon anchoring.
    pub fn player_position_and_facing(
        &self,
        player_id: u64,
    ) -> Result<Option<(forgotten_core::Position, u8)>, HostError> {
        let position = self.player_position(player_id)?;
        let direction = self
            .player_directions
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .get(&player_id)
            .copied()
            .unwrap_or(2);
        Ok(Some((position, direction)))
    }

    /// Any connected player's anchor for console spawns that omit an explicit character.
    pub fn any_online_player_anchor(
        &self,
    ) -> Result<Option<(forgotten_core::Position, u8)>, HostError> {
        let first_id = self.lock()?.registered_player_ids().into_iter().next();
        match first_id {
            Some(player_id) => self.player_position_and_facing(player_id),
            None => Ok(None),
        }
    }

    /// Applies an operator gamemaster tier to a connected player's live state. Persistence is
    /// the caller's responsibility; this only keeps sessions consistent.
    pub fn update_player_gm_level(&self, _player_id: u64, _level: u8) -> Result<(), HostError> {
        // GM tier currently lives only in SQLite plus command authorization at request time;
        // no per-session cached copy exists yet. Kept as an explicit boundary so future
        // client-visible GM indicators have one integration point.
        Ok(())
    }

    /// Operator teleport for a connected player with full map validation and persistence
    /// handled by the bridge caller.
    pub fn teleport_player_for_operator(
        &self,
        player_id: u64,
        destination: forgotten_core::Position,
    ) -> Result<(), HostError> {
        {
            let mut world = self.lock()?;
            world
                .teleport_player(player_id, destination)
                .map_err(HostError::Core)?;
        }
        self.mark_visibility_changed();
        Ok(())
    }

    /// Summons an entity clone one tile in front of the named player's facing, validating
    /// walkability and occupancy through the core transition.
    pub fn spawn_dynamic_entity_in_front_of_player(
        &self,
        player_id: u64,
        entity_name: &str,
    ) -> Result<u32, HostError> {
        let Some((anchor, direction_byte)) = self.player_position_and_facing(player_id)? else {
            return Err(HostError::Core(forgotten_core::CoreError::UnknownPlayer(
                player_id,
            )));
        };
        let delta = match direction_byte {
            0 => (0_i32, -1_i32), // north
            1 => (1, 0),          // east
            2 => (0, 1),          // south
            3 => (-1, 0),         // west
            _ => (0, 1),
        };
        let target = forgotten_core::Position {
            x: (anchor.x as i32 + delta.0).max(0) as u16,
            y: (anchor.y as i32 + delta.1).max(0) as u16,
            z: anchor.z,
        };
        let spawned = {
            let mut world = self.lock()?;
            world
                .spawn_dynamic_entity(entity_name, target, direction_byte)
                .map_err(HostError::Core)?
        };
        self.mark_visibility_changed();
        Ok(spawned)
    }

    /// Summons an entity one tile in front of an explicit anchor position.
    pub fn spawn_dynamic_entity_in_front(
        &self,
        entity_name: &str,
        anchor: forgotten_core::Position,
        direction_byte: u8,
    ) -> Result<u32, HostError> {
        let delta = match direction_byte {
            0 => (0_i32, -1_i32), // north
            1 => (1, 0),          // east
            2 => (0, 1),          // south
            3 => (-1, 0),         // west
            _ => (0, 1),
        };
        let target = forgotten_core::Position {
            x: (anchor.x as i32 + delta.0).max(0) as u16,
            y: (anchor.y as i32 + delta.1).max(0) as u16,
            z: anchor.z,
        };
        let spawned = {
            let mut world = self.lock()?;
            world
                .spawn_dynamic_entity(entity_name, target, direction_byte)
                .map_err(HostError::Core)?
        };
        self.mark_visibility_changed();
        Ok(spawned)
    }

    /// Delivers guild chat to every online member of the sender's guild through the classic
    /// channel-talk record (mode 7, guild channel id 0x00F1). The sender always receives their
    /// own message. Membership resolves from SQLite; delivery is session-local.
    pub fn broadcast_guild_chat(
        &self,
        sender_id: u64,
        message: &str,
        member_ids: &[u64],
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
        // Guild channel id: FE reserves 0x00F1 for the guild chat channel on classic profiles.
        const NATIVE_GUILD_CHANNEL_ID: u16 = 0x00F1;
        let event = SharedPublicChatEvent {
            speaker_name: sender.name,
            speaker_position: native_position(sender.position),
            channel_id: Some(NATIVE_GUILD_CHANNEL_ID),
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
            text: truncate_native_chat_text(&body),
        };
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        for member_id in member_ids {
            let Some(recipient) = recipients.get(member_id) else {
                continue;
            };
            match recipient.sender.try_send(event.clone()) {
                Ok(()) => delivered += 1,
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    recipients.remove(member_id);
                }
                Err(mpsc::TrySendError::Full(_)) => {}
            }
        }
        Ok(delivered)
    }

    /// Requests an orderly disconnect of one connected player at the next session drain.
    /// Returns false when the player is not online.
    pub fn request_kick(&self, player_id: u64) -> Result<bool, HostError> {
        let online = self.has_player(player_id)?;
        if !online {
            return Ok(false);
        }
        self.pending_kicks
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .insert(player_id, std::time::SystemTime::now());
        Ok(true)
    }

    /// Cancels any live trade touching one player and flags the other participant's session
    /// so its drain loop closes their trade window too. Returns the other player id.
    pub fn cancel_player_trade_and_signal(&self, player_id: u64) -> Result<Option<u64>, HostError> {
        let other = {
            let mut world = self.lock()?;
            world.cancel_player_trade(player_id)
        };
        if let Some(other_id) = other {
            self.pending_trades_closed
                .lock()
                .map_err(|_| HostError::SharedWorldUnavailable)?
                .insert(other_id);
        }
        Ok(other)
    }

    /// Consumes a pending kick for one player: returns true exactly once per requested kick.
    /// The native session loop calls this every iteration so operator kicks disconnect the
    /// target within roughly one heartbeat.
    pub fn take_pending_kick(&self, player_id: u64) -> Result<bool, HostError> {
        let mut kicks = self
            .pending_kicks
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        Ok(kicks.remove(&player_id).is_some())
    }

    /// Consumes the trade-closed signal for one player (their window must close).
    pub fn take_pending_trade_close(&self, player_id: u64) -> Result<bool, HostError> {
        let mut closed = self
            .pending_trades_closed
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        Ok(closed.remove(&player_id))
    }

    /// Restores a connected player's health and mana to their maximums. Used by operator
    /// `/heal`; returns false when the player is offline.
    pub fn restore_player_vitals(&self, player_id: u64) -> Result<bool, HostError> {
        let Ok((_, vitals)) = self.player_and_vitals(player_id) else {
            return Ok(false);
        };
        let restored = PlayerVitals {
            health: vitals.max_health,
            mana: vitals.max_mana,
            ..vitals
        };
        {
            let mut world = self.lock()?;
            world
                .update_player_vitals(player_id, restored)
                .map_err(HostError::Core)?;
        }
        self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    /// Bumps the containers epoch so every session re-emits open-container deltas. Operator
    /// item delivery must call this or clients never learn a container changed.
    pub fn mark_containers_changed(&self) {
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Flags both participants' sessions to close their trade windows after a successful swap
    /// or any cancellation path that bypasses per-player signaling.
    pub fn signal_trade_closed(&self, initiator: u64, counterparty: u64) -> Result<(), HostError> {
        let mut closed = self
            .pending_trades_closed
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        closed.insert(initiator);
        closed.insert(counterparty);
        Ok(())
    }
    pub(crate) fn mark_visibility_changed(&self) {
        self.visibility_epoch.fetch_add(1, Ordering::SeqCst);
    }
    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, WorldState>, HostError> {
        self.world
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)
    }
}
