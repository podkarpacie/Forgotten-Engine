//! Authoritative map↔inventory transfer methods for [`SharedNativeMap`]: player stack drops to
//! ground, runtime item pickup, and the revision-bound source-map item transfer family (map →
//! equipment / top-level container, whole-item and partial-count). Each transfer validates
//! identity against the immutable source revision, persists equipment/containers plus the map
//! registry or removal journal in one SQLite transaction under authoritative locks, and only
//! then commits staged memory.

use super::*;

impl SharedNativeMap {
    /// Moves one requested bounded player stack from owned inventory onto an authoritative ground
    /// tile adjacent to (or under) the player, appending it to the durable runtime registry. The
    /// inventory change and registry persist in one SQLite transaction while authoritative locks
    /// are held; staged memory commits only after a successful commit.
    #[allow(clippy::too_many_arguments)]
    pub fn move_player_stack_to_ground(
        &self,
        shared_world: &SharedNativeWorld,
        database: &mut EngineDatabase,
        player_id: u64,
        source: forgotten_core::PlayerGroundDropSource,
        target_position: Position,
        count: u16,
        item_weight_by_server_id: Option<&BTreeMap<u16, u32>>,
    ) -> Result<Option<forgotten_core::PlayerGroundDropOutcome>, HostError> {
        let mut world = shared_world.lock()?;
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut registry = self
            .runtime_tile_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        // Bounded placement rules: existing walkable tile, within one step of the player, and
        // below the per-tile item limit.
        let Some(tile) = map.tile(target_position) else {
            return Ok(None);
        };
        if !tile.walkable {
            return Ok(None);
        }
        let player = world.player(player_id).ok_or(HostError::Core(
            forgotten_core::CoreError::UnknownPlayer(player_id),
        ))?;
        if player.position.z != target_position.z
            || player.position.x.abs_diff(target_position.x) > 1
            || player.position.y.abs_diff(target_position.y) > 1
        {
            return Ok(None);
        }
        let current_items = map
            .tile_items(target_position)
            .map(<[WorldMapItem]>::len)
            .unwrap_or(0);
        if current_items >= forgotten_core::MAX_WORLD_MAP_ITEMS_PER_TILE {
            return Ok(None);
        }
        if count == 0 || count > u16::from(u8::MAX) {
            return Ok(None);
        }
        // Stage the inventory mutation on a clone so nothing publishes before persistence.
        let mut staged_world = world.clone();
        let outcome = match staged_world.take_player_stack_for_ground_drop(player_id, source, count)
        {
            Ok(outcome) => outcome,
            Err(error) => {
                return match error {
                    forgotten_core::CoreError::EmptyEquipmentSlot { .. }
                    | forgotten_core::CoreError::UnknownPlayerContainer { .. }
                    | forgotten_core::CoreError::UnknownPlayerContainerItem { .. } => Ok(None),
                    other => Err(HostError::Core(other)),
                };
            }
        };
        let ordinal = u8::try_from(
            registry
                .iter()
                .filter(|record| record.position == target_position)
                .count(),
        )
        .map_err(|_| {
            HostError::Core(forgotten_core::CoreError::InvalidMap(
                "runtime tile-item ordinals exhausted for this tile".into(),
            ))
        })?;
        let Ok(dropped_count) = u8::try_from(outcome.moved_item.count) else {
            return Err(HostError::InvalidConfiguration(
                "dropped stack exceeds the ground count bound".into(),
            ));
        };
        let record = RuntimeMapItemRecord {
            position: target_position,
            ordinal,
            server_id: outcome.moved_item.server_id,
            count: dropped_count,
            children: Vec::new(),
            despawn_tick: None,
        };
        let mut next_registry = registry.clone();
        next_registry.push(record);
        let mut next_map = (*map).clone();
        let mut next_items = map
            .tile_items(target_position)
            .map(<[WorldMapItem]>::to_vec)
            .unwrap_or_default();
        next_items.push(WorldMapItem {
            server_id: outcome.moved_item.server_id,
            client_thing_id: None,
            count: dropped_count,
            action_id: None,
            unique_id: None,
            text: None,
            description: None,
            teleport_destination: None,
            duration: None,
            charges: None,
            children: Vec::new(),
        });
        next_map
            .set_tile_items(target_position, next_items)
            .map_err(HostError::Core)?;
        let published_equipment = staged_world
            .player_equipment(player_id)
            .map_err(HostError::Core)?
            .clone();
        let published_containers = staged_world
            .player_containers(player_id)
            .map_err(HostError::Core)?
            .clone();
        // Bounded capacity-weight gate: hundredths-of-an-ounce carried weight must stay within
        // the player's ounce capacity after the move. Missing operator weight metadata skips
        // the gate entirely, preserving prior transfer behavior.
        if let Some(weights) = item_weight_by_server_id {
            let capacity = world
                .player_vitals(player_id)
                .map_err(HostError::Core)?
                .capacity;
            let capacity_hundredths = u64::from(capacity).saturating_mul(100);
            if native_carried_weight(weights, &published_equipment, &published_containers)
                > capacity_hundredths
            {
                return Ok(None);
            }
        }
        database.replace_player_inventory_and_runtime_map_items(
            player_id,
            &published_equipment,
            &published_containers,
            self.source_revision(),
            &next_registry,
        )?;
        world
            .replace_player_equipment(player_id, published_equipment)
            .map_err(HostError::Core)?;
        world
            .replace_player_containers(player_id, published_containers)
            .map_err(HostError::Core)?;
        *map = next_map;
        *registry = next_registry;
        self.revision.fetch_add(1, Ordering::SeqCst);
        Ok(Some(outcome))
    }

