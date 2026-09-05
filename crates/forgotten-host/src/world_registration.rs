//! Player registration and lifecycle on the shared world: position-aware registration
//! (with progressive hydration overloads for vitals, equipment, containers, progression,
//! and conditions), removal, outfit updates, and cardinal facing persistence.

use super::*;

impl SharedNativeWorld {
    pub fn register_player_at_available_position(
        &self,
        player: Player,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        self.register_player_at_available_position_with_vitals(
            player,
            PlayerVitals::default(),
            world_map,
        )
    }

    pub fn register_player_at_available_position_with_vitals(
        &self,
        player: Player,
        vitals: PlayerVitals,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        self.register_player_at_available_position_with_vitals_and_progression(
            player,
            vitals,
            PlayerProgression::default(),
            world_map,
        )
    }

    pub fn register_player_at_available_position_with_vitals_and_progression(
        &self,
        mut player: Player,
        vitals: PlayerVitals,
        progression: PlayerProgression,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let mut world = self.lock()?;
        let position = [player.position, world_map.spawn()]
            .into_iter()
            .find(|position| {
                world_map.is_walkable(*position)
                    && !world.is_static_creature_occupied(*position)
                    && !world.is_player_occupied(*position)
            })
            .or_else(|| {
                world_map.tiles().find_map(|(position, tile)| {
                    (tile.walkable
                        && !world.is_static_creature_occupied(position)
                        && !world.is_player_occupied(position))
                    .then_some(position)
                })
            })
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "native map has no walkable tile unoccupied by a player or static creature"
                        .into(),
                )
            })?;
        player.position = position;
        world
            .add_player_with_vitals_and_progression(player, vitals, progression)
            .map_err(HostError::Core)?;
        self.mark_visibility_changed();
        self.online_players.fetch_add(1, Ordering::SeqCst);
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_and_equipment(
        &self,
        player: Player,
        vitals: PlayerVitals,
        equipment: PlayerEquipment,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position =
            self.register_player_at_available_position_with_vitals(player, vitals, world_map)?;
        self.replace_player_equipment(player_id, equipment)?;
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_equipment_and_containers(
        &self,
        player: Player,
        vitals: PlayerVitals,
        equipment: PlayerEquipment,
        containers: PlayerContainers,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position = self.register_player_at_available_position_with_vitals_and_equipment(
            player, vitals, equipment, world_map,
        )?;
        self.replace_player_containers(player_id, containers)?;
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_equipment_containers_and_progression(
        &self,
        player: Player,
        vitals: PlayerVitals,
        progression: PlayerProgression,
        equipment: PlayerEquipment,
        containers: PlayerContainers,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position = self.register_player_at_available_position_with_vitals_and_progression(
            player,
            vitals,
            progression,
            world_map,
        )?;
        self.replace_player_equipment(player_id, equipment)?;
        self.replace_player_containers(player_id, containers)?;
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
        &self,
        player: Player,
        vitals: PlayerVitals,
        hydration: NativePlayerHydration,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position = self
            .register_player_at_available_position_with_vitals_equipment_containers_and_progression(
                player,
                vitals,
                hydration.progression,
                hydration.equipment,
                hydration.containers,
                world_map,
            )?;
        self.replace_player_progression_attempts(player_id, hydration.progression_attempts)?;
        self.replace_player_town(player_id, hydration.town_id)?;
        self.replace_player_conditions(player_id, hydration.conditions)?;
        self.hydrate_player_respawn_state(player_id, hydration.respawn_state)?;
        Ok(position)
    }

    pub fn remove_player(&self, id: u64) -> Result<(), HostError> {
        self.lock()?.remove_player(id).map_err(HostError::Core)?;
        self.online_players.fetch_sub(1, Ordering::SeqCst);
        self.player_outfits
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .remove(&id);
        self.player_directions
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .remove(&id);
        self.mark_visibility_changed();
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    pub fn update_player_outfit(
        &self,
        player_id: u64,
        outfit: NativeOtClientClassicOutfit,
    ) -> Result<(), HostError> {
        self.player_and_vitals(player_id)?;
        self.player_outfits
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .insert(player_id, outfit);
        self.mark_visibility_changed();
        Ok(())
    }
    pub fn update_player_facing(
        &self,
        player_id: u64,
        facing: NativeOtClientCardinalDirection,
    ) -> Result<(), HostError> {
        self.player_and_vitals(player_id)?;
        self.player_directions
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .insert(player_id, facing.protocol_direction());
        self.mark_visibility_changed();
        Ok(())
    }
}
