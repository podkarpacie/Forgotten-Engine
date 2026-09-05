//! Player vitals, god mode, invisibility, food windows, and regeneration scheduling
//! on the authoritative world state. These methods manage the per-player lifecycle
//! attributes that persist between relogs.

use super::*;

impl WorldState {
    pub fn update_player_vitals(
        &mut self,
        player_id: u64,
        vitals: PlayerVitals,
    ) -> Result<(), CoreError> {
        if !vitals.is_valid() {
            return Err(CoreError::InvalidPlayerVitals(player_id));
        }
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_vitals(player_id)? == vitals {
            return Ok(());
        }
        self.player_vitals.insert(player_id, vitals);
        self.mark_changed();
        Ok(())
    }

    /// Grants (or refuses) one classic food window (plan v49 slice 16). While a window is
    /// active the player answers "You are full." and the item is not consumed; otherwise the
    /// window starts now and lasts the declared seconds.
    pub fn grant_player_food_window(
        &mut self,
        player_id: u64,
        seconds: u16,
    ) -> Result<bool, CoreError> {
        if seconds == 0 {
            return Err(CoreError::InvalidRegenerationInterval);
        }
        self.player(player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        let now = self.tick;
        match self.player_food_windows.get(&player_id) {
            Some(window) if window.until_tick > now => Ok(false),
            _ => {
                self.player_food_windows.insert(
                    player_id,
                    PlayerFoodWindow {
                        until_tick: now.saturating_add(u64::from(seconds)),
                        elapsed_seconds: 0,
                    },
                );
                self.mark_changed();
                Ok(true)
            }
        }
    }

    /// Toggles the operator /god flag (invincible, creature-untargetable). Returns the new state.
    pub fn set_player_god_mode(
        &mut self,
        player_id: u64,
        enabled: bool,
    ) -> Result<bool, CoreError> {
        self.player(player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        if enabled {
            self.player_god_mode.insert(player_id);
        } else {
            self.player_god_mode.remove(&player_id);
        }
        self.mark_changed();
        Ok(enabled)
    }

    pub fn player_is_in_god_mode(&self, player_id: u64) -> bool {
        self.player_god_mode.contains(&player_id)
    }

    /// Toggles the operator /invisible flag (hidden from creatures and all other players).
    /// Returns the new state.
    pub fn set_player_invisible(
        &mut self,
        player_id: u64,
        enabled: bool,
    ) -> Result<bool, CoreError> {
        self.player(player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        if enabled {
            self.player_invisible.insert(player_id);
        } else {
            self.player_invisible.remove(&player_id);
        }
        self.mark_changed();
        Ok(enabled)
    }

    pub fn player_is_invisible(&self, player_id: u64) -> bool {
        self.player_invisible.contains(&player_id)
    }

    /// Ids of every currently invisible player, for render-side viewport filtering.
    pub fn invisible_player_ids(&self) -> Vec<u64> {
        self.player_invisible.iter().copied().collect()
    }

    pub fn player_food_window_remaining_ticks(&self, player_id: u64) -> Option<u64> {
        let window = self.player_food_windows.get(&player_id)?;
        window
            .until_tick
            .checked_sub(self.tick)
            .filter(|remaining| *remaining > 0)
    }

    /// Applies a bounded elapsed period to one player's configured health/mana schedules. The
    /// caller owns wall-clock measurement; this method admits only a capped duration and mutates
    /// authoritative vitals when a configured recovery event produces a positive gain.
    pub fn apply_player_regeneration(
        &mut self,
        player_id: u64,
        rules: PlayerRegenerationRules,
        elapsed_seconds: u16,
    ) -> Result<PlayerRegenerationOutcome, CoreError> {
        let current_vitals = self.player_vitals(player_id)?;
        if self.player_respawn_state(player_id)?.dead {
            return Ok(PlayerRegenerationOutcome {
                player_id,
                health_gained: 0,
                mana_gained: 0,
                vitals: current_vitals,
            });
        }
        if elapsed_seconds == 0 {
            return Ok(PlayerRegenerationOutcome {
                player_id,
                health_gained: 0,
                mana_gained: 0,
                vitals: current_vitals,
            });
        }
        let elapsed_seconds = elapsed_seconds.min(MAX_REGENERATION_ELAPSED_SECONDS);
        let schedule = self
            .player_regeneration_schedules
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        let health_events = advance_regeneration_schedule(
            &mut schedule.health_elapsed_seconds,
            rules.health.interval_seconds,
            elapsed_seconds,
        );
        let mana_events = advance_regeneration_schedule(
            &mut schedule.mana_elapsed_seconds,
            rules.mana.interval_seconds,
            elapsed_seconds,
        );
        let health_gained = regeneration_gain(
            current_vitals.health,
            current_vitals.max_health,
            rules.health.amount,
            health_events,
        );
        let mana_gained = regeneration_gain(
            current_vitals.mana,
            current_vitals.max_mana,
            rules.mana.amount,
            mana_events,
        );
        let vitals = PlayerVitals {
            health: current_vitals.health.saturating_add(health_gained),
            mana: current_vitals.mana.saturating_add(mana_gained),
            ..current_vitals
        };
        // Classic fed state (plan v49 slice 16): while the food window lasts, every configured
        // interval restores one health point. The window drains in wall-clock time regardless.
        let mut food_gained = 0_u16;
        let mut final_health = vitals.health;
        if let Some(window) = self.player_food_windows.get_mut(&player_id) {
            if window.until_tick > self.tick {
                window.elapsed_seconds = window.elapsed_seconds.saturating_add(elapsed_seconds);
                let intervals =
                    (window.elapsed_seconds / FOOD_REGENERATION_INTERVAL_SECONDS) as u16;
                if intervals > 0 {
                    window.elapsed_seconds %= FOOD_REGENERATION_INTERVAL_SECONDS;
                    food_gained = (FOOD_REGENERATION_HEALTH_PER_INTERVAL)
                        .saturating_mul(intervals)
                        .min(vitals.max_health.saturating_sub(final_health));
                    final_health = final_health.saturating_add(food_gained);
                }
            }
        }
        let vitals = PlayerVitals {
            health: final_health,
            ..vitals
        };
        if health_gained > 0 || mana_gained > 0 || food_gained > 0 {
            self.player_vitals.insert(player_id, vitals);
            self.mark_changed();
        }
        Ok(PlayerRegenerationOutcome {
            player_id,
            health_gained: health_gained + food_gained,
            mana_gained,
            vitals,
        })
    }

    /// Replaces player vocation and all typed skills atomically in the authoritative world. No-op
    /// replacements do not advance the world revision, matching vitals/equipment semantics.
    pub fn replace_player_progression(
        &mut self,
        player_id: u64,
        progression: PlayerProgression,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_progression(player_id)? == progression {
            return Ok(false);
        }
        self.player_progressions.insert(player_id, progression);
        self.mark_changed();
        Ok(true)
    }

    /// Applies a nonnegative exact skill-try award using caller-supplied validated vocation rules.
    /// This foundation deliberately has no weapon, combat, offline-training, or Lua event source.
    pub fn apply_player_skill_tries(
        &mut self,
        player_id: u64,
        skill: PlayerSkill,
        awarded_tries: u64,
        rules: PlayerProgressionRules,
    ) -> Result<PlayerSkillTryOutcome, CoreError> {
        let mut progression = self.player_progression(player_id)?;
        let mut attempts = self.player_progression_attempts(player_id)?;
        let progress = progression.skills.skill(skill);
        let (progress, stored_tries, gained_levels) = advance_skill_tries(
            progress,
            attempts.skill_tries[skill.code() as usize],
            awarded_tries,
            rules,
            skill,
        );
        progression.skills.set(skill, progress);
        attempts.skill_tries[skill.code() as usize] = stored_tries;
        let changed = progression != self.player_progression(player_id)?
            || attempts != self.player_progression_attempts(player_id)?;
        if changed {
            self.player_progressions.insert(player_id, progression);
            self.player_progression_attempts.insert(player_id, attempts);
            self.mark_changed();
        }
        Ok(PlayerSkillTryOutcome {
            player_id,
            skill,
            gained_levels,
            progress,
            stored_tries,
        })
    }

    /// Applies spent mana to magic-level advancement using caller-supplied validated vocation
    /// rules. It does not consume a player's current mana pool and has no spell integration.
    pub fn apply_player_magic_mana(
        &mut self,
        player_id: u64,
        awarded_mana: u64,
        rules: PlayerProgressionRules,
    ) -> Result<PlayerMagicAdvanceOutcome, CoreError> {
        let mut attempts = self.player_progression_attempts(player_id)?;
        let mut vitals = self.player_vitals(player_id)?;
        let (magic_level, stored_mana, gained_levels) =
            advance_magic_mana(vitals.magic_level, attempts.magic_mana, awarded_mana, rules);
        vitals.magic_level = magic_level;
        attempts.magic_mana = stored_mana;
        let changed = vitals != self.player_vitals(player_id)?
            || attempts != self.player_progression_attempts(player_id)?;
        if changed {
            self.player_vitals.insert(player_id, vitals);
            self.player_progression_attempts.insert(player_id, attempts);
            self.mark_changed();
        }
        Ok(PlayerMagicAdvanceOutcome {
            player_id,
            gained_levels,
            magic_level,
            stored_mana,
        })
    }
}