    /// Resolves the ordered runtime-tail start index for one tile: every map item at or above
    /// this index belongs to the durable runtime registry rather than imported source content.
    fn runtime_tail_start(
        &self,
        map_items_len: Option<usize>,
        position: &Position,
        registry: &[RuntimeMapItemRecord],
    ) -> usize {
        let runtime_count = registry
            .iter()
            .filter(|record| &record.position == position)
            .count();
        map_items_len.unwrap_or(0).saturating_sub(runtime_count)
    }

    /// Moves one bounded count out of a durable runtime tile item into owned player inventory.
    /// `child_index = None` takes from the top-level stack itself (whole or partial), while
    /// `Some(index)` takes from one corpse child. The inventory change and complete registry
    /// persist in one SQLite transaction while authoritative locks are held; staged memory
    /// publishes only after commit. Depleted top-level records are removed from both the map and
    /// the registry together, and surviving sibling ordinals stay contiguous.
    #[allow(clippy::too_many_arguments)]
    pub fn move_runtime_item_to_inventory(
        &self,
        shared_world: &SharedNativeWorld,
        database: &mut EngineDatabase,
        player_id: u64,
        position: Position,
        item_index: usize,
        child_index: Option<usize>,
        count: u16,
        destination: forgotten_core::PlayerGroundDropSource,
        item_weight_by_server_id: Option<&BTreeMap<u16, u32>>,
    ) -> Result<Option<forgotten_core::PlayerGroundDropOutcome>, HostError> {
        if count == 0 || count > u16::from(u8::MAX) {
            return Ok(None);
        }
        let mut world = shared_world.lock()?;
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut registry_guard = self
            .runtime_tile_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let runtime_start = self.runtime_tail_start(
            map.tile_items(position).map(<[WorldMapItem]>::len),
            &position,
            &registry_guard,
        );
        let Some(items) = map.tile_items(position) else {
            return Ok(None);
        };
        if item_index < runtime_start || item_index >= items.len() {
            return Ok(None);
        }
        let Ok(ordinal) = u8::try_from(item_index - runtime_start) else {
            return Ok(None);
        };
        let Some(record_index) = registry_guard
            .iter()
            .position(|record| record.position == position && record.ordinal == ordinal)
        else {
            return Ok(None);
        };
        let map_item = items[item_index].clone();
        let top_level_count_before_take = map_item.count;
        let top_level_server_id = map_item.server_id;

        // Resolve the moved stack plus the staged post-take source shape.
        let (moved_item, next_children, next_top_count, remove_top, source_remaining_count) =
            match child_index {
                Some(child_index) => {
                    let Some(available_child) = map_item.children.get(child_index).cloned() else {
                        return Ok(None);
                    };
                    let Ok(take) = u8::try_from(count) else {
                        return Ok(None);
                    };
                    if available_child.count < take {
                        return Ok(None);
                    }
                    let mut remaining_child = available_child.clone();
                    let mut moved = available_child;
                    moved.count = take;
                    remaining_child.count -= take;
                    let source_remaining_count =
                        (remaining_child.count > 0).then_some(u16::from(remaining_child.count));
                    let mut next_children = map_item.children.clone();
                    if remaining_child.count > 0 {
                        next_children[child_index] = remaining_child;
                    } else {
                        next_children.remove(child_index);
                    }
                    (moved, next_children, None, false, source_remaining_count)
                }
                None => {
                    if u16::from(map_item.count) < count {
                        return Ok(None);
                    }
                    let Ok(take) = u8::try_from(count) else {
                        return Ok(None);
                    };
                    let mut remaining_top = map_item.clone();
                    let mut moved = map_item;
                    moved.count = take;
                    remaining_top.count -= take;
                    if remaining_top.count > 0 {
                        let source_remaining_count = Some(u16::from(remaining_top.count));
                        (
                            moved,
                            remaining_top.children.clone(),
                            Some(remaining_top.count),
                            false,
                            source_remaining_count,
                        )
                    } else {
                        // A fully depleted top-level item leaves nothing on the tile.
                        let source_remaining_count: Option<u16> = None;
                        (moved, Vec::new(), None, true, source_remaining_count)
                    }
                }
            };

        // Stage the inventory destination on a cloned world so nothing publishes before
        // persistence succeeds.
        let moved_item =
            forgotten_core::ItemInstance::new(moved_item.server_id, u16::from(moved_item.count))
                .map_err(HostError::Core)?;
        let mut staged_world = world.clone();
        let destination_admission = match destination {
            forgotten_core::PlayerGroundDropSource::EquipmentSlot(slot) => {
                let mut equipment = staged_world
                    .player_equipment(player_id)
                    .map_err(HostError::Core)?
                    .clone();
                match equipment.item(slot).cloned() {
                    Some(mut existing) => {
                        if existing.merge_stack(&moved_item).is_err() {
                            return Ok(None);
                        }
                        equipment.equip(slot, existing);
                    }
                    None => {
                        equipment.equip(slot, moved_item.clone());
                    }
                }
                staged_world
                    .replace_player_equipment(player_id, equipment)
                    .map_err(HostError::Core)?;
                true
            }
            forgotten_core::PlayerGroundDropSource::ContainerItem { container_id, .. } => {
                let mut containers = staged_world
                    .player_containers(player_id)
                    .map_err(HostError::Core)?
                    .clone();
                let Some(mut container) = containers.remove(container_id) else {
                    return Ok(None);
                };
                if container.has_parent
                    || container
                        .items
                        .merge_or_insert_stack(moved_item.clone())
                        .is_err()
                {
                    return Ok(None);
                }
                containers.insert(container).map_err(HostError::Core)?;
                staged_world
                    .replace_player_containers(player_id, containers)
                    .map_err(HostError::Core)?;
                true
            }
            // Content items drop out of nested storage; destination admission is decided by
            // the ground-tile checks below, not by inventory topology.
            forgotten_core::PlayerGroundDropSource::ContainerContent { .. } => true,
        };
        if !destination_admission {
            return Ok(None);
        }

        // Stage the registry and map mutations.
        let mut next_registry = registry_guard.clone();
        if remove_top {
            next_registry.remove(record_index);
            for sibling in next_registry
                .iter_mut()
                .filter(|record| record.position == position && record.ordinal > ordinal)
            {
                sibling.ordinal -= 1;
            }
        } else {
            match child_index {
                Some(_) => {
                    next_registry[record_index].children = next_children
                        .iter()
                        .map(|child| RuntimeMapItemChildRecord {
                            server_id: child.server_id,
                            count: child.count,
                        })
                        .collect();
                }
                None => {
                    let Some(count) = next_top_count else {
                        return Err(HostError::InvalidConfiguration(
                            "partial top-level take lost its remainder".into(),
                        ));
                    };
                    next_registry[record_index].count = count;
                }
            }
        }
        let mut next_items = items.to_vec();
        if remove_top {
            next_items.remove(item_index);
        } else {
            next_items[item_index] = WorldMapItem {
                server_id: top_level_server_id,
                client_thing_id: None,
                count: next_top_count.unwrap_or(top_level_count_before_take),
                action_id: None,
                unique_id: None,
                text: None,
                description: None,
                teleport_destination: None,
                duration: None,
                charges: None,
                children: next_children,
            };
        }
        let mut next_map = (*map).clone();
        next_map
            .set_tile_items(position, next_items)
            .map_err(HostError::Core)?;
        let published_equipment = staged_world
            .player_equipment(player_id)
            .map_err(HostError::Core)?
            .clone();
        let published_containers = staged_world
            .player_containers(player_id)
            .map_err(HostError::Core)?
            .clone();
        // Bounded capacity-weight gate: hundredths-of-an-ounce carried weight must stay within
        // the player's ounce capacity after the move. Missing operator weight metadata skips
        // the gate entirely, preserving prior transfer behavior.
        if let Some(weights) = item_weight_by_server_id {
            let capacity = world
                .player_vitals(player_id)
                .map_err(HostError::Core)?
                .capacity;
            let capacity_hundredths = u64::from(capacity).saturating_mul(100);
            if native_carried_weight(weights, &published_equipment, &published_containers)
                > capacity_hundredths
            {
                return Ok(None);
            }
        }
        database.replace_player_inventory_and_runtime_map_items(
            player_id,
            &published_equipment,
            &published_containers,
            self.source_revision(),
            &next_registry,
        )?;
        world
            .replace_player_equipment(player_id, published_equipment)
            .map_err(HostError::Core)?;
        world
            .replace_player_containers(player_id, published_containers)
            .map_err(HostError::Core)?;
        *map = next_map;
        *registry_guard = next_registry;
        drop(world);
        drop(map);
        drop(registry_guard);
        self.revision.fetch_add(1, Ordering::SeqCst);
        Ok(Some(forgotten_core::PlayerGroundDropOutcome {
            player_id,
            source: destination,
            moved_item,
            source_remaining_count,
        }))
    }

