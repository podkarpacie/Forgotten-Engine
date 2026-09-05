//! Combat damage and spell-cast processing on the authoritative world state: damage
//! application to known targets, combat event execution (attack cooldowns, defense
//! mitigation, health subtraction), spell cast events (mana debit, magic-level advancement),
//! and direct melee damage resolution.

use super::*;

impl WorldState {
    pub fn apply_player_damage(
        &mut self,
        attacker_id: u64,
        target_id: u64,
        requested_damage: u16,
    ) -> Result<PlayerDamageOutcome, CoreError> {
        if attacker_id == target_id {
            return Err(CoreError::SelfInteractionNotAllowed(attacker_id));
        }
        if self.player_respawn_state(attacker_id)?.dead {
            return Err(CoreError::PlayerIsDead(attacker_id));
        }
        if !self.players.contains_key(&target_id) {
            return Err(CoreError::UnknownPlayer(target_id));
        }
        let (applied_damage, remaining_health) =
            self.apply_damage_to_known_target(target_id, requested_damage)?;
        if applied_damage > 0 {
            self.mark_changed();
        }
        Ok(PlayerDamageOutcome {
            attacker_id,
            target_id,
            requested_damage,
            applied_damage,
            remaining_health,
            defeated: remaining_health == 0,
        })
    }

    /// Resolves one typed, bounded event using the authoritative server tick. Only explicit
    /// adjacent melee is accepted now; formulas, weapons, spells, resistances, effects, PvP
    /// policy, and client delivery must be added through their own tested event variants.
    pub fn apply_player_combat_event(
        &mut self,
        event: PlayerCombatEvent,
    ) -> Result<PlayerCombatEventOutcome, CoreError> {
        if event.attacker_id == event.target_id {
            return Err(CoreError::SelfInteractionNotAllowed(event.attacker_id));
        }
        if event.damage_type != CombatDamageType::Physical {
            return Err(CoreError::InvalidCombatEvent);
        }
        if self.player_respawn_state(event.attacker_id)?.dead {
            return Err(CoreError::PlayerIsDead(event.attacker_id));
        }
        let attacker = self
            .player(event.attacker_id)
            .ok_or(CoreError::UnknownPlayer(event.attacker_id))?;
        let target = self
            .player(event.target_id)
            .ok_or(CoreError::UnknownPlayer(event.target_id))?;
        if matches!(event.delivery, CombatDelivery::AdjacentMelee)
            && !attacker.position.is_adjacent_to(target.position)
        {
            return Err(CoreError::CombatOutOfRange {
                attacker_id: event.attacker_id,
                target_id: event.target_id,
            });
        }
        if let CombatDelivery::RangedDistance { max_range } = event.delivery {
            if attacker.position.z != target.position.z {
                return Err(CoreError::CombatOutOfRange {
                    attacker_id: event.attacker_id,
                    target_id: event.target_id,
                });
            }
            let x_distance = i32::from(attacker.position.x) - i32::from(target.position.x);
            let y_distance = i32::from(attacker.position.y) - i32::from(target.position.y);
            let chebyshev = x_distance.abs().max(y_distance.abs());
            // Distance shots may include the shooter's own tile ring but never point-blank
            // adjacency exclusions: zero means the same tile, which classic servers reject.
            if chebyshev == 0 || chebyshev > i32::from(max_range) {
                return Err(CoreError::CombatOutOfRange {
                    attacker_id: event.attacker_id,
                    target_id: event.target_id,
                });
            }
        }
        if self.player_vitals(event.target_id)?.health == 0 {
            return Err(CoreError::TargetAlreadyDefeated(event.target_id));
        }
        // /god and /invisible players are unattackable by other players (plan v49 slice 18).
        if self.player_god_mode.contains(&event.target_id)
            || self.player_invisible.contains(&event.target_id)
        {
            return Err(CoreError::InvalidCombatEvent);
        }
        let cooldown = self.player_combat_cooldown(event.attacker_id)?;
        if self.tick < cooldown.next_attack_tick {
            return Err(CoreError::CombatCooldownActive {
                attacker_id: event.attacker_id,
                current_tick: self.tick,
                next_attack_tick: cooldown.next_attack_tick,
            });
        }
        let defense = self.player_combat_defense(event.target_id)?;
        let mitigated_damage = defense.mitigate_physical(event.requested_damage);
        let (applied_damage, remaining_health) =
            self.apply_damage_to_known_target(event.target_id, mitigated_damage)?;
        let next_attack_tick = self
            .tick
            .saturating_add(u64::from(event.timing.interval_ticks));
        self.player_combat_cooldowns
            .insert(event.attacker_id, PlayerCombatCooldown { next_attack_tick });
        self.mark_changed();
        Ok(PlayerCombatEventOutcome {
            damage: PlayerDamageOutcome {
                attacker_id: event.attacker_id,
                target_id: event.target_id,
                requested_damage: event.requested_damage,
                applied_damage,
                remaining_health,
                defeated: remaining_health == 0,
            },
            damage_type: event.damage_type,
            mitigated_damage,
            next_attack_tick,
        })
    }

