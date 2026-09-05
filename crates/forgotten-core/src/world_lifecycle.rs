//! Player registration and lifecycle on the authoritative world state: position-aware
//! registration with progressive hydration overloads, removal, and core player queries.

use super::*;

impl WorldState {
    pub fn add_player(&mut self, player: Player) -> Result<(), CoreError> {
        self.add_player_with_vitals(player, PlayerVitals::default())
    }

    pub fn add_player_with_vitals(
        &mut self,
        player: Player,
        vitals: PlayerVitals,
    ) -> Result<(), CoreError> {
        self.add_player_with_vitals_and_progression(player, vitals, PlayerProgression::default())
    }

    pub fn add_player_with_vitals_and_progression(
        &mut self,
        player: Player,
        vitals: PlayerVitals,
        progression: PlayerProgression,
    ) -> Result<(), CoreError> {
        if player.name.trim().is_empty() {
            return Err(CoreError::EmptyPlayerName);
        }
        if !vitals.is_valid() {
            return Err(CoreError::InvalidPlayerVitals(player.id));
        }
        if self.players.contains_key(&player.id) {
            return Err(CoreError::DuplicatePlayer(player.id));
        }
        if self
            .players
            .values()
            .any(|existing| existing.position == player.position)
        {
            return Err(CoreError::PlayerOccupiesPosition(player.position));
        }
        if self.is_static_creature_occupied(player.position) {
            return Err(CoreError::StaticCreatureOccupiesPosition(player.position));
        }
        self.player_vitals.insert(player.id, vitals);
        self.player_progressions.insert(player.id, progression);
        self.player_progression_attempts
            .insert(player.id, PlayerProgressionAttempts::default());
        self.player_towns.insert(player.id, 0);
        self.player_regeneration_schedules
            .insert(player.id, PlayerRegenerationSchedule::default());
        self.player_conditions.insert(player.id, BTreeMap::new());
        self.player_respawn_states
            .insert(player.id, PlayerRespawnState::default());
        self.player_equipments
            .insert(player.id, PlayerEquipment::default());
        self.player_containers
            .insert(player.id, PlayerContainers::default());
        self.player_combat_defenses
            .insert(player.id, PlayerCombatDefense::default());
        self.player_fight_modes
            .insert(player.id, PlayerFightModeState::default());
        self.player_combat_cooldowns
            .insert(player.id, PlayerCombatCooldown::default());
        self.player_spell_cooldowns
            .insert(player.id, PlayerSpellCooldown::default());
        self.players.insert(player.id, player);
        self.mark_changed();
        Ok(())
    }

    pub fn remove_player(&mut self, id: u64) -> Result<Player, CoreError> {
        let player = self
            .players
            .remove(&id)
            .ok_or(CoreError::UnknownPlayer(id))?;
        self.clear_player_party_state(id);
        self.player_vitals.remove(&id);
        self.player_progressions.remove(&id);
        self.player_progression_attempts.remove(&id);
        self.player_towns.remove(&id);
        self.player_regeneration_schedules.remove(&id);
        self.player_conditions.remove(&id);
        self.player_respawn_states.remove(&id);
        self.player_equipments.remove(&id);
        self.player_containers.remove(&id);
        self.player_combat_defenses.remove(&id);
        self.player_fight_modes.remove(&id);
        self.player_combat_cooldowns.remove(&id);
        self.player_spell_cooldowns.remove(&id);
        self.player_interactions.remove(&id);
        self.clear_player_interaction_references(id);
        for runtime in self.static_creatures.values_mut() {
            if runtime.target_player_id == Some(id) {
                runtime.target_player_id = None;
            }
        }
        self.mark_changed();
        Ok(player)
    }

    pub fn is_player_occupied(&self, position: Position) -> bool {
        self.players
            .values()
            .any(|player| player.position == position)
    }

