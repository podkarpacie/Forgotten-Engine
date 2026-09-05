//! Static-creature lifecycle, targeting, combat, movement, and respawn on the
//! authoritative world state. All methods operate on the shared world's private
//! static-creature registry and occupancy tracking.

use super::*;

impl WorldState {
    pub fn install_static_creatures(
        &mut self,
        collection: &FeTfsStaticSpawnCollection,
    ) -> Result<(), CoreError> {
        if collection.entities.len() > MAX_TFS_STATIC_SPAWNS {
            return Err(CoreError::StaticSpawnLimit(MAX_TFS_STATIC_SPAWNS));
        }
        let mut creatures = BTreeMap::new();
        for entity in &collection.entities {
            if entity.name.trim().is_empty() {
                return Err(CoreError::EmptyStaticSpawnName);
            }
            if creatures
                .insert(
                    entity.id,
                    StaticCreatureRuntime {
                        entity: entity.clone(),
                        is_npc: collection.is_npc(entity.id),
                        experience_reward: collection.experience_reward(entity.id),
                        loot: collection.loot_table(entity.id).to_vec(),
                        spawn_position: entity.position,
                        monster_spawn_area: collection.monster_spawn_area(entity.id),
                        active: true,
                        health_percent: entity.health_percent,
                        activated_at_tick: self.tick,
                        inactive_since_tick: None,
                        reactivation_due_tick: None,
                        respawn_interval_seconds: collection.respawn_interval_seconds(entity.id),
                        melee_cooldown_ticks: collection
                            .direct_melee_interval_millis(entity.id)
                            .map(|interval| u64::from(interval).div_ceil(1_000)),
                        next_melee_due_tick: self.tick,
                        direct_melee_damage_range: collection.direct_melee_damage_range(entity.id),
                        direct_melee_damage_sequence: 0,
                        target_player_id: None,
                    },
                )
                .is_some()
            {
                return Err(CoreError::DuplicateStaticSpawnId(entity.id));
            }
            if self
                .players
                .values()
                .any(|player| player.position == entity.position)
            {
                return Err(CoreError::PlayerOccupiesStaticCreaturePosition(
                    entity.position,
                ));
            }
        }
        self.static_creatures = creatures;
        self.refresh_static_creature_occupancy();
        self.mark_changed();
        Ok(())
    }

    pub fn static_creature_count(&self) -> usize {
        self.static_creatures.len()
    }

    /// Summons one operator-requested entity in front of a player. The entity clones the
    /// appearance, health, experience, melee, and loot metadata of an installed static creature
    /// with a case-insensitive matching name; its durable ID is allocated above the imported
    /// range so restart snapshots and persistence never collide. The target tile must be
    /// walkable and free of players and active creatures.
    pub fn spawn_dynamic_entity(
        &mut self,
        name: &str,
        position: Position,
        direction: u8,
    ) -> Result<u32, CoreError> {
        const DYNAMIC_ID_BASE: u32 = 0x7000_0000;
        if self.static_creatures.len() >= MAX_TFS_STATIC_SPAWNS {
            return Err(CoreError::StaticSpawnLimit(MAX_TFS_STATIC_SPAWNS));
        }
        let lowered = name.trim().to_lowercase();
        if lowered.is_empty() {
            return Err(CoreError::EmptyStaticSpawnName);
        }
        let template = self
            .static_creatures
            .values()
            .find(|runtime| runtime.entity.name.to_lowercase() == lowered)
            .ok_or_else(|| CoreError::UnknownEntityName(name.trim().to_string()))?
            .clone();

        // Tile acceptance: prefer the requested tile, then the 8 surrounding tiles, so a
        // blocked primary spot never traps the summoner against their own summon.
        let offsets: [(i32, i32); 9] = [
            (0, 0),
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
        ];
        let mut spawn_position: Option<Position> = None;
        for (dx, dy) in offsets {
            let candidate = Position {
                x: (position.x as i32 + dx).max(0) as u16,
                y: (position.y as i32 + dy).max(0) as u16,
                z: position.z,
            };
            if self
                .players
                .values()
                .any(|player| player.position == candidate)
                || self
                    .static_creatures
                    .values()
                    .any(|runtime| runtime.active && runtime.entity.position == candidate)
            {
                continue;
            }
            spawn_position = Some(candidate);
            break;
        }
        let Some(spawn_position) = spawn_position else {
            return Err(CoreError::SpawnPositionRejected(position));
        };
        let position = spawn_position;
        let next_slot = (DYNAMIC_ID_BASE..)
            .find(|candidate| !self.static_creatures.contains_key(candidate))
            .ok_or(CoreError::DynamicSpawnLimit(MAX_TFS_STATIC_SPAWNS))?;
        let mut entity = template.entity.clone();
        entity.id = next_slot;
        entity.position = position;
        entity.direction = direction;
        let runtime = StaticCreatureRuntime {
            entity,
            is_npc: false,
            experience_reward: template.experience_reward,
            loot: template.loot,
            spawn_position: position,
            // Dynamic summons have no import spawn area: they despawn on deactivation instead
            // of reactivating at an imported pad.
            monster_spawn_area: None,
            active: true,
            health_percent: template.health_percent,
            activated_at_tick: self.tick,
            inactive_since_tick: None,
            reactivation_due_tick: None,
            respawn_interval_seconds: 0,
            melee_cooldown_ticks: template.melee_cooldown_ticks,
            next_melee_due_tick: self.tick,
            direct_melee_damage_range: template.direct_melee_damage_range,
            direct_melee_damage_sequence: 0,
            target_player_id: None,
        };
        self.static_creatures.insert(next_slot, runtime);
        self.refresh_static_creature_occupancy();
        self.mark_changed();
        Ok(next_slot)
    }

