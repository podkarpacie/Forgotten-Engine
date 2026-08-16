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

/// Selection policy for externally triggered static creature movement. It is never enabled by
/// default and does not inspect targets, scripts, combat state, or pathfinding information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticCreatureDecisionPolicy {
    Disabled,
    ClockwiseAdjacent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureMoveDecision {
    pub creature_id: u32,
    pub direction: CardinalDirection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticCreatureDecisionBatch {
    pub decisions: Vec<StaticCreatureMoveDecision>,
    pub skipped: usize,
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

    pub fn town(&self, id: u32) -> Option<&WorldMapTown> {
        self.towns.get(&id)
    }

    pub fn temple_position_for_town(&self, id: u32) -> Option<Position> {
        self.town(id).map(|town| town.temple_position)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerVitals {
    pub health: u16,
    pub max_health: u16,
    pub mana: u16,
    pub max_mana: u16,
    pub capacity: u16,
    pub magic_level: u8,
}

impl PlayerVitals {
    pub fn is_valid(self) -> bool {
        self.max_health > 0 && self.health <= self.max_health && self.mana <= self.max_mana
    }
}

impl Default for PlayerVitals {
    fn default() -> Self {
        Self {
            health: 150,
            max_health: 150,
            mana: 50,
            max_mana: 50,
            capacity: 40_000,
            magic_level: 0,
        }
    }
}

/// The seven classic player skills occupy stable, explicit positions in the native protocol.
/// They remain typed throughout authoritative state rather than being represented by an
/// unvalidated string or integer map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlayerSkill {
    Fist,
    Club,
    Sword,
    Axe,
    Distance,
    Shielding,
    Fishing,
}

impl PlayerSkill {
    pub const ALL: [Self; 7] = [
        Self::Fist,
        Self::Club,
        Self::Sword,
        Self::Axe,
        Self::Distance,
        Self::Shielding,
        Self::Fishing,
    ];

    pub const fn code(self) -> u8 {
        match self {
            Self::Fist => 0,
            Self::Club => 1,
            Self::Sword => 2,
            Self::Axe => 3,
            Self::Distance => 4,
            Self::Shielding => 5,
            Self::Fishing => 6,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Fist),
            1 => Some(Self::Club),
            2 => Some(Self::Sword),
            3 => Some(Self::Axe),
            4 => Some(Self::Distance),
            5 => Some(Self::Shielding),
            6 => Some(Self::Fishing),
            _ => None,
        }
    }
}

/// The player-visible level and percentage progress for one named skill. The percentage is
/// deliberately bounded to the client-visible 0–100 range; skill tries and advancement formulas
/// are separate future runtime concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillProgress {
    pub level: u16,
    pub percent: u8,
}

impl SkillProgress {
    pub fn new(level: u16, percent: u8) -> Result<Self, CoreError> {
        if level == 0 || percent > 100 {
            return Err(CoreError::InvalidSkillProgress { level, percent });
        }
        Ok(Self { level, percent })
    }
}

impl Default for SkillProgress {
    fn default() -> Self {
        Self {
            level: 10,
            percent: 0,
        }
    }
}

/// A fixed, validated collection of all classic skills. The public iterator keeps the packet
/// ordering explicit and avoids leaking a mutable backing collection to callers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerSkills {
    values: [SkillProgress; 7],
}

impl PlayerSkills {
    pub fn skill(self, skill: PlayerSkill) -> SkillProgress {
        self.values[skill.code() as usize]
    }

    pub fn set(&mut self, skill: PlayerSkill, progress: SkillProgress) -> bool {
        let current = &mut self.values[skill.code() as usize];
        if *current == progress {
            return false;
        }
        *current = progress;
        true
    }

    pub fn iter(self) -> impl Iterator<Item = (PlayerSkill, SkillProgress)> {
        PlayerSkill::ALL
            .into_iter()
            .map(move |skill| (skill, self.skill(skill)))
    }
}

/// A numeric vocation identity. The numeric form remains extensible for operator-owned legacy
/// vocation registries, while `BaseVocation` exposes the portable five-identity foundation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct VocationId(u16);

impl VocationId {
    pub const NONE: Self = Self(0);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u16 {
        self.0
    }

    pub const fn base(self) -> Option<BaseVocation> {
        BaseVocation::from_id(self.0)
    }
}

/// The portable base-vocation identities. Promotion and custom vocation entries remain data-driven
/// registry work and are not implied by this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseVocation {
    None,
    Sorcerer,
    Druid,
    Paladin,
    Knight,
}

impl BaseVocation {
    pub const fn id(self) -> VocationId {
        match self {
            Self::None => VocationId::new(0),
            Self::Sorcerer => VocationId::new(1),
            Self::Druid => VocationId::new(2),
            Self::Paladin => VocationId::new(3),
            Self::Knight => VocationId::new(4),
        }
    }

    pub const fn from_id(id: u16) -> Option<Self> {
        match id {
            0 => Some(Self::None),
            1 => Some(Self::Sorcerer),
            2 => Some(Self::Druid),
            3 => Some(Self::Paladin),
            4 => Some(Self::Knight),
            _ => None,
        }
    }
}

/// Persisted player progression state that is independent of equipment, vitals, and map position.
/// It intentionally stores no formula multipliers: those are loaded later from a validated
/// operator-owned vocation registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerProgression {
    pub vocation: VocationId,
    pub skills: PlayerSkills,
}

pub const MAX_REGENERATION_ELAPSED_SECONDS: u16 = 60;

/// One bounded player-resource regeneration rule. Intervals are expressed in wall-clock seconds
/// by the host boundary; the core never assumes that a network heartbeat is itself a second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegenerationRule {
    pub interval_seconds: u16,
    pub amount: u16,
}

impl RegenerationRule {
    pub fn new(interval_seconds: u16, amount: u16) -> Result<Self, CoreError> {
        if interval_seconds == 0 {
            return Err(CoreError::InvalidRegenerationInterval);
        }
        Ok(Self {
            interval_seconds,
            amount,
        })
    }
}

/// A player's health and mana recovery rules. Soul recovery is intentionally deferred because
/// current persisted vitals do not yet own soul state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRegenerationRules {
    pub health: RegenerationRule,
    pub mana: RegenerationRule,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PlayerRegenerationSchedule {
    health_elapsed_seconds: u16,
    mana_elapsed_seconds: u16,
}

/// The observable result of a bounded regeneration application. An elapsed period can advance
/// schedule state without restoring a full resource, so only positive gains trigger world/vital
/// updates at the host boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRegenerationOutcome {
    pub player_id: u64,
    pub health_gained: u16,
    pub mana_gained: u16,
    pub vitals: PlayerVitals,
}

