//! Deterministic domain primitives for Forgotten Engine.

use std::collections::BTreeMap;

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
        self.players.insert(player.id, player);
        Ok(())
    }

    pub fn player(&self, id: u64) -> Option<&Player> {
        self.players.get(&id)
    }

    pub fn move_player(&mut self, id: u64, destination: Position) -> Result<(), CoreError> {
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
}