    /// Removes one dynamic operator summon. Imported entities are protected: only IDs from the
    /// dynamic range can be despawned.
    pub fn despawn_dynamic_entity(&mut self, id: u32) -> Result<(), CoreError> {
        const DYNAMIC_ID_BASE: u32 = 0x7000_0000;
        if id < DYNAMIC_ID_BASE {
            return Err(CoreError::UnknownStaticCreature(id));
        }
        if self.static_creatures.remove(&id).is_none() {
            return Err(CoreError::UnknownStaticCreature(id));
        }
        self.refresh_static_creature_occupancy();
        self.mark_changed();
        Ok(())
    }

    /// Lists every dynamic summon as (id, name, position) records.
    /// Records an unjustified player kill against the killer (classic frag). Returns the
    /// killer's new total. Called from lethal PvP transitions; monster and condition deaths
    /// never route through here.
    pub fn record_player_frag(&mut self, killer_id: u64) -> u32 {
        let frags = self.player_frags.entry(killer_id).or_insert(0);
        *frags = frags.saturating_add(1);
        let total = *frags;
        self.mark_changed();
        total
    }

    /// Reads one player's current unjustified-kill count.
    pub fn player_frag_count(&self, player_id: u64) -> u32 {
        self.player_frags.get(&player_id).copied().unwrap_or(0)
    }

    /// Classic skull classification: white skull at one or more unjustified kills.
    pub fn player_has_white_skull(&self, player_id: u64) -> bool {
        self.player_frag_count(player_id) > 0
    }

    pub fn dynamic_spawn_records(&self) -> Vec<(u32, String, Position)> {
        const DYNAMIC_ID_BASE: u32 = 0x7000_0000;
        self.static_creatures
            .iter()
            .filter(|(id, _)| **id >= DYNAMIC_ID_BASE)
            .map(|(id, runtime)| (*id, runtime.entity.name.clone(), runtime.entity.position))
            .collect()
    }

    pub fn static_creature(&self, id: u32) -> Option<&FeTfsStaticEntity> {
        self.static_creatures
            .get(&id)
            .map(|runtime| &runtime.entity)
    }

    pub fn static_creature_lifecycle(&self, id: u32) -> Option<StaticCreatureLifecycle> {
        self.static_creatures
            .get(&id)
            .map(|runtime| StaticCreatureLifecycle {
                id,
                spawn_position: runtime.spawn_position,
                position: runtime.entity.position,
                active: runtime.active,
                health_percent: runtime.health_percent,
                activated_at_tick: runtime.activated_at_tick,
                inactive_since_tick: runtime.inactive_since_tick,
                reactivation_due_tick: runtime.reactivation_due_tick,
                respawn_interval_seconds: runtime.respawn_interval_seconds,
            })
    }

    /// Returns an ordered, bounded restart snapshot for every installed static creature.
    pub fn static_creature_runtime_snapshot(&self) -> Vec<StaticCreatureRuntimeSnapshot> {
        self.static_creatures
            .iter()
            .map(|(id, runtime)| StaticCreatureRuntimeSnapshot {
                id: *id,
                position: runtime.entity.position,
                active: runtime.active,
                health_percent: runtime.health_percent,
                reactivation_remaining_seconds: runtime.reactivation_due_tick.map(|due_tick| {
                    u32::try_from(due_tick.saturating_sub(self.tick)).unwrap_or(u32::MAX)
                }),
                direct_melee_cooldown_remaining_ticks: runtime.melee_cooldown_ticks.map(|_| {
                    u32::try_from(runtime.next_melee_due_tick.saturating_sub(self.tick))
                        .unwrap_or(u32::MAX)
                }),
                direct_melee_damage_sequence: runtime.direct_melee_damage_sequence,
            })
            .collect()
    }