pub const MAX_CONDITION_DURATION_SECONDS: u16 = 60 * 60;

/// Bounded damage-over-time condition families. Their visual effects, immunity rules, Lua hooks,
/// and death policy remain separate protocol and scripting concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlayerConditionKind {
    Poison,
    Burning,
    Energy,
}

/// A single validated condition schedule. The condition is stored by kind, so applying the same
/// kind replaces its timing/damage record instead of creating an unbounded stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCondition {
    pub kind: PlayerConditionKind,
    pub interval_seconds: u16,
    pub damage: u16,
    pub remaining_seconds: u16,
    elapsed_seconds: u16,
}

impl PlayerCondition {
    pub fn new(
        kind: PlayerConditionKind,
        interval_seconds: u16,
        damage: u16,
        remaining_seconds: u16,
    ) -> Result<Self, CoreError> {
        if interval_seconds == 0 || damage == 0 || remaining_seconds == 0 {
            return Err(CoreError::InvalidPlayerCondition);
        }
        if remaining_seconds > MAX_CONDITION_DURATION_SECONDS {
            return Err(CoreError::InvalidPlayerCondition);
        }
        Ok(Self {
            kind,
            interval_seconds,
            damage,
            remaining_seconds,
            elapsed_seconds: 0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerConditionOutcome {
    pub player_id: u64,
    pub applied_damage: u16,
    pub remaining_health: u16,
    pub expired_conditions: u8,
}

/// Explicit configuration modes for character-death loss. The default-formula mode is retained
/// as data only until profile-specific compatibility evidence supports an implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathLossPolicy {
    DefaultFormula,
    None,
    FixedPercent(u8),
}

impl DeathLossPolicy {
    pub fn from_config(value: i32) -> Result<Self, CoreError> {
        match value {
            -1 => Ok(Self::DefaultFormula),
            0 => Ok(Self::None),
            1..=100 => Ok(Self::FixedPercent(value as u8)),
            _ => Err(CoreError::InvalidDeathLossPolicy),
        }
    }
}

pub const MAX_ITEM_STACK_COUNT: u16 = 100;

/// A bounded runtime instance of an operator-supplied item type. Map placement metadata remains
/// separate in `WorldMapItem`; this type is the foundation for player inventory, containers,
/// equipment, loot, and persistent attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInstance {
    pub server_id: u16,
    pub count: u16,
    pub action_id: Option<u16>,
    pub unique_id: Option<u16>,
}

impl ItemInstance {
    pub fn new(server_id: u16, count: u16) -> Result<Self, CoreError> {
        if server_id == 0 {
            return Err(CoreError::InvalidItemId(server_id));
        }
        if !(1..=MAX_ITEM_STACK_COUNT).contains(&count) {
            return Err(CoreError::InvalidItemStackCount(count));
        }
        Ok(Self {
            server_id,
            count,
            action_id: None,
            unique_id: None,
        })
    }
}

/// Presentation metadata validated from an operator-supplied item catalog. It deliberately holds
/// only the data needed to construct a classic client item record; gameplay properties stay in
/// the content/runtime layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeItemPresentation {
    pub client_thing_id: u16,
    pub requires_classic_740_subtype: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeItemPresentationCatalog {
    entries: BTreeMap<u16, NativeItemPresentation>,
}

impl NativeItemPresentationCatalog {
    pub fn insert(
        &mut self,
        server_id: u16,
        presentation: NativeItemPresentation,
    ) -> Result<(), CoreError> {
        if server_id == 0 {
            return Err(CoreError::InvalidItemId(server_id));
        }
        if presentation.client_thing_id == 0 {
            return Err(CoreError::InvalidClientThingId(server_id));
        }
        if self.entries.contains_key(&server_id) {
            return Err(CoreError::DuplicateItemPresentation(server_id));
        }
        self.entries.insert(server_id, presentation);
        Ok(())
    }

    pub fn presentation(&self, server_id: u16) -> Option<NativeItemPresentation> {
        self.entries.get(&server_id).copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The fixed equipment locations recognized by Tibia-style player inventories. Containers and
/// depot/inbox storage will be modeled separately because they need ordered item trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EquipmentSlot {
    Head,
    Neck,
    Backpack,
    Armor,
    RightHand,
    LeftHand,
    Legs,
    Feet,
    Ring,
    Ammo,
}

impl EquipmentSlot {
    pub const fn code(self) -> u8 {
        match self {
            Self::Head => 1,
            Self::Neck => 2,
            Self::Backpack => 3,
            Self::Armor => 4,
            Self::RightHand => 5,
            Self::LeftHand => 6,
            Self::Legs => 7,
            Self::Feet => 8,
            Self::Ring => 9,
            Self::Ammo => 10,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Head),
            2 => Some(Self::Neck),
            3 => Some(Self::Backpack),
            4 => Some(Self::Armor),
            5 => Some(Self::RightHand),
            6 => Some(Self::LeftHand),
            7 => Some(Self::Legs),
            8 => Some(Self::Feet),
            9 => Some(Self::Ring),
            10 => Some(Self::Ammo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerEquipment {
    items: BTreeMap<EquipmentSlot, ItemInstance>,
}

impl PlayerEquipment {
    pub fn item(&self, slot: EquipmentSlot) -> Option<&ItemInstance> {
        self.items.get(&slot)
    }

    pub fn equip(&mut self, slot: EquipmentSlot, item: ItemInstance) -> Option<ItemInstance> {
        self.items.insert(slot, item)
    }

    pub fn unequip(&mut self, slot: EquipmentSlot) -> Option<ItemInstance> {
        self.items.remove(&slot)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (EquipmentSlot, &ItemInstance)> + '_ {
        self.items.iter().map(|(slot, item)| (*slot, item))
    }
}

pub const MAX_CONTAINER_CAPACITY: u16 = 100;
pub const MAX_PLAYER_CONTAINERS: usize = 16;
pub const MAX_PLAYER_CONTAINER_NAME_BYTES: usize = 64;

/// Ordered bounded storage for validated item instances. Recursive containers, player ownership,
/// persistence, and client window synchronization are added by later inventory milestones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemContainer {
    capacity: u16,
    items: Vec<ItemInstance>,
}

impl ItemContainer {
    pub fn new(capacity: u16) -> Result<Self, CoreError> {
        if capacity == 0 || capacity > MAX_CONTAINER_CAPACITY {
            return Err(CoreError::InvalidContainerCapacity(capacity));
        }
        Ok(Self {
            capacity,
            items: Vec::new(),
        })
    }

    pub fn capacity(&self) -> u16 {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn item(&self, index: usize) -> Option<&ItemInstance> {
        self.items.get(index)
    }

    pub fn insert(&mut self, item: ItemInstance) -> Result<(), CoreError> {
        if self.items.len() >= usize::from(self.capacity) {
            return Err(CoreError::ContainerFull {
                capacity: self.capacity,
            });
        }
        self.items.push(item);
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Option<ItemInstance> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    pub fn iter(&self) -> impl Iterator<Item = &ItemInstance> + '_ {
        self.items.iter()
    }
}

/// One player-owned, non-recursive container window. The `container_id` is a client window
/// identifier; the runtime does not infer nested ownership or item-use semantics from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerContainer {
    pub container_id: u8,
    pub container_item: ItemInstance,
    pub name: String,
    pub has_parent: bool,
    pub items: ItemContainer,
}

impl PlayerContainer {
    pub fn new(
        container_id: u8,
        container_item: ItemInstance,
        name: impl Into<String>,
        has_parent: bool,
        capacity: u16,
    ) -> Result<Self, CoreError> {
        let name = name.into();
        if name.is_empty() || name.len() > MAX_PLAYER_CONTAINER_NAME_BYTES {
            return Err(CoreError::InvalidContainerName(name.len()));
        }
        Ok(Self {
            container_id,
            container_item,
            name,
            has_parent,
            items: ItemContainer::new(capacity)?,
        })
    }
}

/// Bounded player-owned container windows. Entries are keyed by their client window identifier;
/// an inserted matching identifier replaces the prior window without creating a duplicate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerContainers {
    containers: BTreeMap<u8, PlayerContainer>,
}

impl PlayerContainers {
    pub fn len(&self) -> usize {
        self.containers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.containers.is_empty()
    }

    pub fn container(&self, container_id: u8) -> Option<&PlayerContainer> {
        self.containers.get(&container_id)
    }

    pub fn insert(
        &mut self,
        container: PlayerContainer,
    ) -> Result<Option<PlayerContainer>, CoreError> {
        if !self.containers.contains_key(&container.container_id)
            && self.containers.len() >= MAX_PLAYER_CONTAINERS
        {
            return Err(CoreError::TooManyPlayerContainers(MAX_PLAYER_CONTAINERS));
        }
        Ok(self.containers.insert(container.container_id, container))
    }

    pub fn remove(&mut self, container_id: u8) -> Option<PlayerContainer> {
        self.containers.remove(&container_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, &PlayerContainer)> + '_ {
        self.containers
            .iter()
            .map(|(id, container)| (*id, container))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerDamageOutcome {
    pub attacker_id: u64,
    pub target_id: u64,
    pub requested_damage: u16,
    pub applied_damage: u16,
    pub remaining_health: u16,
    pub defeated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRenderSnapshot {
    pub id: u64,
    pub name: String,
    pub position: Position,
    pub level: u32,
}

/// Stored interaction intent only. It carries no attack resolution, automatic movement, combat,
/// scripting, spell, or action behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerInteractionIntent {
    pub target_player_id: Option<u64>,
    pub follow_player_id: Option<u64>,
}

#[derive(Debug, Default)]
pub struct WorldState {
    players: BTreeMap<u64, Player>,
    player_vitals: BTreeMap<u64, PlayerVitals>,
    player_progressions: BTreeMap<u64, PlayerProgression>,
    player_regeneration_schedules: BTreeMap<u64, PlayerRegenerationSchedule>,
    player_conditions: BTreeMap<u64, BTreeMap<PlayerConditionKind, PlayerCondition>>,
    player_equipments: BTreeMap<u64, PlayerEquipment>,
    player_containers: BTreeMap<u64, PlayerContainers>,
    player_interactions: BTreeMap<u64, PlayerInteractionIntent>,
    static_creatures: BTreeMap<u32, StaticCreatureRuntime>,
    static_occupied_positions: BTreeSet<Position>,
    tick: u64,
    revision: u64,
}

impl WorldState {
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Monotonically advances after an accepted authoritative world mutation. The revision is a
    /// generic synchronization baseline for future item, condition, combat, and creature events;
    /// profile-specific delivery still uses its dedicated visibility and vitals epochs.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.mark_changed();
        self.tick
    }

    pub fn add_player(&mut self, player: Player) -> Result<(), CoreError> {
        self.add_player_with_vitals(player, PlayerVitals::default())
    }

    pub fn add_player_with_vitals(
        &mut self,
        player: Player,
        vitals: PlayerVitals,
    ) -> Result<(), CoreError> {
        self.add_player_with_vitals_and_progression(player, vitals, PlayerProgression::default())
    }

    pub fn add_player_with_vitals_and_progression(
        &mut self,
        player: Player,
        vitals: PlayerVitals,
        progression: PlayerProgression,
    ) -> Result<(), CoreError> {
        if player.name.trim().is_empty() {
            return Err(CoreError::EmptyPlayerName);
        }
        if !vitals.is_valid() {
            return Err(CoreError::InvalidPlayerVitals(player.id));
        }
        if self.players.contains_key(&player.id) {
            return Err(CoreError::DuplicatePlayer(player.id));
        }
        if self
            .players
            .values()
            .any(|existing| existing.position == player.position)
        {
            return Err(CoreError::PlayerOccupiesPosition(player.position));
        }
        if self.is_static_creature_occupied(player.position) {
            return Err(CoreError::StaticCreatureOccupiesPosition(player.position));
        }
        self.player_vitals.insert(player.id, vitals);
        self.player_progressions.insert(player.id, progression);
        self.player_regeneration_schedules
            .insert(player.id, PlayerRegenerationSchedule::default());
        self.player_conditions.insert(player.id, BTreeMap::new());
        self.player_equipments
            .insert(player.id, PlayerEquipment::default());
        self.player_containers
            .insert(player.id, PlayerContainers::default());
        self.players.insert(player.id, player);
        self.mark_changed();
        Ok(())
    }

    pub fn remove_player(&mut self, id: u64) -> Result<Player, CoreError> {
        let player = self
            .players
            .remove(&id)
            .ok_or(CoreError::UnknownPlayer(id))?;
        self.player_vitals.remove(&id);
        self.player_progressions.remove(&id);
        self.player_regeneration_schedules.remove(&id);
        self.player_conditions.remove(&id);
        self.player_equipments.remove(&id);
        self.player_containers.remove(&id);
        self.player_interactions.remove(&id);
        self.player_interactions.retain(|_, intent| {
            if intent.target_player_id == Some(id) {
                intent.target_player_id = None;
            }
            if intent.follow_player_id == Some(id) {
                intent.follow_player_id = None;
            }
            intent.target_player_id.is_some() || intent.follow_player_id.is_some()
        });
        self.mark_changed();
        Ok(player)
    }

    pub fn is_player_occupied(&self, position: Position) -> bool {
        self.players
            .values()
            .any(|player| player.position == position)
    }

    pub fn player_render_snapshots(&self) -> Vec<PlayerRenderSnapshot> {
        self.players
            .values()
            .map(|player| PlayerRenderSnapshot {
                id: player.id,
                name: player.name.clone(),
                position: player.position,
                level: player.level,
            })
            .collect()
    }

    pub fn player_interaction_intent(
        &self,
        player_id: u64,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        Ok(self
            .player_interactions
            .get(&player_id)
            .copied()
            .unwrap_or_default())
    }

    pub fn set_player_target(
        &mut self,
        player_id: u64,
        target_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        self.set_player_interaction(player_id, target_player_id, None, true)
    }

    pub fn set_player_follow(
        &mut self,
        player_id: u64,
        follow_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        self.set_player_interaction(player_id, None, follow_player_id, false)
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
        self.mark_changed();
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
                .then_some(direction)
            });
            if let Some(direction) = selected {
                let destination = source.step(direction).expect("selected cardinal step");
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

    /// Marks a static creature inactive without moving it or scheduling any respawn behavior.
    pub fn deactivate_static_creature(&mut self, id: u32) -> Result<bool, CoreError> {
        let runtime = self
            .static_creatures
            .get_mut(&id)
            .ok_or(CoreError::UnknownStaticCreature(id))?;
        let changed = runtime.active;
        runtime.active = false;
        self.refresh_static_creature_occupancy();
        if changed {
            self.mark_changed();
        }
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

    fn mark_changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub fn player(&self, id: u64) -> Option<&Player> {
        self.players.get(&id)
    }

    pub fn player_vitals(&self, player_id: u64) -> Result<PlayerVitals, CoreError> {
        self.player_vitals
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_progression(&self, player_id: u64) -> Result<PlayerProgression, CoreError> {
        self.player_progressions
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_equipment(&self, player_id: u64) -> Result<&PlayerEquipment, CoreError> {
        self.player_equipments
            .get(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_containers(&self, player_id: u64) -> Result<&PlayerContainers, CoreError> {
        self.player_containers
            .get(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    /// Replaces a player's fixed equipment set only after that player is known to the
    /// authoritative world. Native inventory windows and container synchronization remain
    /// deferred, but later protocol paths can use this validated state directly.
    pub fn replace_player_equipment(
        &mut self,
        player_id: u64,
        equipment: PlayerEquipment,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_equipment(player_id)? == &equipment {
            return Ok(false);
        }
        self.player_equipments.insert(player_id, equipment);
        self.mark_changed();
        Ok(true)
    }

    /// Replaces a player's bounded non-recursive container collection after the player is known
    /// to the authoritative world. Persistence and native container-window synchronization are
    /// intentionally separate layers.
    pub fn replace_player_containers(
        &mut self,
        player_id: u64,
        containers: PlayerContainers,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_containers(player_id)? == &containers {
            return Ok(false);
        }
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(true)
    }

    pub fn update_player_vitals(
        &mut self,
        player_id: u64,
        vitals: PlayerVitals,
    ) -> Result<(), CoreError> {
        if !vitals.is_valid() {
            return Err(CoreError::InvalidPlayerVitals(player_id));
        }
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_vitals(player_id)? == vitals {
            return Ok(());
        }
        self.player_vitals.insert(player_id, vitals);
        self.mark_changed();
        Ok(())
    }

    /// Applies a bounded elapsed period to one player's configured health/mana schedules. The
    /// caller owns wall-clock measurement; this method admits only a capped duration and mutates
    /// authoritative vitals when a configured recovery event produces a positive gain.
    pub fn apply_player_regeneration(
        &mut self,
        player_id: u64,
        rules: PlayerRegenerationRules,
        elapsed_seconds: u16,
    ) -> Result<PlayerRegenerationOutcome, CoreError> {
        let current_vitals = self.player_vitals(player_id)?;
        if elapsed_seconds == 0 {
            return Ok(PlayerRegenerationOutcome {
                player_id,
                health_gained: 0,
                mana_gained: 0,
                vitals: current_vitals,
            });
        }
        let elapsed_seconds = elapsed_seconds.min(MAX_REGENERATION_ELAPSED_SECONDS);
        let schedule = self
            .player_regeneration_schedules
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        let health_events = advance_regeneration_schedule(
            &mut schedule.health_elapsed_seconds,
            rules.health.interval_seconds,
            elapsed_seconds,
        );
        let mana_events = advance_regeneration_schedule(
            &mut schedule.mana_elapsed_seconds,
            rules.mana.interval_seconds,
            elapsed_seconds,
        );
        let health_gained = regeneration_gain(
            current_vitals.health,
            current_vitals.max_health,
            rules.health.amount,
            health_events,
        );
        let mana_gained = regeneration_gain(
            current_vitals.mana,
            current_vitals.max_mana,
            rules.mana.amount,
            mana_events,
        );
        let vitals = PlayerVitals {
            health: current_vitals.health.saturating_add(health_gained),
            mana: current_vitals.mana.saturating_add(mana_gained),
            ..current_vitals
        };
        if health_gained > 0 || mana_gained > 0 {
            self.player_vitals.insert(player_id, vitals);
            self.mark_changed();
        }
        Ok(PlayerRegenerationOutcome {
            player_id,
            health_gained,
            mana_gained,
            vitals,
        })
    }

    pub fn player_conditions(
        &self,
        player_id: u64,
    ) -> Result<&BTreeMap<PlayerConditionKind, PlayerCondition>, CoreError> {
        self.player_conditions
            .get(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    /// Applies or replaces a single condition kind. Replacing a condition never creates an
    /// unbounded stack and is observable through the authoritative world revision.
    pub fn apply_player_condition(
        &mut self,
        player_id: u64,
        condition: PlayerCondition,
    ) -> Result<bool, CoreError> {
        let conditions = self
            .player_conditions
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        if conditions.get(&condition.kind) == Some(&condition) {
            return Ok(false);
        }
        conditions.insert(condition.kind, condition);
        self.mark_changed();
        Ok(true)
    }

    /// Advances condition schedules by bounded elapsed time. Damage is capped by current health;
    /// zero health is represented in the outcome but death/respawn policy remains deferred.
    pub fn apply_player_conditions(
        &mut self,
        player_id: u64,
        elapsed_seconds: u16,
    ) -> Result<PlayerConditionOutcome, CoreError> {
        let current_vitals = self.player_vitals(player_id)?;
        let elapsed_seconds = elapsed_seconds.min(MAX_REGENERATION_ELAPSED_SECONDS);
        let conditions = self
            .player_conditions
            .get_mut(&player_id)
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        if elapsed_seconds == 0 || conditions.is_empty() {
            return Ok(PlayerConditionOutcome {
                player_id,
                applied_damage: 0,
                remaining_health: current_vitals.health,
                expired_conditions: 0,
            });
        }
        let mut requested_damage = 0_u32;
        let mut expired = Vec::new();
        for (kind, condition) in conditions.iter_mut() {
            let active_seconds = elapsed_seconds.min(condition.remaining_seconds);
            let total = condition.elapsed_seconds.saturating_add(active_seconds);
            let events = total / condition.interval_seconds;
            condition.elapsed_seconds = total % condition.interval_seconds;
            condition.remaining_seconds =
                condition.remaining_seconds.saturating_sub(active_seconds);
            requested_damage = requested_damage
                .saturating_add(u32::from(events).saturating_mul(u32::from(condition.damage)));
            if condition.remaining_seconds == 0 {
                expired.push(*kind);
            }
        }
        for kind in &expired {
            conditions.remove(kind);
        }
        let applied_damage = requested_damage.min(u32::from(current_vitals.health)) as u16;
        let remaining_health = current_vitals.health.saturating_sub(applied_damage);
        if applied_damage > 0 {
            self.player_vitals.insert(
                player_id,
                PlayerVitals {
                    health: remaining_health,
                    ..current_vitals
                },
            );
        }
        if applied_damage > 0 || !expired.is_empty() {
            self.mark_changed();
        }
        Ok(PlayerConditionOutcome {
            player_id,
            applied_damage,
            remaining_health,
            expired_conditions: expired.len().min(usize::from(u8::MAX)) as u8,
        })
    }

    /// Replaces player vocation and all typed skills atomically in the authoritative world. No-op
    /// replacements do not advance the world revision, matching vitals/equipment semantics.
    pub fn replace_player_progression(
        &mut self,
        player_id: u64,
        progression: PlayerProgression,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_progression(player_id)? == progression {
            return Ok(false);
        }
        self.player_progressions.insert(player_id, progression);
        self.mark_changed();
        Ok(true)
    }

    pub fn apply_player_damage(
        &mut self,
        attacker_id: u64,
        target_id: u64,
        requested_damage: u16,
    ) -> Result<PlayerDamageOutcome, CoreError> {
        if attacker_id == target_id {
            return Err(CoreError::SelfInteractionNotAllowed(attacker_id));
        }
        if !self.players.contains_key(&attacker_id) {
            return Err(CoreError::UnknownPlayer(attacker_id));
        }
        if !self.players.contains_key(&target_id) {
            return Err(CoreError::UnknownPlayer(target_id));
        }
        let (applied_damage, remaining_health) = {
            let vitals = self
                .player_vitals
                .get_mut(&target_id)
                .ok_or(CoreError::UnknownPlayer(target_id))?;
            let applied_damage = requested_damage.min(vitals.health);
            vitals.health = vitals.health.saturating_sub(applied_damage);
            (applied_damage, vitals.health)
        };
        if applied_damage > 0 {
            self.mark_changed();
        }
        Ok(PlayerDamageOutcome {
            attacker_id,
            target_id,
            requested_damage,
            applied_damage,
            remaining_health,
            defeated: remaining_health == 0,
        })
    }

    pub fn apply_player_melee_damage(
        &mut self,
        attacker_id: u64,
        target_id: u64,
        requested_damage: u16,
    ) -> Result<PlayerDamageOutcome, CoreError> {
        let attacker = self
            .player(attacker_id)
            .ok_or(CoreError::UnknownPlayer(attacker_id))?;
        let target = self
            .player(target_id)
            .ok_or(CoreError::UnknownPlayer(target_id))?;
        if !attacker.position.is_adjacent_to(target.position) {
            return Err(CoreError::CombatOutOfRange {
                attacker_id,
                target_id,
            });
        }
        self.apply_player_damage(attacker_id, target_id, requested_damage)
    }

    pub fn move_player(&mut self, id: u64, destination: Position) -> Result<(), CoreError> {
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

    fn set_player_interaction(
        &mut self,
        player_id: u64,
        target_player_id: Option<u64>,
        follow_player_id: Option<u64>,
        replace_target: bool,
    ) -> Result<PlayerInteractionIntent, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        let previous = self.player_interaction_intent(player_id)?;
        let selected_player_id = if replace_target {
            target_player_id
        } else {
            follow_player_id
        };
        if let Some(selected_player_id) = selected_player_id {
            if selected_player_id == player_id {
                return Err(CoreError::SelfInteractionNotAllowed(player_id));
            }
            if !self.players.contains_key(&selected_player_id) {
                return Err(CoreError::UnknownPlayer(selected_player_id));
            }
        }

        let intent = {
            let intent = self.player_interactions.entry(player_id).or_default();
            if replace_target {
                intent.target_player_id = target_player_id;
            } else {
                intent.follow_player_id = follow_player_id;
            }
            *intent
        };
        if intent == PlayerInteractionIntent::default() {
            self.player_interactions.remove(&player_id);
        }
        if previous != intent {
            self.mark_changed();
        }
        Ok(intent)
    }
}

pub fn level_for_experience(experience: u64) -> u32 {
    // A stable progression curve for the MVP. The exact historical formula is a future protocol test concern.
    1 + (((experience / 100) as f64).sqrt() as u32)
}

fn advance_regeneration_schedule(elapsed: &mut u16, interval: u16, delta: u16) -> u16 {
    let total = elapsed.saturating_add(delta);
    let events = total / interval;
    *elapsed = total % interval;
    events
}

fn regeneration_gain(current: u16, maximum: u16, amount: u16, events: u16) -> u16 {
    let requested = u32::from(amount).saturating_mul(u32::from(events));
    current
        .saturating_add(requested.min(u32::from(u16::MAX)) as u16)
        .min(maximum)
        .saturating_sub(current)
}

#[derive(Debug, PartialEq, Eq)]
pub enum CoreError {
    DuplicatePlayer(u64),
    EmptyPlayerName,
    InvalidContainerCapacity(u16),
    InvalidContainerName(usize),
    TooManyPlayerContainers(usize),
    ContainerFull {
        capacity: u16,
    },
    InvalidItemId(u16),
    InvalidClientThingId(u16),
    DuplicateItemPresentation(u16),
    InvalidItemStackCount(u16),
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
    PlayerOccupiesPosition(Position),
    UnknownStaticCreature(u32),
    InactiveStaticCreature(u32),
    StaticCreatureMovementBlocked(Position),
    InvalidMap(String),
    InvalidTransition {
        state: ServerStatus,
        command: LifecycleCommand,
    },
    UnknownPlayer(u64),
    SelfInteractionNotAllowed(u64),
    InvalidPlayerVitals(u64),
    InvalidSkillProgress {
        level: u16,
        percent: u8,
    },
    InvalidRegenerationInterval,
    InvalidPlayerCondition,
    InvalidDeathLossPolicy,
    CombatOutOfRange {
        attacker_id: u64,
        target_id: u64,
    },
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
    fn runtime_item_instances_and_equipment_slots_are_bounded_and_replaceable() {
        assert_eq!(ItemInstance::new(0, 1), Err(CoreError::InvalidItemId(0)));
        assert_eq!(
            ItemInstance::new(2376, 0),
            Err(CoreError::InvalidItemStackCount(0))
        );
        assert_eq!(
            ItemInstance::new(2376, MAX_ITEM_STACK_COUNT + 1),
            Err(CoreError::InvalidItemStackCount(MAX_ITEM_STACK_COUNT + 1))
        );

        let sword = ItemInstance::new(2376, 1).unwrap();
        let shield = ItemInstance::new(2512, 1).unwrap();
        let replacement_sword = ItemInstance::new(2400, 1).unwrap();
        let mut equipment = PlayerEquipment::default();
        assert!(equipment.is_empty());
        assert_eq!(
            equipment.equip(EquipmentSlot::RightHand, sword.clone()),
            None
        );
        assert_eq!(
            equipment.equip(EquipmentSlot::LeftHand, shield.clone()),
            None
        );
        assert_eq!(equipment.len(), 2);
        assert_eq!(equipment.item(EquipmentSlot::RightHand), Some(&sword));
        assert_eq!(
            equipment.equip(EquipmentSlot::RightHand, replacement_sword.clone()),
            Some(sword)
        );
        assert_eq!(equipment.unequip(EquipmentSlot::LeftHand), Some(shield));
        assert_eq!(
            equipment.item(EquipmentSlot::RightHand),
            Some(&replacement_sword)
        );
        assert_eq!(equipment.len(), 1);
        assert_eq!(EquipmentSlot::RightHand.code(), 5);
        assert_eq!(EquipmentSlot::from_code(5), Some(EquipmentSlot::RightHand));
        assert_eq!(EquipmentSlot::from_code(11), None);
    }

    #[test]
    fn native_item_presentation_catalog_is_validated_and_duplicate_safe() {
        let presentation = NativeItemPresentation {
            client_thing_id: 102,
            requires_classic_740_subtype: true,
        };
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog.insert(4526, presentation).unwrap();
        assert_eq!(catalog.presentation(4526), Some(presentation));
        assert_eq!(catalog.len(), 1);
        assert!(matches!(
            catalog.insert(
                1,
                NativeItemPresentation {
                    client_thing_id: 0,
                    requires_classic_740_subtype: false,
                },
            ),
            Err(CoreError::InvalidClientThingId(1))
        ));
        assert_eq!(
            catalog.insert(4526, presentation),
            Err(CoreError::DuplicateItemPresentation(4526))
        );
    }

    #[test]
    fn ordered_item_containers_are_bounded_and_preserve_item_order() {
        assert_eq!(
            ItemContainer::new(0),
            Err(CoreError::InvalidContainerCapacity(0))
        );
        assert_eq!(
            ItemContainer::new(MAX_CONTAINER_CAPACITY + 1),
            Err(CoreError::InvalidContainerCapacity(
                MAX_CONTAINER_CAPACITY + 1
            ))
        );

        let first = ItemInstance::new(2376, 1).unwrap();
        let second = ItemInstance::new(2512, 1).unwrap();
        let mut container = ItemContainer::new(2).unwrap();
        assert!(container.is_empty());
        container.insert(first.clone()).unwrap();
        container.insert(second.clone()).unwrap();
        assert_eq!(container.capacity(), 2);
        assert_eq!(container.item(0), Some(&first));
        assert_eq!(container.item(1), Some(&second));
        assert_eq!(
            container.insert(ItemInstance::new(2463, 1).unwrap()),
            Err(CoreError::ContainerFull { capacity: 2 })
        );
        assert_eq!(container.remove(0), Some(first));
        assert_eq!(container.item(0), Some(&second));
        assert_eq!(container.remove(9), None);
    }

    #[test]
    fn authoritative_player_equipment_replaces_only_when_state_changes() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        assert!(world.player_equipment(7).unwrap().is_empty());
        assert_eq!(world.revision(), 1);

        let mut equipment = PlayerEquipment::default();
        let sword = ItemInstance::new(2376, 1).unwrap();
        equipment.equip(EquipmentSlot::RightHand, sword.clone());
        assert!(world
            .replace_player_equipment(7, equipment.clone())
            .unwrap());
        assert_eq!(world.revision(), 2);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&sword)
        );
        assert!(!world.replace_player_equipment(7, equipment).unwrap());
        assert_eq!(world.revision(), 2);

        world.remove_player(7).unwrap();
        assert_eq!(world.player_equipment(7), Err(CoreError::UnknownPlayer(7)));
    }

    #[test]
    fn authoritative_player_containers_are_bounded_and_replace_only_when_changed() {
        let backpack = ItemInstance::new(1988, 1).unwrap();
        let mut container = PlayerContainer::new(0, backpack, "Backpack", false, 2).unwrap();
        container
            .items
            .insert(ItemInstance::new(2376, 1).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        assert_eq!(containers.insert(container.clone()).unwrap(), None);
        assert_eq!(containers.container(0), Some(&container));

        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        assert!(world.player_containers(7).unwrap().is_empty());
        assert!(world
            .replace_player_containers(7, containers.clone())
            .unwrap());
        assert_eq!(world.revision(), 2);
        assert_eq!(
            world.player_containers(7).unwrap().container(0),
            Some(&container)
        );
        assert!(!world.replace_player_containers(7, containers).unwrap());
        assert_eq!(world.revision(), 2);

        assert_eq!(
            PlayerContainer::new(
                0,
                ItemInstance::new(1988, 1).unwrap(),
                "x".repeat(MAX_PLAYER_CONTAINER_NAME_BYTES + 1),
                false,
                1,
            ),
            Err(CoreError::InvalidContainerName(
                MAX_PLAYER_CONTAINER_NAME_BYTES + 1
            ))
        );

        world.remove_player(7).unwrap();
        assert_eq!(world.player_containers(7), Err(CoreError::UnknownPlayer(7)));
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
    fn world_revision_tracks_accepted_authoritative_mutations_only() {
        let mut world = WorldState::default();
        assert_eq!(world.revision(), 0);

        world.advance_tick();
        assert_eq!(world.revision(), 1);

        world.add_player(player()).unwrap();
        assert_eq!(world.revision(), 2);

        let unchanged_vitals = world.player_vitals(7).unwrap();
        world.update_player_vitals(7, unchanged_vitals).unwrap();
        assert_eq!(world.revision(), 2);

        let mut changed_vitals = unchanged_vitals;
        changed_vitals.health -= 1;
        world.update_player_vitals(7, changed_vitals).unwrap();
        assert_eq!(world.revision(), 3);

        assert!(world
            .move_player(
                7,
                Position {
                    x: 102,
                    y: 100,
                    z: 7,
                },
            )
            .is_err());
        assert_eq!(world.revision(), 3);

        world
            .move_player_cardinal(7, CardinalDirection::East)
            .unwrap();
        assert_eq!(world.revision(), 4);

        world.set_player_target(7, None).unwrap();
        assert_eq!(world.revision(), 4);

        world.remove_player(7).unwrap();
        assert_eq!(world.revision(), 5);
    }

    #[test]
    fn shared_world_player_registration_is_exclusive_and_releases_on_removal() {
        let mut world = WorldState::default();
        let first = player();
        world.add_player(first.clone()).unwrap();
        let second = Player {
            id: 8,
            account_id: 3,
            name: "Druid".into(),
            ..first.clone()
        };
        assert_eq!(
            world.add_player(second.clone()),
            Err(CoreError::PlayerOccupiesPosition(first.position))
        );
        assert_eq!(world.remove_player(first.id), Ok(first));
        world.add_player(second).unwrap();
        assert!(world.player(8).is_some());
        assert_eq!(
            world.player_render_snapshots(),
            vec![PlayerRenderSnapshot {
                id: 8,
                name: "Druid".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 1,
            }]
        );
        world.remove_player(8).unwrap();
        assert!(world.player_render_snapshots().is_empty());
        assert_eq!(world.remove_player(7), Err(CoreError::UnknownPlayer(7)));
    }

    #[test]
    fn player_interaction_intent_is_bounded_and_clears_when_a_selected_player_leaves() {
        let mut world = WorldState::default();
        let source = player();
        let selected = Player {
            id: 8,
            account_id: 3,
            name: "Druid".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            ..source.clone()
        };
        world.add_player(source.clone()).unwrap();
        world.add_player(selected.clone()).unwrap();

        assert_eq!(
            world.player_interaction_intent(source.id),
            Ok(PlayerInteractionIntent::default())
        );
        assert_eq!(
            world.set_player_target(source.id, Some(selected.id)),
            Ok(PlayerInteractionIntent {
                target_player_id: Some(selected.id),
                follow_player_id: None,
            })
        );
        assert_eq!(
            world.set_player_follow(source.id, Some(selected.id)),
            Ok(PlayerInteractionIntent {
                target_player_id: Some(selected.id),
                follow_player_id: Some(selected.id),
            })
        );
        assert_eq!(
            world.set_player_target(source.id, Some(source.id)),
            Err(CoreError::SelfInteractionNotAllowed(source.id))
        );
        assert_eq!(
            world.set_player_follow(source.id, Some(9)),
            Err(CoreError::UnknownPlayer(9))
        );

        world.remove_player(selected.id).unwrap();
        assert_eq!(
            world.player_interaction_intent(source.id),
            Ok(PlayerInteractionIntent::default())
        );
        assert_eq!(
            world.set_player_target(source.id, None),
            Ok(PlayerInteractionIntent::default())
        );
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
        assert_eq!(map.town(1).unwrap().temple_position, spawn);
        assert_eq!(map.temple_position_for_town(1), Some(spawn));
        assert_eq!(map.temple_position_for_town(99), None);
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
    fn deterministic_static_creature_policy_selects_safe_adjacent_steps_only() {
        let mut map = WorldMap::new(
            "policy",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        for position in [
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
            Position {
                x: 101,
                y: 100,
                z: 7,
            },
            Position {
                x: 102,
                y: 100,
                z: 7,
            },
            Position {
                x: 101,
                y: 101,
                z: 7,
            },
        ] {
            map.set_tile(
                position,
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: true,
                },
            )
            .unwrap();
        }
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
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
        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        world
            .add_player(Player {
                id: 9,
                account_id: 9,
                name: "Blocker".into(),
                position: Position {
                    x: 102,
                    y: 100,
                    z: 7,
                },
                level: 1,
                experience: 0,
                skill_points: 0,
            })
            .unwrap();
        assert_eq!(
            world.plan_static_creature_moves(StaticCreatureDecisionPolicy::Disabled, &map),
            StaticCreatureDecisionBatch::default()
        );
        let expected = StaticCreatureDecisionBatch {
            decisions: vec![StaticCreatureMoveDecision {
                creature_id,
                direction: CardinalDirection::South,
            }],
            skipped: 0,
        };
        assert_eq!(
            world.plan_static_creature_moves(StaticCreatureDecisionPolicy::ClockwiseAdjacent, &map),
            expected
        );
        assert_eq!(
            world
                .apply_static_creature_policy(StaticCreatureDecisionPolicy::ClockwiseAdjacent, &map)
                .unwrap(),
            expected
        );
        assert_eq!(
            world.static_creature(creature_id).unwrap().position,
            Position {
                x: 101,
                y: 101,
                z: 7,
            }
        );
        world.deactivate_static_creature(creature_id).unwrap();
        assert_eq!(
            world.plan_static_creature_moves(StaticCreatureDecisionPolicy::ClockwiseAdjacent, &map),
            StaticCreatureDecisionBatch::default()
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

    #[test]
    fn authoritative_player_vitals_validate_update_and_bound_damage() {
        let mut world = WorldState::default();
        let invalid = PlayerVitals {
            health: 151,
            max_health: 150,
            ..PlayerVitals::default()
        };
        assert_eq!(
            world.add_player_with_vitals(player(), invalid),
            Err(CoreError::InvalidPlayerVitals(7))
        );

        world.add_player(player()).unwrap();
        let mut target = player();
        target.id = 8;
        target.name = "Druid".into();
        target.position.x = 101;
        world
            .add_player_with_vitals(
                target,
                PlayerVitals {
                    health: 30,
                    max_health: 50,
                    mana: 20,
                    max_mana: 50,
                    capacity: 32_000,
                    magic_level: 4,
                },
            )
            .unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 120,
                    max_health: 150,
                    mana: 42,
                    max_mana: 50,
                    capacity: 35_000,
                    magic_level: 3,
                },
            )
            .unwrap();

        assert_eq!(
            world.apply_player_damage(7, 8, 12).unwrap(),
            PlayerDamageOutcome {
                attacker_id: 7,
                target_id: 8,
                requested_damage: 12,
                applied_damage: 12,
                remaining_health: 18,
                defeated: false,
            }
        );
        assert_eq!(world.player_vitals(8).unwrap().health, 18);
        assert_eq!(
            world.apply_player_damage(7, 8, 99).unwrap(),
            PlayerDamageOutcome {
                attacker_id: 7,
                target_id: 8,
                requested_damage: 99,
                applied_damage: 18,
                remaining_health: 0,
                defeated: true,
            }
        );
        assert_eq!(
            world.apply_player_damage(7, 7, 1),
            Err(CoreError::SelfInteractionNotAllowed(7))
        );

        let mut distant = player();
        distant.id = 9;
        distant.name = "Sorcerer".into();
        distant.position.x = 103;
        world.add_player(distant).unwrap();
        assert_eq!(
            world.apply_player_melee_damage(7, 9, 5),
            Err(CoreError::CombatOutOfRange {
                attacker_id: 7,
                target_id: 9,
            })
        );
    }

    #[test]
    fn typed_player_skills_are_bounded_ordered_and_authoritative() {
        assert_eq!(
            PlayerSkill::ALL.map(PlayerSkill::code),
            [0, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(PlayerSkill::from_code(4), Some(PlayerSkill::Distance));
        assert_eq!(PlayerSkill::from_code(7), None);
        assert_eq!(
            SkillProgress::new(0, 0),
            Err(CoreError::InvalidSkillProgress {
                level: 0,
                percent: 0
            })
        );
        assert_eq!(
            SkillProgress::new(10, 101),
            Err(CoreError::InvalidSkillProgress {
                level: 10,
                percent: 101
            })
        );

        let mut skills = PlayerSkills::default();
        assert!(skills.set(PlayerSkill::Sword, SkillProgress::new(42, 73).unwrap()));
        assert!(!skills.set(PlayerSkill::Sword, SkillProgress::new(42, 73).unwrap()));
        assert_eq!(
            skills.skill(PlayerSkill::Sword),
            SkillProgress::new(42, 73).unwrap()
        );
        assert_eq!(
            skills.iter().collect::<Vec<_>>()[2],
            (PlayerSkill::Sword, SkillProgress::new(42, 73).unwrap())
        );

        assert_eq!(BaseVocation::Knight.id().base(), Some(BaseVocation::Knight));
        assert_eq!(VocationId::new(8).base(), None);

        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let revision = world.revision();
        let progression = PlayerProgression {
            vocation: BaseVocation::Knight.id(),
            skills,
        };
        assert!(world.replace_player_progression(7, progression).unwrap());
        assert_eq!(world.revision(), revision + 1);
        assert!(!world.replace_player_progression(7, progression).unwrap());
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(world.player_progression(7).unwrap(), progression);
        world.remove_player(7).unwrap();
        assert_eq!(
            world.player_progression(7),
            Err(CoreError::UnknownPlayer(7))
        );
    }

    #[test]
    fn authoritative_regeneration_is_interval_bound_capped_and_cleanup_safe() {
        assert_eq!(
            RegenerationRule::new(0, 1),
            Err(CoreError::InvalidRegenerationInterval)
        );
        let rules = PlayerRegenerationRules {
            health: RegenerationRule::new(3, 5).unwrap(),
            mana: RegenerationRule::new(2, 4).unwrap(),
        };
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 140,
                    max_health: 150,
                    mana: 45,
                    max_mana: 50,
                    capacity: 40_000,
                    magic_level: 0,
                },
            )
            .unwrap();
        let revision = world.revision();
        assert_eq!(
            world.apply_player_regeneration(7, rules, 1).unwrap(),
            PlayerRegenerationOutcome {
                player_id: 7,
                health_gained: 0,
                mana_gained: 0,
                vitals: world.player_vitals(7).unwrap(),
            }
        );
        assert_eq!(world.revision(), revision);
        let outcome = world.apply_player_regeneration(7, rules, 2).unwrap();
        assert_eq!(outcome.health_gained, 5);
        assert_eq!(outcome.mana_gained, 4);
        assert_eq!(outcome.vitals.health, 145);
        assert_eq!(outcome.vitals.mana, 49);
        let capped = world
            .apply_player_regeneration(7, rules, MAX_REGENERATION_ELAPSED_SECONDS)
            .unwrap();
        assert_eq!(capped.vitals.health, 150);
        assert_eq!(capped.vitals.mana, 50);
        world.remove_player(7).unwrap();
        assert_eq!(
            world.apply_player_regeneration(7, rules, 1),
            Err(CoreError::UnknownPlayer(7))
        );
    }

    #[test]
    fn authoritative_conditions_are_bounded_replacing_and_expire_cleanly() {
        assert_eq!(
            PlayerCondition::new(PlayerConditionKind::Poison, 0, 1, 1),
            Err(CoreError::InvalidPlayerCondition)
        );
        assert_eq!(
            PlayerCondition::new(PlayerConditionKind::Poison, 1, 0, 1),
            Err(CoreError::InvalidPlayerCondition)
        );
        let poison = PlayerCondition::new(PlayerConditionKind::Poison, 2, 7, 5).unwrap();
        let burning = PlayerCondition::new(PlayerConditionKind::Burning, 3, 4, 3).unwrap();
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 18,
                    max_health: 150,
                    mana: 50,
                    max_mana: 50,
                    capacity: 40_000,
                    magic_level: 0,
                },
            )
            .unwrap();
        assert!(world.apply_player_condition(7, poison).unwrap());
        assert!(!world.apply_player_condition(7, poison).unwrap());
        assert!(world.apply_player_condition(7, burning).unwrap());
        let first = world.apply_player_conditions(7, 2).unwrap();
        assert_eq!(first.applied_damage, 7);
        assert_eq!(first.remaining_health, 11);
        assert_eq!(first.expired_conditions, 0);
        let final_tick = world.apply_player_conditions(7, 3).unwrap();
        assert_eq!(final_tick.applied_damage, 11);
        assert_eq!(final_tick.remaining_health, 0);
        assert_eq!(final_tick.expired_conditions, 2);
        assert!(world.player_conditions(7).unwrap().is_empty());
        world.remove_player(7).unwrap();
        assert_eq!(
            world.apply_player_conditions(7, 1),
            Err(CoreError::UnknownPlayer(7))
        );
    }

    #[test]
    fn death_loss_policy_preserves_documented_configuration_modes() {
        assert_eq!(
            DeathLossPolicy::from_config(-1),
            Ok(DeathLossPolicy::DefaultFormula)
        );
        assert_eq!(DeathLossPolicy::from_config(0), Ok(DeathLossPolicy::None));
        assert_eq!(
            DeathLossPolicy::from_config(10),
            Ok(DeathLossPolicy::FixedPercent(10))
        );
        assert_eq!(
            DeathLossPolicy::from_config(101),
            Err(CoreError::InvalidDeathLossPolicy)
        );
    }
}