    pub fn player_render_snapshots(&self) -> Vec<PlayerRenderSnapshot> {
        self.players
            .values()
            .map(|player| {
                let vitals = self
                    .player_vitals
                    .get(&player.id)
                    .copied()
                    .unwrap_or_default();
                PlayerRenderSnapshot {
                    id: player.id,
                    name: player.name.clone(),
                    position: player.position,
                    level: player.level,
                    health_percent: ((u32::from(vitals.health) * 100)
                        / u32::from(vitals.max_health.max(1)))
                    .min(100) as u8,
                }
            })
            .collect()
    }

    /// Captures one detached ID-to-position map for every registered player. Chat range checks
    /// and similar read-only policies can consult this snapshot without retaining the world lock.
    pub fn player_positions(&self) -> BTreeMap<u64, Position> {
        self.players
            .iter()
            .map(|(id, player)| (*id, player.position))
            .collect()
    }

    pub fn player_interaction_intent(
        &self,
        player_id: u64,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        Ok(self
            .player_interactions
            .get(&player_id)
            .copied()
            .unwrap_or_default())
    }

    /// Validates one bounded, top-level map-item use intent against server-owned world state.
    /// Same-tile use is allowed for items under a player; otherwise the item must be adjacent.
    /// Validation does not mutate the world or make an action-compatibility claim.
    pub fn validate_player_item_use(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseIntent,
    ) -> Result<PlayerItemUseOutcome, CoreError> {
        if intent.expected_server_id == 0 {
            return Err(CoreError::InvalidItemUseIntent);
        }
        let player = self
            .player(intent.player_id)
            .ok_or(CoreError::UnknownPlayer(intent.player_id))?;
        if player.position != intent.position && !player.position.is_adjacent_to(intent.position) {
            return Err(CoreError::ItemUseOutOfRange {
                player_id: intent.player_id,
                from: player.position,
                to: intent.position,
            });
        }
        if map.tile(intent.position).is_none() {
            return Err(CoreError::MissingMapTile(intent.position));
        }
        let item = map
            .tile_items(intent.position)
            .and_then(|items| items.get(usize::from(intent.stack_index)))
            .filter(|item| item.server_id == intent.expected_server_id)
            .ok_or(CoreError::UnknownMapItem {
                position: intent.position,
                stack_index: intent.stack_index,
                expected_server_id: intent.expected_server_id,
            })?;
        Ok(PlayerItemUseOutcome {
            player_id: intent.player_id,
            position: intent.position,
            stack_index: intent.stack_index,
            server_id: item.server_id,
            count: item.count,
            action_id: item.action_id,
            unique_id: item.unique_id,
            has_text: item.text.as_deref().is_some_and(|text| !text.is_empty()),
            charges: item.charges,
            teleport_destination: item.teleport_destination,
        })
    }

    /// Validates both source and destination map-item references for a bounded two-target item-use
    /// request. Both items must independently be same-tile or adjacent to the player, belong to
    /// existing map tiles, and match their expected server IDs at the requested top-level stack
    /// indexes. The world is never mutated and no action execution is claimed.
    pub fn validate_player_item_use_ex(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseExIntent,
    ) -> Result<PlayerItemUseExOutcome, CoreError> {
        Ok(PlayerItemUseExOutcome {
            source: self.validate_player_item_use(map, intent.source)?,
            target: self.validate_player_item_use(map, intent.target)?,
        })
    }

