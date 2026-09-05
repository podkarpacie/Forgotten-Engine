//! Death, respawn, and town assignment on the authoritative world state: town lookup and
//! replacement, respawn state hydration, death processing with town-based temple selection,
//! fixed-percent death loss, and the respawn primitive that restores vitals and position.

use super::*;

impl WorldState {
    /// Returns the persisted town identifier selected for a known player. Zero represents an
    /// unassigned town and cannot later resolve a temple position.
    pub fn player_town(&self, player_id: u64) -> Result<u32, CoreError> {
        self.player_towns
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    /// Replaces a known player's authoritative town assignment. Imported-map validation is
    /// intentionally deferred until a death transition resolves a concrete temple position.
    pub fn replace_player_town(&mut self, player_id: u64, town_id: u32) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_town(player_id)? == town_id {
            return Ok(false);
        }
        self.player_towns.insert(player_id, town_id);
        self.mark_changed();
        Ok(true)
    }

    pub fn player_respawn_state(&self, player_id: u64) -> Result<PlayerRespawnState, CoreError> {
        self.player_respawn_states
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    /// Hydrates a persisted lifecycle record after the player has entered the authoritative
    /// world. A living player must use the exact empty lifecycle state; a dead player must retain
    /// both its previously validated temple position and deterministic death tick. Packet delivery
    /// and automatic timing remain separate host responsibilities.
    pub fn hydrate_player_respawn_state(
        &mut self,
        player_id: u64,
        state: PlayerRespawnState,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        let valid = if state.dead {
            state.respawn_at.is_some() && state.death_time.is_some()
        } else {
            state == PlayerRespawnState::default()
        };
        if !valid {
            return Err(CoreError::InvalidPlayerRespawnState(player_id));
        }
        if self.player_respawn_state(player_id)? == state {
            return Ok(false);
        }
        self.player_respawn_states.insert(player_id, state);
        self.mark_changed();
        Ok(true)
    }

    /// Records a death against a known imported town temple without performing the future
    /// respawn teleport, loss-policy execution, persistence, or client death-screen delivery.
    /// Repeating a death for an already-dead player is a no-op that returns the original state.
    pub fn apply_player_death(
        &mut self,
        player_id: u64,
        town_id: u32,
        world_map: &WorldMap,
    ) -> Result<PlayerRespawnState, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        let existing = self.player_respawn_state(player_id)?;
        if existing.dead {
            return Ok(existing);
        }
        let respawn_at = world_map
            .temple_position_for_town(town_id)
            .ok_or(CoreError::UnknownTown(town_id))?;
        let vitals = self
            .player_vitals
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        vitals.health = 0;
        self.player_conditions
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?
            .clear();
        let state = PlayerRespawnState {
            dead: true,
            respawn_at: Some(respawn_at),
            death_time: Some(self.tick),
            loss_applied: false,
        };
        self.player_respawn_states.insert(player_id, state);
        self.clear_player_interaction_references(player_id);
        self.mark_changed();
        Ok(state)
    }

    /// Applies one bounded fixed-percent loss transition to a player with an accepted death state.
    /// The caller must supply data-driven vocation rules. Default-formula behavior, promotion and
    /// blessing reductions, persistence, and client packets remain outside this core operation.
    pub fn apply_fixed_percent_death_loss(
        &mut self,
        player_id: u64,
        percent: u8,
        rules: PlayerProgressionRules,
    ) -> Result<PlayerDeathLossOutcome, CoreError> {
        if !(1..=100).contains(&percent) {
            return Err(CoreError::InvalidFixedDeathLossPercent(percent));
        }
        let mut death_state = self.player_respawn_state(player_id)?;
        if !death_state.dead {
            return Err(CoreError::PlayerIsNotDead(player_id));
        }
        if death_state.loss_applied {
            return Err(CoreError::DeathLossAlreadyApplied(player_id));
        }
        let mut player = self
            .players
            .get(&player_id)
            .cloned()
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        let mut progression = self.player_progression(player_id)?;
        let mut attempts = self.player_progression_attempts(player_id)?;
        let mut vitals = self.player_vitals(player_id)?;

        let experience_lost = fixed_percent_of(player.experience, percent);
        player.experience = player.experience.saturating_sub(experience_lost);
        player.level = level_for_experience(player.experience);

        let mut skill_tries_lost = [0_u64; 7];
        for skill in PlayerSkill::ALL {
            let index = skill.code() as usize;
            let total_tries = cumulative_skill_tries(
                progression.skills.skill(skill),
                attempts.skill_tries[index],
                rules,
                skill,
            );
            let lost_tries = fixed_percent_of(total_tries, percent);
            let (progress, stored_tries) =
                skill_progress_from_total(total_tries.saturating_sub(lost_tries), rules, skill);
            progression.skills.set(skill, progress);
            attempts.skill_tries[index] = stored_tries;
            skill_tries_lost[index] = lost_tries;
        }

        let total_mana = cumulative_magic_mana(vitals.magic_level, attempts.magic_mana, rules);
        let magic_mana_lost = fixed_percent_of(total_mana, percent);
        let (magic_level, stored_mana) =
            magic_progress_from_total(total_mana.saturating_sub(magic_mana_lost), rules);
        vitals.magic_level = magic_level;
        attempts.magic_mana = stored_mana;
        let level = player.level;

        death_state.loss_applied = true;
        self.players.insert(player_id, player);
        self.player_progressions.insert(player_id, progression);
        self.player_progression_attempts.insert(player_id, attempts);
        self.player_vitals.insert(player_id, vitals);
        self.player_respawn_states.insert(player_id, death_state);
        self.mark_changed();
        Ok(PlayerDeathLossOutcome {
            player_id,
            percent,
            experience_lost,
            skill_tries_lost,
            magic_mana_lost,
            level,
            progression,
            progression_attempts: attempts,
            vitals,
        })
    }

    /// Restores a player who has an accepted death state at its already-validated temple. This
    /// transition intentionally does not calculate loss, write persistence, or emit client death
    /// and respawn packets. A blocked temple is rejected without altering the death state.
    pub fn respawn_player(&mut self, player_id: u64) -> Result<PlayerRespawnOutcome, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        let state = self.player_respawn_state(player_id)?;
        if !state.dead {
            return Err(CoreError::PlayerIsNotDead(player_id));
        }
        let position = state
            .respawn_at
            .ok_or(CoreError::MissingRespawnPosition(player_id))?;
        if self.is_static_creature_occupied(position) {
            return Err(CoreError::StaticCreatureOccupiesPosition(position));
        }
        if self
            .players
            .iter()
            .any(|(id, player)| *id != player_id && player.position == position)
        {
            return Err(CoreError::PlayerOccupiesPosition(position));
        }
        let vitals = {
            let vitals = self
                .player_vitals
                .get_mut(&player_id)
                .ok_or(CoreError::UnknownPlayer(player_id))?;
            vitals.health = vitals.max_health;
            vitals.mana = vitals.max_mana;
            *vitals
        };
        self.players
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?
            .position = position;
        self.player_respawn_states
            .insert(player_id, PlayerRespawnState::default());
        self.mark_changed();
        Ok(PlayerRespawnOutcome {
            player_id,
            position,
            vitals,
        })
    }
}
