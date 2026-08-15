//! Deterministic domain primitives for Forgotten Engine.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    Offline,
    Starting,
    Online,
    Stopping,
    Failed,
}

impl ServerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Starting => "starting",
            Self::Online => "online",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }

    pub fn apply(self, command: LifecycleCommand) -> Result<Self, CoreError> {
        match (self, command) {
            (Self::Offline | Self::Failed, LifecycleCommand::Start) => Ok(Self::Starting),
            (Self::Starting, LifecycleCommand::Ready) => Ok(Self::Online),
            (Self::Online, LifecycleCommand::Stop) => Ok(Self::Stopping),
            (Self::Stopping, LifecycleCommand::Stopped) => Ok(Self::Offline),
            (Self::Online, LifecycleCommand::Restart) => Ok(Self::Stopping),
            (_, LifecycleCommand::Fail) => Ok(Self::Failed),
            _ => Err(CoreError::InvalidTransition {
                state: self,
                command,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleCommand {
    Start,
    Ready,
    Stop,
    Stopped,
    Restart,
    Fail,
}

impl LifecycleCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Ready => "ready",
            Self::Stop => "stop",
            Self::Stopped => "stopped",
            Self::Restart => "restart",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub x: u16,
    pub y: u16,
    pub z: u8,
}

impl Position {
    pub fn is_adjacent_to(self, other: Self) -> bool {
        self.z == other.z
            && self.x.abs_diff(other.x) <= 1
            && self.y.abs_diff(other.y) <= 1
            && self != other
    }

    pub fn step(self, direction: CardinalDirection) -> Result<Self, CoreError> {
        let (x, y) = match direction {
            CardinalDirection::North => (Some(self.x), self.y.checked_sub(1)),
            CardinalDirection::East => (self.x.checked_add(1), Some(self.y)),
            CardinalDirection::South => (Some(self.x), self.y.checked_add(1)),
            CardinalDirection::West => (self.x.checked_sub(1), Some(self.y)),
        };
        match (x, y) {
            (Some(x), Some(y)) => Ok(Self { x, y, z: self.z }),
            _ => Err(CoreError::MapBoundary { position: self }),
        }
    }
}

pub const MAX_WORLD_MAP_TILES: usize = 65_536;
pub const MAX_WORLD_MAP_ITEMS_PER_TILE: usize = 64;
pub const MAX_WORLD_MAP_TOWNS: usize = 8_192;
pub const MAX_WORLD_MAP_WAYPOINTS: usize = 8_192;
pub const MAX_TFS_STATIC_SPAWNS: usize = 65_536;

/// A deterministic, display-only entity materialized from a verified private TFS spawn record.
/// It intentionally excludes AI, combat, movement scheduling, Lua state, and lifecycle behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeTfsStaticEntity {
    pub id: u32,
    pub name: String,
    pub position: Position,
    pub look_type: u8,
    pub head: u8,
    pub body: u8,
    pub legs: u8,
    pub feet: u8,
    pub addons: u8,
    pub speed: u16,
    pub health_percent: u8,
    pub direction: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeTfsStaticSpawnCollection {
    pub entities: Vec<FeTfsStaticEntity>,
}

impl FeTfsStaticSpawnCollection {
    pub fn new(entities: Vec<FeTfsStaticEntity>) -> Result<Self, CoreError> {
        if entities.len() > MAX_TFS_STATIC_SPAWNS {
            return Err(CoreError::StaticSpawnLimit(MAX_TFS_STATIC_SPAWNS));
        }
        let mut ids = std::collections::BTreeSet::new();
        for entity in &entities {
            if entity.name.trim().is_empty() {
                return Err(CoreError::EmptyStaticSpawnName);
            }
            if !ids.insert(entity.id) {
                return Err(CoreError::DuplicateStaticSpawnId(entity.id));
            }
        }
        Ok(Self { entities })
    }

    pub fn at(&self, position: Position) -> impl Iterator<Item = &FeTfsStaticEntity> {
        self.entities
            .iter()
            .filter(move |entity| entity.position == position)
    }
}

/// Immutable spawn identity with the bounded lifecycle state needed for future gameplay slices.
/// Static creatures do not move, respawn on timers, execute scripts, or perform AI here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticCreatureLifecycle {
    pub id: u32,
    pub spawn_position: Position,
    pub position: Position,
    pub active: bool,
    pub activated_at_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureResetSummary {
    pub reactivated: usize,
    pub deferred_by_player_occupancy: usize,
    pub deferred_by_static_creature_occupancy: usize,
}

#[derive(Debug, Clone)]
struct StaticCreatureRuntime {
    entity: FeTfsStaticEntity,
    spawn_position: Position,
    active: bool,
    activated_at_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldMapTile {
    pub ground_thing_id: u16,
    pub walkable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMapItem {
    /// The operator-provided server item identifier, not a redistributed client asset.
    pub server_id: u16,
    /// The mapped client thing identifier, when the operator supplied an OTB definition.
    pub client_thing_id: Option<u16>,
    pub count: u8,
    pub action_id: Option<u16>,
    pub unique_id: Option<u16>,
    pub text: Option<String>,
    pub description: Option<String>,
    pub teleport_destination: Option<Position>,
    pub duration: Option<u32>,
    pub charges: Option<u16>,
    pub children: Vec<WorldMapItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtbmMapHeader {
    pub version: u32,
    pub width: u16,
    pub height: u16,
    pub item_major_version: u32,
    pub item_minor_version: u32,
    pub description: Option<String>,
    pub spawn_file: Option<String>,
    pub house_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldMapSource {
    FeMapV1,
    Otbm(OtbmMapHeader),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMapTown {
    pub id: u32,
    pub name: String,
    pub temple_position: Position,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMap {
    identifier: String,
    spawn: Position,
    tiles: BTreeMap<Position, WorldMapTile>,
    source: WorldMapSource,
    tile_items: BTreeMap<Position, Vec<WorldMapItem>>,
    tile_flags: BTreeMap<Position, u32>,
    house_tiles: BTreeMap<Position, u32>,
    towns: BTreeMap<u32, WorldMapTown>,
    waypoints: BTreeMap<String, Position>,
}

impl WorldMap {
    pub fn new(identifier: impl Into<String>, spawn: Position) -> Self {
        Self {
            identifier: identifier.into(),
            spawn,
            tiles: BTreeMap::new(),
            source: WorldMapSource::FeMapV1,
            tile_items: BTreeMap::new(),
            tile_flags: BTreeMap::new(),
            house_tiles: BTreeMap::new(),
            towns: BTreeMap::new(),
            waypoints: BTreeMap::new(),
        }
    }

    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    pub fn spawn(&self) -> Position {
        self.spawn
    }

    pub fn set_spawn(&mut self, spawn: Position) {
        self.spawn = spawn;
    }

    pub fn source(&self) -> &WorldMapSource {
        &self.source
    }

    pub fn set_source(&mut self, source: WorldMapSource) {
        self.source = source;
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn tiles(&self) -> impl Iterator<Item = (Position, WorldMapTile)> + '_ {
        self.tiles.iter().map(|(position, tile)| (*position, *tile))
    }

    pub fn tile(&self, position: Position) -> Option<WorldMapTile> {
        self.tiles.get(&position).copied()
    }

    pub fn is_walkable(&self, position: Position) -> bool {
        self.tile(position)
            .map(|tile| tile.walkable)
            .unwrap_or(false)
    }

    pub fn first_walkable_position(&self) -> Option<Position> {
        self.tiles
            .iter()
            .find_map(|(position, tile)| tile.walkable.then_some(*position))
    }

    pub fn set_tile(&mut self, position: Position, tile: WorldMapTile) -> Result<(), CoreError> {
        if !self.tiles.contains_key(&position) && self.tiles.len() >= MAX_WORLD_MAP_TILES {
            return Err(CoreError::MapTileLimit(MAX_WORLD_MAP_TILES));
        }
        self.tiles.insert(position, tile);
        Ok(())
    }

    pub fn tile_items(&self, position: Position) -> Option<&[WorldMapItem]> {
        self.tile_items.get(&position).map(Vec::as_slice)
    }

    pub fn tile_item_entries(&self) -> impl Iterator<Item = (Position, &[WorldMapItem])> + '_ {
        self.tile_items
            .iter()
            .map(|(position, items)| (*position, items.as_slice()))
    }

    pub fn set_tile_items(
        &mut self,
        position: Position,
        items: Vec<WorldMapItem>,
    ) -> Result<(), CoreError> {
        if items.len() > MAX_WORLD_MAP_ITEMS_PER_TILE {
            return Err(CoreError::MapTileItemLimit(MAX_WORLD_MAP_ITEMS_PER_TILE));
        }
        self.tile_items.insert(position, items);
        Ok(())
    }

    pub fn tile_flags(&self, position: Position) -> u32 {
        self.tile_flags.get(&position).copied().unwrap_or_default()
    }

    pub fn tile_flag_entries(&self) -> impl Iterator<Item = (Position, u32)> + '_ {
        self.tile_flags
            .iter()
            .map(|(position, flags)| (*position, *flags))
    }

    pub fn set_tile_flags(&mut self, position: Position, flags: u32) {
        if flags == 0 {
            self.tile_flags.remove(&position);
        } else {
            self.tile_flags.insert(position, flags);
        }
    }

    pub fn house_tile_id(&self, position: Position) -> Option<u32> {
        self.house_tiles.get(&position).copied()
    }

    pub fn house_tile_entries(&self) -> impl Iterator<Item = (Position, u32)> + '_ {
        self.house_tiles
            .iter()
            .map(|(position, house_id)| (*position, *house_id))
    }

    pub fn set_house_tile(&mut self, position: Position, house_id: u32) -> Result<(), CoreError> {
        if house_id == 0 {
            return Err(CoreError::InvalidMap("house IDs must be nonzero".into()));
        }
        self.house_tiles.insert(position, house_id);
        Ok(())
    }

    pub fn towns(&self) -> impl Iterator<Item = &WorldMapTown> {
        self.towns.values()
    }

    pub fn set_town(&mut self, town: WorldMapTown) -> Result<(), CoreError> {
        if town.id == 0 || town.name.trim().is_empty() {
            return Err(CoreError::InvalidMap(
                "town IDs must be nonzero and town names cannot be empty".into(),
            ));
        }
        if !self.towns.contains_key(&town.id) && self.towns.len() >= MAX_WORLD_MAP_TOWNS {
            return Err(CoreError::MapTownLimit(MAX_WORLD_MAP_TOWNS));
        }
        self.towns.insert(town.id, town);
        Ok(())
    }

    pub fn waypoint(&self, name: &str) -> Option<Position> {
        self.waypoints.get(name).copied()
    }

    pub fn waypoints(&self) -> impl Iterator<Item = (&str, Position)> + '_ {
        self.waypoints
            .iter()
            .map(|(name, position)| (name.as_str(), *position))
    }

    pub fn set_waypoint(
        &mut self,
        name: impl Into<String>,
        position: Position,
    ) -> Result<(), CoreError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(CoreError::InvalidMap(
                "waypoint names cannot be empty".into(),
            ));
        }
        if !self.waypoints.contains_key(&name) && self.waypoints.len() >= MAX_WORLD_MAP_WAYPOINTS {
            return Err(CoreError::MapWaypointLimit(MAX_WORLD_MAP_WAYPOINTS));
        }
        self.waypoints.insert(name, position);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.identifier.trim().is_empty() {
            return Err(CoreError::InvalidMap(
                "map identifier cannot be empty".into(),
            ));
        }
        if self.tiles.is_empty() {
            return Err(CoreError::InvalidMap(
                "map must contain at least one tile".into(),
            ));
        }
        if !self.is_walkable(self.spawn) {
            return Err(CoreError::InvalidMap(
                "map spawn must reference a walkable tile".into(),
            ));
        }
        for position in self.tile_items.keys() {
            if !self.tiles.contains_key(position) {
                return Err(CoreError::InvalidMap(format!(
                    "tile item stack references missing tile at {},{},{}",
                    position.x, position.y, position.z
                )));
            }
        }
        for position in self.tile_flags.keys() {
            if !self.tiles.contains_key(position) {
                return Err(CoreError::InvalidMap(format!(
                    "tile flags reference missing tile at {},{},{}",
                    position.x, position.y, position.z
                )));
            }
        }
        for position in self.house_tiles.keys() {
            if !self.tiles.contains_key(position) {
                return Err(CoreError::InvalidMap(format!(
                    "house tile references missing tile at {},{},{}",
                    position.x, position.y, position.z
                )));
            }
        }
        if let WorldMapSource::Otbm(header) = &self.source {
            if !(1..=2).contains(&header.version) || header.width == 0 || header.height == 0 {
                return Err(CoreError::InvalidMap(
                    "OTBM source metadata has an unsupported version or empty dimensions".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardinalDirection {
    North,
    East,
    South,
    West,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyWorldManifest {
    pub identifier: String,
    pub viewport_radius_x: u8,
    pub viewport_radius_y: u8,
}

impl Default for EmptyWorldManifest {
    fn default() -> Self {
        Self {
            identifier: "fe.empty-world.v1".into(),
            viewport_radius_x: 8,
            viewport_radius_y: 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyWorldViewport {
    pub tick: u64,
    pub center: Position,
    pub manifest: EmptyWorldManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    pub id: u64,
    pub account_id: u64,
    pub name: String,
    pub position: Position,
    pub level: u32,
    pub experience: u64,
    pub skill_points: u32,
}

impl Player {
    pub fn add_experience(&mut self, amount: u64) {
        self.experience = self.experience.saturating_add(amount);
        self.level = level_for_experience(self.experience);
    }
}

#[derive(Debug, Default)]
pub struct WorldState {
    players: BTreeMap<u64, Player>,
    static_creatures: BTreeMap<u32, StaticCreatureRuntime>,
    static_occupied_positions: BTreeSet<Position>,
    tick: u64,
}

impl WorldState {
    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    pub fn add_player(&mut self, player: Player) -> Result<(), CoreError> {
        if player.name.trim().is_empty() {
            return Err(CoreError::EmptyPlayerName);
        }
        if self.players.contains_key(&player.id) {
            return Err(CoreError::DuplicatePlayer(player.id));
        }
        if self.is_static_creature_occupied(player.position) {
            return Err(CoreError::StaticCreatureOccupiesPosition(player.position));
        }
        self.players.insert(player.id, player);
        Ok(())
    }

    /// Replaces the immutable display-only static creature set. This intentionally carries no
    /// respawn, AI, combat, movement, script, or lifecycle scheduling behavior.
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
                        spawn_position: entity.position,
                        active: true,
                        activated_at_tick: self.tick,
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
        Ok(())
    }

    pub fn static_creature_count(&self) -> usize {
        self.static_creatures.len()
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
                activated_at_tick: runtime.activated_at_tick,
            })
    }

    pub fn active_static_creature_count(&self) -> usize {
        self.static_creatures
            .values()
            .filter(|runtime| runtime.active)
            .count()
    }

    /// Returns only active authoritative entities for a protocol viewport. This derives a
    /// temporary immutable collection rather than exposing runtime mutation through the codec.
    pub fn active_static_spawn_collection(&self) -> FeTfsStaticSpawnCollection {
        FeTfsStaticSpawnCollection {
            entities: self
                .static_creatures
                .values()
                .filter(|runtime| runtime.active)
                .map(|runtime| runtime.entity.clone())
                .collect(),
        }
    }

    /// Marks a static creature inactive without moving it or scheduling any respawn behavior.
    pub fn deactivate_static_creature(&mut self, id: u32) -> Result<bool, CoreError> {
        let runtime = self
            .static_creatures
            .get_mut(&id)
            .ok_or(CoreError::UnknownStaticCreature(id))?;
        let changed = runtime.active;
        runtime.active = false;
        self.refresh_static_creature_occupancy();
        Ok(changed)
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
            runtime.active = true;
            runtime.activated_at_tick = self.tick;
            active_positions.insert(runtime.entity.position);
            summary.reactivated += 1;
        }
        self.refresh_static_creature_occupancy();
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

    pub fn player(&self, id: u64) -> Option<&Player> {
        self.players.get(&id)
    }

    pub fn move_player(&mut self, id: u64, destination: Position) -> Result<(), CoreError> {
        if self.is_static_creature_occupied(destination) {
            return Err(CoreError::StaticCreatureOccupiesPosition(destination));
        }
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
        Ok(())
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

pub fn level_for_experience(experience: u64) -> u32 {
    // A stable progression curve for the MVP. The exact historical formula is a future protocol test concern.
    1 + (((experience / 100) as f64).sqrt() as u32)
}

#[derive(Debug, PartialEq, Eq)]
pub enum CoreError {
    DuplicatePlayer(u64),
    EmptyPlayerName,
    InvalidMovement {
        from: Position,
        to: Position,
    },
    MapBoundary {
        position: Position,
    },
    MapTileLimit(usize),
    MapTileItemLimit(usize),
    MapTownLimit(usize),
    MapWaypointLimit(usize),
    StaticSpawnLimit(usize),
    DuplicateStaticSpawnId(u32),
    EmptyStaticSpawnName,
    StaticCreatureOccupiesPosition(Position),
    PlayerOccupiesStaticCreaturePosition(Position),
    UnknownStaticCreature(u32),
    InactiveStaticCreature(u32),
    StaticCreatureMovementBlocked(Position),
    InvalidMap(String),
    InvalidTransition {
        state: ServerStatus,
        command: LifecycleCommand,
    },
    UnknownPlayer(u64),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> Player {
        Player {
            id: 7,
            account_id: 2,
            name: "Knight".to_owned(),
            position: Position {
                x: 100,
                y: 100,
                z: 7,
            },
            level: 1,
            experience: 0,
            skill_points: 0,
        }
    }

    #[test]
    fn lifecycle_is_explicit_and_safe() {
        assert_eq!(
            ServerStatus::Offline.apply(LifecycleCommand::Start),
            Ok(ServerStatus::Starting)
        );
        assert_eq!(
            ServerStatus::Starting.apply(LifecycleCommand::Ready),
            Ok(ServerStatus::Online)
        );
        assert!(ServerStatus::Online.apply(LifecycleCommand::Ready).is_err());
    }

    #[test]
    fn movement_rejects_teleporting() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        assert!(world
            .move_player(
                7,
                Position {
                    x: 102,
                    y: 100,
                    z: 7
                }
            )
            .is_err());
        world
            .move_player(
                7,
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            )
            .unwrap();
    }

    #[test]
    fn empty_world_tick_viewport_and_cardinal_movement_are_deterministic() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        assert_eq!(world.advance_tick(), 1);
        let (from, to) = world
            .move_player_cardinal(7, CardinalDirection::East)
            .unwrap();
        assert_eq!(from.x, 100);
        assert_eq!(to.x, 101);
        let viewport = world
            .empty_world_viewport(7, EmptyWorldManifest::default())
            .unwrap();
        assert_eq!(viewport.tick, 1);
        assert_eq!(viewport.center, to);
        assert_eq!(viewport.manifest.identifier, "fe.empty-world.v1");
    }

    #[test]
    fn cardinal_movement_rejects_a_map_boundary() {
        let position = Position { x: 0, y: 0, z: 7 };
        assert!(matches!(
            position.step(CardinalDirection::North),
            Err(CoreError::MapBoundary { .. })
        ));
    }

    #[test]
    fn experience_increases_level_deterministically() {
        let mut value = player();
        value.add_experience(900);
        assert_eq!(value.level, 4);
    }

    #[test]
    fn legacy_world_metadata_is_bounded_and_preserved() {
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("legacy", spawn);
        map.set_tile(
            spawn,
            WorldMapTile {
                ground_thing_id: 4526,
                walkable: true,
            },
        )
        .unwrap();
        map.set_source(WorldMapSource::Otbm(OtbmMapHeader {
            version: 2,
            width: 512,
            height: 512,
            item_major_version: 57,
            item_minor_version: 1098,
            description: Some("private operator map".into()),
            spawn_file: Some("legacy-spawn.xml".into()),
            house_file: Some("legacy-house.xml".into()),
        }));
        map.set_tile_items(
            spawn,
            vec![WorldMapItem {
                server_id: 4526,
                client_thing_id: Some(102),
                count: 1,
                action_id: None,
                unique_id: None,
                text: None,
                description: None,
                teleport_destination: None,
                duration: None,
                charges: None,
                children: Vec::new(),
            }],
        )
        .unwrap();
        map.set_tile_flags(spawn, 1);
        map.set_house_tile(spawn, 42).unwrap();
        map.set_town(WorldMapTown {
            id: 1,
            name: "Thais".into(),
            temple_position: spawn,
        })
        .unwrap();
        map.set_waypoint("temple", spawn).unwrap();

        assert_eq!(map.tile_items(spawn).unwrap()[0].server_id, 4526);
        assert_eq!(map.tile_flags(spawn), 1);
        assert_eq!(map.house_tile_id(spawn), Some(42));
        assert_eq!(map.towns().next().unwrap().name, "Thais");
        assert_eq!(map.waypoint("temple"), Some(spawn));
        assert!(map.validate().is_ok());
        assert!(map.set_waypoint("", spawn).is_err());
        assert!(map.set_house_tile(spawn, 0).is_err());
    }

    #[test]
    fn legacy_tile_metadata_requires_an_existing_tile() {
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let missing = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("legacy", spawn);
        map.set_tile(
            spawn,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile_items(missing, Vec::new()).unwrap();
        assert!(matches!(map.validate(), Err(CoreError::InvalidMap(_))));
    }

    #[test]
    fn static_tfs_spawn_collection_is_bounded_and_position_addressable() {
        let position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let collection = FeTfsStaticSpawnCollection::new(vec![FeTfsStaticEntity {
            id: 0x4000_0001,
            name: "Rat".into(),
            position,
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        }])
        .unwrap();
        assert_eq!(collection.at(position).count(), 1);
        assert_eq!(
            FeTfsStaticSpawnCollection::new(vec![
                collection.entities[0].clone(),
                collection.entities[0].clone(),
            ]),
            Err(CoreError::DuplicateStaticSpawnId(0x4000_0001))
        );
    }

    #[test]
    fn static_creatures_are_authoritative_occupants_without_runtime_behavior() {
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: 0x4000_0001,
            name: "Rat".into(),
            position,
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let collection = FeTfsStaticSpawnCollection::new(vec![creature.clone()]).unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();
        assert_eq!(world.static_creature_count(), 1);
        assert_eq!(world.static_creature(creature.id), Some(&creature));
        assert!(world.is_static_creature_occupied(position));

        world.add_player(player()).unwrap();
        assert_eq!(
            world.move_player(
                7,
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ),
            Err(CoreError::StaticCreatureOccupiesPosition(position))
        );
        assert_eq!(world.player(7).unwrap().position.x, 100);
        assert_eq!(world.static_creature(creature.id), Some(&creature));
    }

    #[test]
    fn static_creature_reset_is_deterministic_and_defers_player_occupied_spawns() {
        let spawn_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: 0x4000_0001,
            name: "Rat".into(),
            position: spawn_position,
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let collection = FeTfsStaticSpawnCollection::new(vec![creature]).unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();
        assert_eq!(
            world.static_creature_lifecycle(0x4000_0001),
            Some(StaticCreatureLifecycle {
                id: 0x4000_0001,
                spawn_position,
                position: spawn_position,
                active: true,
                activated_at_tick: 0,
            })
        );
        assert!(world.deactivate_static_creature(0x4000_0001).unwrap());
        assert!(!world.deactivate_static_creature(0x4000_0001).unwrap());
        assert!(!world.is_static_creature_occupied(spawn_position));
        assert_eq!(world.active_static_creature_count(), 0);

        world.add_player(player()).unwrap();
        world
            .move_player_cardinal(7, CardinalDirection::East)
            .unwrap();
        world.advance_tick();
        let deferred = world.reset_static_creatures();
        assert_eq!(
            deferred,
            StaticCreatureResetSummary {
                reactivated: 0,
                deferred_by_player_occupancy: 1,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        assert!(!world.is_static_creature_occupied(spawn_position));

        world
            .move_player_cardinal(7, CardinalDirection::West)
            .unwrap();
        world.advance_tick();
        assert_eq!(
            world.reset_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 1,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        assert!(world.is_static_creature_occupied(spawn_position));
        assert_eq!(world.active_static_creature_count(), 1);
        assert_eq!(
            world
                .static_creature_lifecycle(0x4000_0001)
                .unwrap()
                .activated_at_tick,
            2
        );
        assert_eq!(
            world.deactivate_static_creature(0x4000_0002),
            Err(CoreError::UnknownStaticCreature(0x4000_0002))
        );
    }

    #[test]
    fn static_creature_moves_are_authoritative_bounded_and_render_active_only() {
        let mut map = WorldMap::new(
            "movement",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        for x in 100..=103 {
            map.set_tile(
                Position { x, y: 100, z: 7 },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: x != 103,
                },
            )
            .unwrap();
        }
        let first = FeTfsStaticEntity {
            id: 0x4000_0001,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let second = FeTfsStaticEntity {
            id: 0x4000_0002,
            name: "Snake".into(),
            position: Position {
                x: 102,
                y: 100,
                z: 7,
            },
            ..first.clone()
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(
                &FeTfsStaticSpawnCollection::new(vec![first.clone(), second.clone()]).unwrap(),
            )
            .unwrap();
        world.add_player(player()).unwrap();

        assert_eq!(
            world.move_static_creature_cardinal(0x4000_0001, CardinalDirection::East, &map),
            Err(CoreError::StaticCreatureOccupiesPosition(second.position))
        );
        world.deactivate_static_creature(second.id).unwrap();
        assert_eq!(
            world
                .move_static_creature_cardinal(0x4000_0001, CardinalDirection::East, &map)
                .unwrap(),
            (first.position, second.position)
        );
        assert!(!world.is_static_creature_occupied(first.position));
        assert!(world.is_static_creature_occupied(second.position));
        assert_eq!(world.active_static_spawn_collection().entities.len(), 1);
        assert_eq!(
            world.active_static_spawn_collection().entities[0].position,
            second.position
        );
        assert_eq!(
            world.move_static_creature_cardinal(0x4000_0001, CardinalDirection::East, &map),
            Err(CoreError::StaticCreatureMovementBlocked(Position {
                x: 103,
                y: 100,
                z: 7,
            }))
        );
        assert_eq!(
            world.move_static_creature_cardinal(0x4000_0002, CardinalDirection::West, &map),
            Err(CoreError::InactiveStaticCreature(0x4000_0002))
        );
    }

    #[test]
    fn static_creature_registration_rejects_existing_player_overlap() {
        let position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let collection = FeTfsStaticSpawnCollection::new(vec![FeTfsStaticEntity {
            id: 0x4000_0001,
            name: "Rat".into(),
            position,
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        }])
        .unwrap();
        assert_eq!(
            world.install_static_creatures(&collection),
            Err(CoreError::PlayerOccupiesStaticCreaturePosition(position))
        );
        assert_eq!(world.static_creature_count(), 0);
    }
}
