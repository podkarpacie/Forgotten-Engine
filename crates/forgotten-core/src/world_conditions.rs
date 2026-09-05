//! Condition (status effect) management on the authoritative world state: damage-over-time
//! conditions (poison, burning, energy) and the timed speed modifier (haste). Conditions
//! are stored per-player with bounded durations; their tick processing, death interaction,
//! and persistence are handled by the tick methods and the host layer respectively.

use super::*;

impl WorldState {
    pub fn player_conditions(
        &self,
        player_id: u64,
    ) -> Result<&BTreeMap<PlayerConditionKind, PlayerCondition>, CoreError> {
        self.player_conditions
            .get(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    /// Replaces a known player's complete bounded condition set. Persistence and client effects
    /// remain separate layers, but native-session hydration can use this atomic state transfer.
    pub fn replace_player_conditions(
        &mut self,
        player_id: u64,
        conditions: BTreeMap<PlayerConditionKind, PlayerCondition>,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_conditions(player_id)? == &conditions {
            return Ok(false);
        }
        self.player_conditions.insert(player_id, conditions);
        self.mark_changed();
        Ok(true)
    }

    /// Applies or replaces a single condition kind. Replacing a condition never creates an
    /// unbounded stack and is observable through the authoritative world revision.
    pub fn apply_player_condition(
        &mut self,
        player_id: u64,
        condition: PlayerCondition,
    ) -> Result<bool, CoreError> {
        let conditions = self
            .player_conditions
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        if conditions.get(&condition.kind) == Some(&condition) {
            return Ok(false);
        }
        conditions.insert(condition.kind, condition);
        self.mark_changed();
        Ok(true)
    }

    /// Advances condition schedules by bounded elapsed time. Damage is capped by current health;
    /// zero health is represented in the outcome but death/respawn policy remains deferred.
    pub fn apply_player_conditions(
        &mut self,
        player_id: u64,
        elapsed_seconds: u16,
    ) -> Result<PlayerConditionOutcome, CoreError> {
        let current_vitals = self.player_vitals(player_id)?;
        if self.player_respawn_state(player_id)?.dead {
            return Ok(PlayerConditionOutcome {
                player_id,
                applied_damage: 0,
                remaining_health: current_vitals.health,
                expired_conditions: 0,
            });
        }
        let elapsed_seconds = elapsed_seconds.min(MAX_REGENERATION_ELAPSED_SECONDS);
        let requested_damage = self.pending_player_condition_damage(player_id, elapsed_seconds)?;
        let conditions = self
            .player_conditions
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        if elapsed_seconds == 0 || conditions.is_empty() {
            return Ok(PlayerConditionOutcome {
                player_id,
                applied_damage: 0,
                remaining_health: current_vitals.health,
                expired_conditions: 0,
            });
        }
        let mut expired = Vec::new();
        for (kind, condition) in conditions.iter_mut() {
            let active_seconds = elapsed_seconds.min(condition.remaining_seconds);
            let total = condition.elapsed_seconds.saturating_add(active_seconds);
            condition.elapsed_seconds = total % condition.interval_seconds;
            condition.remaining_seconds =
                condition.remaining_seconds.saturating_sub(active_seconds);
            if condition.remaining_seconds == 0 {
                expired.push(*kind);
            }
        }
        for kind in &expired {
            conditions.remove(kind);
        }
        let applied_damage = requested_damage;
        let remaining_health = current_vitals.health.saturating_sub(applied_damage);
        if applied_damage > 0 {
            self.player_vitals.insert(
                player_id,
                PlayerVitals {
                    health: remaining_health,
                    ..current_vitals
                },
            );
        }
        if applied_damage > 0 || !expired.is_empty() {
            self.mark_changed();
        }
        Ok(PlayerConditionOutcome {
            player_id,
            applied_damage,
            remaining_health,
            expired_conditions: expired.len().min(usize::from(u8::MAX)) as u8,
        })
    }

    /// Applies a bounded condition heartbeat and, only when it is lethal, enters the already
    /// established authoritative death state. Town and temple validity are checked before any
    /// schedule or vitality mutation, allowing callers to retry after correcting configuration.
    /// Client effects, death screens, timers, and respawn packets remain outside this core path.
    pub fn apply_player_conditions_with_death(
        &mut self,
        player_id: u64,
        town_id: u32,
        world_map: &WorldMap,
        elapsed_seconds: u16,
    ) -> Result<(PlayerConditionOutcome, Option<PlayerRespawnState>), CoreError> {
        let vitals = self.player_vitals(player_id)?;
        if self.player_respawn_state(player_id)?.dead {
            return Ok((
                PlayerConditionOutcome {
                    player_id,
                    applied_damage: 0,
                    remaining_health: vitals.health,
                    expired_conditions: 0,
                },
                None,
            ));
        }
        let elapsed_seconds = elapsed_seconds.min(MAX_REGENERATION_ELAPSED_SECONDS);
        let pending_damage = self.pending_player_condition_damage(player_id, elapsed_seconds)?;
        if pending_damage > 0 && pending_damage >= vitals.health {
            if town_id == 0 {
                return Err(CoreError::PlayerTownUnassigned(player_id));
            }
            if world_map.temple_position_for_town(town_id).is_none() {
                return Err(CoreError::UnknownTown(town_id));
            }
        }
        let outcome = self.apply_player_conditions(player_id, elapsed_seconds)?;
        let death_state = (outcome.applied_damage > 0 && outcome.remaining_health == 0)
            .then(|| self.apply_player_death(player_id, town_id, world_map))
            .transpose()?;
        Ok((outcome, death_state))
    }

    pub(crate) fn pending_player_condition_damage(
        &self,
        player_id: u64,
        elapsed_seconds: u16,
    ) -> Result<u16, CoreError> {
        let current_vitals = self.player_vitals(player_id)?;
        let conditions = self.player_conditions(player_id)?;
        let requested_damage = conditions.values().fold(0_u32, |total_damage, condition| {
            // Speed conditions carry no damage; their tick only expires remaining_seconds.
            if !condition.kind.is_damage_over_time() {
                return total_damage;
            }
            let active_seconds = elapsed_seconds.min(condition.remaining_seconds);
            let elapsed = condition.elapsed_seconds.saturating_add(active_seconds);
            let events = elapsed / condition.interval_seconds;
            total_damage
                .saturating_add(u32::from(events).saturating_mul(u32::from(condition.damage)))
        });
        Ok(requested_damage.min(u32::from(current_vitals.health)) as u16)
    }

    /// Active haste modifier for one player, in additive percent (0 when none). Expired entries
    /// are pruned by the condition tick, so a stale row never lingers in the reported value.
    pub fn player_speed_bonus_percent(&self, player_id: u64) -> u16 {
        self.player_conditions
            .get(&player_id)
            .and_then(|conditions| conditions.get(&PlayerConditionKind::Haste))
            .map(|condition| condition.speed_bonus_percent)
            .unwrap_or(0)
    }

    /// Applies or refreshes the bounded haste speed condition. Non-stacking: a re-application
    /// overwrites the previous modifier and duration. Persists through the regular condition set.
    pub fn apply_player_speed_condition(
        &mut self,
        player_id: u64,
        speed_bonus_percent: u16,
        remaining_seconds: u16,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        let condition = PlayerCondition::new_haste(speed_bonus_percent, remaining_seconds)?;
        self.player_conditions
            .entry(player_id)
            .or_default()
            .insert(PlayerConditionKind::Haste, condition);
        self.mark_changed();
        Ok(true)
    }
}