    /// Restores runtime state for matching installed static spawn IDs after a restart. Unknown
    /// records are ignored to make map/catalog upgrades safe. Target state and timing metadata
    /// are explicitly non-durable and are cleared/reset. This method validates all matching
    /// records and their prospective occupancy before it changes any authoritative state.
    pub fn restore_static_creature_runtime(
        &mut self,
        records: &[StaticCreatureRuntimeSnapshot],
    ) -> Result<StaticCreatureRuntimeRestoreSummary, CoreError> {
        let mut seen = BTreeSet::new();
        let mut known_records = Vec::new();
        let mut ignored_unknown = 0;
        for record in records {
            if !seen.insert(record.id) {
                return Err(CoreError::DuplicateStaticSpawnId(record.id));
            }
            if record.health_percent > 100 {
                return Err(CoreError::InvalidStaticCreatureHealthPercent(
                    record.health_percent,
                ));
            }
            if let Some(runtime) = self.static_creatures.get(&record.id) {
                if record.active && record.reactivation_remaining_seconds.is_some() {
                    return Err(CoreError::InvalidStaticCreatureReactivationDelay {
                        id: record.id,
                        remaining_seconds: record.reactivation_remaining_seconds.unwrap_or(0),
                        interval_seconds: runtime.respawn_interval_seconds,
                    });
                }
                if let Some(remaining_seconds) = record.reactivation_remaining_seconds {
                    if runtime.respawn_interval_seconds == 0
                        || remaining_seconds > runtime.respawn_interval_seconds
                    {
                        return Err(CoreError::InvalidStaticCreatureReactivationDelay {
                            id: record.id,
                            remaining_seconds,
                            interval_seconds: runtime.respawn_interval_seconds,
                        });
                    }
                }
                if record.direct_melee_cooldown_remaining_ticks.is_some()
                    && (!record.active || runtime.melee_cooldown_ticks.is_none())
                {
                    return Err(CoreError::InvalidStaticCreatureMeleeCooldownDelay {
                        id: record.id,
                        remaining_ticks: record.direct_melee_cooldown_remaining_ticks.unwrap_or(0),
                        cooldown_ticks: runtime.melee_cooldown_ticks.unwrap_or(0),
                    });
                }
                if let (Some(remaining_ticks), Some(cooldown_ticks)) = (
                    record.direct_melee_cooldown_remaining_ticks,
                    runtime.melee_cooldown_ticks,
                ) {
                    if u64::from(remaining_ticks) > cooldown_ticks {
                        return Err(CoreError::InvalidStaticCreatureMeleeCooldownDelay {
                            id: record.id,
                            remaining_ticks,
                            cooldown_ticks,
                        });
                    }
                }
                known_records.push(*record);
            } else {
                ignored_unknown += 1;
            }
        }

        let restored_ids = known_records
            .iter()
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();
        let mut occupied = self
            .static_creatures
            .iter()
            .filter(|(id, runtime)| runtime.active && !restored_ids.contains(id))
            .map(|(_, runtime)| runtime.entity.position)
            .collect::<BTreeSet<_>>();
        for record in &known_records {
            if !record.active {
                continue;
            }
            if self.is_player_occupied(record.position) {
                return Err(CoreError::PlayerOccupiesStaticCreaturePosition(
                    record.position,
                ));
            }
            if !occupied.insert(record.position) {
                return Err(CoreError::StaticCreatureOccupiesPosition(record.position));
            }
        }

        let mut changed = false;
        for record in known_records {
            // Skip records whose creature vanished from the runtime map; the next full
            // reconciliation pass re-installs or prunes them without crashing the tick.
            let Some(runtime) = self.static_creatures.get_mut(&record.id) else {
                continue;
            };
            if runtime.entity.position != record.position
                || runtime.active != record.active
                || runtime.health_percent != record.health_percent
                || runtime.target_player_id.is_some()
                || runtime.reactivation_due_tick
                    != record
                        .reactivation_remaining_seconds
                        .map(|remaining| self.tick.saturating_add(u64::from(remaining)))
                || runtime.next_melee_due_tick
                    != record
                        .direct_melee_cooldown_remaining_ticks
                        .map_or(self.tick, |remaining| {
                            self.tick.saturating_add(u64::from(remaining))
                        })
                || runtime.direct_melee_damage_sequence != record.direct_melee_damage_sequence
            {
                changed = true;
            }
            runtime.entity.position = record.position;
            runtime.active = record.active;
            runtime.health_percent = record.health_percent;
            runtime.target_player_id = None;
            runtime.direct_melee_damage_sequence = record.direct_melee_damage_sequence;
            runtime.next_melee_due_tick = record
                .direct_melee_cooldown_remaining_ticks
                .map_or(self.tick, |remaining| {
                    self.tick.saturating_add(u64::from(remaining))
                });
            if record.active {
                runtime.activated_at_tick = self.tick;
                runtime.inactive_since_tick = None;
                runtime.reactivation_due_tick = None;
            } else {
                runtime.inactive_since_tick = Some(self.tick);
                runtime.reactivation_due_tick = record
                    .reactivation_remaining_seconds
                    .map(|remaining| self.tick.saturating_add(u64::from(remaining)));
            }
        }
        if changed {
            self.refresh_static_creature_occupancy();
            self.player_interactions.retain(|_, intent| {
                intent.target_static_creature_id.map_or(true, |id| {
                    self.static_creatures
                        .get(&id)
                        .is_some_and(|runtime| runtime.active)
                })
            });
            self.mark_changed();
        }
        Ok(StaticCreatureRuntimeRestoreSummary {
            restored: restored_ids.len(),
            ignored_unknown,
        })
    }

    pub fn active_static_creature_count(&self) -> usize {
        self.static_creatures
            .values()
            .filter(|runtime| runtime.active)
            .count()
    }

    pub fn static_creature_health_percent(&self, id: u32) -> Result<u8, CoreError> {
        self.static_creatures
            .get(&id)
            .map(|runtime| runtime.health_percent)
            .ok_or(CoreError::UnknownStaticCreature(id))
    }

    pub fn static_creature_experience_reward(&self, id: u32) -> Result<u64, CoreError> {
        self.static_creatures
            .get(&id)
            .map(|runtime| runtime.experience_reward)
            .ok_or(CoreError::UnknownStaticCreature(id))
    }

    /// Rolls one deterministic bounded loot result for an active static monster. The caller
    /// supplies the seed (for example the authoritative defeat tick); equal seeds always produce
    /// equal results. NPCs, inactive creatures, and empty loot tables yield no items.
    pub fn roll_static_creature_loot(
        &self,
        id: u32,
        seed: u64,
    ) -> Result<StaticCreatureLootRoll, CoreError> {
        let runtime = self
            .static_creatures
            .get(&id)
            .ok_or(CoreError::UnknownStaticCreature(id))?;
        if !runtime.active {
            return Ok(StaticCreatureLootRoll {
                creature_id: id,
                items: Vec::new(),
            });
        }
        self.roll_defeated_static_creature_loot(id, seed)
    }

    /// Rolls one deterministic bounded loot result for one authoritatively defeated static
    /// monster. Unlike `roll_static_creature_loot`, this transition stays valid for the
    /// deactivated creature in its immediate post-defeat state, which is the only moment a
    /// corpse can be populated. Unknown ids, NPCs, and empty loot tables yield no items.
    pub fn roll_defeated_static_creature_loot(
        &self,
        id: u32,
        seed: u64,
    ) -> Result<StaticCreatureLootRoll, CoreError> {
        let runtime = self
            .static_creatures
            .get(&id)
            .ok_or(CoreError::UnknownStaticCreature(id))?;
        if runtime.is_npc || runtime.loot.is_empty() {
            return Ok(StaticCreatureLootRoll {
                creature_id: id,
                items: Vec::new(),
            });
        }
        let mut items = Vec::new();
        let mut state = seed
            ^ (u64::from(id) << 32)
            ^ u64::from(runtime.entity.position.x)
            ^ (u64::from(runtime.entity.position.y) << 16);
        for entry in &runtime.loot {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let roll = (state >> 33) % u64::from(LOOT_CHANCE_SCALE.max(1));
            if roll < u64::from(entry.chance.min(LOOT_CHANCE_SCALE)) {
                let span = u64::from(entry.max_count) - u64::from(entry.min_count) + 1;
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let count = u64::from(entry.min_count) + (state >> 33) % span;
                items.push((
                    entry.item_id,
                    u16::try_from(count).unwrap_or(entry.max_count),
                ));
            }
        }
        Ok(StaticCreatureLootRoll {
            creature_id: id,
            items,
        })
    }

