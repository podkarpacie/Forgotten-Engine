//! Authoritative equipment/container transfer methods on the shared world: complete-item
//! and stack swaps between equipment slots and owned top-level containers. Native sessions
//! observe the resulting state through separate epochs; this module neither accepts a client
//! request nor persists the mutation (persistence is the caller's responsibility).

use super::*;

impl SharedNativeWorld {
    /// Applies the existing authoritative complete-item transfer under the shared world lock.
    /// Native sessions observe the resulting equipment and container state through their separate
    /// epochs; this method neither accepts a client request nor persists the mutation.
    pub fn move_equipment_item_to_container(
        &self,
        player_id: u64,
        from_slot: EquipmentSlot,
        container_id: u8,
    ) -> Result<PlayerEquipmentToContainerOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_equipment_item_to_container(player_id, from_slot, container_id)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies the reverse existing complete-item transfer under the shared world lock. Client
    /// requests, persistence, equipment compatibility, swaps, stack rules, ground transfer, and
    /// recursive containers remain outside this bounded host integration.
    pub fn move_container_item_to_equipment(
        &self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerContainerToEquipmentOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_container_item_to_equipment(player_id, container_id, item_index, to_slot)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Exchanges a complete owned top-level container item with a complete occupied equipment
    /// item under the shared-world lock. Persistence and native request validation remain the
    /// caller's responsibilities; both refresh epochs advance only after core acceptance.
    pub fn swap_container_item_with_equipment(
        &self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerContainerToEquipmentSwapOutcome, HostError> {
        let outcome = self
            .lock()?
            .swap_container_item_with_equipment(player_id, container_id, item_index, to_slot)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Exchanges complete items stored in two distinct occupied equipment slots under the
    /// shared-world lock. Persistence and native request validation remain the caller's
    /// responsibilities; the equipment refresh epoch advances only after core acceptance.
    pub fn swap_equipment_items(
        &self,
        player_id: u64,
        from_slot: EquipmentSlot,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerEquipmentSlotSwapOutcome, HostError> {
        let outcome = self
            .lock()?
            .swap_equipment_items(player_id, from_slot, to_slot)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies one bounded partial equipment stack movement under the shared world lock. It
    /// advances the two existing inventory epochs only after the core accepts the atomic update;
    /// request decoding, persistence routing, slot rules, and ground/nested inventory remain out
    /// of scope.
    pub fn move_equipment_stack_to_container(
        &self,
        player_id: u64,
        from_slot: EquipmentSlot,
        container_id: u8,
        count: u16,
    ) -> Result<PlayerEquipmentStackToContainerOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_equipment_stack_to_container(player_id, from_slot, container_id, count)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies one bounded partial top-level-container stack movement under the shared world
    /// lock. It shares the established inventory refresh contract with full-item transfers and
    /// deliberately does not claim native client request or general inventory semantics.
    /// Moves one depth-one content item into an empty equipment slot (nested-window takes).
    /// Moves one depth-one content item into another top-level owned container
    /// (nested-window takes). Persistence stays the caller's responsibility.
    pub fn move_content_item_to_container(
        &self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        content_index: usize,
        to_container_id: u8,
    ) -> Result<(), HostError> {
        self.lock()?
            .move_content_item_to_container(
                player_id,
                container_id,
                item_index,
                content_index,
                to_container_id,
            )
            .map_err(HostError::Core)?;
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub fn move_content_item_to_equipment(
        &self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        content_index: usize,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerContainerToEquipmentOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_content_item_to_equipment(
                player_id,
                container_id,
                item_index,
                content_index,
                to_slot,
            )
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    pub fn move_container_stack_to_equipment(
        &self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
        count: u16,
    ) -> Result<PlayerContainerStackToEquipmentOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_container_stack_to_equipment(player_id, container_id, item_index, to_slot, count)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies one bounded stack transfer between two distinct current container windows. It
    /// advances only the existing container refresh epoch after the core accepts the atomic
    /// update; native request validation, persistence, nesting, and ground behavior stay outside
    /// this shared-world wrapper.
    pub fn move_container_stack_to_container(
        &self,
        player_id: u64,
        from_container_id: u8,
        item_index: usize,
        to_container_id: u8,
        count: u16,
    ) -> Result<forgotten_core::PlayerContainerStackToContainerOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_container_stack_to_container(
                player_id,
                from_container_id,
                item_index,
                to_container_id,
                count,
            )
            .map_err(HostError::Core)?;
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    pub fn player_conditions(
        &self,
        player_id: u64,
    ) -> Result<BTreeMap<PlayerConditionKind, PlayerCondition>, HostError> {
        self.lock()?
            .player_conditions(player_id)
            .cloned()
            .map_err(HostError::Core)
    }
}