    /// Applies only the resource and timing portion of a typed spell cast. No target resolution,
    /// combat damage, healing, projectile, visual effect, script, PvP, profile packet, or legacy
    /// spell behavior is implied by a successful result.
    pub fn apply_player_spell_cast_event(
        &mut self,
        event: PlayerSpellCastEvent,
    ) -> Result<PlayerSpellCastOutcome, CoreError> {
        if event.spell_id == 0
            || event.mana_cost == 0
            || event.mana_cost > MAX_SPELL_MANA_COST
            || event.timing.interval_ticks == 0
            || event.timing.interval_ticks > MAX_COMBAT_INTERVAL_TICKS
        {
            return Err(CoreError::InvalidSpellCastEvent);
        }
        if self.player_respawn_state(event.caster_id)?.dead {
            return Err(CoreError::PlayerIsDead(event.caster_id));
        }
        let cooldown = self.player_spell_cooldown(event.caster_id)?;
        if self.tick < cooldown.next_cast_tick {
            return Err(CoreError::SpellCooldownActive {
                caster_id: event.caster_id,
                current_tick: self.tick,
                next_cast_tick: cooldown.next_cast_tick,
            });
        }
        let remaining_mana = {
            let vitals = self
                .player_vitals
                .get_mut(&event.caster_id)
                .ok_or(CoreError::UnknownPlayer(event.caster_id))?;
            if vitals.mana < event.mana_cost {
                return Err(CoreError::InsufficientMana {
                    player_id: event.caster_id,
                    required_mana: event.mana_cost,
                    available_mana: vitals.mana,
                });
            }
            vitals.mana -= event.mana_cost;
            vitals.mana
        };
        let next_cast_tick = self
            .tick
            .saturating_add(u64::from(event.timing.interval_ticks));
        self.player_spell_cooldowns
            .insert(event.caster_id, PlayerSpellCooldown { next_cast_tick });
        self.mark_changed();
        Ok(PlayerSpellCastOutcome {
            caster_id: event.caster_id,
            spell_id: event.spell_id,
            mana_spent: event.mana_cost,
            remaining_mana,
            next_cast_tick,
        })
    }

    pub fn apply_damage_to_known_target(
        &mut self,
        target_id: u64,
        requested_damage: u16,
    ) -> Result<(u16, u16), CoreError> {
        // /god players are invincible: every damage source funnels through here.
        if self.player_god_mode.contains(&target_id) {
            let health = self
                .player_vitals
                .get(&target_id)
                .map(|vitals| vitals.health)
                .unwrap_or(0);
            return Ok((0, health));
        }
        let vitals = self
            .player_vitals
            .get_mut(&target_id)
            .ok_or(CoreError::UnknownPlayer(target_id))?;
        let applied_damage = requested_damage.min(vitals.health);
        vitals.health = vitals.health.saturating_sub(applied_damage);
        Ok((applied_damage, vitals.health))
    }

    pub fn apply_player_melee_damage(
        &mut self,
        attacker_id: u64,
        target_id: u64,
        requested_damage: u16,
    ) -> Result<PlayerDamageOutcome, CoreError> {
        let attacker = self
            .player(attacker_id)
            .ok_or(CoreError::UnknownPlayer(attacker_id))?;
        let target = self
            .player(target_id)
            .ok_or(CoreError::UnknownPlayer(target_id))?;
        if !attacker.position.is_adjacent_to(target.position) {
            return Err(CoreError::CombatOutOfRange {
                attacker_id,
                target_id,
            });
        }
        self.apply_player_damage(attacker_id, target_id, requested_damage)
    }
}