    /// Changes one active static creature's display health only. This bounded state is not
    /// connected to damage, death, targeting consequences, loot, corpses, AI, or scripts.
    /// A zero value remains a valid display percentage and does not deactivate the creature.
    pub fn set_static_creature_health_percent(
        &mut self,
        id: u32,
        health_percent: u8,
    ) -> Result<bool, CoreError> {
        if health_percent > 100 {
            return Err(CoreError::InvalidStaticCreatureHealthPercent(
                health_percent,
            ));
        }
        let runtime = self
            .static_creatures
            .get_mut(&id)
            .ok_or(CoreError::UnknownStaticCreature(id))?;
        if !runtime.active {
            return Err(CoreError::InactiveStaticCreature(id));
        }
        if runtime.health_percent == health_percent {
            return Ok(false);
        }
        runtime.health_percent = health_percent;
        self.mark_changed();
        Ok(true)
    }

    /// Returns only active authoritative entities for a protocol viewport. This derives a
    /// temporary immutable collection rather than exposing runtime mutation through the codec.
    pub fn active_static_spawn_collection(&self) -> FeTfsStaticSpawnCollection {
        FeTfsStaticSpawnCollection {
            entities: self
                .static_creatures
                .values()
                .filter(|runtime| runtime.active)
                .map(|runtime| {
                    let mut entity = runtime.entity.clone();
                    entity.health_percent = runtime.health_percent;
                    entity
                })
                .collect(),
            respawn_intervals_seconds: BTreeMap::new(),
            experience_rewards: self
                .static_creatures
                .iter()
                .filter(|(_, runtime)| runtime.active && runtime.experience_reward > 0)
                .map(|(id, runtime)| (*id, runtime.experience_reward))
                .collect(),
            direct_melee_intervals_millis: BTreeMap::new(),
            direct_melee_damage_ranges: self
                .static_creatures
                .iter()
                .filter_map(|(id, runtime)| {
                    runtime
                        .active
                        .then_some(runtime.direct_melee_damage_range.map(|range| (*id, range)))
                        .flatten()
                })
                .collect(),
            loot_tables: self
                .static_creatures
                .iter()
                .filter(|(_, runtime)| runtime.active && !runtime.loot.is_empty())
                .map(|(id, runtime)| (*id, runtime.loot.clone()))
                .collect(),
            npc_ids: self
                .static_creatures
                .iter()
                .filter(|(_, runtime)| runtime.active && runtime.is_npc)
                .map(|(id, _)| *id)
                .collect(),
            monster_spawn_areas: BTreeMap::new(),
        }
    }

    /// Selects the nearest living registered player on the same floor within a bounded Chebyshev
    /// range. Equal-distance candidates resolve by stable player ID. This updates typed target
    /// state only; movement, follow behavior, pathfinding, combat, scripts, and packet delivery
    /// remain separate systems.
    pub fn select_static_creature_target(
        &mut self,
        creature_id: u32,
        max_range: u8,
    ) -> Result<StaticCreatureTargetSelection, CoreError> {
        if max_range == 0 || max_range > MAX_STATIC_CREATURE_TARGET_RANGE {
            return Err(CoreError::InvalidStaticCreatureTargetRange(max_range));
        }
        let (position, is_npc) = {
            let runtime = self
                .static_creatures
                .get(&creature_id)
                .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
            if !runtime.active {
                return Err(CoreError::InactiveStaticCreature(creature_id));
            }
            (runtime.entity.position, runtime.is_npc)
        };
        if is_npc {
            self.clear_static_creature_target(creature_id)?;
            return Ok(StaticCreatureTargetSelection {
                creature_id,
                target_player_id: None,
                max_range,
            });
        }
        let max_range_distance = u16::from(max_range);
        let target_player_id = self
            .players
            .iter()
            // Plan v49: /god and /invisible players are never targeted by creatures.
            .filter(|(player_id, player)| {
                player.position.z == position.z
                    && !self.player_god_mode.contains(player_id)
                    && !self.player_invisible.contains(player_id)
                    && self
                        .player_respawn_states
                        .get(player_id)
                        .map_or(true, |state| !state.dead)
            })
            .filter_map(|(player_id, player)| {
                let distance = player
                    .position
                    .x
                    .abs_diff(position.x)
                    .max(player.position.y.abs_diff(position.y));
                (distance <= max_range_distance).then_some((distance, *player_id))
            })
            .min()
            .map(|(_, player_id)| player_id);
        let changed = {
            let runtime = self
                .static_creatures
                .get_mut(&creature_id)
                .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
            if runtime.target_player_id == target_player_id {
                false
            } else {
                runtime.target_player_id = target_player_id;
                true
            }
        };
        if changed {
            self.mark_changed();
        }
        Ok(StaticCreatureTargetSelection {
            creature_id,
            target_player_id,
            max_range,
        })
    }

    pub fn static_creature_target(&self, creature_id: u32) -> Result<Option<u64>, CoreError> {
        let runtime = self
            .static_creatures
            .get(&creature_id)
            .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
        if !runtime.active {
            return Err(CoreError::InactiveStaticCreature(creature_id));
        }
        Ok(runtime.target_player_id)
    }