    /// Replaces one imported tile's complete ordered item list after `WorldMap` validates its
    /// per-tile limit. This is an ownership primitive only; it does not transfer an item into a
    /// player inventory, persist a change, or emit any native packet.
    pub fn replace_tile_items(
        &self,
        position: Position,
        items: Vec<WorldMapItem>,
    ) -> Result<u64, HostError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut source_item_indices = self
            .source_item_indices
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        map.set_tile_items(position, items)
            .map_err(HostError::Core)?;
        source_item_indices.remove(&position);
        Ok(self.revision.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Transfers one exact complete top-level imported source stack into one equipment slot. The
    /// slot is either empty or contains one exact compatible stack with bounded remaining room.
    /// The lock order is always map, authoritative player world, source-index map, then removal
    /// journal.
    /// It creates candidate map/inventory/journal state first, commits the candidate inventory and
    /// journal through one SQLite transaction, then publishes the already validated in-memory
    /// state and advances both affected epochs. Native ThrowItem decoding and map refresh packets
    /// deliberately remain outside this primitive.
    pub fn move_source_item_to_empty_equipment(
        &self,
        shared_world: &SharedNativeWorld,
        database: &mut EngineDatabase,
        player_id: u64,
        position: Position,
        runtime_item_index: usize,
        equipment_slot: EquipmentSlot,
    ) -> Result<SourceMapItemToEquipmentTransferOutcome, HostError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut world = shared_world.lock()?;
        let mut source_item_indices = self
            .source_item_indices
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut removed_source_items = self
            .removed_source_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;

        let runtime_item = map
            .tile_items(position)
            .and_then(|items| items.get(runtime_item_index))
            .cloned()
            .ok_or(HostError::Core(forgotten_core::CoreError::UnknownMapItem {
                position,
                stack_index: u8::try_from(runtime_item_index).unwrap_or(u8::MAX),
                expected_server_id: 0,
            }))?;
        let source_item_index = source_item_indices
            .get(&position)
            .and_then(|indices| indices.get(runtime_item_index))
            .copied()
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map item has no source-bound runtime identity".into(),
                )
            })?;
        let source_identity = self
            .source
            .source_item_identity(position, usize::from(source_item_index))
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map source item identity no longer resolves".into(),
                )
            })?;
        let source_item = self
            .source
            .tile_items(position)
            .and_then(|items| items.get(usize::from(source_item_index)))
            .ok_or_else(|| {
                HostError::InvalidConfiguration("map source item no longer resolves".into())
            })?;
        if source_item != &runtime_item {
            return Err(HostError::InvalidConfiguration(
                "map runtime item no longer matches its immutable source item".into(),
            ));
        }
        let item = plain_source_map_item_to_inventory_item(&runtime_item)?;
        let mut equipment = world
            .player_equipment(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        match equipment.item(equipment_slot).cloned() {
            None => {
                equipment.equip(equipment_slot, item.clone());
            }
            Some(mut existing) => {
                existing.merge_stack(&item).map_err(HostError::Core)?;
                equipment.equip(equipment_slot, existing);
            }
        }
        let containers = world
            .player_containers(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        if removed_source_items.contains(&source_identity) {
            return Err(HostError::InvalidConfiguration(
                "map source item identity has already been removed".into(),
            ));
        }
        let mut next_map = map.clone();
        let mut next_items = next_map
            .tile_items(position)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                HostError::Core(forgotten_core::CoreError::InvalidMap(
                    "validated runtime item list disappeared".into(),
                ))
            })?;
        next_items.remove(runtime_item_index);
        next_map
            .set_tile_items(position, next_items)
            .map_err(HostError::Core)?;
        let mut next_removed_source_items = removed_source_items.clone();
        next_removed_source_items.insert(source_identity);
        let next_journal = MapItemRemovalJournal {
            map_revision: self.source_revision(),
            removed_items: next_removed_source_items.iter().copied().collect(),
        };

        database.replace_player_inventory_and_map_item_removal_journal(
            player_id,
            &equipment,
            &containers,
            &next_journal,
        )?;

        *map = next_map;
        let changed = world
            .replace_player_equipment(player_id, equipment)
            .map_err(HostError::Core)?;
        debug_assert!(changed);
        let source_indices = source_item_indices.get_mut(&position).ok_or_else(|| {
            HostError::InvalidConfiguration("validated source index list disappeared".into())
        })?;
        source_indices.remove(runtime_item_index);
        if source_indices.is_empty() {
            source_item_indices.remove(&position);
        }
        *removed_source_items = next_removed_source_items;
        let map_revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        shared_world.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(SourceMapItemToEquipmentTransferOutcome {
            player_id,
            source_identity,
            item,
            equipment_slot,
            map_revision,
        })
    }

    /// Transfers one exact plain source-bound map stack count into one equipment slot. The slot is
    /// empty or contains an exact compatible bounded stack. A whole-count request completes the
    /// source removal, while a smaller request reduces the runtime source stack and persists its
    /// remaining count in the same SQLite transaction as the equipment change.
    #[allow(clippy::too_many_arguments)] // Explicit map identity and destination fields preserve the authoritative transfer contract.
    pub fn move_source_item_stack_to_equipment(
        &self,
        shared_world: &SharedNativeWorld,
        database: &mut EngineDatabase,
        player_id: u64,
        position: Position,
        runtime_item_index: usize,
        requested_count: u16,
        equipment_slot: EquipmentSlot,
    ) -> Result<SourceMapItemToEquipmentTransferOutcome, HostError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut world = shared_world.lock()?;
        let mut source_item_indices = self
            .source_item_indices
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut removed_source_items = self
            .removed_source_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut source_item_count_overrides = self
            .source_item_count_overrides
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let runtime_item = map
            .tile_items(position)
            .and_then(|items| items.get(runtime_item_index))
            .cloned()
            .ok_or(HostError::Core(forgotten_core::CoreError::UnknownMapItem {
                position,
                stack_index: u8::try_from(runtime_item_index).unwrap_or(u8::MAX),
                expected_server_id: 0,
            }))?;
        let source_item_index = source_item_indices
            .get(&position)
            .and_then(|indices| indices.get(runtime_item_index))
            .copied()
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map item has no source-bound runtime identity".into(),
                )
            })?;
        let source_identity = self
            .source
            .source_item_identity(position, usize::from(source_item_index))
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map source item identity no longer resolves".into(),
                )
            })?;
        let source_item = self
            .source
            .tile_items(position)
            .and_then(|items| items.get(usize::from(source_item_index)))
            .ok_or_else(|| {
                HostError::InvalidConfiguration("map source item no longer resolves".into())
            })?;
        if runtime_item.server_id != source_item.server_id
            || runtime_item.action_id != source_item.action_id
            || runtime_item.unique_id != source_item.unique_id
            || runtime_item.children != source_item.children
            || runtime_item.text != source_item.text
            || runtime_item.description != source_item.description
            || runtime_item.teleport_destination != source_item.teleport_destination
            || runtime_item.duration != source_item.duration
            || runtime_item.charges != source_item.charges
        {
            return Err(HostError::InvalidConfiguration(
                "map runtime item no longer matches its immutable source item identity".into(),
            ));
        }
        if removed_source_items.contains(&source_identity) {
            return Err(HostError::InvalidConfiguration(
                "map source item identity has already been removed".into(),
            ));
        }
        let available_count = u16::from(runtime_item.count);
        if requested_count == 0 || requested_count > available_count {
            return Err(HostError::Core(
                forgotten_core::CoreError::InvalidItemTransferCount {
                    requested: requested_count,
                    available: available_count,
                },
            ));
        }
        let mut item = plain_source_map_item_to_inventory_item(&runtime_item)?;
        item.count = requested_count;
        let mut equipment = world
            .player_equipment(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        match equipment.item(equipment_slot).cloned() {
            None => {
                equipment.equip(equipment_slot, item.clone());
            }
            Some(mut existing) => {
                existing.merge_stack(&item).map_err(HostError::Core)?;
                equipment.equip(equipment_slot, existing);
            }
        }
        let containers = world
            .player_containers(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        let mut next_map = map.clone();
        let mut next_items = next_map
            .tile_items(position)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                HostError::Core(forgotten_core::CoreError::InvalidMap(
                    "validated runtime item list disappeared".into(),
                ))
            })?;
        let mut next_removed_source_items = removed_source_items.clone();
        let mut next_source_item_count_overrides = source_item_count_overrides.clone();
        let whole_source = requested_count == available_count;
        if whole_source {
            next_items.remove(runtime_item_index);
            next_removed_source_items.insert(source_identity);
            next_source_item_count_overrides.remove(&source_identity);
        } else {
            let remaining_count = available_count - requested_count;
            next_items[runtime_item_index].count = u8::try_from(remaining_count).map_err(|_| {
                HostError::Core(forgotten_core::CoreError::InvalidMap(
                    "partial take remainder exceeds the map u8 stack bound".into(),
                ))
            })?;
            next_source_item_count_overrides.insert(source_identity, remaining_count);
        }
        next_map
            .set_tile_items(position, next_items)
            .map_err(HostError::Core)?;
        let next_journal = MapItemRemovalJournal {
            map_revision: self.source_revision(),
            removed_items: next_removed_source_items.iter().copied().collect(),
        };
        let next_override_records = next_source_item_count_overrides
            .iter()
            .map(
                |(source_identity, remaining_count)| MapItemCountOverrideRecord {
                    source_identity: *source_identity,
                    remaining_count: *remaining_count,
                },
            )
            .collect::<Vec<_>>();
        database.replace_player_inventory_and_map_item_state(
            player_id,
            &equipment,
            &containers,
            &next_journal,
            &next_override_records,
        )?;
        *map = next_map;
        let changed = world
            .replace_player_equipment(player_id, equipment)
            .map_err(HostError::Core)?;
        debug_assert!(changed);
        if whole_source {
            let source_indices = source_item_indices.get_mut(&position).ok_or_else(|| {
                HostError::InvalidConfiguration("validated source index list disappeared".into())
            })?;
            source_indices.remove(runtime_item_index);
            if source_indices.is_empty() {
                source_item_indices.remove(&position);
            }
        }
        *removed_source_items = next_removed_source_items;
        *source_item_count_overrides = next_source_item_count_overrides;
        let map_revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        shared_world.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(SourceMapItemToEquipmentTransferOutcome {
            player_id,
            source_identity,
            item,
            equipment_slot,
            map_revision,
        })
    }

    /// Transfers one exact complete plain top-level source-bound map stack into one existing,
    /// owned, non-nested container. It merges only an exact compatible stack or appends one
    /// bounded stack, then follows the established map, world, source-index, journal lock order.
    pub fn move_source_item_to_top_level_container(
        &self,
        shared_world: &SharedNativeWorld,
        database: &mut EngineDatabase,
        player_id: u64,
        position: Position,
        runtime_item_index: usize,
        container_id: u8,
    ) -> Result<SourceMapItemToContainerTransferOutcome, HostError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut world = shared_world.lock()?;
        let mut source_item_indices = self
            .source_item_indices
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut removed_source_items = self
            .removed_source_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let runtime_item = map
            .tile_items(position)
            .and_then(|items| items.get(runtime_item_index))
            .cloned()
            .ok_or(HostError::Core(forgotten_core::CoreError::UnknownMapItem {
                position,
                stack_index: u8::try_from(runtime_item_index).unwrap_or(u8::MAX),
                expected_server_id: 0,
            }))?;
        let source_item_index = source_item_indices
            .get(&position)
            .and_then(|indices| indices.get(runtime_item_index))
            .copied()
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map item has no source-bound runtime identity".into(),
                )
            })?;
        let source_identity = self
            .source
            .source_item_identity(position, usize::from(source_item_index))
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map source item identity no longer resolves".into(),
                )
            })?;
        if self
            .source
            .tile_items(position)
            .and_then(|items| items.get(usize::from(source_item_index)))
            != Some(&runtime_item)
        {
            return Err(HostError::InvalidConfiguration(
                "map runtime item no longer matches its immutable source item".into(),
            ));
        }
        let item = plain_source_map_item_to_inventory_item(&runtime_item)?;
        if removed_source_items.contains(&source_identity) {
            return Err(HostError::InvalidConfiguration(
                "map source item identity has already been removed".into(),
            ));
        }
        let equipment = world
            .player_equipment(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        let mut containers = world
            .player_containers(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        let mut container = containers.remove(container_id).ok_or_else(|| {
            HostError::InvalidConfiguration("map item destination container is not owned".into())
        })?;
        if container.has_parent {
            return Err(HostError::InvalidConfiguration(
                "map item destination container must be top-level".into(),
            ));
        }
        container
            .items
            .merge_or_insert_stack(item.clone())
            .map_err(HostError::Core)?;
        containers.insert(container).map_err(HostError::Core)?;
        let mut next_map = map.clone();
        let mut next_items = next_map
            .tile_items(position)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                HostError::Core(forgotten_core::CoreError::InvalidMap(
                    "validated runtime item list disappeared".into(),
                ))
            })?;
        next_items.remove(runtime_item_index);
        next_map
            .set_tile_items(position, next_items)
            .map_err(HostError::Core)?;
        let mut next_removed_source_items = removed_source_items.clone();
        next_removed_source_items.insert(source_identity);
        let next_journal = MapItemRemovalJournal {
            map_revision: self.source_revision(),
            removed_items: next_removed_source_items.iter().copied().collect(),
        };
        database.replace_player_inventory_and_map_item_removal_journal(
            player_id,
            &equipment,
            &containers,
            &next_journal,
        )?;
        *map = next_map;
        let changed = world
            .replace_player_containers(player_id, containers)
            .map_err(HostError::Core)?;
        debug_assert!(changed);
        let source_indices = source_item_indices.get_mut(&position).ok_or_else(|| {
            HostError::InvalidConfiguration("validated source index list disappeared".into())
        })?;
        source_indices.remove(runtime_item_index);
        if source_indices.is_empty() {
            source_item_indices.remove(&position);
        }
        *removed_source_items = next_removed_source_items;
        let map_revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        shared_world.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(SourceMapItemToContainerTransferOutcome {
            player_id,
            source_identity,
            item,
            container_id,
            map_revision,
        })
    }

    /// Transfers one exact plain source-bound map stack count into one existing owned top-level
    /// container. A whole-count request preserves the established complete-removal path, while a
    /// smaller bounded request persists the remaining source count through the override journal in
    /// the same SQLite transaction as the container change.
    #[allow(clippy::too_many_arguments)] // Explicit map identity and destination fields preserve the authoritative transfer contract.
    pub fn move_source_item_stack_to_top_level_container(
        &self,
        shared_world: &SharedNativeWorld,
        database: &mut EngineDatabase,
        player_id: u64,
        position: Position,
        runtime_item_index: usize,
        requested_count: u16,
        container_id: u8,
    ) -> Result<SourceMapItemToContainerTransferOutcome, HostError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut world = shared_world.lock()?;
        let mut source_item_indices = self
            .source_item_indices
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut removed_source_items = self
            .removed_source_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut source_item_count_overrides = self
            .source_item_count_overrides
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let runtime_item = map
            .tile_items(position)
            .and_then(|items| items.get(runtime_item_index))
            .cloned()
            .ok_or(HostError::Core(forgotten_core::CoreError::UnknownMapItem {
                position,
                stack_index: u8::try_from(runtime_item_index).unwrap_or(u8::MAX),
                expected_server_id: 0,
            }))?;
        let source_item_index = source_item_indices
            .get(&position)
            .and_then(|indices| indices.get(runtime_item_index))
            .copied()
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map item has no source-bound runtime identity".into(),
                )
            })?;
        let source_identity = self
            .source
            .source_item_identity(position, usize::from(source_item_index))
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "map source item identity no longer resolves".into(),
                )
            })?;
        let source_item = self
            .source
            .tile_items(position)
            .and_then(|items| items.get(usize::from(source_item_index)))
            .ok_or_else(|| {
                HostError::InvalidConfiguration("map source item no longer resolves".into())
            })?;
        if runtime_item.server_id != source_item.server_id
            || runtime_item.action_id != source_item.action_id
            || runtime_item.unique_id != source_item.unique_id
            || runtime_item.children != source_item.children
            || runtime_item.text != source_item.text
            || runtime_item.description != source_item.description
            || runtime_item.teleport_destination != source_item.teleport_destination
            || runtime_item.duration != source_item.duration
            || runtime_item.charges != source_item.charges
        {
            return Err(HostError::InvalidConfiguration(
                "map runtime item no longer matches its immutable source item identity".into(),
            ));
        }
        if removed_source_items.contains(&source_identity) {
            return Err(HostError::InvalidConfiguration(
                "map source item identity has already been removed".into(),
            ));
        }
        let available_count = u16::from(runtime_item.count);
        if requested_count == 0 || requested_count > available_count {
            return Err(HostError::Core(
                forgotten_core::CoreError::InvalidItemTransferCount {
                    requested: requested_count,
                    available: available_count,
                },
            ));
        }
        let mut item = plain_source_map_item_to_inventory_item(&runtime_item)?;
        item.count = requested_count;
        let equipment = world
            .player_equipment(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        let mut containers = world
            .player_containers(player_id)
            .cloned()
            .map_err(HostError::Core)?;
        let mut container = containers.remove(container_id).ok_or_else(|| {
            HostError::InvalidConfiguration("map item destination container is not owned".into())
        })?;
        if container.has_parent {
            return Err(HostError::InvalidConfiguration(
                "map item destination container must be top-level".into(),
            ));
        }
        container
            .items
            .merge_or_insert_stack(item.clone())
            .map_err(HostError::Core)?;
        containers.insert(container).map_err(HostError::Core)?;
        let mut next_map = map.clone();
        let mut next_items = next_map
            .tile_items(position)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                HostError::Core(forgotten_core::CoreError::InvalidMap(
                    "validated runtime item list disappeared".into(),
                ))
            })?;
        let mut next_removed_source_items = removed_source_items.clone();
        let mut next_source_item_count_overrides = source_item_count_overrides.clone();
        let whole_source = requested_count == available_count;
        if whole_source {
            next_items.remove(runtime_item_index);
            next_removed_source_items.insert(source_identity);
            next_source_item_count_overrides.remove(&source_identity);
        } else {
            let remaining_count = available_count - requested_count;
            next_items[runtime_item_index].count = u8::try_from(remaining_count).map_err(|_| {
                HostError::Core(forgotten_core::CoreError::InvalidMap(
                    "partial take remainder exceeds the map u8 stack bound".into(),
                ))
            })?;
            next_source_item_count_overrides.insert(source_identity, remaining_count);
        }
        next_map
            .set_tile_items(position, next_items)
            .map_err(HostError::Core)?;
        let next_journal = MapItemRemovalJournal {
            map_revision: self.source_revision(),
            removed_items: next_removed_source_items.iter().copied().collect(),
        };
        let next_override_records = next_source_item_count_overrides
            .iter()
            .map(
                |(source_identity, remaining_count)| MapItemCountOverrideRecord {
                    source_identity: *source_identity,
                    remaining_count: *remaining_count,
                },
            )
            .collect::<Vec<_>>();
        database.replace_player_inventory_and_map_item_state(
            player_id,
            &equipment,
            &containers,
            &next_journal,
            &next_override_records,
        )?;
        *map = next_map;
        let changed = world
            .replace_player_containers(player_id, containers)
            .map_err(HostError::Core)?;
        debug_assert!(changed);
        if whole_source {
            let source_indices = source_item_indices.get_mut(&position).ok_or_else(|| {
                HostError::InvalidConfiguration("validated source index list disappeared".into())
            })?;
            source_indices.remove(runtime_item_index);
            if source_indices.is_empty() {
                source_item_indices.remove(&position);
            }
        }
        *removed_source_items = next_removed_source_items;
        *source_item_count_overrides = next_source_item_count_overrides;
        let map_revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        shared_world.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(SourceMapItemToContainerTransferOutcome {
            player_id,
            source_identity,
            item,
            container_id,
            map_revision,
        })
    }
}

fn plain_source_map_item_to_inventory_item(item: &WorldMapItem) -> Result<ItemInstance, HostError> {
    if !item.children.is_empty()
        || item.text.is_some()
        || item.description.is_some()
        || item.teleport_destination.is_some()
        || item.duration.is_some()
        || item.charges.is_some()
    {
        return Err(HostError::InvalidConfiguration(
            "map item carries unsupported runtime attributes for inventory transfer".into(),
        ));
    }
    let mut inventory_item =
        ItemInstance::new(item.server_id, u16::from(item.count)).map_err(HostError::Core)?;
    inventory_item.action_id = item.action_id;
    inventory_item.unique_id = item.unique_id;
    Ok(inventory_item)
}
