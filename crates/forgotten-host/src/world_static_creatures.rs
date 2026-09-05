//! Static-creature combat and lifecycle on the shared world: target acquisition/pursuit,
//! heartbeat attacks, melee damage application, defeat loot rolls, runtime snapshots and
//! restoration, and target stepping. All methods hold the shared lock and persist through
//! the caller-provided database where the contract requires it.

use super::*;

impl SharedNativeWorld {
    pub fn active_static_spawns(&self) -> Result<FeTfsStaticSpawnCollection, HostError> {
        Ok(self.lock()?.active_static_spawn_collection())
    }

    /// Applies an explicitly selected, bounded target-acquisition pass to active static creatures.
    /// It records only the core target ID. It does not schedule movement, pursue a target, attack,
    /// send a packet, or change native visibility because target state is not yet client-rendered.
    pub fn acquire_static_creature_targets(
        &self,
        policy: StaticTargetAcquisitionPolicy,
    ) -> Result<StaticTargetAcquisitionSummary, HostError> {
        let StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range } = policy else {
            return Ok(StaticTargetAcquisitionSummary::default());
        };
        if !(1..=forgotten_core::MAX_STATIC_CREATURE_TARGET_RANGE).contains(&max_range) {
            return Err(HostError::Core(
                forgotten_core::CoreError::InvalidStaticCreatureTargetRange(max_range),
            ));
        }
        let mut world = self.lock()?;
        let creature_ids = world
            .active_static_spawn_collection()
            .entities
            .into_iter()
            .map(|entity| entity.id)
            .collect::<Vec<_>>();
        let mut summary = StaticTargetAcquisitionSummary {
            examined_static_creatures: creature_ids.len(),
            changed_static_targets: 0,
        };
        for creature_id in creature_ids {
            let previous = world
                .static_creature_target(creature_id)
                .map_err(HostError::Core)?;
            let selected = world
                .select_static_creature_target(creature_id, max_range)
                .map_err(HostError::Core)?;
            if previous != selected.target_player_id {
                summary.changed_static_targets += 1;
            }
        }
        Ok(summary)
    }

    /// Applies one explicit bounded pursue pass. For every active static creature, it chooses the
    /// nearest living player within the provided range and attempts at most one existing legal
    /// cardinal target step. It does not install a scheduler, retry blocked paths, attack, emit a
    /// target packet, or otherwise implement general creature AI.
    pub fn pursue_static_creature_targets_once(
        &self,
        world_map: &WorldMap,
        policy: StaticTargetPursuitPolicy,
    ) -> Result<StaticTargetPursuitSummary, HostError> {
        let StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range } = policy else {
            return Ok(StaticTargetPursuitSummary::default());
        };
        if !(1..=forgotten_core::MAX_STATIC_CREATURE_TARGET_RANGE).contains(&max_range) {
            return Err(HostError::Core(
                forgotten_core::CoreError::InvalidStaticCreatureTargetRange(max_range),
            ));
        }
        let mut world = self.lock()?;
        let creature_ids = world
            .active_static_spawn_collection()
            .entities
            .into_iter()
            .map(|entity| entity.id)
            .collect::<Vec<_>>();
        let mut summary = StaticTargetPursuitSummary {
            examined_static_creatures: creature_ids.len(),
            changed_static_targets: 0,
            moved_static_creatures: 0,
        };
        for creature_id in creature_ids {
            let previous = world
                .static_creature_target(creature_id)
                .map_err(HostError::Core)?;
            let selected = world
                .select_static_creature_target(creature_id, max_range)
                .map_err(HostError::Core)?;
            if previous != selected.target_player_id {
                summary.changed_static_targets += 1;
            }
            if matches!(
                world
                    .step_static_creature_toward_target_with_detour(
                        creature_id,
                        world_map,
                        max_range,
                    )
                    .map_err(HostError::Core)?,
                StaticCreatureTargetStepOutcome::Moved { .. }
            ) {
                summary.moved_static_creatures += 1;
            }
        }
        drop(world);
        if summary.moved_static_creatures > 0 {
            self.mark_visibility_changed();
        }
        Ok(summary)
    }

    /// Runs one deterministic player-follow pass under the authoritative world lock. It does not
    /// pathfind, retry blocked routes, attack, emit a target packet, or alter follow selection.
    pub fn follow_player_targets_once(
        &self,
        world_map: &WorldMap,
    ) -> Result<BTreeSet<u64>, HostError> {
        let moved_player_ids = self
            .lock()?
            .follow_player_targets_once(world_map)
            .map_err(HostError::Core)?;
        if !moved_player_ids.is_empty() {
            self.mark_visibility_changed();
        }
        Ok(moved_player_ids)
    }

    /// Applies one explicit bounded static target-attack pass under the shared-world lock. It
    /// does not select targets, install timing beyond the caller, persist state, emit packets, or
    /// claim formulas, loot, corpses, scripts, or general creature AI.
    pub fn attack_static_creature_targets_once(
        &self,
        policy: StaticTargetAttackPolicy,
        world_map: &WorldMap,
    ) -> Result<StaticTargetAttackSummary, HostError> {
        if matches!(policy, StaticTargetAttackPolicy::Disabled) {
            return Ok(StaticTargetAttackSummary::default());
        }
        let fixed_damage = match policy {
            StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage } => Some(damage),
            _ => None,
        };
        if let Some(damage) = fixed_damage {
            if !(1..=100).contains(&damage) {
                return Err(HostError::InvalidConfiguration(
                    "static target attack damage must be between 1 and 100".into(),
                ));
            }
        }
        let mut world = self.lock()?;
        let creature_ids = world
            .active_static_spawn_collection()
            .entities
            .into_iter()
            .map(|entity| entity.id)
            .collect::<Vec<_>>();
        let mut summary = StaticTargetAttackSummary {
            examined_static_creatures: creature_ids.len(),
            ..StaticTargetAttackSummary::default()
        };
        for creature_id in creature_ids {
            // Declared-melee policy (plan v49 slice 5): creatures whose definition declares no
            // bounded direct-melee range stay outside the attack pass entirely, so the core
            // fixed-damage fallback can never fire for them.
            if fixed_damage.is_none() && !world.static_creature_declares_direct_melee(creature_id) {
                continue;
            }
            let requested_damage = fixed_damage.unwrap_or_else(|| {
                world
                    .static_creature_declared_damage_for_next_hit(creature_id)
                    .unwrap_or(0)
            });
            if requested_damage == 0 {
                continue;
            }
            let outcome = world
                .apply_static_creature_target_damage(creature_id, requested_damage, world_map)
                .map_err(HostError::Core)?;
            match outcome {
                StaticCreatureTargetAttackOutcome::CooldownNotDue { .. } => {
                    summary.cooldown_skipped_attacks += 1;
                }
                StaticCreatureTargetAttackOutcome::Applied {
                    target_player_id,
                    applied_damage,
                    ..
                } => {
                    if applied_damage > 0 {
                        summary.applied_attacks += 1;
                        summary.total_applied_damage += u64::from(applied_damage);
                        summary.affected_player_ids.insert(target_player_id);
                    }
                }
                StaticCreatureTargetAttackOutcome::NoTarget
                | StaticCreatureTargetAttackOutcome::TargetNotAdjacent { .. } => {}
            }
        }
        drop(world);
        if summary.applied_attacks > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(summary)
    }

    pub fn set_static_creature_health_percent(
        &self,
        creature_id: u32,
        health_percent: u8,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .set_static_creature_health_percent(creature_id, health_percent)
            .map_err(HostError::Core)?;
        if changed {
            self.mark_visibility_changed();
        }
        Ok(changed)
    }

    pub fn static_creature_experience_reward(&self, creature_id: u32) -> Result<u64, HostError> {
        self.lock()?
            .static_creature_experience_reward(creature_id)
            .map_err(HostError::Core)
    }

    /// Rolls one deterministic loot result for an active static creature under the shared-world
    /// lock. The seed is caller-supplied so equal defeat ticks always produce equal corpses.
    pub fn roll_static_creature_loot(
        &self,
        creature_id: u32,
        seed: u64,
    ) -> Result<forgotten_core::StaticCreatureLootRoll, HostError> {
        self.lock()?
            .roll_static_creature_loot(creature_id, seed)
            .map_err(HostError::Core)
    }

    /// Rolls one deterministic loot result for one authoritatively defeated static monster under
    /// the shared-world lock. This stays valid for the deactivated post-defeat creature state.
    pub fn roll_defeated_static_creature_loot(
        &self,
        creature_id: u32,
        seed: u64,
    ) -> Result<forgotten_core::StaticCreatureLootRoll, HostError> {
        self.lock()?
            .roll_defeated_static_creature_loot(creature_id, seed)
            .map_err(HostError::Core)
    }

    pub fn static_creature_runtime_snapshot(
        &self,
    ) -> Result<Vec<StaticCreatureRuntimeSnapshot>, HostError> {
        Ok(self.lock()?.static_creature_runtime_snapshot())
    }

    pub fn restore_static_creature_runtime(
        &self,
        records: &[StaticCreatureRuntimeSnapshot],
    ) -> Result<StaticCreatureRuntimeRestoreSummary, HostError> {
        let summary = self
            .lock()?
            .restore_static_creature_runtime(records)
            .map_err(HostError::Core)?;
        if summary.restored > 0 {
            self.mark_visibility_changed();
        }
        Ok(summary)
    }

    pub fn apply_static_creature_melee_damage(
        &self,
        attacker_id: u64,
        target_id: u32,
        requested_damage: u16,
    ) -> Result<StaticCreatureDamageOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_static_creature_melee_damage(attacker_id, target_id, requested_damage)
            .map_err(HostError::Core)?;
        if outcome.applied_damage > 0 {
            self.mark_visibility_changed();
        }
        Ok(outcome)
    }

    /// Exposes one explicit core-only static target attack under the shared world lock. A real
    /// player-vitals mutation advances the existing refresh epoch, but scheduling, persistence,
    /// native delivery, formulas, loot, corpses, scripts, and creature AI remain caller-owned
    /// deferred concerns.
    pub fn apply_static_creature_target_damage(
        &self,
        creature_id: u32,
        requested_damage: u16,
        world_map: &WorldMap,
    ) -> Result<StaticCreatureTargetAttackOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_static_creature_target_damage(creature_id, requested_damage, world_map)
            .map_err(HostError::Core)?;
        if matches!(
            outcome,
            StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 1..,
                ..
            }
        ) {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    /// Applies one caller-triggered bounded target step. Only a real movement increments the
    /// shared visibility epoch; target acquisition, scheduling, AI, combat, and packets remain
    /// outside this state transition.
    pub fn step_static_creature_toward_target(
        &self,
        creature_id: u32,
        world_map: &WorldMap,
    ) -> Result<StaticCreatureTargetStepOutcome, HostError> {
        let outcome = self
            .lock()?
            .step_static_creature_toward_target(creature_id, world_map)
            .map_err(HostError::Core)?;
        if matches!(outcome, StaticCreatureTargetStepOutcome::Moved { .. }) {
            self.mark_visibility_changed();
        }
        Ok(outcome)
    }
}