    fn clear_static_creature_target(&mut self, creature_id: u32) -> Result<(), CoreError> {
        let runtime = self
            .static_creatures
            .get_mut(&creature_id)
            .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
        if runtime.target_player_id.take().is_some() {
            self.mark_changed();
        }
        Ok(())
    }

    /// Applies one bounded fixed damage event from an active static creature to its already
    /// selected living player target. The target must remain adjacent. A potentially lethal hit
    /// validates the assigned town temple before health changes, then reuses the established
    /// authoritative death state. This does not schedule attacks or imply creature-AI parity.
    /// Peeks the next declared direct-melee damage value for a static creature without
    /// advancing its deterministic cycling sequence; the consuming transition remains
    /// `apply_static_creature_target_damage`. `None` when no range is declared.
    pub fn static_creature_declared_damage_for_next_hit(&self, id: u32) -> Option<u16> {
        let runtime = self.static_creatures.get(&id)?;
        let range = runtime.direct_melee_damage_range?;
        let span = u64::from(range.max_damage) - u64::from(range.min_damage) + 1;
        let offset = runtime.direct_melee_damage_sequence % span;
        u16::try_from(u64::from(range.min_damage) + offset).ok()
    }

    pub fn apply_static_creature_target_damage(
        &mut self,
        creature_id: u32,
        fallback_damage: u16,
        world_map: &WorldMap,
    ) -> Result<StaticCreatureTargetAttackOutcome, CoreError> {
        let (source, target_player_id, melee_cooldown_ticks, direct_melee_damage_range, is_npc) = {
            let runtime = self
                .static_creatures
                .get(&creature_id)
                .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
            if !runtime.active {
                return Err(CoreError::InactiveStaticCreature(creature_id));
            }
            if self.tick < runtime.next_melee_due_tick {
                return Ok(StaticCreatureTargetAttackOutcome::CooldownNotDue {
                    creature_id,
                    due_tick: runtime.next_melee_due_tick,
                });
            }
            (
                runtime.entity.position,
                runtime.target_player_id,
                runtime.melee_cooldown_ticks,
                runtime.direct_melee_damage_range,
                runtime.is_npc,
            )
        };
        if is_npc {
            self.clear_static_creature_target(creature_id)?;
            return Ok(StaticCreatureTargetAttackOutcome::NoTarget);
        }
        let Some(target_player_id) = target_player_id else {
            return Ok(StaticCreatureTargetAttackOutcome::NoTarget);
        };
        let Some(target) = self.players.get(&target_player_id).cloned() else {
            self.clear_static_creature_target(creature_id)?;
            return Ok(StaticCreatureTargetAttackOutcome::NoTarget);
        };
        if self
            .player_respawn_states
            .get(&target_player_id)
            .is_some_and(|state| state.dead)
        {
            self.clear_static_creature_target(creature_id)?;
            return Ok(StaticCreatureTargetAttackOutcome::NoTarget);
        }
        if !source.is_adjacent_to(target.position) {
            return Ok(StaticCreatureTargetAttackOutcome::TargetNotAdjacent {
                creature_id,
                target_player_id,
            });
        }
        let requested_damage = match (
            direct_melee_damage_range,
            self.static_creatures.get_mut(&creature_id),
        ) {
            (Some(range), Some(runtime)) => {
                let span = u64::from(range.max_damage) - u64::from(range.min_damage) + 1;
                let offset = runtime.direct_melee_damage_sequence % span;
                runtime.direct_melee_damage_sequence =
                    runtime.direct_melee_damage_sequence.saturating_add(1);
                // The validated range is u16-bounded; clamp defensively instead of
                // panicking if a future config source ever violates that bound.
                u16::try_from(u64::from(range.min_damage) + offset).unwrap_or(range.max_damage)
            }
            _ => fallback_damage,
        };
        let current_health = self.player_vitals(target_player_id)?.health;
        let mitigated_damage = self
            .player_combat_defense(target_player_id)?
            .mitigate_physical(requested_damage);
        let potentially_lethal = current_health > 0 && mitigated_damage >= current_health;
        let town_id = if potentially_lethal {
            let town_id = self.player_town(target_player_id)?;
            world_map
                .temple_position_for_town(town_id)
                .ok_or(CoreError::UnknownTown(town_id))?;
            Some(town_id)
        } else {
            None
        };
        let (applied_damage, remaining_health) =
            self.apply_damage_to_known_target(target_player_id, mitigated_damage)?;
        let death_state = town_id
            .filter(|_| remaining_health == 0 && applied_damage > 0)
            .map(|town_id| self.apply_player_death(target_player_id, town_id, world_map))
            .transpose()?;
        if death_state.is_some() {
            self.clear_static_creature_target(creature_id)?;
        }
        if applied_damage > 0 {
            self.mark_changed();
        }
        if let Some(cooldown_ticks) = melee_cooldown_ticks {
            // The creature was validated above; a missing entry just skips the cooldown write.
            if let Some(runtime) = self.static_creatures.get_mut(&creature_id) {
                runtime.next_melee_due_tick = self.tick.saturating_add(cooldown_ticks);
            }
        }
        Ok(StaticCreatureTargetAttackOutcome::Applied {
            creature_id,
            target_player_id,
            requested_damage,
            applied_damage,
            remaining_health,
            death_state,
        })
    }

    /// Attempts at most one deterministic distance-reducing cardinal step toward an already
    /// selected living target. It does not acquire targets, retry later, move diagonally, route
    /// around an obstacle, attack, or create autonomous creature behavior.
    pub fn step_static_creature_toward_target(
        &mut self,
        creature_id: u32,
        world_map: &WorldMap,
    ) -> Result<StaticCreatureTargetStepOutcome, CoreError> {
        self.step_static_creature_toward_target_with_detour(creature_id, world_map, 0)
    }