    /// Validates one exact authoritative top-level map item and one authoritative creature target.
    /// The source item must meet the existing map, stack, server-ID, and range requirements. The
    /// target must be a live player or an active static creature on the same or an adjacent tile.
    /// This is validation-only and does not select, attack, affect, or otherwise mutate a target.
    pub fn validate_player_item_use_creature(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseCreatureIntent,
    ) -> Result<PlayerItemUseCreatureOutcome, CoreError> {
        let source = self.validate_player_item_use(map, intent.source)?;
        let player = self
            .player(intent.source.player_id)
            .ok_or(CoreError::UnknownPlayer(intent.source.player_id))?;
        let target = match intent.target {
            PlayerItemUseCreatureTarget::Player(player_id) => {
                let target = self
                    .player(player_id)
                    .ok_or(CoreError::UnknownPlayer(player_id))?;
                if self.player_respawn_state(player_id)?.dead {
                    return Err(CoreError::PlayerIsDead(player_id));
                }
                PlayerItemUseCreatureTargetOutcome::Player {
                    player_id,
                    position: target.position,
                }
            }
            PlayerItemUseCreatureTarget::StaticCreature(creature_id) => {
                let target = self
                    .static_creatures
                    .get(&creature_id)
                    .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
                if !target.active {
                    return Err(CoreError::InactiveStaticCreature(creature_id));
                }
                PlayerItemUseCreatureTargetOutcome::StaticCreature {
                    creature_id,
                    position: target.entity.position,
                    health_percent: target.health_percent,
                }
            }
        };
        let target_position = match target {
            PlayerItemUseCreatureTargetOutcome::Player { position, .. }
            | PlayerItemUseCreatureTargetOutcome::StaticCreature { position, .. } => position,
        };
        if player.position != target_position && !player.position.is_adjacent_to(target_position) {
            return Err(CoreError::ItemUseOutOfRange {
                player_id: intent.source.player_id,
                from: player.position,
                to: target_position,
            });
        }
        Ok(PlayerItemUseCreatureOutcome { source, target })
    }

    pub fn set_player_target(
        &mut self,
        player_id: u64,
        target_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        self.set_player_interaction(player_id, target_player_id, None, true)
    }

    /// Selects an active static creature as a target only. It does not move, follow, attack,
    /// damage, despawn, loot, execute scripts, or produce a client packet.
    pub fn set_player_static_target(
        &mut self,
        player_id: u64,
        target_static_creature_id: Option<u32>,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if target_static_creature_id.is_some() && self.player_respawn_state(player_id)?.dead {
            return Err(CoreError::PlayerIsDead(player_id));
        }
        if let Some(target_static_creature_id) = target_static_creature_id {
            let target = self
                .static_creatures
                .get(&target_static_creature_id)
                .ok_or(CoreError::UnknownStaticCreature(target_static_creature_id))?;
            if !target.active {
                return Err(CoreError::InactiveStaticCreature(target_static_creature_id));
            }
        }
        let previous = self.player_interaction_intent(player_id)?;
        let intent = {
            let intent = self.player_interactions.entry(player_id).or_default();
            intent.target_player_id = None;
            intent.target_static_creature_id = target_static_creature_id;
            intent.follow_player_id = None;
            *intent
        };
        if intent == PlayerInteractionIntent::default() {
            self.player_interactions.remove(&player_id);
        }
        if previous != intent {
            self.mark_changed();
        }
        Ok(intent)
    }

    pub fn set_player_follow(
        &mut self,
        player_id: u64,
        follow_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        self.set_player_interaction(player_id, None, follow_player_id, false)
    }

    /// Replaces the immutable display-only static creature set. Per-spawn reactivation intervals
    /// are retained as data, but this installation path starts no scheduler and adds no AI,
    /// combat, movement, or script behavior.

    pub fn player(&self, id: u64) -> Option<&Player> {
        self.players.get(&id)
    }

    /// Returns the IDs of every registered player, sorted. Callers use this for bounded periodic
    /// snapshot flushes; it grants no mutation access.
    pub fn registered_player_ids(&self) -> Vec<u64> {
        self.players.keys().copied().collect()
    }

    /// Applies a prevalidated global/stage experience policy to a known player in one
    /// authoritative transition. Event sources such as weapons, spells, quests, or monsters are
    /// intentionally separate from this arithmetic and client delivery remains a host concern.
    pub fn award_player_experience(
        &mut self,
        player_id: u64,
        raw_experience: u64,
        policy: &ExperienceAwardPolicy,
    ) -> Result<PlayerExperienceAwardOutcome, CoreError> {
        self.award_player_experience_with_vocation_gains(
            player_id,
            raw_experience,
            policy,
            VocationLevelUpGains::default(),
        )
    }
}
