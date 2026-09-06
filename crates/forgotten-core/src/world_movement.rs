//! Player movement on the authoritative world state: destination moves, follow-target
//! stepping, cardinal moves, and teleportation. All methods hold the world lock and
//! enforce occupancy, walkability, and death-state invariants.

use super::*;

impl WorldState {
    pub fn move_player(&mut self, id: u64, destination: Position) -> Result<(), CoreError> {
        if self.player_respawn_state(id)?.dead {
            return Err(CoreError::PlayerIsDead(id));
        }
        if self.is_static_creature_occupied(destination) {
            return Err(CoreError::StaticCreatureOccupiesPosition(destination));
        }
        {
            let player = self
                .players
                .get_mut(&id)
                .ok_or(CoreError::UnknownPlayer(id))?;
            if !player.position.is_adjacent_to(destination) {
                return Err(CoreError::InvalidMovement {
                    from: player.position,
                    to: destination,
                });
            }
            player.position = destination;
        }
        self.mark_changed();
        Ok(())
    }

    /// Executes one deterministic follow pass over current player follow intents. Each living
    /// source may take at most one direct cardinal distance-reducing step toward a living player
    /// target. This does not pathfind, retry blocked routes, move diagonally, attack, or change
    /// interaction state.
    pub fn follow_player_targets_once(
        &mut self,
        world_map: &WorldMap,
    ) -> Result<BTreeSet<u64>, CoreError> {
        let player_ids = self.players.keys().copied().collect::<Vec<_>>();
        let mut moved_player_ids = BTreeSet::new();
        for player_id in player_ids {
            let Some(target_player_id) = self
                .player_interactions
                .get(&player_id)
                .and_then(|intent| intent.follow_player_id)
            else {
                continue;
            };
            let Some(source) = self.players.get(&player_id).cloned() else {
                continue;
            };
            let Some(target) = self.players.get(&target_player_id).cloned() else {
                continue;
            };
            if self.player_respawn_state(player_id)?.dead
                || self.player_respawn_state(target_player_id)?.dead
                || source.position.is_adjacent_to(target.position)
                || source.position.z != target.position.z
            {
                continue;
            }
            let x_distance = source.position.x.abs_diff(target.position.x);
            let y_distance = source.position.y.abs_diff(target.position.y);
            let x_direction = match target.position.x.cmp(&source.position.x) {
                std::cmp::Ordering::Less => Some(CardinalDirection::West),
                std::cmp::Ordering::Greater => Some(CardinalDirection::East),
                std::cmp::Ordering::Equal => None,
            };
            let y_direction = match target.position.y.cmp(&source.position.y) {
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
                let destination = source.position.step(direction)?;
                if !world_map.is_walkable(destination)
                    || self.is_static_creature_occupied(destination)
                    || self
                        .players
                        .values()
                        .any(|player| player.id != player_id && player.position == destination)
                {
                    continue;
                }
                self.move_player_cardinal(player_id, direction)?;
                moved_player_ids.insert(player_id);
                break;
            }
        }
        Ok(moved_player_ids)
    }

    pub fn move_player_cardinal(
        &mut self,
        id: u64,
        direction: CardinalDirection,
    ) -> Result<(Position, Position), CoreError> {
        let from = self
            .player(id)
            .ok_or(CoreError::UnknownPlayer(id))?
            .position;
        let to = from.step(direction)?;
        self.move_player(id, to)?;
        Ok((from, to))
    }

    /// Moves one living player to an explicit server-owned destination after the caller has
    /// validated the destination's map semantics. This is intentionally distinct from ordinary
    /// adjacent movement and retains both player and active static-creature occupancy guards.
    pub fn teleport_player(
        &mut self,
        id: u64,
        destination: Position,
    ) -> Result<(Position, Position), CoreError> {
        if self.player_respawn_state(id)?.dead {
            return Err(CoreError::PlayerIsDead(id));
        }
        if self.is_static_creature_occupied(destination) {
            return Err(CoreError::StaticCreatureOccupiesPosition(destination));
        }
        if self
            .players
            .values()
            .any(|player| player.id != id && player.position == destination)
        {
            return Err(CoreError::PlayerOccupiesPosition(destination));
        }
        let player = self
            .players
            .get_mut(&id)
            .ok_or(CoreError::UnknownPlayer(id))?;
        let source = player.position;
        player.position = destination;
        self.mark_changed();
        Ok((source, destination))
    }

    pub fn empty_world_viewport(
        &self,
        id: u64,
        manifest: EmptyWorldManifest,
    ) -> Result<EmptyWorldViewport, CoreError> {
        let player = self.player(id).ok_or(CoreError::UnknownPlayer(id))?;
        Ok(EmptyWorldViewport {
            tick: self.tick,
            center: player.position,
            manifest,
        })
    }
}