    /// Attempts the established direct target step first, then may choose the first deterministic
    /// cardinal detour found within the supplied path-length budget. It performs at most one
    /// authoritative move and does not add retry scheduling, diagonal movement, combat, or AI.
    pub fn step_static_creature_toward_target_with_detour(
        &mut self,
        creature_id: u32,
        world_map: &WorldMap,
        max_detour_steps: u8,
    ) -> Result<StaticCreatureTargetStepOutcome, CoreError> {
        let (source, target_player_id, is_npc) = {
            let runtime = self
                .static_creatures
                .get(&creature_id)
                .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
            if !runtime.active {
                return Err(CoreError::InactiveStaticCreature(creature_id));
            }
            (
                runtime.entity.position,
                runtime.target_player_id,
                runtime.is_npc,
            )
        };
        if is_npc {
            self.clear_static_creature_target(creature_id)?;
            return Ok(StaticCreatureTargetStepOutcome::NoTarget);
        }
        let Some(target_player_id) = target_player_id else {
            return Ok(StaticCreatureTargetStepOutcome::NoTarget);
        };
        let target = self.players.get(&target_player_id).filter(|_| {
            self.player_respawn_states
                .get(&target_player_id)
                .map_or(true, |state| !state.dead)
        });
        let Some(target) = target else {
            let runtime = self
                .static_creatures
                .get_mut(&creature_id)
                .ok_or(CoreError::UnknownStaticCreature(creature_id))?;
            runtime.target_player_id = None;
            self.mark_changed();
            return Ok(StaticCreatureTargetStepOutcome::NoTarget);
        };
        if source.is_adjacent_to(target.position) {
            return Ok(StaticCreatureTargetStepOutcome::AlreadyAdjacent { target_player_id });
        }
        if source.z != target.position.z {
            return Ok(StaticCreatureTargetStepOutcome::Blocked { target_player_id });
        }
        let x_distance = source.x.abs_diff(target.position.x);
        let y_distance = source.y.abs_diff(target.position.y);
        let x_direction = match target.position.x.cmp(&source.x) {
            std::cmp::Ordering::Less => Some(CardinalDirection::West),
            std::cmp::Ordering::Greater => Some(CardinalDirection::East),
            std::cmp::Ordering::Equal => None,
        };
        let y_direction = match target.position.y.cmp(&source.y) {
            std::cmp::Ordering::Less => Some(CardinalDirection::North),
            std::cmp::Ordering::Greater => Some(CardinalDirection::South),
            std::cmp::Ordering::Equal => None,
        };
        let preferred = if x_distance >= y_distance {
            [x_direction, y_direction]
        } else {
            [y_direction, x_direction]
        };
        for direction in preferred.into_iter().flatten() {
            let destination = source.step(direction)?;
            if !world_map.is_walkable(destination)
                || self.is_player_occupied(destination)
                || self.static_creatures.iter().any(|(id, runtime)| {
                    *id != creature_id && runtime.active && runtime.entity.position == destination
                })
            {
                continue;
            }
            let (from, to) =
                self.move_static_creature_cardinal(creature_id, direction, world_map)?;
            return Ok(StaticCreatureTargetStepOutcome::Moved {
                target_player_id,
                direction,
                from,
                to,
            });
        }
        let Some(direction) = self.static_creature_detour_direction(
            source,
            target.position,
            world_map,
            max_detour_steps,
        ) else {
            return Ok(StaticCreatureTargetStepOutcome::Blocked { target_player_id });
        };
        let (from, to) = self.move_static_creature_cardinal(creature_id, direction, world_map)?;
        Ok(StaticCreatureTargetStepOutcome::Moved {
            target_player_id,
            direction,
            from,
            to,
        })
    }

    fn static_creature_detour_direction(
        &self,
        source: Position,
        target: Position,
        world_map: &WorldMap,
        max_detour_steps: u8,
    ) -> Option<CardinalDirection> {
        if max_detour_steps == 0 {
            return None;
        }
        let directions = [
            CardinalDirection::North,
            CardinalDirection::East,
            CardinalDirection::South,
            CardinalDirection::West,
        ];
        let mut occupied_static_positions = self.static_occupied_positions.clone();
        occupied_static_positions.remove(&source);
        let player_positions = self
            .players
            .values()
            .map(|player| player.position)
            .collect::<BTreeSet<_>>();
        let mut visited = BTreeSet::from([source]);
        let mut frontier = VecDeque::from([(source, None, 0_u8)]);
        while let Some((position, first_direction, distance)) = frontier.pop_front() {
            if distance >= max_detour_steps {
                continue;
            }
            for direction in directions {
                let Ok(destination) = position.step(direction) else {
                    continue;
                };
                if !visited.insert(destination)
                    || !world_map.is_walkable(destination)
                    || player_positions.contains(&destination)
                    || occupied_static_positions.contains(&destination)
                {
                    continue;
                }
                let first_direction = first_direction.or(Some(direction));
                if destination.is_adjacent_to(target) {
                    return first_direction;
                }
                frontier.push_back((destination, first_direction, distance + 1));
            }
        }
        None
    }

