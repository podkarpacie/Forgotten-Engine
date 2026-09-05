//! Player interaction intents and viewport: item-use validation (map, ex, creature),
//! fight-mode state, declarative melee event resolution, target/follow setters, and
//! visible-player snapshot construction for the native viewport.

use super::*;

impl SharedNativeWorld {
    /// Validates a server-owned map-item use intent under the shared world lock. This exposes no
    /// client route and does not execute an item action, mutate map state, persist data, or claim
    /// doors, switches, container, script, or protocol behavior.
    pub fn validate_player_item_use(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseIntent,
    ) -> Result<PlayerItemUseOutcome, HostError> {
        self.lock()?
            .validate_player_item_use(map, intent)
            .map_err(HostError::Core)
    }

    /// Validates two server-owned map-item references under the same shared-world lock. It does
    /// not execute an item action, mutate map state, persist data, or produce client packets.
    pub fn validate_player_item_use_ex(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseExIntent,
    ) -> Result<PlayerItemUseExOutcome, HostError> {
        self.lock()?
            .validate_player_item_use_ex(map, intent)
            .map_err(HostError::Core)
    }

    /// Validates one server-owned map item and one authoritative creature under the shared-world
    /// lock. It does not select or affect the target, execute an item action, mutate state,
    /// persist data, advance an epoch, or emit client packets.
    pub fn validate_player_item_use_creature(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseCreatureIntent,
    ) -> Result<PlayerItemUseCreatureOutcome, HostError> {
        self.lock()?
            .validate_player_item_use_creature(map, intent)
            .map_err(HostError::Core)
    }

    pub fn player_interaction_intent(
        &self,
        player_id: u64,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .player_interaction_intent(player_id)
            .map_err(HostError::Core)
    }

    pub fn player_fight_mode_state(
        &self,
        player_id: u64,
    ) -> Result<PlayerFightModeState, HostError> {
        self.lock()?
            .player_fight_mode_state(player_id)
            .map_err(HostError::Core)
    }

    /// Replaces one parsed native fight-mode request through the authoritative core boundary.
    /// This does not change combat formulas, pursuit, persistence, or client output.
    pub fn replace_player_fight_mode_state(
        &self,
        player_id: u64,
        state: PlayerFightModeState,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_fight_mode_state(player_id, state)
            .map_err(HostError::Core)
    }

    /// Builds a typed event only when the authoritative right-hand equipment slot contains an
    /// item declared in the operator-owned scriptless catalog. The client never supplies an item
    /// identifier to this path, and missing or unknown items intentionally produce no event.
    pub fn equipped_declarative_melee_event(
        &self,
        attacker_id: u64,
        target_id: u64,
        catalog: &DeclarativeWeaponCatalog,
    ) -> Result<Option<PlayerCombatEvent>, HostError> {
        let world = self.lock()?;
        let Some(item) = world
            .player_equipment(attacker_id)
            .map_err(HostError::Core)?
            .item(EquipmentSlot::RightHand)
        else {
            return Ok(None);
        };
        catalog
            .get(item.server_id)
            .map(|definition| {
                definition
                    .adjacent_melee_event(attacker_id, target_id)
                    .map_err(|_| {
                        HostError::InvalidConfiguration(
                            "validated declarative weapon did not build a combat event".into(),
                        )
                    })
            })
            .transpose()
    }

    pub fn set_player_target(
        &self,
        player_id: u64,
        target_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_target(player_id, target_player_id)
            .map_err(HostError::Core)
    }

    pub fn set_player_static_target(
        &self,
        player_id: u64,
        target_static_creature_id: Option<u32>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_static_target(player_id, target_static_creature_id)
            .map_err(HostError::Core)
    }

    pub fn set_player_follow(
        &self,
        player_id: u64,
        follow_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_follow(player_id, follow_player_id)
            .map_err(HostError::Core)
    }

    pub fn visible_players(
        &self,
        observer_id: u64,
        look_type: u8,
        speed: u16,
    ) -> Result<Vec<NativeOtClientVisiblePlayer>, HostError> {
        let (player_snapshots, invisible_ids) = {
            let world = self.lock()?;
            (
                world.player_render_snapshots(),
                world.invisible_player_ids(),
            )
        };
        let invisible_ids: std::collections::BTreeSet<u64> = invisible_ids.into_iter().collect();
        let player_outfits = self
            .player_outfits
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let player_directions = self
            .player_directions
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        player_snapshots
            .into_iter()
            // /invisible players are hidden from every other player's viewport.
            .filter(|player| player.id != observer_id && !invisible_ids.contains(&player.id))
            .map(|player| {
                Ok(NativeOtClientVisiblePlayer {
                    player_id: native_player_id(player.id)?,
                    name: player.name,
                    position: native_position(player.position),
                    health_percent: player.health_percent,
                    outfit: player_outfits.get(&player.id).copied().unwrap_or(
                        NativeOtClientClassicOutfit {
                            look_type,
                            head: 0,
                            body: 0,
                            legs: 0,
                            feet: 0,
                        },
                    ),
                    direction: player_directions
                        .get(&player.id)
                        .copied()
                        .unwrap_or(NativeOtClientCardinalDirection::South.protocol_direction()),
                    speed,
                })
            })
            .collect()
    }
}
