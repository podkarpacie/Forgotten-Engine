//! Equipment and container transfer methods on the authoritative world state:
//! complete-item and stack moves between equipment slots, owned top-level containers,
//! and nested content, plus ground-drop stack extraction.

use super::*;

impl WorldState {
    /// Moves one complete item instance from a known player's fixed equipment slot into one of
    /// that same player's already-owned bounded containers. All checks occur on cloned state, so
    /// a missing source, missing container, or full container leaves the world unchanged.
    pub fn move_equipment_item_to_container(
        &mut self,
        player_id: u64,
        from_slot: EquipmentSlot,
        container_id: u8,
    ) -> Result<PlayerEquipmentToContainerOutcome, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        let mut containers = self.player_containers(player_id)?.clone();
        let item = equipment
            .unequip(from_slot)
            .ok_or(CoreError::EmptyEquipmentSlot {
                player_id,
                slot: from_slot,
            })?;
        let mut container =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        container.items.insert(item.clone())?;
        containers.insert(container)?;
        self.player_equipments.insert(player_id, equipment);
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerEquipmentToContainerOutcome {
            player_id,
            from_slot,
            container_id,
            item,
        })
    }

    /// Moves one complete item from an already-owned non-recursive container to an empty fixed
    /// equipment slot. All checks occur on cloned state, so an occupied slot, missing container,
    /// or invalid item index leaves the authoritative world unchanged.
    pub fn move_container_item_to_equipment(
        &mut self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerContainerToEquipmentOutcome, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        if equipment.item(to_slot).is_some() {
            return Err(CoreError::OccupiedEquipmentSlot {
                player_id,
                slot: to_slot,
            });
        }
        let mut containers = self.player_containers(player_id)?.clone();
        let mut container =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        let item =
            container
                .items
                .remove(item_index)
                .ok_or(CoreError::UnknownPlayerContainerItem {
                    player_id,
                    container_id,
                    item_index,
                })?;
        equipment.equip(to_slot, item.clone());
        containers.insert(container)?;
        self.player_equipments.insert(player_id, equipment);
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerContainerToEquipmentOutcome {
            player_id,
            container_id,
            item_index,
            to_slot,
            item,
        })
    }

    /// Moves one depth-one content item out of a container item into an empty equipment slot.
    /// Cloned-state preparation keeps every error path atomic, matching the container-item
    /// equipment transfer semantics.
    pub fn move_content_item_to_equipment(
        &mut self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        content_index: usize,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerContainerToEquipmentOutcome, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        if equipment.item(to_slot).is_some() {
            return Err(CoreError::OccupiedEquipmentSlot {
                player_id,
                slot: to_slot,
            });
        }
        let mut containers = self.player_containers(player_id)?.clone();
        let mut container =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        let item = container
            .items
            .take_content(item_index, content_index)
            .ok_or(CoreError::UnknownPlayerContainerItem {
                player_id,
                container_id,
                item_index,
            })?;
        equipment.equip(to_slot, item.clone());
        containers.insert(container)?;
        self.player_equipments.insert(player_id, equipment);
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerContainerToEquipmentOutcome {
            player_id,
            container_id,
            item_index,
            to_slot,
            item,
        })
    }

    /// Consumes one unit from an owned equipment slot stack (plan v49 slice 9: distance-weapon
    /// ammunition). Removing the final unit clears the slot. Returns false when the slot is
    /// already empty.
    pub fn consume_player_equipment_item_unit(
        &mut self,
        player_id: u64,
        slot: EquipmentSlot,
    ) -> Result<bool, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        let Some(item) = equipment.item_mut(slot) else {
            return Ok(false);
        };
        if item.count > 1 {
            item.count -= 1;
        } else {
            equipment.unequip(slot);
        }
        self.player_equipments.insert(player_id, equipment);
        self.mark_changed();
        Ok(true)
    }

    /// Consumes one unit of an owned top-level container stack (plan v49 slice 10: rune
    /// charges). Removing the final unit drops the entry entirely, matching legacy
    /// rune-stack semantics. Returns false when the slot does not resolve.
    pub fn consume_player_container_item_unit(
        &mut self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
    ) -> Result<bool, CoreError> {
        let mut containers = self.player_containers(player_id)?.clone();
        let mut container =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        let consumed = container.items.consume_item_unit(item_index);
        if consumed {
            containers.insert(container)?;
            self.player_containers.insert(player_id, containers);
            self.mark_changed();
        }
        Ok(consumed)
    }

    /// Moves one depth-one content item out of a container item into another top-level owned
    /// container. Cloned-state preparation keeps every error path atomic.
    pub fn move_content_item_to_container(
        &mut self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        content_index: usize,
        to_container_id: u8,
    ) -> Result<(), CoreError> {
        let mut containers = self.player_containers(player_id)?.clone();
        let mut source =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        let moved = source.items.take_content(item_index, content_index).ok_or(
            CoreError::UnknownPlayerContainerItem {
                player_id,
                container_id,
                item_index,
            },
        )?;
        containers.insert(source)?;
        let mut target =
            containers
                .remove(to_container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id: to_container_id,
                })?;
        if target.has_parent {
            containers.insert(target)?;
            self.player_containers.insert(player_id, containers);
            return Err(CoreError::UnknownPlayerContainer {
                player_id,
                container_id: to_container_id,
            });
        }
        target.items.merge_or_insert_stack(moved)?;
        containers.insert(target)?;
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(())
    }

    /// Exchanges one complete item in an existing non-recursive owned container with an occupied
    /// equipment slot. Both values are prepared on cloned state, keeping all error paths atomic.
    pub fn swap_container_item_with_equipment(
        &mut self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerContainerToEquipmentSwapOutcome, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        let equipped_item = equipment
            .unequip(to_slot)
            .ok_or(CoreError::EmptyEquipmentSlot {
                player_id,
                slot: to_slot,
            })?;
        let mut containers = self.player_containers(player_id)?.clone();
        let mut container =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        let container_item =
            container
                .items
                .remove(item_index)
                .ok_or(CoreError::UnknownPlayerContainerItem {
                    player_id,
                    container_id,
                    item_index,
                })?;
        equipment.equip(to_slot, container_item.clone());
        container
            .items
            .items
            .insert(item_index, equipped_item.clone());
        containers.insert(container)?;
        self.player_equipments.insert(player_id, equipment);
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerContainerToEquipmentSwapOutcome {
            player_id,
            container_id,
            item_index,
            to_slot,
            equipped_item,
            container_item,
        })
    }

    /// Exchanges the complete items in two distinct occupied equipment slots. Both items are
    /// prepared on cloned state, so rejected source, target, or self-swap paths leave the
    /// authoritative inventory and its revision unchanged.
    pub fn swap_equipment_items(
        &mut self,
        player_id: u64,
        from_slot: EquipmentSlot,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerEquipmentSlotSwapOutcome, CoreError> {
        if from_slot == to_slot {
            return Err(CoreError::SameEquipmentSlotTransfer {
                player_id,
                slot: from_slot,
            });
        }
        let mut equipment = self.player_equipment(player_id)?.clone();
        let from_item = equipment
            .unequip(from_slot)
            .ok_or(CoreError::EmptyEquipmentSlot {
                player_id,
                slot: from_slot,
            })?;
        let to_item = equipment
            .unequip(to_slot)
            .ok_or(CoreError::EmptyEquipmentSlot {
                player_id,
                slot: to_slot,
            })?;
        equipment.equip(from_slot, to_item.clone());
        equipment.equip(to_slot, from_item.clone());
        self.player_equipments.insert(player_id, equipment);
        self.mark_changed();
        Ok(PlayerEquipmentSlotSwapOutcome {
            player_id,
            from_slot,
            to_slot,
            from_item,
            to_item,
        })
    }

    /// Moves a requested bounded count from one equipment item into an existing top-level
    /// container. The destination merges only with an identical item instance and otherwise
    /// creates a new bounded stack. No item metadata-driven stackability is inferred.
    pub fn move_equipment_stack_to_container(
        &mut self,
        player_id: u64,
        from_slot: EquipmentSlot,
        container_id: u8,
        count: u16,
    ) -> Result<PlayerEquipmentStackToContainerOutcome, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        let mut source = equipment
            .unequip(from_slot)
            .ok_or(CoreError::EmptyEquipmentSlot {
                player_id,
                slot: from_slot,
            })?;
        let moved_item = source.split_off(count)?;
        let source_remaining_count = (source.count > 0).then_some(source.count);
        if source_remaining_count.is_some() {
            equipment.equip(from_slot, source);
        }
        let mut containers = self.player_containers(player_id)?.clone();
        let mut container =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        let (destination_index, destination_count) =
            container.items.merge_or_insert_stack(moved_item.clone())?;
        containers.insert(container)?;
        self.player_equipments.insert(player_id, equipment);
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerEquipmentStackToContainerOutcome {
            player_id,
            from_slot,
            container_id,
            destination_index,
            moved_item,
            source_remaining_count,
            destination_count,
        })
    }

    /// Moves a requested bounded count from one existing top-level container item into a fixed
    /// equipment slot. An occupied slot can accept the move only when it has identical item
    /// attributes and enough space within the existing 100-count bound.
    pub fn move_container_stack_to_equipment(
        &mut self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
        count: u16,
    ) -> Result<PlayerContainerStackToEquipmentOutcome, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        let mut containers = self.player_containers(player_id)?.clone();
        let mut container =
            containers
                .remove(container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id,
                })?;
        let source =
            container
                .items
                .remove(item_index)
                .ok_or(CoreError::UnknownPlayerContainerItem {
                    player_id,
                    container_id,
                    item_index,
                })?;
        let mut remaining = source;
        let moved_item = remaining.split_off(count)?;
        let destination_count = if let Some(destination) = equipment.item(to_slot).cloned() {
            let mut destination = destination;
            destination.merge_stack(&moved_item)?;
            let count = destination.count;
            equipment.equip(to_slot, destination);
            count
        } else {
            equipment.equip(to_slot, moved_item.clone());
            moved_item.count
        };
        let source_remaining_count = (remaining.count > 0).then_some(remaining.count);
        if source_remaining_count.is_some() {
            container.items.items.insert(item_index, remaining);
        }
        containers.insert(container)?;
        self.player_equipments.insert(player_id, equipment);
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerContainerStackToEquipmentOutcome {
            player_id,
            container_id,
            item_index,
            to_slot,
            moved_item,
            source_remaining_count,
            destination_count,
        })
    }

    /// Moves a requested bounded count between two distinct existing player containers. The
    /// target merges only identical instances or appends a new bounded stack; no item metadata
    /// stackability is inferred.
    pub fn move_container_stack_to_container(
        &mut self,
        player_id: u64,
        from_container_id: u8,
        item_index: usize,
        to_container_id: u8,
        count: u16,
    ) -> Result<PlayerContainerStackToContainerOutcome, CoreError> {
        if from_container_id == to_container_id {
            return Err(CoreError::SamePlayerContainerTransfer {
                player_id,
                container_id: from_container_id,
            });
        }
        let mut containers = self.player_containers(player_id)?.clone();
        let mut source_container =
            containers
                .remove(from_container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id: from_container_id,
                })?;
        let mut destination_container =
            containers
                .remove(to_container_id)
                .ok_or(CoreError::UnknownPlayerContainer {
                    player_id,
                    container_id: to_container_id,
                })?;
        let source = source_container.items.remove(item_index).ok_or(
            CoreError::UnknownPlayerContainerItem {
                player_id,
                container_id: from_container_id,
                item_index,
            },
        )?;
        let mut remaining = source;
        let moved_item = remaining.split_off(count)?;
        let (destination_index, destination_count) = destination_container
            .items
            .merge_or_insert_stack(moved_item.clone())?;
        let source_remaining_count = (remaining.count > 0).then_some(remaining.count);
        if source_remaining_count.is_some() {
            source_container.items.items.insert(item_index, remaining);
        }
        containers.insert(source_container)?;
        containers.insert(destination_container)?;
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerContainerStackToContainerOutcome {
            player_id,
            from_container_id,
            item_index,
            to_container_id,
            destination_index,
            moved_item,
            source_remaining_count,
            destination_count,
        })
    }

    /// Removes a requested bounded count from one owned equipment slot or top-level container
    /// item so the caller can place the returned stack onto the ground. The map position is a
    /// caller-owned concern; this transition only mutates inventory state.
    pub fn take_player_stack_for_ground_drop(
        &mut self,
        player_id: u64,
        source: PlayerGroundDropSource,
        count: u16,
    ) -> Result<PlayerGroundDropOutcome, CoreError> {
        let mut equipment = self.player_equipment(player_id)?.clone();
        let mut containers = self.player_containers(player_id)?.clone();
        let (moved_item, source_remaining_count) =
            match source {
                PlayerGroundDropSource::EquipmentSlot(slot) => {
                    let mut stack = equipment
                        .unequip(slot)
                        .ok_or(CoreError::EmptyEquipmentSlot { player_id, slot })?;
                    let moved = stack.split_off(count)?;
                    let remaining = (stack.count > 0).then_some(stack.count);
                    if remaining.is_some() {
                        equipment.equip(slot, stack);
                    }
                    (moved, remaining)
                }
                PlayerGroundDropSource::ContainerItem {
                    container_id,
                    item_index,
                } => {
                    let mut container = containers.remove(container_id).ok_or(
                        CoreError::UnknownPlayerContainer {
                            player_id,
                            container_id,
                        },
                    )?;
                    let stack = container.items.remove(item_index).ok_or(
                        CoreError::UnknownPlayerContainerItem {
                            player_id,
                            container_id,
                            item_index,
                        },
                    )?;
                    let mut remaining_stack = stack;
                    let moved = remaining_stack.split_off(count)?;
                    let remaining = (remaining_stack.count > 0).then_some(remaining_stack.count);
                    if remaining.is_some() {
                        container.items.items.insert(item_index, remaining_stack);
                    }
                    containers.insert(container)?;
                    (moved, remaining)
                }
                PlayerGroundDropSource::ContainerContent {
                    container_id,
                    item_index,
                    content_index,
                } => {
                    let mut container = containers.remove(container_id).ok_or(
                        CoreError::UnknownPlayerContainer {
                            player_id,
                            container_id,
                        },
                    )?;
                    let mut stack = container
                        .items
                        .take_content(item_index, content_index)
                        .ok_or(CoreError::UnknownPlayerContainerItem {
                            player_id,
                            container_id,
                            item_index,
                        })?;
                    let moved = stack.split_off(count)?;
                    containers.insert(container)?;
                    (moved, None)
                }
            };
        if let PlayerGroundDropSource::EquipmentSlot(_) = source {
            self.player_equipments.insert(player_id, equipment);
        }
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(PlayerGroundDropOutcome {
            player_id,
            source,
            moved_item,
            source_remaining_count,
        })
    }
}