    /// Selects safe adjacent steps without mutating state. Direction preference rotates by the
    /// stable creature ID and current world tick, while creature IDs provide a deterministic
    /// serial order for contention. This is not target selection, AI, or pathfinding.
    pub fn plan_static_creature_moves(
        &self,
        policy: StaticCreatureDecisionPolicy,
        world_map: &WorldMap,
    ) -> StaticCreatureDecisionBatch {
        if policy == StaticCreatureDecisionPolicy::Disabled {
            return StaticCreatureDecisionBatch::default();
        }
        let directions = [
            CardinalDirection::North,
            CardinalDirection::East,
            CardinalDirection::South,
            CardinalDirection::West,
        ];
        let player_positions: BTreeSet<Position> = self
            .players
            .values()
            .map(|player| player.position)
            .collect();
        let mut occupied_positions = self.static_occupied_positions.clone();
        let mut batch = StaticCreatureDecisionBatch::default();
        for (id, runtime) in &self.static_creatures {
            if !runtime.active {
                continue;
            }
            let source = runtime.entity.position;
            occupied_positions.remove(&source);
            let direction_offset = ((*id as u64 + self.tick) % directions.len() as u64) as usize;
            let selected = (0..directions.len()).find_map(|step| {
                let direction = directions[(direction_offset + step) % directions.len()];
                let destination = source.step(direction).ok()?;
                (world_map.is_walkable(destination)
                    && !player_positions.contains(&destination)
                    && !occupied_positions.contains(&destination))
                .then_some((direction, destination))
            });
            if let Some((direction, destination)) = selected {
                occupied_positions.insert(destination);
                batch.decisions.push(StaticCreatureMoveDecision {
                    creature_id: *id,
                    direction,
                });
            } else {
                occupied_positions.insert(source);
                batch.skipped += 1;
            }
        }
        batch
    }

    /// Applies the complete current policy batch. The caller chooses when to invoke this method;
    /// this foundation adds no autonomous thread, interval, target selection, or combat behavior.
    pub fn apply_static_creature_policy(
        &mut self,
        policy: StaticCreatureDecisionPolicy,
        world_map: &WorldMap,
    ) -> Result<StaticCreatureDecisionBatch, CoreError> {
        let batch = self.plan_static_creature_moves(policy, world_map);
        for decision in &batch.decisions {
            self.move_static_creature_cardinal(
                decision.creature_id,
                decision.direction,
                world_map,
            )?;
        }
        Ok(batch)
    }

    /// Whether the installed static creature declares a bounded direct-melee damage range.
    /// Undeclared creatures stay outside declared-melee attack policies (plan v49 slice 5).
    pub fn static_creature_declares_direct_melee(&self, id: u32) -> bool {
        self.static_creatures
            .get(&id)
            .is_some_and(|runtime| runtime.direct_melee_damage_range.is_some())
    }

    /// Marks a static creature inactive without moving it. The deterministic world tick is
    /// recorded so an explicit due-reactivation caller can later evaluate its configured interval.
    pub fn deactivate_static_creature(&mut self, id: u32) -> Result<bool, CoreError> {
        let tick = self.tick;
        let changed = {
            let runtime = self
                .static_creatures
                .get_mut(&id)
                .ok_or(CoreError::UnknownStaticCreature(id))?;
            let changed = runtime.active;
            runtime.active = false;
            runtime.target_player_id = None;
            if changed {
                runtime.inactive_since_tick = Some(tick);
                runtime.reactivation_due_tick = (runtime.respawn_interval_seconds > 0)
                    .then(|| tick.saturating_add(u64::from(runtime.respawn_interval_seconds)));
            }
            changed
        };
        if changed {
            self.player_interactions.retain(|_, intent| {
                if intent.target_static_creature_id == Some(id) {
                    intent.target_static_creature_id = None;
                }
                intent.target_player_id.is_some()
                    || intent.target_static_creature_id.is_some()
                    || intent.follow_player_id.is_some()
            });
        }
        self.refresh_static_creature_occupancy();
        if changed {
            self.mark_changed();
        }
        Ok(changed)
    }

    /// Applies a bounded adjacent-melee transition to an active static creature. The requested
    /// value is capped at 100 percentage points. A real hit consumes the existing one-tick
    /// per-player combat cooldown. Only a hit that lowers positive health to zero deactivates the
    /// entity; externally assigned zero display health remains display-only.
    pub fn apply_static_creature_melee_damage(
        &mut self,
        attacker_id: u64,
        target_id: u32,
        requested_damage: u16,
    ) -> Result<StaticCreatureDamageOutcome, CoreError> {
        if requested_damage == 0 {
            return Err(CoreError::InvalidCombatEvent);
        }
        let attacker = self
            .player(attacker_id)
            .ok_or(CoreError::UnknownPlayer(attacker_id))?;
        let target = self
            .static_creatures
            .get(&target_id)
            .ok_or(CoreError::UnknownStaticCreature(target_id))?;
        if !target.active {
            return Err(CoreError::InactiveStaticCreature(target_id));
        }
        if target.is_npc {
            return Err(CoreError::StaticNpcNotAttackable(target_id));
        }
        if !attacker.position.is_adjacent_to(target.entity.position) {
            return Err(CoreError::StaticCreatureCombatOutOfRange {
                attacker_id,
                target_id,
            });
        }
        let cooldown = self.player_combat_cooldown(attacker_id)?;
        if self.tick < cooldown.next_attack_tick {
            return Err(CoreError::CombatCooldownActive {
                attacker_id,
                current_tick: self.tick,
                next_attack_tick: cooldown.next_attack_tick,
            });
        }
        let (applied_damage, remaining_health_percent, deactivated) = {
            let runtime = self
                .static_creatures
                .get_mut(&target_id)
                .ok_or(CoreError::UnknownStaticCreature(target_id))?;
            let applied_damage = runtime.health_percent.min(requested_damage.min(100) as u8);
            if applied_damage == 0 {
                (0, runtime.health_percent, false)
            } else {
                runtime.health_percent -= applied_damage;
                (
                    applied_damage,
                    runtime.health_percent,
                    runtime.health_percent == 0,
                )
            }
        };
        if applied_damage > 0 {
            self.player_combat_cooldowns.insert(
                attacker_id,
                PlayerCombatCooldown {
                    next_attack_tick: self.tick.saturating_add(1),
                },
            );
            if deactivated {
                self.deactivate_static_creature(target_id)?;
            } else {
                self.mark_changed();
            }
        }
        Ok(StaticCreatureDamageOutcome {
            attacker_id,
            target_id,
            requested_damage,
            applied_damage,
            remaining_health_percent,
            deactivated,
        })
    }

    /// Attempts a deterministic static-creature reset. An inactive creature whose spawn tile is
    /// occupied by a player is left inactive; no timer, teleport, combat, AI, or script behavior
    /// is performed.
    pub fn reset_static_creatures(&mut self) -> StaticCreatureResetSummary {
        let player_positions: BTreeSet<Position> = self
            .players
            .values()
            .map(|player| player.position)
            .collect();
        let mut summary = StaticCreatureResetSummary {
            reactivated: 0,
            deferred_by_player_occupancy: 0,
            deferred_by_static_creature_occupancy: 0,
        };
        let mut active_positions = self.static_occupied_positions.clone();
        for runtime in self.static_creatures.values_mut() {
            if runtime.active {
                continue;
            }
            if player_positions.contains(&runtime.spawn_position) {
                summary.deferred_by_player_occupancy += 1;
                continue;
            }
            if active_positions.contains(&runtime.spawn_position) {
                summary.deferred_by_static_creature_occupancy += 1;
                continue;
            }
            runtime.entity.position = runtime.spawn_position;
            runtime.health_percent = runtime.entity.health_percent;
            runtime.active = true;
            runtime.activated_at_tick = self.tick;
            runtime.inactive_since_tick = None;
            runtime.reactivation_due_tick = None;
            active_positions.insert(runtime.entity.position);
            summary.reactivated += 1;
        }
        self.refresh_static_creature_occupancy();
        if summary.reactivated > 0 {
            self.mark_changed();
        }
        summary
    }

    /// Reactivates only inactive static creatures whose nonzero per-spawn interval has elapsed
    /// on the deterministic world clock. A due attempt blocked by player or static occupancy
    /// re-arms one full configured interval before the next attempt. The caller owns clock
    /// advancement and invocation; this method does not create an autonomous scheduler, AI loop,
    /// combat path, or script behavior.
    pub fn reactivate_due_static_creatures(&mut self) -> StaticCreatureResetSummary {
        let player_positions: BTreeSet<Position> = self
            .players
            .values()
            .map(|player| player.position)
            .collect();
        let mut summary = StaticCreatureResetSummary {
            reactivated: 0,
            deferred_by_player_occupancy: 0,
            deferred_by_static_creature_occupancy: 0,
        };
        let mut active_positions = self.static_occupied_positions.clone();
        for runtime in self.static_creatures.values_mut() {
            if runtime.active {
                continue;
            }
            let Some(reactivation_due_tick) = runtime.reactivation_due_tick else {
                continue;
            };
            if self.tick < reactivation_due_tick {
                continue;
            }
            let player_blocks_spawn = player_positions.contains(&runtime.spawn_position)
                || runtime.monster_spawn_area.is_some_and(|area| {
                    player_positions
                        .iter()
                        .any(|position| area.contains(*position))
                });
            if player_blocks_spawn {
                summary.deferred_by_player_occupancy += 1;
                runtime.reactivation_due_tick = Some(
                    self.tick
                        .saturating_add(u64::from(runtime.respawn_interval_seconds)),
                );
                continue;
            }
            if active_positions.contains(&runtime.spawn_position) {
                summary.deferred_by_static_creature_occupancy += 1;
                runtime.reactivation_due_tick = Some(
                    self.tick
                        .saturating_add(u64::from(runtime.respawn_interval_seconds)),
                );
                continue;
            }
            runtime.entity.position = runtime.spawn_position;
            runtime.health_percent = runtime.entity.health_percent;
            runtime.active = true;
            runtime.activated_at_tick = self.tick;
            runtime.inactive_since_tick = None;
            runtime.reactivation_due_tick = None;
            active_positions.insert(runtime.entity.position);
            summary.reactivated += 1;
        }
        self.refresh_static_creature_occupancy();
        if summary.reactivated > 0 {
            self.mark_changed();
        }
        summary
    }

    /// Applies one explicitly requested cardinal step. Selection, pacing, AI, combat, scripts,
    /// and autonomous movement remain outside this foundation.
    pub fn move_static_creature_cardinal(
        &mut self,
        id: u32,
        direction: CardinalDirection,
        world_map: &WorldMap,
    ) -> Result<(Position, Position), CoreError> {
        let source = self
            .static_creatures
            .get(&id)
            .ok_or(CoreError::UnknownStaticCreature(id))?;
        if !source.active {
            return Err(CoreError::InactiveStaticCreature(id));
        }
        let previous = source.entity.position;
        let destination = previous.step(direction)?;
        if !world_map.is_walkable(destination) {
            return Err(CoreError::StaticCreatureMovementBlocked(destination));
        }
        if self
            .players
            .values()
            .any(|player| player.position == destination)
        {
            return Err(CoreError::PlayerOccupiesStaticCreaturePosition(destination));
        }
        if self.static_creatures.iter().any(|(other_id, runtime)| {
            *other_id != id && runtime.active && runtime.entity.position == destination
        }) {
            return Err(CoreError::StaticCreatureOccupiesPosition(destination));
        }
        let runtime = self
            .static_creatures
            .get_mut(&id)
            .ok_or(CoreError::UnknownStaticCreature(id))?;
        runtime.entity.position = destination;
        self.refresh_static_creature_occupancy();
        self.mark_changed();
        Ok((previous, destination))
    }

    pub fn is_static_creature_occupied(&self, position: Position) -> bool {
        self.static_occupied_positions.contains(&position)
    }

    fn refresh_static_creature_occupancy(&mut self) {
        self.static_occupied_positions = self
            .static_creatures
            .values()
            .filter(|runtime| runtime.active)
            .map(|runtime| runtime.entity.position)
            .collect();
    }

    pub(crate) fn mark_changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}
