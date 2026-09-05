//! Deterministic domain primitives for Forgotten Engine.

mod world_combat;
mod world_conditions;
mod world_death;
mod world_lifecycle;
mod world_party;
mod world_static;
mod world_vitals;
pub(crate) use world_combat::*;
mod world_trade;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

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

/// Maximum items one side may stage in a player trade window. Classic clients show a fixed
/// grid; FE bounds the authoritative staging set identically.
pub const MAX_TRADE_ITEMS_PER_SIDE: usize = 20;

/// Legacy loot-chance scale: the TFS convention where a declared chance of this value means the
/// item always drops. Smaller declared chances are proportional probabilities.
pub const LOOT_CHANCE_SCALE: u32 = 100_000;

/// Bounded number of declarative loot entries retained per static monster.
pub const MAX_STATIC_LOOT_ENTRIES: usize = 32;

/// A deterministic, display-only entity materialized from a verified private TFS spawn record.
/// It intentionally excludes AI, combat, movement scheduling, Lua state, and lifecycle behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeTfsStaticEntity {
    pub id: u32,
    pub name: String,
    pub name_description: String,
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
    respawn_intervals_seconds: BTreeMap<u32, u32>,
    experience_rewards: BTreeMap<u32, u64>,
    direct_melee_intervals_millis: BTreeMap<u32, u32>,
    direct_melee_damage_ranges: BTreeMap<u32, StaticCreatureDirectMeleeDamageRange>,
    loot_tables: BTreeMap<u32, Vec<StaticCreatureLootEntry>>,
    npc_ids: BTreeSet<u32>,
    monster_spawn_areas: BTreeMap<u32, StaticCreatureSpawnArea>,
}

/// Bounded non-negative direct-melee damage values materialized from one legacy monster
/// declaration. The runtime deliberately chooses values deterministically; it does not claim the
/// randomized TFS combat formula or broader combat semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureDirectMeleeDamageRange {
    pub min_damage: u16,
    pub max_damage: u16,
}

/// One bounded declarative loot entry retained per static monster. `chance` uses the legacy
/// convention where 100000 equals always. Roll policy is a separate deterministic transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureLootEntry {
    pub item_id: u16,
    pub chance: u32,
    pub min_count: u16,
    pub max_count: u16,
}

/// The deterministic loot roll result for one defeated static creature. Counts are inclusive of
/// both bounds and every selected item is a distinct top-level corpse child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticCreatureLootRoll {
    pub creature_id: u32,
    pub items: Vec<(u16, u16)>,
}

/// The validated rectangular legacy spawn area which owns one materialized monster. This is
/// immutable import metadata; spectator flags, chance selection, rates, and event hooks remain
/// outside the bounded reactivation model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureSpawnArea {
    pub center: Position,
    pub radius: u16,
}

impl StaticCreatureSpawnArea {
    fn contains(self, position: Position) -> bool {
        position.z == self.center.z
            && position.x.abs_diff(self.center.x) <= self.radius
            && position.y.abs_diff(self.center.y) <= self.radius
    }
}

impl FeTfsStaticSpawnCollection {
    pub fn new(entities: Vec<FeTfsStaticEntity>) -> Result<Self, CoreError> {
        Self::with_respawn_intervals(entities, BTreeMap::new())
    }

    /// Merges summon-template entities into this collection, keeping only templates whose IDs
    /// and names do not collide with existing entries. Imported spawns always win; this is
    /// used at startup to add operator-summon templates alongside imported world spawns.
    pub fn extend_templates(
        &mut self,
        templates: FeTfsStaticSpawnCollection,
    ) -> Result<(), CoreError> {
        let existing_ids: BTreeSet<u32> = self.entities.iter().map(|entity| entity.id).collect();
        let existing_names: BTreeSet<String> = self
            .entities
            .iter()
            .map(|entity| entity.name.to_lowercase())
            .collect();
        for template in templates.entities {
            if existing_ids.contains(&template.id)
                || existing_names.contains(&template.name.to_lowercase())
            {
                continue;
            }
            if let Some(experience) = templates.experience_rewards.get(&template.id) {
                self.experience_rewards.insert(template.id, *experience);
            }
            if let Some(melee_interval) = templates.direct_melee_intervals_millis.get(&template.id)
            {
                self.direct_melee_intervals_millis
                    .insert(template.id, *melee_interval);
            }
            if let Some(damage_range) = templates.direct_melee_damage_ranges.get(&template.id) {
                self.direct_melee_damage_ranges
                    .insert(template.id, *damage_range);
            }
            if let Some(loot_table) = templates.loot_tables.get(&template.id) {
                self.loot_tables.insert(template.id, loot_table.clone());
            }
            self.entities.push(template);
        }
        Ok(())
    }

    pub fn with_respawn_intervals(
        entities: Vec<FeTfsStaticEntity>,
        respawn_intervals_seconds: BTreeMap<u32, u32>,
    ) -> Result<Self, CoreError> {
        Self::with_respawn_intervals_and_experience_rewards(
            entities,
            respawn_intervals_seconds,
            BTreeMap::new(),
        )
    }

    /// Retains immutable raw reward metadata for known static spawn IDs. Reward application is
    /// intentionally separate from installation and remains an explicit world transition.
    pub fn with_respawn_intervals_and_experience_rewards(
        entities: Vec<FeTfsStaticEntity>,
        respawn_intervals_seconds: BTreeMap<u32, u32>,
        experience_rewards: BTreeMap<u32, u64>,
    ) -> Result<Self, CoreError> {
        Self::with_runtime_metadata(
            entities,
            respawn_intervals_seconds,
            experience_rewards,
            BTreeMap::new(),
        )
    }

    /// Retains validated imported direct-melee intervals by stable static creature ID. This is
    /// immutable spawn metadata only; attack scheduling remains an explicit runtime policy.
    pub fn with_runtime_metadata(
        entities: Vec<FeTfsStaticEntity>,
        respawn_intervals_seconds: BTreeMap<u32, u32>,
        experience_rewards: BTreeMap<u32, u64>,
        direct_melee_intervals_millis: BTreeMap<u32, u32>,
    ) -> Result<Self, CoreError> {
        Self::with_combat_metadata(
            entities,
            respawn_intervals_seconds,
            experience_rewards,
            direct_melee_intervals_millis,
            BTreeMap::new(),
        )
    }

    /// Retains validated direct-melee interval and damage-range metadata together by stable
    /// static creature ID. Damage selection remains a bounded deterministic runtime policy.
    pub fn with_combat_metadata(
        entities: Vec<FeTfsStaticEntity>,
        respawn_intervals_seconds: BTreeMap<u32, u32>,
        experience_rewards: BTreeMap<u32, u64>,
        direct_melee_intervals_millis: BTreeMap<u32, u32>,
        direct_melee_damage_ranges: BTreeMap<u32, StaticCreatureDirectMeleeDamageRange>,
    ) -> Result<Self, CoreError> {
        Self::with_combat_metadata_and_npc_ids(
            entities,
            respawn_intervals_seconds,
            experience_rewards,
            direct_melee_intervals_millis,
            direct_melee_damage_ranges,
            BTreeSet::new(),
        )
    }

    /// Retains authoritative NPC identity by materialized static ID. It does not attach dialogue,
    /// scripts, shops, travel, or mutable conversation state to any entity.
    pub fn with_combat_metadata_and_npc_ids(
        entities: Vec<FeTfsStaticEntity>,
        respawn_intervals_seconds: BTreeMap<u32, u32>,
        experience_rewards: BTreeMap<u32, u64>,
        direct_melee_intervals_millis: BTreeMap<u32, u32>,
        direct_melee_damage_ranges: BTreeMap<u32, StaticCreatureDirectMeleeDamageRange>,
        npc_ids: BTreeSet<u32>,
    ) -> Result<Self, CoreError> {
        Self::with_loot_tables(
            entities,
            respawn_intervals_seconds,
            experience_rewards,
            direct_melee_intervals_millis,
            direct_melee_damage_ranges,
            npc_ids,
            BTreeMap::new(),
        )
    }

    /// Retains validated declarative loot tables by stable static creature ID. Loot entries are
    /// immutable import metadata; roll policy remains an explicit deterministic transition.
    #[allow(clippy::too_many_arguments)]
    pub fn with_loot_tables(
        entities: Vec<FeTfsStaticEntity>,
        respawn_intervals_seconds: BTreeMap<u32, u32>,
        experience_rewards: BTreeMap<u32, u64>,
        direct_melee_intervals_millis: BTreeMap<u32, u32>,
        direct_melee_damage_ranges: BTreeMap<u32, StaticCreatureDirectMeleeDamageRange>,
        npc_ids: BTreeSet<u32>,
        loot_tables: BTreeMap<u32, Vec<StaticCreatureLootEntry>>,
    ) -> Result<Self, CoreError> {
        if entities.len() > MAX_TFS_STATIC_SPAWNS {
            return Err(CoreError::StaticSpawnLimit(MAX_TFS_STATIC_SPAWNS));
        }
        let mut ids = std::collections::BTreeSet::new();
        for entity in &entities {
            if entity.name.trim().is_empty() {
                return Err(CoreError::EmptyStaticSpawnName);
            }
            if entity.health_percent > 100 {
                return Err(CoreError::InvalidStaticCreatureHealthPercent(
                    entity.health_percent,
                ));
            }
            if !ids.insert(entity.id) {
                return Err(CoreError::DuplicateStaticSpawnId(entity.id));
            }
        }
        if respawn_intervals_seconds.keys().any(|id| !ids.contains(id)) {
            return Err(CoreError::UnknownStaticCreatureSchedule);
        }
        if experience_rewards.keys().any(|id| !ids.contains(id)) {
            return Err(CoreError::UnknownStaticCreatureSchedule);
        }
        if direct_melee_intervals_millis
            .iter()
            .any(|(id, interval)| !ids.contains(id) || *interval == 0)
        {
            return Err(CoreError::UnknownStaticCreatureSchedule);
        }
        if direct_melee_damage_ranges
            .iter()
            .any(|(id, range)| !ids.contains(id) || range.min_damage > range.max_damage)
        {
            return Err(CoreError::UnknownStaticCreatureSchedule);
        }
        if npc_ids.iter().any(|id| !ids.contains(id)) {
            return Err(CoreError::UnknownStaticCreatureSchedule);
        }
        for (id, loot) in &loot_tables {
            if !ids.contains(id) || npc_ids.contains(id) || loot.len() > MAX_STATIC_LOOT_ENTRIES {
                return Err(CoreError::UnknownStaticCreatureSchedule);
            }
            for entry in loot {
                if entry.item_id == 0 || entry.min_count == 0 || entry.min_count > entry.max_count {
                    return Err(CoreError::UnknownStaticCreatureSchedule);
                }
            }
        }
        Ok(Self {
            entities,
            respawn_intervals_seconds,
            experience_rewards,
            direct_melee_intervals_millis,
            direct_melee_damage_ranges,
            loot_tables,
            npc_ids,
            monster_spawn_areas: BTreeMap::new(),
        })
    }

    /// Attaches one validated owning area only to materialized monster IDs. NPC reactivation
    /// remains deliberately outside this bounded lifecycle slice.
    pub fn with_monster_spawn_areas(
        mut self,
        monster_spawn_areas: BTreeMap<u32, StaticCreatureSpawnArea>,
    ) -> Result<Self, CoreError> {
        let ids = self
            .entities
            .iter()
            .map(|entity| entity.id)
            .collect::<BTreeSet<_>>();
        if monster_spawn_areas
            .keys()
            .any(|id| !ids.contains(id) || self.npc_ids.contains(id))
        {
            return Err(CoreError::UnknownStaticCreatureSchedule);
        }
        self.monster_spawn_areas = monster_spawn_areas;
        Ok(self)
    }

    pub fn monster_spawn_area(&self, id: u32) -> Option<StaticCreatureSpawnArea> {
        self.monster_spawn_areas.get(&id).copied()
    }

    pub fn respawn_interval_seconds(&self, id: u32) -> u32 {
        self.respawn_intervals_seconds
            .get(&id)
            .copied()
            .unwrap_or(0)
    }

    pub fn experience_reward(&self, id: u32) -> u64 {
        self.experience_rewards
            .get(&id)
            .copied()
            .unwrap_or_default()
    }

    pub fn direct_melee_interval_millis(&self, id: u32) -> Option<u32> {
        self.direct_melee_intervals_millis.get(&id).copied()
    }

    pub fn direct_melee_damage_range(
        &self,
        id: u32,
    ) -> Option<StaticCreatureDirectMeleeDamageRange> {
        self.direct_melee_damage_ranges.get(&id).copied()
    }

    pub fn loot_table(&self, id: u32) -> &[StaticCreatureLootEntry] {
        self.loot_tables
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn is_npc(&self, id: u32) -> bool {
        self.npc_ids.contains(&id)
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
    pub health_percent: u8,
    pub activated_at_tick: u64,
    pub inactive_since_tick: Option<u64>,
    pub reactivation_due_tick: Option<u64>,
    pub respawn_interval_seconds: u32,
}

/// The compact restart snapshot for an installed static creature. It retains the remaining delay
/// for the current bounded reactivation schedule, direct-melee cooldown, and deterministic
/// direct-melee selection sequence; it deliberately excludes spawn identity, appearance, targets,
/// autonomous AI cadence, combat formulas, loot, and scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureRuntimeSnapshot {
    pub id: u32,
    pub position: Position,
    pub active: bool,
    pub health_percent: u8,
    pub reactivation_remaining_seconds: Option<u32>,
    pub direct_melee_cooldown_remaining_ticks: Option<u32>,
    pub direct_melee_damage_sequence: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticCreatureRuntimeRestoreSummary {
    pub restored: usize,
    pub ignored_unknown: usize,
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

pub const MAX_STATIC_CREATURE_TARGET_RANGE: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureTargetSelection {
    pub creature_id: u32,
    pub target_player_id: Option<u64>,
    pub max_range: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticCreatureTargetStepOutcome {
    NoTarget,
    AlreadyAdjacent {
        target_player_id: u64,
    },
    Blocked {
        target_player_id: u64,
    },
    Moved {
        target_player_id: u64,
        direction: CardinalDirection,
        from: Position,
        to: Position,
    },
}

/// Result of one explicit static-creature attack against its already selected player target.
/// This is a core-only transition: callers remain responsible for scheduling, persistence, and
/// client delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticCreatureTargetAttackOutcome {
    NoTarget,
    CooldownNotDue {
        creature_id: u32,
        due_tick: u64,
    },
    TargetNotAdjacent {
        creature_id: u32,
        target_player_id: u64,
    },
    Applied {
        creature_id: u32,
        target_player_id: u64,
        requested_damage: u16,
        applied_damage: u16,
        remaining_health: u16,
        death_state: Option<PlayerRespawnState>,
    },
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
    is_npc: bool,
    experience_reward: u64,
    loot: Vec<StaticCreatureLootEntry>,
    spawn_position: Position,
    monster_spawn_area: Option<StaticCreatureSpawnArea>,
    active: bool,
    health_percent: u8,
    activated_at_tick: u64,
    inactive_since_tick: Option<u64>,
    reactivation_due_tick: Option<u64>,
    respawn_interval_seconds: u32,
    melee_cooldown_ticks: Option<u64>,
    next_melee_due_tick: u64,
    direct_melee_damage_range: Option<StaticCreatureDirectMeleeDamageRange>,
    direct_melee_damage_sequence: u64,
    target_player_id: Option<u64>,
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

/// Deterministic non-cryptographic revision of the complete immutable map content. It is suitable
/// for rejecting incompatible runtime-map journals after an operator changes map content, but it
/// is not a security hash and does not identify a mutable runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldMapSourceRevision(pub u64);

/// Stable identity for one top-level item in one immutable source-map revision. It intentionally
/// becomes invalid when the complete source map changes, forcing a future runtime journal to
/// reconcile rather than applying a stale ordered index to new operator content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorldMapItemSourceIdentity {
    pub map_revision: WorldMapSourceRevision,
    pub position: Position,
    pub item_index: u8,
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

struct StableMapHasher(u64);

impl StableMapHasher {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    fn finish(self) -> u64 {
        self.0
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, value: &[u8]) {
        for byte in value {
            self.byte(*byte);
        }
    }

    fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u32(value.len().try_into().unwrap_or(u32::MAX));
        self.bytes(value.as_bytes());
    }

    fn optional_string(&mut self, value: Option<&str>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.string(value);
        }
    }

    fn position(&mut self, value: Position) {
        self.u16(value.x);
        self.u16(value.y);
        self.byte(value.z);
    }

    fn optional_u16(&mut self, value: Option<u16>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.u16(value);
        }
    }

    fn optional_u32(&mut self, value: Option<u32>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.u32(value);
        }
    }

    fn optional_position(&mut self, value: Option<Position>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.position(value);
        }
    }

    fn world_map_item(&mut self, item: &WorldMapItem) {
        self.u16(item.server_id);
        self.optional_u16(item.client_thing_id);
        self.byte(item.count);
        self.optional_u16(item.action_id);
        self.optional_u16(item.unique_id);
        self.optional_string(item.text.as_deref());
        self.optional_string(item.description.as_deref());
        self.optional_position(item.teleport_destination);
        self.optional_u32(item.duration);
        self.optional_u16(item.charges);
        self.u32(item.children.len().try_into().unwrap_or(u32::MAX));
        for child in &item.children {
            self.world_map_item(child);
        }
    }
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

    /// Fingerprints the complete ordered source map content with an explicitly specified FNV-1a
    /// byte stream. BTree-backed collections provide stable key order, while tile item order and
    /// child order are kept exactly as loaded. Runtime ownership and persistence are deliberately
    /// outside this immutable source-revision contract.
    pub fn source_revision(&self) -> WorldMapSourceRevision {
        let mut hash = StableMapHasher::new();
        hash.string(&self.identifier);
        hash.position(self.spawn);
        match &self.source {
            WorldMapSource::FeMapV1 => hash.byte(0),
            WorldMapSource::Otbm(header) => {
                hash.byte(1);
                hash.u32(header.version);
                hash.u16(header.width);
                hash.u16(header.height);
                hash.u32(header.item_major_version);
                hash.u32(header.item_minor_version);
                hash.optional_string(header.description.as_deref());
                hash.optional_string(header.spawn_file.as_deref());
                hash.optional_string(header.house_file.as_deref());
            }
        }
        hash.u32(self.tiles.len().try_into().unwrap_or(u32::MAX));
        for (position, tile) in &self.tiles {
            hash.position(*position);
            hash.u16(tile.ground_thing_id);
            hash.bool(tile.walkable);
        }
        hash.u32(self.tile_items.len().try_into().unwrap_or(u32::MAX));
        for (position, items) in &self.tile_items {
            hash.position(*position);
            hash.u32(items.len().try_into().unwrap_or(u32::MAX));
            for item in items {
                hash.world_map_item(item);
            }
        }
        hash.u32(self.tile_flags.len().try_into().unwrap_or(u32::MAX));
        for (position, flags) in &self.tile_flags {
            hash.position(*position);
            hash.u32(*flags);
        }
        hash.u32(self.house_tiles.len().try_into().unwrap_or(u32::MAX));
        for (position, house_id) in &self.house_tiles {
            hash.position(*position);
            hash.u32(*house_id);
        }
        hash.u32(self.towns.len().try_into().unwrap_or(u32::MAX));
        for (id, town) in &self.towns {
            hash.u32(*id);
            hash.string(&town.name);
            hash.position(town.temple_position);
        }
        hash.u32(self.waypoints.len().try_into().unwrap_or(u32::MAX));
        for (name, position) in &self.waypoints {
            hash.string(name);
            hash.position(*position);
        }
        WorldMapSourceRevision(hash.finish())
    }

    /// Returns an identity only for an existing top-level source item. Child items remain part of
    /// their root item's immutable source content and are deliberately not independently mutable.
    pub fn source_item_identity(
        &self,
        position: Position,
        item_index: usize,
    ) -> Option<WorldMapItemSourceIdentity> {
        let items = self.tile_items(position)?;
        items.get(item_index)?;
        Some(WorldMapItemSourceIdentity {
            map_revision: self.source_revision(),
            position,
            item_index: item_index.try_into().ok()?,
        })
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

    /// Applies revision-bound top-level source-item removals only after validating the complete
    /// journal against the immutable source revision and exact current ordered items. This is a
    /// recovery primitive; persistence coordination and client delivery remain separate.
    pub fn apply_source_item_removals(
        &mut self,
        removals: &[WorldMapItemSourceIdentity],
    ) -> Result<(), CoreError> {
        let revision = self.source_revision();
        let mut by_position: BTreeMap<Position, Vec<usize>> = BTreeMap::new();
        for removal in removals {
            if removal.map_revision != revision {
                return Err(CoreError::InvalidMap(
                    "map-item journal source revision does not match the loaded map".into(),
                ));
            }
            let index = usize::from(removal.item_index);
            let items = self.tile_items(removal.position).ok_or_else(|| {
                CoreError::InvalidMap("map-item journal references a missing tile item list".into())
            })?;
            if items.get(index).is_none() {
                return Err(CoreError::InvalidMap(
                    "map-item journal references a missing ordered source item".into(),
                ));
            }
            let indices = by_position.entry(removal.position).or_default();
            if indices.contains(&index) {
                return Err(CoreError::InvalidMap(
                    "map-item journal repeats one source item identity".into(),
                ));
            }
            indices.push(index);
        }
        for (position, mut indices) in by_position {
            indices.sort_unstable_by(|left, right| right.cmp(left));
            let items = self.tile_items.get_mut(&position).ok_or_else(|| {
                CoreError::InvalidMap("map-item journal references a missing tile item list".into())
            })?;
            for index in indices {
                items.remove(index);
            }
        }
        Ok(())
    }

    /// Classic OTBM tile-flag bit 0x01 marks a protection zone. FE treats the flag as
    /// authoritative import metadata for PvP blocking and logout safety.
    pub const OTBM_TILE_FLAG_PROTECTION_ZONE: u32 = 0x01;

    /// True when this tile carries the imported protection-zone flag.
    pub fn is_protection_zone(&self, position: Position) -> bool {
        self.tile_flags
            .get(&position)
            .is_some_and(|flags| flags & Self::OTBM_TILE_FLAG_PROTECTION_ZONE != 0)
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
/// deliberately bounded to the client-visible 0Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă„ÄľÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ä‚â€žĂ˘â‚¬Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇĂ„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ă„â€šĂ˘â‚¬ĹľÄ‚â€žĂ˘â‚¬Â¦Ă„â€šĂ˘â‚¬Ä…Ä‚ËĂ˘â€šÂ¬Ă‹â€ˇĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬Ă‚Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ă„Ä…ÄąĹş100 range; skill tries and advancement formulas
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

pub const MINIMUM_PLAYER_SKILL_LEVEL: u16 = 10;
pub const MAX_PROGRESSION_MULTIPLIER_MILLI: u32 = 100_000;
pub const MAX_EXPERIENCE_AWARD_RATE: u32 = 100_000;
const PROGRESSION_MULTIPLIER_SCALE: u64 = 1_000;
const SKILL_BASE_TRIES: [u64; 7] = [50, 50, 50, 50, 30, 100, 20];
const MAGIC_LEVEL_BASE_MANA: u64 = 1_600;

/// A validated fixed-point multiplier loaded from an operator-owned vocation registry. The core
/// uses integer arithmetic so advancement remains deterministic across platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressionMultiplier {
    milli: u32,
}

impl ProgressionMultiplier {
    pub fn new(milli: u32) -> Result<Self, CoreError> {
        if milli == 0 || milli > MAX_PROGRESSION_MULTIPLIER_MILLI {
            return Err(CoreError::InvalidProgressionMultiplier(milli));
        }
        Ok(Self { milli })
    }

    pub const fn milli(self) -> u32 {
        self.milli
    }
}

/// One inclusive level range used by an authoritative experience-award policy. Values are stored
/// in thousandths so a stage multiplier remains deterministic across platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExperienceAwardStage {
    pub min_level: u32,
    pub max_level: u32,
    pub multiplier_milli: u32,
}

impl ExperienceAwardStage {
    pub fn new(min_level: u32, max_level: u32, multiplier_milli: u32) -> Result<Self, CoreError> {
        if min_level == 0
            || max_level < min_level
            || multiplier_milli == 0
            || multiplier_milli > MAX_PROGRESSION_MULTIPLIER_MILLI
        {
            return Err(CoreError::InvalidExperienceAwardPolicy);
        }
        Ok(Self {
            min_level,
            max_level,
            multiplier_milli,
        })
    }
}

/// Validated operator-owned experience-award inputs. `flat_rate` corresponds to a configured
/// global rate such as `rateExp`; a matching stage additionally scales the award. A zero flat
/// rate is valid and intentionally yields no awarded experience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceAwardPolicy {
    flat_rate: u32,
    stages: Vec<ExperienceAwardStage>,
}

impl ExperienceAwardPolicy {
    pub fn new(flat_rate: u32, stages: Vec<ExperienceAwardStage>) -> Result<Self, CoreError> {
        if flat_rate > MAX_EXPERIENCE_AWARD_RATE
            || stages
                .windows(2)
                .any(|pair| pair[0].max_level >= pair[1].min_level)
        {
            return Err(CoreError::InvalidExperienceAwardPolicy);
        }
        Ok(Self { flat_rate, stages })
    }

    pub const fn flat_rate(&self) -> u32 {
        self.flat_rate
    }

    pub fn stages(&self) -> &[ExperienceAwardStage] {
        &self.stages
    }

    pub fn award_for(&self, level: u32, raw_experience: u64) -> u64 {
        let stage_multiplier_milli = self
            .stages
            .iter()
            .find(|stage| stage.min_level <= level && level <= stage.max_level)
            .map_or(PROGRESSION_MULTIPLIER_SCALE, |stage| {
                u64::from(stage.multiplier_milli)
            });
        raw_experience
            .saturating_mul(u64::from(self.flat_rate))
            .saturating_mul(stage_multiplier_milli)
            / PROGRESSION_MULTIPLIER_SCALE
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerExperienceAwardOutcome {
    pub player_id: u64,
    pub raw_experience: u64,
    pub awarded_experience: u64,
    pub experience: u64,
    pub level: u32,
    pub gained_levels: u32,
    pub vitals: PlayerVitals,
}

/// Validated per-level vitality and capacity gains derived by a configuration boundary from one
/// operator-owned vocation. The core does not infer values from a numeric vocation identity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VocationLevelUpGains {
    pub health: u16,
    pub mana: u16,
    pub capacity: u16,
}

impl VocationLevelUpGains {
    pub const fn new(health: u16, mana: u16, capacity: u16) -> Self {
        Self {
            health,
            mana,
            capacity,
        }
    }

    fn scaled_amount(amount: u16, gained_levels: u32) -> u16 {
        u16::try_from(u32::from(amount).saturating_mul(gained_levels)).unwrap_or(u16::MAX)
    }

    fn apply(self, vitals: PlayerVitals, gained_levels: u32) -> PlayerVitals {
        let health = Self::scaled_amount(self.health, gained_levels);
        let mana = Self::scaled_amount(self.mana, gained_levels);
        let capacity = Self::scaled_amount(self.capacity, gained_levels);
        PlayerVitals {
            health: vitals.health.saturating_add(health),
            max_health: vitals.max_health.saturating_add(health),
            mana: vitals.mana.saturating_add(mana),
            max_mana: vitals.max_mana.saturating_add(mana),
            capacity: vitals.capacity.saturating_add(capacity),
            ..vitals
        }
    }
}

/// Formula inputs derived from one validated vocation definition. The formula is intentionally
/// parameterized by data rather than by FE release or a hard-coded vocation identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerProgressionRules {
    pub magic_level_multiplier: ProgressionMultiplier,
    pub skill_multipliers: [ProgressionMultiplier; 7],
}

impl PlayerProgressionRules {
    /// Returns the required tries for the supplied target skill level. The classic seven skill
    /// bases are retained as a profile-research input; profile parity still requires validation
    /// against the selected operator data and client profile.
    pub fn required_skill_tries(self, skill: PlayerSkill, target_level: u16) -> u64 {
        let multiplier = self.skill_multipliers[skill.code() as usize];
        let exponent = target_level.saturating_sub(MINIMUM_PLAYER_SKILL_LEVEL + 1);
        scale_progression_requirement(
            SKILL_BASE_TRIES[skill.code() as usize],
            multiplier,
            exponent,
        )
    }

    /// Returns the required spent mana for the supplied target magic level. Level zero has no
    /// requirement because the first advancement consumes the level-one requirement.
    pub fn required_magic_mana(self, target_magic_level: u8) -> u64 {
        if target_magic_level == 0 {
            return 0;
        }
        scale_progression_requirement(
            MAGIC_LEVEL_BASE_MANA,
            self.magic_level_multiplier,
            u16::from(target_magic_level.saturating_sub(1)),
        )
    }
}

/// Exact remaining progression counters that cannot be represented by client-visible percentage
/// fields alone. Weapon hits and spell casts remain future sources of these values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerProgressionAttempts {
    skill_tries: [u64; 7],
    magic_mana: u64,
}

impl PlayerProgressionAttempts {
    pub const fn new(skill_tries: [u64; 7], magic_mana: u64) -> Self {
        Self {
            skill_tries,
            magic_mana,
        }
    }

    pub const fn all_skill_tries(self) -> [u64; 7] {
        self.skill_tries
    }

    pub const fn skill_tries(self, skill: PlayerSkill) -> u64 {
        self.skill_tries[skill.code() as usize]
    }

    pub const fn magic_mana(self) -> u64 {
        self.magic_mana
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSkillTryOutcome {
    pub player_id: u64,
    pub skill: PlayerSkill,
    pub gained_levels: u16,
    pub progress: SkillProgress,
    pub stored_tries: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerMagicAdvanceOutcome {
    pub player_id: u64,
    pub gained_levels: u8,
    pub magic_level: u8,
    pub stored_mana: u64,
}

pub const MAX_REGENERATION_ELAPSED_SECONDS: u16 = 60;
/// Classic food cadence (plan v49 slice 16): one health per interval while the food window
/// from the last eaten item is active.
pub const FOOD_REGENERATION_INTERVAL_SECONDS: u16 = 4;
pub const FOOD_REGENERATION_HEALTH_PER_INTERVAL: u16 = 1;

/// One player's classic fed state. `until_tick` is authoritative-tick absolute; the
/// accumulator paces one-health gains at the fixed food cadence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PlayerFoodWindow {
    until_tick: u64,
    elapsed_seconds: u16,
}

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

/// Bounded damage-over-time condition families plus the timed speed modifier (haste). Their
/// visual effects, immunity rules, Lua hooks, and death policy remain separate protocol and
/// scripting concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlayerConditionKind {
    Poison,
    Burning,
    Energy,
    Haste,
}

impl PlayerConditionKind {
    pub const fn code(self) -> u8 {
        match self {
            Self::Poison => 0,
            Self::Burning => 1,
            Self::Energy => 2,
            Self::Haste => 3,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Poison),
            1 => Some(Self::Burning),
            2 => Some(Self::Energy),
            3 => Some(Self::Haste),
            _ => None,
        }
    }

    /// Classic 7.4 haste has no PlayerState icon bit; the modifier is felt through walk cadence.
    pub const fn is_damage_over_time(self) -> bool {
        !matches!(self, Self::Haste)
    }
}

/// A single validated condition schedule. The condition is stored by kind, so applying the same
/// kind replaces its timing/damage record instead of creating an unbounded stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCondition {
    pub kind: PlayerConditionKind,
    pub interval_seconds: u16,
    pub damage: u16,
    /// Haste modifier: additive percent applied to effective walk speed. Zero for DoT kinds.
    pub speed_bonus_percent: u16,
    pub remaining_seconds: u16,
    elapsed_seconds: u16,
}

/// Bounded cap for the haste speed modifier so no operator configuration can exceed 2x speed.
pub const MAX_SPEED_BONUS_PERCENT: u16 = 100;

impl PlayerCondition {
    /// Damage-over-time constructor. Haste requires `new_haste`.
    pub fn new(
        kind: PlayerConditionKind,
        interval_seconds: u16,
        damage: u16,
        remaining_seconds: u16,
    ) -> Result<Self, CoreError> {
        if kind.is_damage_over_time() {
            if interval_seconds == 0 || damage == 0 || remaining_seconds == 0 {
                return Err(CoreError::InvalidPlayerCondition);
            }
        } else {
            return Err(CoreError::InvalidPlayerCondition);
        }
        if remaining_seconds > MAX_CONDITION_DURATION_SECONDS {
            return Err(CoreError::InvalidPlayerCondition);
        }
        Ok(Self {
            kind,
            interval_seconds,
            damage,
            speed_bonus_percent: 0,
            remaining_seconds,
            elapsed_seconds: 0,
        })
    }

    /// Timed speed modifier: no damage, no per-interval schedule; interval stays 1 so the
    /// persisted elapsed invariant (elapsed < interval) holds.
    pub fn new_haste(speed_bonus_percent: u16, remaining_seconds: u16) -> Result<Self, CoreError> {
        if speed_bonus_percent == 0 || speed_bonus_percent > MAX_SPEED_BONUS_PERCENT {
            return Err(CoreError::InvalidPlayerCondition);
        }
        if remaining_seconds == 0 || remaining_seconds > MAX_CONDITION_DURATION_SECONDS {
            return Err(CoreError::InvalidPlayerCondition);
        }
        Ok(Self {
            kind: PlayerConditionKind::Haste,
            interval_seconds: 1,
            damage: 0,
            speed_bonus_percent,
            remaining_seconds,
            elapsed_seconds: 0,
        })
    }

    /// Restores a previously validated bounded schedule. Elapsed progress is always less than one
    /// interval because the authoritative scheduler stores the remainder after every tick.
    pub fn from_persisted(
        kind: PlayerConditionKind,
        interval_seconds: u16,
        damage: u16,
        speed_bonus_percent: u16,
        remaining_seconds: u16,
        elapsed_seconds: u16,
    ) -> Result<Self, CoreError> {
        let mut condition = if kind.is_damage_over_time() {
            if speed_bonus_percent != 0 {
                return Err(CoreError::InvalidPlayerCondition);
            }
            Self::new(kind, interval_seconds, damage, remaining_seconds)?
        } else {
            if interval_seconds == 0 || damage != 0 || speed_bonus_percent == 0 {
                return Err(CoreError::InvalidPlayerCondition);
            }
            Self::new_haste(speed_bonus_percent, remaining_seconds)?
        };
        if elapsed_seconds >= interval_seconds {
            return Err(CoreError::InvalidPlayerCondition);
        }
        condition.elapsed_seconds = elapsed_seconds;
        Ok(condition)
    }

    pub const fn elapsed_seconds(self) -> u16 {
        self.elapsed_seconds
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

/// Authoritative in-memory lifecycle state for a player who has died. `death_time` is expressed
/// as the deterministic world tick at which the transition was accepted; no wall-clock source is
/// consulted by the core. A later respawn slice will consume `respawn_at` to perform validated
/// teleportation and client delivery.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerRespawnState {
    pub dead: bool,
    pub respawn_at: Option<Position>,
    pub death_time: Option<u64>,
    pub loss_applied: bool,
}

/// The deterministic state transition returned after a player is restored at a previously
/// validated temple. Client delivery and persistence remain host-layer responsibilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRespawnOutcome {
    pub player_id: u64,
    pub position: Position,
    pub vitals: PlayerVitals,
}

/// Exact authoritative loss result. It contains no client packet data and does not imply
/// persistence; callers decide when to commit or display a verified lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerDeathLossOutcome {
    pub player_id: u64,
    pub percent: u8,
    pub experience_lost: u64,
    pub skill_tries_lost: [u64; 7],
    pub magic_mana_lost: u64,
    pub level: u32,
    pub progression: PlayerProgression,
    pub progression_attempts: PlayerProgressionAttempts,
    pub vitals: PlayerVitals,
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
    /// Nested content when this instance is used as a container. Bounded depth; see
    /// `insert_content`.
    contents: Vec<ItemInstance>,
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
            contents: Vec::new(),
        })
    }

    /// Maximum nested container depth accepted by `insert_content`. Depth one means a bag
    /// inside a bag; deeper nesting stays closed until persistence and protocol support land.
    pub const MAX_CONTENT_DEPTH: u8 = 1;

    /// Maximum number of direct content slots per container instance.
    pub const MAX_CONTENT_SLOTS: usize = 20;

    /// Immutable view of this instance's direct contents.
    pub fn contents(&self) -> &[ItemInstance] {
        &self.contents
    }

    /// Inserts one item as direct content of this container instance. Rejects zero IDs,
    /// exhausted slots, and content deeper than `MAX_CONTENT_DEPTH` (measured from the
    /// inserted subtree's own root).
    pub fn insert_content(&mut self, item: ItemInstance) -> Result<(), CoreError> {
        if item.server_id == 0 {
            return Err(CoreError::InvalidItemId(item.server_id));
        }
        if self.contents.len() >= Self::MAX_CONTENT_SLOTS {
            return Err(CoreError::ContainerFull {
                capacity: Self::MAX_CONTENT_SLOTS as u16,
            });
        }
        if Self::content_depth(&item) >= Self::MAX_CONTENT_DEPTH {
            return Err(CoreError::InvalidItemContentDepth);
        }
        self.contents.push(item);
        Ok(())
    }

    /// Removes and returns one direct content item by index.
    pub fn take_content(&mut self, index: usize) -> Option<ItemInstance> {
        if index < self.contents.len() {
            Some(self.contents.remove(index))
        } else {
            None
        }
    }

    /// Height of an item's content subtree; empty items have depth zero.
    fn content_depth(item: &ItemInstance) -> u8 {
        item.contents
            .iter()
            .map(|child| Self::content_depth(child).saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// Returns true only when two runtime instances have the same type and persistent attributes.
    /// Item-definition stackability remains outside this bounded core layer, so callers opt into
    /// merge semantics only by invoking the explicit stack transfer methods below.
    pub fn is_stack_compatible_with(&self, other: &Self) -> bool {
        self.server_id == other.server_id
            && self.action_id == other.action_id
            && self.unique_id == other.unique_id
    }

    pub fn split_off(&mut self, count: u16) -> Result<Self, CoreError> {
        if count == 0 || count > self.count {
            return Err(CoreError::InvalidItemTransferCount {
                requested: count,
                available: self.count,
            });
        }
        self.count -= count;
        let mut split = self.clone();
        split.count = count;
        Ok(split)
    }

    /// Merges one exact compatible complete stack while enforcing the bounded stack count.
    /// Ownership and source-transfer policy remain the responsibility of the caller.
    pub fn merge_stack(&mut self, incoming: &Self) -> Result<(), CoreError> {
        if !self.is_stack_compatible_with(incoming) {
            return Err(CoreError::IncompatibleItemStacks);
        }
        let count =
            self.count
                .checked_add(incoming.count)
                .ok_or(CoreError::ItemStackCountOverflow {
                    existing: self.count,
                    incoming: incoming.count,
                })?;
        if count > MAX_ITEM_STACK_COUNT {
            return Err(CoreError::ItemStackCountOverflow {
                existing: self.count,
                incoming: incoming.count,
            });
        }
        self.count = count;
        Ok(())
    }
}

/// Result of an authoritative, non-recursive player inventory transfer. Client inventory window
/// delivery, item properties, and map-ground transfers remain host/runtime responsibilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerEquipmentToContainerOutcome {
    pub player_id: u64,
    pub from_slot: EquipmentSlot,
    pub container_id: u8,
    pub item: ItemInstance,
}

/// Result of moving one complete item from a bounded owned top-level container into an empty
/// equipment slot. Stack splitting, slot compatibility, capacity, ground interaction, recursive
/// containers, and client-driven item requests remain outside this foundation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerContainerToEquipmentOutcome {
    pub player_id: u64,
    pub container_id: u8,
    pub item_index: usize,
    pub to_slot: EquipmentSlot,
    pub item: ItemInstance,
}

/// Result of exchanging one complete top-level container item with the complete item already in
/// an occupied fixed equipment slot. This narrow operation does not infer slot compatibility,
/// stackability, capacity, nested containers, map interaction, or generic item-move semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerContainerToEquipmentSwapOutcome {
    pub player_id: u64,
    pub container_id: u8,
    pub item_index: usize,
    pub to_slot: EquipmentSlot,
    pub equipped_item: ItemInstance,
    pub container_item: ItemInstance,
}

/// Result of exchanging two complete items already stored in distinct occupied equipment slots.
/// This narrow operation does not infer slot compatibility, stackability, capacity, containers,
/// map interaction, or generic item-move semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerEquipmentSlotSwapOutcome {
    pub player_id: u64,
    pub from_slot: EquipmentSlot,
    pub to_slot: EquipmentSlot,
    pub from_item: ItemInstance,
    pub to_item: ItemInstance,
}

/// Result of a bounded partial-stack movement from equipment to an existing top-level container.
/// It does not imply item metadata stackability, ownership checks beyond the current player,
/// capacity rules, ground transfer, recursive containers, client requests, or client delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerEquipmentStackToContainerOutcome {
    pub player_id: u64,
    pub from_slot: EquipmentSlot,
    pub container_id: u8,
    pub destination_index: usize,
    pub moved_item: ItemInstance,
    pub source_remaining_count: Option<u16>,
    pub destination_count: u16,
}

/// Result of a bounded partial-stack movement from one existing top-level container into a fixed
/// equipment slot. It retains the same deliberately narrow behavior as the forward transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerContainerStackToEquipmentOutcome {
    pub player_id: u64,
    pub container_id: u8,
    pub item_index: usize,
    pub to_slot: EquipmentSlot,
    pub moved_item: ItemInstance,
    pub source_remaining_count: Option<u16>,
    pub destination_count: u16,
}

/// Result of a bounded stack movement between two distinct caller-owned container windows. This
/// core primitive does not infer nesting, map, capacity-weight, or client semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerContainerStackToContainerOutcome {
    pub player_id: u64,
    pub from_container_id: u8,
    pub item_index: usize,
    pub to_container_id: u8,
    pub destination_index: usize,
    pub moved_item: ItemInstance,
    pub source_remaining_count: Option<u16>,
    pub destination_count: u16,
}

/// Typed source of one authoritative player stack drop onto the ground. Container sources
/// address one exact top-level item index inside one owned non-nested window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerGroundDropSource {
    EquipmentSlot(EquipmentSlot),
    ContainerItem {
        container_id: u8,
        item_index: usize,
    },
    /// Depth-one content inside a container item; see `ItemInstance::contents`.
    ContainerContent {
        container_id: u8,
        item_index: usize,
        content_index: usize,
    },
}

/// Result of one bounded player stack drop onto the ground. The moved stack is removed from
/// inventory state; map placement and persistence remain caller-owned boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerGroundDropOutcome {
    pub player_id: u64,
    pub source: PlayerGroundDropSource,
    pub moved_item: ItemInstance,
    pub source_remaining_count: Option<u16>,
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

    /// Returns a server item ID only when this catalog has exactly one presentation with the
    /// requested client thing ID. Client IDs can legitimately be reused by distinct server items,
    /// so ambiguous or unknown reverse lookups are rejected instead of being guessed.
    pub fn unique_server_id_for_client_thing_id(&self, client_thing_id: u16) -> Option<u16> {
        let mut matching = self
            .entries
            .iter()
            .filter_map(|(&server_id, presentation)| {
                (presentation.client_thing_id == client_thing_id).then_some(server_id)
            });
        let server_id = matching.next()?;
        matching.next().is_none().then_some(server_id)
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

    pub fn item_mut(&mut self, slot: EquipmentSlot) -> Option<&mut ItemInstance> {
        self.items.get_mut(&slot)
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
    pub fn item_mut(&mut self, index: usize) -> Option<&mut ItemInstance> {
        self.items.get_mut(index)
    }

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

    /// Merges into the first compatible bounded stack that has room, or inserts a new stack.
    /// If compatible stacks exist but all would overflow, no new stack is created because that
    /// would hide an invalid merge behind arbitrary stack fragmentation.
    /// Merges one compatible complete stack or inserts it as a new bounded entry. Callers must
    /// separately validate ownership, source identity, and persistence boundaries.
    pub fn merge_or_insert_stack(&mut self, item: ItemInstance) -> Result<(usize, u16), CoreError> {
        if let Some((index, existing)) = self.items.iter_mut().enumerate().find(|(_, existing)| {
            existing.is_stack_compatible_with(&item)
                && existing.count.saturating_add(item.count) <= MAX_ITEM_STACK_COUNT
        }) {
            existing.merge_stack(&item)?;
            return Ok((index, existing.count));
        }
        if let Some(existing) = self
            .items
            .iter()
            .find(|existing| existing.is_stack_compatible_with(&item))
        {
            return Err(CoreError::ItemStackCountOverflow {
                existing: existing.count,
                incoming: item.count,
            });
        }
        self.insert(item.clone())?;
        Ok((self.items.len() - 1, item.count))
    }

    pub fn remove(&mut self, index: usize) -> Option<ItemInstance> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    /// Removes one depth-one content item from the container item at `item_index`.
    pub fn take_content(
        &mut self,
        item_index: usize,
        content_index: usize,
    ) -> Option<ItemInstance> {
        self.items.get_mut(item_index)?.take_content(content_index)
    }

    /// Consumes one unit of the bounded stack at `index`, removing the entry entirely when the
    /// last unit is used. Returns false when the index does not resolve.
    pub fn consume_item_unit(&mut self, index: usize) -> bool {
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        if item.count > 1 {
            item.count -= 1;
        } else {
            self.items.remove(index);
        }
        true
    }

    /// Takes up to `units` units from the bounded stack at `index`, decrementing or removing the
    /// entry as needed. Returns the number of units actually taken.
    pub fn take_item_units(&mut self, index: usize, mut units: u16) -> u16 {
        let Some(item) = self.items.get_mut(index) else {
            return 0;
        };
        let available = item.count.min(units);
        item.count -= available;
        if item.count == 0 {
            self.items.remove(index);
        }
        units -= available;
        let _ = &mut units;
        available
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

    /// Mutable lookup for one owned container; used by persistence round-trip tests and
    /// bounded hydration paths.
    pub fn container_mut(&mut self, container_id: u8) -> Option<&mut PlayerContainer> {
        self.containers.get_mut(&container_id)
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

/// A bounded damage transition for an imported static creature. Health is expressed in integer
/// percentage points, not TFS creature hit points; formula combat, mitigation, rewards, corpses,
/// AI, and script callbacks remain outside this foundation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureDamageOutcome {
    pub attacker_id: u64,
    pub target_id: u32,
    pub requested_damage: u16,
    pub applied_damage: u8,
    pub remaining_health_percent: u8,
    pub deactivated: bool,
}

/// Damage families remain typed even before their profile-specific formulas, resistances, visual
/// effects, and client delivery are implemented. The current bounded combat-event foundation
/// admits only physical adjacent melee resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatDamageType {
    Physical,
    Fire,
    Energy,
    Earth,
    Ice,
    Holy,
    Death,
}

/// The validated authoritative delivery rule for a bounded combat event. Future weapons, spells,
/// and projectiles must add separate explicitly tested variants rather than overloading melee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatDelivery {
    AdjacentMelee,
    /// Declarative distance shot (plan v49 slice 9): validated against Chebyshev tile
    /// distance and same-floor adjacency at application time.
    RangedDistance {
        max_range: u8,
    },
}

/// Bounded maximum for declarative distance weapons; legacy bows/crossbows stay within five.
pub const MAX_DISTANCE_WEAPON_RANGE: u8 = 5;

pub const MAX_COMBAT_EVENT_DAMAGE: u16 = 10_000;
pub const MAX_COMBAT_INTERVAL_TICKS: u16 = 60;
pub const MAX_SPELL_MANA_COST: u16 = 10_000;

/// A deliberately small, profile-neutral mitigation surface. It does not interpret TFS armor,
/// shield, equipment, skill, vocation, or PvP formulas; those require separate compatibility
/// evidence before they can populate this authoritative value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerCombatDefense {
    pub physical_flat_reduction: u16,
}

impl PlayerCombatDefense {
    pub fn new(physical_flat_reduction: u16) -> Result<Self, CoreError> {
        if physical_flat_reduction > MAX_COMBAT_EVENT_DAMAGE {
            return Err(CoreError::InvalidCombatDefense);
        }
        Ok(Self {
            physical_flat_reduction,
        })
    }

    pub const fn mitigate_physical(self, requested_damage: u16) -> u16 {
        requested_damage.saturating_sub(self.physical_flat_reduction)
    }
}

/// A bounded authoritative combat-preference selection. Its effect on damage and movement remains
/// separate until profile-specific formula and chase evidence are implemented.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlayerFightMode {
    #[default]
    Attack,
    Balanced,
    Defense,
}

/// Player-owned fight preferences. The state is intentionally non-persistent for now because
/// reconnect semantics and client delivery require separate compatibility evidence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerFightModeState {
    pub mode: PlayerFightMode,
    pub chase: bool,
    pub secure: bool,
}

/// Server-tick spacing between two accepted events from the same attacker. It is a deterministic
/// state model, not a claim of any profile-specific weapon or spell timing formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatAttackTiming {
    pub interval_ticks: u16,
}

impl CombatAttackTiming {
    pub fn new(interval_ticks: u16) -> Result<Self, CoreError> {
        if interval_ticks == 0 || interval_ticks > MAX_COMBAT_INTERVAL_TICKS {
            return Err(CoreError::InvalidCombatEvent);
        }
        Ok(Self { interval_ticks })
    }
}

/// One fully validated server-owned combat request. It has no protocol payload, random roll,
/// equipment lookup, spell definition, immunity, resistance, or PvP policy embedded in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCombatEvent {
    pub attacker_id: u64,
    pub target_id: u64,
    pub delivery: CombatDelivery,
    pub damage_type: CombatDamageType,
    pub requested_damage: u16,
    pub timing: CombatAttackTiming,
}

impl PlayerCombatEvent {
    pub fn adjacent_melee(
        attacker_id: u64,
        target_id: u64,
        damage_type: CombatDamageType,
        requested_damage: u16,
        timing: CombatAttackTiming,
    ) -> Result<Self, CoreError> {
        if requested_damage == 0 || requested_damage > MAX_COMBAT_EVENT_DAMAGE {
            return Err(CoreError::InvalidCombatEvent);
        }
        Ok(Self {
            attacker_id,
            target_id,
            delivery: CombatDelivery::AdjacentMelee,
            damage_type,
            requested_damage,
            timing,
        })
    }

    /// Declares one distance-weapon shot. The declared range is clamped to the bounded legacy
    /// maximum; per-application range and floor validation happen in the authoritative combat
    /// transition against live positions.
    pub fn distance_shot(
        attacker_id: u64,
        target_id: u64,
        damage_type: CombatDamageType,
        requested_damage: u16,
        timing: CombatAttackTiming,
        declared_range: u8,
    ) -> Result<Self, CoreError> {
        if requested_damage == 0
            || requested_damage > MAX_COMBAT_EVENT_DAMAGE
            || declared_range == 0
            || declared_range > MAX_DISTANCE_WEAPON_RANGE
        {
            return Err(CoreError::InvalidCombatEvent);
        }
        Ok(Self {
            attacker_id,
            target_id,
            delivery: CombatDelivery::RangedDistance {
                max_range: declared_range,
            },
            damage_type,
            requested_damage,
            timing,
        })
    }
}

/// Ephemeral authoritative readiness state. It is initialized for every active player and is
/// intentionally not persisted until a future profile-specific reconnect/cooldown contract exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerCombatCooldown {
    pub next_attack_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCombatEventOutcome {
    pub damage: PlayerDamageOutcome,
    pub damage_type: CombatDamageType,
    pub mitigated_damage: u16,
    pub next_attack_tick: u64,
}

/// A server-owned spell-cast request. This foundation performs only bounded resource and timing
/// accounting. Profile-specific names, words, rune IDs, targets, formulas, effects, Lua hooks,
/// PvP rules, and client delivery are deliberately outside this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSpellCastEvent {
    pub caster_id: u64,
    pub spell_id: u16,
    pub mana_cost: u16,
    pub timing: CombatAttackTiming,
}

impl PlayerSpellCastEvent {
    pub fn new(
        caster_id: u64,
        spell_id: u16,
        mana_cost: u16,
        timing: CombatAttackTiming,
    ) -> Result<Self, CoreError> {
        if spell_id == 0 || mana_cost == 0 || mana_cost > MAX_SPELL_MANA_COST {
            return Err(CoreError::InvalidSpellCastEvent);
        }
        Ok(Self {
            caster_id,
            spell_id,
            mana_cost,
            timing,
        })
    }
}

/// Ephemeral authoritative readiness for the next successful bounded spell-cast event. It is
/// intentionally separate from attack readiness and non-persistent until a reconnect contract is
/// defined by an active compatibility profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerSpellCooldown {
    pub next_cast_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSpellCastOutcome {
    pub caster_id: u64,
    pub spell_id: u16,
    pub mana_spent: u16,
    pub remaining_mana: u16,
    pub next_cast_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRenderSnapshot {
    pub id: u64,
    pub name: String,
    pub position: Position,
    pub level: u32,
    pub health_percent: u8,
}

/// Stored interaction intent only. It carries no attack resolution, automatic movement, combat,
/// scripting, spell, or action behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerInteractionIntent {
    pub target_player_id: Option<u64>,
    pub target_static_creature_id: Option<u32>,
    pub follow_player_id: Option<u64>,
}

/// A bounded request to address one top-level item in an authoritative map tile. It intentionally
/// does not execute an action, consume charges, open containers, toggle doors or switches, run
/// scripts, or produce a client packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerItemUseIntent {
    pub player_id: u64,
    pub position: Position,
    pub stack_index: u8,
    pub expected_server_id: u16,
}

impl PlayerItemUseIntent {
    pub fn new(
        player_id: u64,
        position: Position,
        stack_index: u8,
        expected_server_id: u16,
    ) -> Result<Self, CoreError> {
        if expected_server_id == 0 {
            return Err(CoreError::InvalidItemUseIntent);
        }
        Ok(Self {
            player_id,
            position,
            stack_index,
            expected_server_id,
        })
    }
}

/// Immutable authoritative item metadata observed by a successfully validated item-use intent.
/// A later bounded action runtime may consume this outcome as input, but this validation step
/// intentionally has no side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerItemUseOutcome {
    pub player_id: u64,
    pub position: Position,
    pub stack_index: u8,
    pub server_id: u16,
    pub count: u8,
    pub action_id: Option<u16>,
    pub unique_id: Option<u16>,
    pub has_text: bool,
    pub charges: Option<u16>,
    pub teleport_destination: Option<Position>,
}

/// A bounded request to use one authoritative top-level map item on another authoritative
/// top-level map item. It is validation-only: no action, charge, inventory, script, or packet
/// behavior is activated by constructing or validating this request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerItemUseExIntent {
    pub source: PlayerItemUseIntent,
    pub target: PlayerItemUseIntent,
}

impl PlayerItemUseExIntent {
    pub fn new(
        player_id: u64,
        source_position: Position,
        source_stack_index: u8,
        source_expected_server_id: u16,
        target_position: Position,
        target_stack_index: u8,
        target_expected_server_id: u16,
    ) -> Result<Self, CoreError> {
        Ok(Self {
            source: PlayerItemUseIntent::new(
                player_id,
                source_position,
                source_stack_index,
                source_expected_server_id,
            )?,
            target: PlayerItemUseIntent::new(
                player_id,
                target_position,
                target_stack_index,
                target_expected_server_id,
            )?,
        })
    }
}

/// Immutable metadata observed after both halves of a bounded two-target item-use request pass
/// the existing authoritative map, position, stack, and server-ID validation boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerItemUseExOutcome {
    pub source: PlayerItemUseOutcome,
    pub target: PlayerItemUseOutcome,
}

/// A server-owned creature identity addressed by a bounded map-item-on-creature validation
/// request. This is identity data only; it neither changes selected targets nor initiates combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerItemUseCreatureTarget {
    Player(u64),
    StaticCreature(u32),
}

/// A bounded request to validate one authoritative top-level map item against one authoritative
/// player or active static-creature target. It performs no combat, action execution, Lua call,
/// charge use, mutation, persistence, or packet delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerItemUseCreatureIntent {
    pub source: PlayerItemUseIntent,
    pub target: PlayerItemUseCreatureTarget,
}

/// Immutable target metadata observed after a bounded map-item-on-creature request passes the
/// existing map-item validation and target activity/range checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerItemUseCreatureTargetOutcome {
    Player {
        player_id: u64,
        position: Position,
    },
    StaticCreature {
        creature_id: u32,
        position: Position,
        health_percent: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerItemUseCreatureOutcome {
    pub source: PlayerItemUseOutcome,
    pub target: PlayerItemUseCreatureTargetOutcome,
}

/// Maximum number of validated worker handoff commands that one future authoritative tick may
/// accept. This primitive is intentionally separate from `WorldState`: current gameplay mutation
/// remains synchronous while the command-queue integration contract is still being designed.
pub const MAX_DETERMINISTIC_COMMAND_BATCH: usize = 4_096;

/// Stable key for a future validated command handoff. Sorting by this key makes application order
/// independent of worker scheduling order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeterministicWorldCommandKey {
    pub tick: u64,
    pub player_id: u64,
    pub session_sequence: u64,
}

/// One payload awaiting future authoritative-world application. The payload has no execution
/// trait and cannot mutate a world through this foundational type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicWorldCommand<T> {
    pub key: DeterministicWorldCommandKey,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeterministicWorldCommandBatchError {
    InvalidLimit(usize),
    CapacityExceeded { limit: usize },
    DuplicateKey(DeterministicWorldCommandKey),
}

/// A bounded, caller-synchronized collection of already validated commands. It offers no worker
/// threads, no world reference, and no execution function. A future authoritative tick may drain
/// it in stable key order after it has taken exclusive ownership of the batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicWorldCommandBatch<T> {
    limit: usize,
    commands: Vec<DeterministicWorldCommand<T>>,
}

impl<T> DeterministicWorldCommandBatch<T> {
    pub fn new(limit: usize) -> Result<Self, DeterministicWorldCommandBatchError> {
        if limit == 0 || limit > MAX_DETERMINISTIC_COMMAND_BATCH {
            return Err(DeterministicWorldCommandBatchError::InvalidLimit(limit));
        }
        Ok(Self {
            limit,
            commands: Vec::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Accepts a command only when its stable key is unique inside the current batch. Callers may
    /// submit concurrently only through their own synchronization boundary.
    pub fn push(
        &mut self,
        command: DeterministicWorldCommand<T>,
    ) -> Result<(), DeterministicWorldCommandBatchError> {
        if self
            .commands
            .iter()
            .any(|existing| existing.key == command.key)
        {
            return Err(DeterministicWorldCommandBatchError::DuplicateKey(
                command.key,
            ));
        }
        if self.commands.len() >= self.limit {
            return Err(DeterministicWorldCommandBatchError::CapacityExceeded {
                limit: self.limit,
            });
        }
        self.commands.push(command);
        Ok(())
    }

    /// Removes every queued command in deterministic application order. This does not apply a
    /// payload and does not access `WorldState`.
    pub fn drain_sorted(&mut self) -> Vec<DeterministicWorldCommand<T>> {
        let mut commands = std::mem::take(&mut self.commands);
        commands.sort_by_key(|command| command.key);
        commands
    }
}

impl<T> Default for DeterministicWorldCommandBatch<T> {
    fn default() -> Self {
        // MAX_DETERMINISTIC_COMMAND_BATCH is a compile-time constant validated once here; the
        // default constructor has no error channel, and the constant cannot be invalid.
        Self::new(MAX_DETERMINISTIC_COMMAND_BATCH)
            .expect("the built-in deterministic command-batch limit is valid")
    }
}

/// The bounded party relationship that one active observer may display for another active player.
/// It deliberately excludes shared-experience, blinking, guild, skull, and spectator policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartyDisplayRelation {
    None,
    InvitationFromLeader,
    InvitationToLeader,
    Member,
    Leader,
}

/// Fixed bounded inputs for the first session-local shared-experience eligibility model. These
/// values are explicit clean-room defaults rather than an unvalidated import of legacy config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartySharedExperienceRules {
    pub maximum_range: u16,
    pub maximum_floor_delta: u8,
    pub activity_window_ticks: u64,
}

impl Default for PartySharedExperienceRules {
    fn default() -> Self {
        Self {
            maximum_range: 30,
            maximum_floor_delta: 1,
            activity_window_ticks: 60,
        }
    }
}

/// One deterministic reason why the requested session-local shared experience is not active.
/// Experience awards, client shield colours, messages, and TFS activity semantics remain host
/// concerns outside this initial relationship-state model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartySharedExperienceEligibility {
    NotRequested,
    Eligible,
    EmptyParty,
    LevelSpreadTooLarge,
    TooFarAway,
    MemberInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartySharedExperienceState {
    pub requested: bool,
    pub eligibility: PartySharedExperienceEligibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericMoveSource {
    pub kind: GenericMoveSourceKind,
    /// Whole-stack moves leave this None; partial moves specify the exact unit count.
    pub count: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericMoveSourceKind {
    Equipment { slot: EquipmentSlot },
    Container { container_id: u8, index: usize },
}

/// Destination descriptor for a generic inventory move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericMoveTargetKind {
    /// `allow_swap` permits displacing an occupied slot back into the vacated source.
    Equipment {
        slot: EquipmentSlot,
        allow_swap: bool,
    },
    Container {
        container_id: u8,
    },
}

/// Fully resolved one-shot inventory move plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenericInventoryMove {
    pub source_kind: GenericMoveSourceKind,
    pub source_count: Option<u16>,
    pub target: GenericMoveTargetKind,
}

/// How the destination handled the moved item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenericPlacement {
    Placed,
    MergedOrInserted,
    Swapped { displaced: ItemInstance },
}

/// Outcome of a successful generic inventory move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericInventoryMoveOutcome {
    pub player_id: u64,
    pub moved_item: ItemInstance,
    pub placement: GenericPlacement,
    /// Units left at the source after a partial stack split (zero for whole moves).
    pub remaining_source_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct WorldState {
    pub(crate) players: BTreeMap<u64, Player>,
    pub(crate) player_vitals: BTreeMap<u64, PlayerVitals>,
    pub(crate) player_progressions: BTreeMap<u64, PlayerProgression>,
    pub(crate) player_progression_attempts: BTreeMap<u64, PlayerProgressionAttempts>,
    pub(crate) player_towns: BTreeMap<u64, u32>,
    pub(crate) player_regeneration_schedules: BTreeMap<u64, PlayerRegenerationSchedule>,
    pub(crate) player_food_windows: BTreeMap<u64, PlayerFoodWindow>,
    /// Operator /god toggles (plan v49 slice 18 follow-up): invincible and creature-untargetable.
    pub(crate) player_god_mode: BTreeSet<u64>,
    /// Operator /invisible toggles: hidden from creatures and every other player's viewport.
    pub(crate) player_invisible: BTreeSet<u64>,
    pub(crate) player_conditions: BTreeMap<u64, BTreeMap<PlayerConditionKind, PlayerCondition>>,
    pub(crate) player_respawn_states: BTreeMap<u64, PlayerRespawnState>,
    pub(crate) player_equipments: BTreeMap<u64, PlayerEquipment>,
    pub(crate) player_containers: BTreeMap<u64, PlayerContainers>,
    pub(crate) player_combat_defenses: BTreeMap<u64, PlayerCombatDefense>,
    pub(crate) player_fight_modes: BTreeMap<u64, PlayerFightModeState>,
    pub(crate) player_combat_cooldowns: BTreeMap<u64, PlayerCombatCooldown>,
    pub(crate) player_spell_cooldowns: BTreeMap<u64, PlayerSpellCooldown>,
    pub(crate) player_interactions: BTreeMap<u64, PlayerInteractionIntent>,
    /// Unjustified player-kill counts keyed by killer id. Classic skulls derive from this;
    /// decay and client skull frames remain separate slices.
    pub(crate) player_frags: BTreeMap<u64, u32>,
    // Party state is intentionally session-local. These indexes model the authoritative
    // participant relationship only; client icons, private channels, shared experience, loot,
    // Lua hooks, and durable persistence belong to later independently verified slices.
    pub(crate) party_leaders: BTreeSet<u64>,
    pub(crate) party_memberships: BTreeMap<u64, u64>,
    pub(crate) party_invitations: BTreeMap<u64, BTreeSet<u64>>,
    pub(crate) party_shared_experience_requested: BTreeSet<u64>,
    pub(crate) party_shared_experience_activity_ticks: BTreeMap<u64, u64>,
    pub(crate) static_creatures: BTreeMap<u32, StaticCreatureRuntime>,
    pub(crate) static_occupied_positions: BTreeSet<Position>,
    /// At most one live trade at a time per participant. The staging lists hold item snapshots
    /// for window display; the authoritative swap re-validates both inventories atomically.
    pub(crate) active_trades: BTreeMap<u64, PlayerTradeSession>,
    pub(crate) tick: u64,
    pub(crate) revision: u64,
}

/// One staged player-to-player trade. `initiator` proposed the trade to `counterparty`; each
/// side stages items by (container id, item index) references into their own inventory, plus
/// the acceptance flags that gate the final atomic swap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerTradeSession {
    pub initiator: u64,
    pub counterparty: u64,
    pub initiator_items: Vec<TradeItemReference>,
    pub counterparty_items: Vec<TradeItemReference>,
    pub initiator_accepted: bool,
    pub counterparty_accepted: bool,
    pub tick_opened: u64,
}

/// Result of an executed player trade: who traded with whom and what each side gave (now in
/// the other's inventory). Delivered to callers for packet construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerTradeExecution {
    pub initiator: u64,
    pub counterparty: u64,
    /// Items that left the initiator's inventory (the counterparty received these).
    pub initiator_gave: Vec<ItemInstance>,
    /// Items that left the counterparty's inventory (the initiator received these).
    pub counterparty_gave: Vec<ItemInstance>,
}

/// A reference into one player's owned container inventory for trade staging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradeItemReference {
    pub container_id: u8,
    pub item_index: usize,
}

impl WorldState {
    /// True when either player stands on an imported protection-zone tile (OTBM flag 0x01).
    /// Used to block PvP interactions before any combat transition.
    pub fn either_player_in_protection_zone(
        &self,
        world_map: &WorldMap,
        attacker: u64,
        defender: u64,
    ) -> bool {
        self.players
            .values()
            .filter(|player| player.id == attacker || player.id == defender)
            .any(|player| world_map.is_protection_zone(player.position))
    }

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
        self.advance_ticks(1)
    }

    /// Advances the authoritative server clock by a bounded caller-measured elapsed duration.
    /// A zero-duration update is intentionally a no-op, while any positive elapsed value advances
    /// the clock once atomically and produces one world revision for downstream snapshots.
    pub fn advance_ticks(&mut self, elapsed_seconds: u16) -> u64 {
        if elapsed_seconds == 0 {
            return self.tick;
        }
        self.tick = self.tick.saturating_add(u64::from(elapsed_seconds));
        self.mark_changed();
        self.tick
    }

    /// Applies a prevalidated experience policy and explicit per-level vocation gains in one
    /// authoritative core transition. Gains are added to current and maximum health/mana plus
    /// capacity only when the corrected level resolver reports a real positive level increase.
    /// Client HUD delivery, promotion effects, formulas, and scripting remain separate concerns.
    pub fn award_player_experience_with_vocation_gains(
        &mut self,
        player_id: u64,
        raw_experience: u64,
        policy: &ExperienceAwardPolicy,
        gains: VocationLevelUpGains,
    ) -> Result<PlayerExperienceAwardOutcome, CoreError> {
        let current_vitals = self
            .player_vitals
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))?;
        let (awarded_experience, experience, level, gained_levels) = {
            let player = self
                .players
                .get_mut(&player_id)
                .ok_or(CoreError::UnknownPlayer(player_id))?;
            let previous_level = player.level;
            let awarded_experience = policy.award_for(player.level, raw_experience);
            if awarded_experience > 0 {
                player.add_experience(awarded_experience);
            }
            (
                awarded_experience,
                player.experience,
                player.level,
                player.level.saturating_sub(previous_level),
            )
        };
        let vitals = gains.apply(current_vitals, gained_levels);
        if gained_levels > 0 {
            self.player_vitals.insert(player_id, vitals);
        }
        if awarded_experience > 0 {
            self.mark_changed();
        }
        Ok(PlayerExperienceAwardOutcome {
            player_id,
            raw_experience,
            awarded_experience,
            experience,
            level,
            gained_levels,
            vitals,
        })
    }

    pub fn player_vitals(&self, player_id: u64) -> Result<PlayerVitals, CoreError> {
        self.player_vitals
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_combat_cooldown(
        &self,
        player_id: u64,
    ) -> Result<PlayerCombatCooldown, CoreError> {
        self.player_combat_cooldowns
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_spell_cooldown(&self, player_id: u64) -> Result<PlayerSpellCooldown, CoreError> {
        self.player_spell_cooldowns
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_combat_defense(&self, player_id: u64) -> Result<PlayerCombatDefense, CoreError> {
        self.player_combat_defenses
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_fight_mode_state(
        &self,
        player_id: u64,
    ) -> Result<PlayerFightModeState, CoreError> {
        self.player_fight_modes
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    /// Replaces bounded player combat preferences without implying a damage, pursuit, persistence,
    /// or client-delivery effect. No-op replacements leave the authoritative revision unchanged.
    pub fn replace_player_fight_mode_state(
        &mut self,
        player_id: u64,
        state: PlayerFightModeState,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_fight_mode_state(player_id)? == state {
            return Ok(false);
        }
        self.player_fight_modes.insert(player_id, state);
        self.mark_changed();
        Ok(true)
    }

    /// Replaces one player's explicit profile-neutral defense value. It is not persisted yet
    /// because profile-specific armor, shielding, equipment, and reconnect semantics remain
    /// deferred; no-op replacements leave the authoritative revision unchanged.
    pub fn replace_player_combat_defense(
        &mut self,
        player_id: u64,
        defense: PlayerCombatDefense,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if defense.physical_flat_reduction > MAX_COMBAT_EVENT_DAMAGE {
            return Err(CoreError::InvalidCombatDefense);
        }
        if self.player_combat_defense(player_id)? == defense {
            return Ok(false);
        }
        self.player_combat_defenses.insert(player_id, defense);
        self.mark_changed();
        Ok(true)
    }

    pub fn player_progression(&self, player_id: u64) -> Result<PlayerProgression, CoreError> {
        self.player_progressions
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    pub fn player_progression_attempts(
        &self,
        player_id: u64,
    ) -> Result<PlayerProgressionAttempts, CoreError> {
        self.player_progression_attempts
            .get(&player_id)
            .copied()
            .ok_or(CoreError::UnknownPlayer(player_id))
    }

    /// Replaces exact progression counters after the player is known to the authoritative world.
    /// The caller is responsible for pairing these counters with a matching visible progression
    /// state during persistence hydration.
    pub fn replace_player_progression_attempts(
        &mut self,
        player_id: u64,
        attempts: PlayerProgressionAttempts,
    ) -> Result<bool, CoreError> {
        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if self.player_progression_attempts(player_id)? == attempts {
            return Ok(false);
        }
        self.player_progression_attempts.insert(player_id, attempts);
        self.mark_changed();
        Ok(true)
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

    /// Replaces one player's containers WITHOUT bumping world visibility. Used for operator
    /// item delivery where a full viewport resend would visually reset the client avatar; the
    /// caller signals the change through the dedicated containers epoch instead.
    pub fn replace_player_containers_quiet(
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
        Ok(true)
    }

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

    /// The single generic inventory-move primitive. Every public transfer function
    /// (equipmentĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă„ÄľÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ä‚â€žĂ˘â‚¬Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬Ă‚Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă„Äľcontainer, containerĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă„ÄľÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ä‚â€žĂ˘â‚¬Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬Ă‚Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă„Äľcontainer, stacks, swaps) resolves to a
    /// `GenericInventoryMove` describing source and destination, and this method executes it
    /// atomically on cloned state: any validation failure leaves the world untouched.
    pub fn execute_generic_inventory_move(
        &mut self,
        player_id: u64,
        plan: GenericInventoryMove,
    ) -> Result<GenericInventoryMoveOutcome, CoreError> {
        use GenericMoveSourceKind as Src;
        use GenericMoveTargetKind as Dst;

        if !self.players.contains_key(&player_id) {
            return Err(CoreError::UnknownPlayer(player_id));
        }
        if let (
            GenericMoveSourceKind::Equipment { slot: source_slot },
            GenericMoveTargetKind::Equipment {
                slot: target_slot, ..
            },
        ) = (plan.source_kind, plan.target)
        {
            if source_slot == target_slot {
                return Err(CoreError::SameEquipmentSlotTransfer {
                    player_id,
                    slot: source_slot,
                });
            }
        }
        let mut equipment = self.player_equipment(player_id)?.clone();
        let mut containers = self.player_containers(player_id)?.clone();

        // ---- Resolve + detach the moved item from its source -------------------------
        let (moved_item, remaining_source_count) = match plan.source_kind {
            Src::Equipment { slot } => {
                let Some(item) = equipment.item(slot).cloned() else {
                    return Err(CoreError::EmptyEquipmentSlot { player_id, slot });
                };
                let take = plan.source_count.unwrap_or(item.count);
                if take == 0 || take > item.count {
                    return Err(CoreError::InvalidItemTransferCount {
                        requested: take,
                        available: item.count,
                    });
                }
                let original_count = item.count;
                let moved = if take == item.count {
                    equipment.unequip(slot);
                    item
                } else {
                    let mut whole = item;
                    let split = whole.split_off(take)?;
                    // remainder stays equipped
                    if let Some(remaining) = equipment.item_mut(slot) {
                        *remaining = whole;
                    }
                    split
                };
                (moved, u32::from(original_count.saturating_sub(take)))
            }
            Src::Container {
                container_id,
                index,
            } => {
                let Some(container) = containers.container_mut(container_id) else {
                    return Err(CoreError::UnknownPlayerContainer {
                        player_id,
                        container_id,
                    });
                };
                let Some(item) = container.items.item(index).cloned() else {
                    return Err(CoreError::UnknownPlayerContainerItem {
                        player_id,
                        container_id,
                        item_index: index,
                    });
                };
                let take = plan.source_count.unwrap_or(item.count);
                if take == 0 || take > item.count {
                    return Err(CoreError::InvalidItemTransferCount {
                        requested: take,
                        available: item.count,
                    });
                }
                let original_count = item.count;
                let moved = if take == item.count {
                    container
                        .items
                        .remove(index)
                        .ok_or(CoreError::UnknownPlayerContainerItem {
                            player_id,
                            container_id,
                            item_index: index,
                        })?
                } else {
                    let mut whole = item;
                    let split = whole.split_off(take)?;
                    if let Some(remaining) = container.items.item_mut(index) {
                        *remaining = whole;
                    }
                    split
                };
                (moved, u32::from(original_count.saturating_sub(take)))
            }
        };

        // ---- Place the item at the destination ---------------------------------------
        let placement = match plan.target {
            Dst::Equipment { slot, allow_swap } => {
                let existing = equipment.item(slot).cloned();
                match existing {
                    None => {
                        equipment.equip(slot, moved_item.clone());
                        GenericPlacement::Placed
                    }
                    Some(current) if allow_swap && plan.source_kind != Src::Equipment { slot } => {
                        equipment.equip(slot, moved_item.clone());
                        // The displaced item returns to the source location.
                        if let Src::Container { container_id, .. } = plan.source_kind {
                            let insert_result =
                                if let Some(container) = containers.container_mut(container_id) {
                                    container.items.insert(current.clone())
                                } else {
                                    Ok(())
                                };
                            insert_result?;
                        }
                        GenericPlacement::Swapped { displaced: current }
                    }
                    Some(_) => {
                        // Restore source before failing (atomicity).
                        Self::restore_moved_item(
                            player_id,
                            &mut equipment,
                            &mut containers,
                            plan.source_kind,
                            &moved_item,
                        )?;
                        return Err(CoreError::OccupiedEquipmentSlot { player_id, slot });
                    }
                }
            }
            Dst::Container { container_id } => {
                let Some(container) = containers.container_mut(container_id) else {
                    Self::restore_moved_item(
                        player_id,
                        &mut equipment,
                        &mut containers,
                        plan.source_kind,
                        &moved_item,
                    )?;
                    return Err(CoreError::UnknownPlayerContainer {
                        player_id,
                        container_id,
                    });
                };
                match container.items.merge_or_insert_stack(moved_item.clone()) {
                    Ok(_slot) => GenericPlacement::MergedOrInserted,
                    Err(error) => {
                        Self::restore_moved_item(
                            player_id,
                            &mut equipment,
                            &mut containers,
                            plan.source_kind,
                            &moved_item,
                        )?;
                        return Err(error);
                    }
                }
            }
        };

        self.player_equipments.insert(player_id, equipment);
        self.player_containers.insert(player_id, containers);
        self.mark_changed();
        Ok(GenericInventoryMoveOutcome {
            player_id,
            moved_item,
            placement,
            remaining_source_count,
        })
    }

    fn restore_moved_item(
        player_id: u64,
        equipment: &mut PlayerEquipment,
        containers: &mut PlayerContainers,
        source: GenericMoveSourceKind,
        item: &ItemInstance,
    ) -> Result<(), CoreError> {
        match source {
            GenericMoveSourceKind::Equipment { slot } => {
                equipment.equip(slot, item.clone());
                Ok(())
            }
            GenericMoveSourceKind::Container {
                container_id,
                index: _,
            } => {
                let Some(container) = containers.container_mut(container_id) else {
                    return Err(CoreError::UnknownPlayerContainer {
                        player_id,
                        container_id,
                    });
                };
                // Best-effort reinsert at the original index position semantics.
                container.items.insert(item.clone())?;
                Ok(())
            }
        }
    }

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
        if selected_player_id.is_some() && self.player_respawn_state(player_id)?.dead {
            return Err(CoreError::PlayerIsDead(player_id));
        }
        if let Some(selected_player_id) = selected_player_id {
            if selected_player_id == player_id {
                return Err(CoreError::SelfInteractionNotAllowed(player_id));
            }
            if !self.players.contains_key(&selected_player_id) {
                return Err(CoreError::UnknownPlayer(selected_player_id));
            }
            if self.player_respawn_state(selected_player_id)?.dead {
                return Err(CoreError::SelectedPlayerIsDead(selected_player_id));
            }
        }

        let intent = {
            let intent = self.player_interactions.entry(player_id).or_default();
            if replace_target {
                intent.target_player_id = target_player_id;
                intent.target_static_creature_id = None;
            } else {
                intent.follow_player_id = follow_player_id;
                intent.target_static_creature_id = None;
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

/// Returns the classic cumulative experience requirement for a one-based level. `None` means the
/// requirement exceeds FE's `u64` experience storage and cannot be reached safely.
pub fn classic_experience_for_level(level: u32) -> Option<u64> {
    if level == 0 {
        return None;
    }
    let level = i128::from(level);
    let numerator = level
        .checked_mul(level)?
        .checked_mul(level)?
        .checked_sub(6_i128.checked_mul(level)?.checked_mul(level)?)?
        .checked_add(17_i128.checked_mul(level)?)?
        .checked_sub(12)?;
    let experience = numerator.checked_div(6)?.checked_mul(100)?;
    u64::try_from(experience).ok()
}

/// Resolves the highest classic level whose cumulative experience requirement does not exceed the
/// supplied total. This integer-only calculation changes level only where existing authoritative
/// experience award and loss paths already do so; it does not mutate vocation gains or vitals.
pub fn level_for_experience(experience: u64) -> u32 {
    let mut low = 1_u32;
    let mut high = u32::MAX;
    while low < high {
        let midpoint = low + (high - low).div_ceil(2);
        match classic_experience_for_level(midpoint) {
            Some(required) if required <= experience => low = midpoint,
            _ => high = midpoint - 1,
        }
    }
    low
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

fn scale_progression_requirement(
    base: u64,
    multiplier: ProgressionMultiplier,
    exponent: u16,
) -> u64 {
    let factor = multiplier.milli() as f64 / PROGRESSION_MULTIPLIER_SCALE as f64;
    let required = (base as f64 * factor.powi(i32::from(exponent))).floor();
    required.clamp(1.0, u64::MAX as f64) as u64
}

fn advance_skill_tries(
    mut progress: SkillProgress,
    stored_tries: u64,
    awarded_tries: u64,
    rules: PlayerProgressionRules,
    skill: PlayerSkill,
) -> (SkillProgress, u64, u16) {
    let mut stored_tries = stored_tries;
    let mut awarded_tries = awarded_tries;
    let mut gained_levels = 0_u16;
    while awarded_tries > 0 && progress.level < u16::MAX {
        let required = rules.required_skill_tries(skill, progress.level.saturating_add(1));
        let needed = required.saturating_sub(stored_tries);
        if awarded_tries < needed {
            stored_tries = stored_tries.saturating_add(awarded_tries);
            break;
        }
        awarded_tries = awarded_tries.saturating_sub(needed);
        progress.level = progress.level.saturating_add(1);
        stored_tries = 0;
        gained_levels = gained_levels.saturating_add(1);
    }
    if progress.level == u16::MAX {
        stored_tries = 0;
    }
    let required = rules.required_skill_tries(skill, progress.level.saturating_add(1));
    progress.percent = progress_percent(stored_tries, required);
    (progress, stored_tries, gained_levels)
}

fn advance_magic_mana(
    mut magic_level: u8,
    stored_mana: u64,
    awarded_mana: u64,
    rules: PlayerProgressionRules,
) -> (u8, u64, u8) {
    let mut stored_mana = stored_mana;
    let mut awarded_mana = awarded_mana;
    let mut gained_levels = 0_u8;
    while awarded_mana > 0 && magic_level < u8::MAX {
        let required = rules.required_magic_mana(magic_level.saturating_add(1));
        let needed = required.saturating_sub(stored_mana);
        if awarded_mana < needed {
            stored_mana = stored_mana.saturating_add(awarded_mana);
            break;
        }
        awarded_mana = awarded_mana.saturating_sub(needed);
        magic_level = magic_level.saturating_add(1);
        stored_mana = 0;
        gained_levels = gained_levels.saturating_add(1);
    }
    if magic_level == u8::MAX {
        stored_mana = 0;
    }
    (magic_level, stored_mana, gained_levels)
}

fn progress_percent(stored: u64, required: u64) -> u8 {
    if required == 0 {
        return 0;
    }
    ((stored.saturating_mul(100) / required).min(100)) as u8
}

fn fixed_percent_of(value: u64, percent: u8) -> u64 {
    let whole = (value / 100).saturating_mul(u64::from(percent));
    let remainder = (value % 100).saturating_mul(u64::from(percent)) / 100;
    whole.saturating_add(remainder)
}

fn cumulative_skill_tries(
    progress: SkillProgress,
    stored_tries: u64,
    rules: PlayerProgressionRules,
    skill: PlayerSkill,
) -> u64 {
    let mut total = stored_tries;
    for target_level in (MINIMUM_PLAYER_SKILL_LEVEL + 1)..=progress.level {
        total = total.saturating_add(rules.required_skill_tries(skill, target_level));
    }
    total
}

fn skill_progress_from_total(
    mut total_tries: u64,
    rules: PlayerProgressionRules,
    skill: PlayerSkill,
) -> (SkillProgress, u64) {
    let mut level = MINIMUM_PLAYER_SKILL_LEVEL;
    while level < u16::MAX {
        let required = rules.required_skill_tries(skill, level.saturating_add(1));
        if total_tries < required {
            return (
                SkillProgress {
                    level,
                    percent: progress_percent(total_tries, required),
                },
                total_tries,
            );
        }
        total_tries = total_tries.saturating_sub(required);
        level = level.saturating_add(1);
    }
    (SkillProgress { level, percent: 0 }, 0)
}

fn cumulative_magic_mana(magic_level: u8, stored_mana: u64, rules: PlayerProgressionRules) -> u64 {
    let mut total = stored_mana;
    for target_level in 1..=magic_level {
        total = total.saturating_add(rules.required_magic_mana(target_level));
    }
    total
}

fn magic_progress_from_total(mut total_mana: u64, rules: PlayerProgressionRules) -> (u8, u64) {
    let mut magic_level = 0_u8;
    while magic_level < u8::MAX {
        let required = rules.required_magic_mana(magic_level.saturating_add(1));
        if total_mana < required {
            return (magic_level, total_mana);
        }
        total_mana = total_mana.saturating_sub(required);
        magic_level = magic_level.saturating_add(1);
    }
    (magic_level, 0)
}

#[derive(Debug, PartialEq, Eq)]
pub enum CoreError {
    DuplicatePlayer(u64),
    EmptyPlayerName,
    InvalidContainerCapacity(u16),
    InvalidContainerName(usize),
    TooManyPlayerContainers(usize),
    /// A persisted party snapshot violated membership or live-state invariants.
    InvalidPartySnapshot(String),
    ContainerFull {
        capacity: u16,
    },
    /// Nested item content would exceed the bounded container depth.
    InvalidItemContentDepth,
    InvalidItemId(u16),
    InvalidClientThingId(u16),
    InvalidStaticCreatureTargetRange(u8),
    EmptyEquipmentSlot {
        player_id: u64,
        slot: EquipmentSlot,
    },
    OccupiedEquipmentSlot {
        player_id: u64,
        slot: EquipmentSlot,
    },
    UnknownPlayerContainer {
        player_id: u64,
        container_id: u8,
    },
    UnknownPlayerContainerItem {
        player_id: u64,
        container_id: u8,
        item_index: usize,
    },
    DuplicateItemPresentation(u16),
    InvalidItemStackCount(u16),
    InvalidItemTransferCount {
        requested: u16,
        available: u16,
    },
    IncompatibleItemStacks,
    SamePlayerContainerTransfer {
        player_id: u64,
        container_id: u8,
    },
    SameEquipmentSlotTransfer {
        player_id: u64,
        slot: EquipmentSlot,
    },
    ItemStackCountOverflow {
        existing: u16,
        incoming: u16,
    },
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
    /// A dynamic operator spawn request named an entity that no installed static creature or
    /// imported catalog definition can satisfy.
    UnknownEntityName(String),
    /// A dynamic spawn target tile was rejected by walkability or occupancy validation.
    SpawnPositionRejected(Position),
    /// The dynamic spawn registry is exhausted; operators must despawn before summoning more.
    DynamicSpawnLimit(usize),
    /// A player attempted to trade with themself.
    TradeWithSelf,
    /// One trade participant is already in another trade.
    PlayerAlreadyTrading(u64),
    /// No live trade session exists for this player.
    NoActiveTrade(u64),
    /// The trade-side staging set is full.
    TradeItemLimit(usize),
    /// The same container slot was staged twice on one side.
    DuplicateTradeItem,
    /// The referenced staged item was not found.
    UnknownTradeItem,
    /// One side's staged item no longer resolves against their live inventory, so the swap
    /// was refused atomically without mutating either player.
    TradeItemMissing {
        player_id: u64,
        container_id: u8,
        item_index: usize,
    },
    /// Both sides accepted but the authoritative re-validation failed before any mutation;
    /// inventories are untouched and the trade must be renegotiated.
    TradeValidationFailed(String),
    InvalidStaticCreatureHealthPercent(u8),
    InvalidStaticCreatureReactivationDelay {
        id: u32,
        remaining_seconds: u32,
        interval_seconds: u32,
    },
    InvalidStaticCreatureMeleeCooldownDelay {
        id: u32,
        remaining_ticks: u32,
        cooldown_ticks: u64,
    },
    UnknownStaticCreatureSchedule,
    StaticCreatureOccupiesPosition(Position),
    PlayerOccupiesStaticCreaturePosition(Position),
    PlayerOccupiesPosition(Position),
    UnknownStaticCreature(u32),
    InactiveStaticCreature(u32),
    StaticNpcNotAttackable(u32),
    StaticCreatureMovementBlocked(Position),
    InvalidMap(String),
    InvalidTransition {
        state: ServerStatus,
        command: LifecycleCommand,
    },
    UnknownPlayer(u64),
    PlayerNotInParty(u64),
    PlayerAlreadyInParty(u64),
    PlayerNotInPartyFree(u64),
    PartyInvitationNotFound {
        leader_id: u64,
        invitee_id: u64,
    },
    DuplicatePartyInvitation {
        leader_id: u64,
        invitee_id: u64,
    },
    PartyLeadershipTargetNotMember {
        leader_id: u64,
        new_leader_id: u64,
    },
    UnknownTown(u32),
    PlayerTownUnassigned(u64),
    InvalidPlayerRespawnState(u64),
    PlayerIsDead(u64),
    SelectedPlayerIsDead(u64),
    PlayerIsNotDead(u64),
    MissingRespawnPosition(u64),
    DeathLossAlreadyApplied(u64),
    InvalidFixedDeathLossPercent(u8),
    SelfInteractionNotAllowed(u64),
    InvalidPlayerVitals(u64),
    InvalidProgressionMultiplier(u32),
    InvalidExperienceAwardPolicy,
    InvalidSkillProgress {
        level: u16,
        percent: u8,
    },
    InvalidRegenerationInterval,
    InvalidPlayerCondition,
    InvalidDeathLossPolicy,
    InvalidCombatEvent,
    InvalidCombatDefense,
    InvalidSpellCastEvent,
    InvalidItemUseIntent,
    ItemUseOutOfRange {
        player_id: u64,
        from: Position,
        to: Position,
    },
    MissingMapTile(Position),
    UnknownMapItem {
        position: Position,
        stack_index: u8,
        expected_server_id: u16,
    },
    InsufficientMana {
        player_id: u64,
        required_mana: u16,
        available_mana: u16,
    },
    SpellCooldownActive {
        caster_id: u64,
        current_tick: u64,
        next_cast_tick: u64,
    },
    TargetAlreadyDefeated(u64),
    CombatCooldownActive {
        attacker_id: u64,
        current_tick: u64,
        next_attack_tick: u64,
    },
    CombatOutOfRange {
        attacker_id: u64,
        target_id: u64,
    },
    StaticCreatureCombatOutOfRange {
        attacker_id: u64,
        target_id: u32,
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

    fn loot_test_creature(id: u32) -> FeTfsStaticEntity {
        FeTfsStaticEntity {
            id,
            name: "Rat".into(),
            name_description: String::new(),
            position: Position {
                x: 104,
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
        }
    }

    fn always_loot(item_id: u16) -> StaticCreatureLootEntry {
        StaticCreatureLootEntry {
            item_id,
            chance: LOOT_CHANCE_SCALE,
            min_count: 2,
            max_count: 5,
        }
    }

    #[test]
    fn static_loot_roll_is_deterministic_and_respects_bounds() {
        let creature_id = 0x4000_0001;
        let collection = FeTfsStaticSpawnCollection::with_loot_tables(
            vec![loot_test_creature(creature_id)],
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::new(),
            BTreeMap::from([(
                creature_id,
                vec![
                    always_loot(2148),
                    StaticCreatureLootEntry {
                        item_id: 2666,
                        chance: 0,
                        min_count: 1,
                        max_count: 1,
                    },
                ],
            )]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();

        let first = world.roll_static_creature_loot(creature_id, 42).unwrap();
        let second = world.roll_static_creature_loot(creature_id, 42).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.items.len(), 1);
        let (item_id, count) = first.items[0];
        assert_eq!(item_id, 2148);
        assert!((2..=5).contains(&count));
    }

    #[test]
    fn static_loot_roll_skips_npcs_inactive_and_empty_tables() {
        let npc_id = 0x4000_0001;
        let monster_id = 0x4000_0002;
        let mut npc = loot_test_creature(npc_id);
        npc.name = "Guide".into();
        let collection = FeTfsStaticSpawnCollection::with_loot_tables(
            vec![npc, loot_test_creature(monster_id)],
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::from([npc_id]),
            BTreeMap::from([(monster_id, vec![always_loot(2148)])]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();

        assert_eq!(
            world
                .roll_static_creature_loot(npc_id, 7)
                .unwrap()
                .items
                .len(),
            0
        );
        assert_eq!(
            world
                .roll_static_creature_loot(monster_id, 7)
                .unwrap()
                .items
                .len(),
            1
        );
        world.deactivate_static_creature(monster_id).unwrap();
        assert_eq!(
            world
                .roll_static_creature_loot(monster_id, 7)
                .unwrap()
                .items
                .len(),
            0
        );
    }

    #[test]
    fn defeated_creature_loot_roll_stays_valid_for_the_deactivated_monster_only() {
        let npc_id = 0x4000_0001;
        let monster_id = 0x4000_0002;
        let mut npc = loot_test_creature(npc_id);
        npc.name = "Guide".into();
        let collection = FeTfsStaticSpawnCollection::with_loot_tables(
            vec![npc, loot_test_creature(monster_id)],
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::from([npc_id]),
            BTreeMap::from([(monster_id, vec![always_loot(2148)])]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();

        // The deactivated post-defeat monster still rolls its table for corpse population,
        // while an NPC never does.
        world.deactivate_static_creature(monster_id).unwrap();
        let defeated = world
            .roll_defeated_static_creature_loot(monster_id, 7)
            .unwrap();
        assert_eq!(defeated.items.len(), 1);
        assert_eq!(
            defeated.items,
            world
                .roll_defeated_static_creature_loot(monster_id, 7)
                .unwrap()
                .items,
            "equal seeds must produce equal loot"
        );
        assert_eq!(
            world
                .roll_defeated_static_creature_loot(npc_id, 7)
                .unwrap()
                .items
                .len(),
            0
        );
        assert!(matches!(
            world.roll_defeated_static_creature_loot(999, 7),
            Err(CoreError::UnknownStaticCreature(999))
        ));
    }

    #[test]
    fn static_loot_tables_reject_unknown_and_invalid_entries() {
        let creature_id = 0x4000_0001;
        assert!(FeTfsStaticSpawnCollection::with_loot_tables(
            vec![loot_test_creature(creature_id)],
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::new(),
            BTreeMap::from([(
                999,
                vec![StaticCreatureLootEntry {
                    item_id: 2148,
                    chance: LOOT_CHANCE_SCALE,
                    min_count: 1,
                    max_count: 1,
                }]
            )]),
        )
        .is_err());
        assert!(FeTfsStaticSpawnCollection::with_loot_tables(
            vec![loot_test_creature(creature_id)],
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::new(),
            BTreeMap::from([(
                creature_id,
                vec![StaticCreatureLootEntry {
                    item_id: 2148,
                    chance: LOOT_CHANCE_SCALE,
                    min_count: 5,
                    max_count: 2,
                }]
            )]),
        )
        .is_err());
    }

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

    fn party_player(id: u64, name: &str, x: u16) -> Player {
        Player {
            id,
            account_id: id,
            name: name.to_owned(),
            position: Position { x, y: 100, z: 7 },
            level: 1,
            experience: 0,
            skill_points: 0,
        }
    }

    #[test]
    fn party_invitation_acceptance_forms_one_deterministic_live_party() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Knight", 100);
        let member = party_player(8, "Druid", 101);
        world.add_player(leader.clone()).unwrap();
        world.add_player(member.clone()).unwrap();

        assert_eq!(world.invite_to_party(leader.id, member.id), Ok(()));
        assert_eq!(world.player_party_leader(leader.id), Ok(Some(leader.id)));
        assert_eq!(world.player_party_leader(member.id), Ok(None));
        assert_eq!(
            world.invite_to_party(leader.id, member.id),
            Err(CoreError::DuplicatePartyInvitation {
                leader_id: leader.id,
                invitee_id: member.id,
            })
        );

        assert_eq!(world.accept_party_invitation(member.id, leader.id), Ok(()));
        assert_eq!(world.player_party_leader(member.id), Ok(Some(leader.id)));
        assert_eq!(world.player_party_members(leader.id), Ok(vec![member.id]));
    }

    #[test]
    fn party_shared_experience_eligibility_is_bounded_and_rekeys_with_leadership() {
        let rules = PartySharedExperienceRules::default();
        let mut world = WorldState::default();
        let mut leader = party_player(7, "Knight", 100);
        leader.level = 30;
        let mut member = party_player(8, "Druid", 102);
        member.level = 20;
        world.add_player(leader.clone()).unwrap();
        world.add_player(member.clone()).unwrap();
        world.invite_to_party(leader.id, member.id).unwrap();

        assert_eq!(
            world
                .set_party_shared_experience_requested(leader.id, true, rules)
                .unwrap(),
            PartySharedExperienceState {
                requested: true,
                eligibility: PartySharedExperienceEligibility::EmptyParty,
            }
        );
        world.accept_party_invitation(member.id, leader.id).unwrap();
        assert_eq!(
            world
                .party_shared_experience_state(leader.id, rules)
                .unwrap(),
            PartySharedExperienceState {
                requested: true,
                eligibility: PartySharedExperienceEligibility::MemberInactive,
            }
        );
        world
            .record_party_shared_experience_activity(leader.id)
            .unwrap();
        world
            .record_party_shared_experience_activity(member.id)
            .unwrap();
        assert_eq!(
            world
                .party_shared_experience_state(leader.id, rules)
                .unwrap(),
            PartySharedExperienceState {
                requested: true,
                eligibility: PartySharedExperienceEligibility::Eligible,
            }
        );
        assert_eq!(
            world
                .party_shared_experience_recipients(member.id, rules)
                .unwrap(),
            Some(vec![leader.id, member.id])
        );
        world
            .transfer_party_leadership(leader.id, member.id)
            .unwrap();
        assert_eq!(
            world
                .party_shared_experience_state(member.id, rules)
                .unwrap(),
            PartySharedExperienceState {
                requested: true,
                eligibility: PartySharedExperienceEligibility::Eligible,
            }
        );
        world.advance_ticks(rules.activity_window_ticks as u16 + 1);
        assert_eq!(
            world
                .party_shared_experience_state(member.id, rules)
                .unwrap(),
            PartySharedExperienceState {
                requested: true,
                eligibility: PartySharedExperienceEligibility::MemberInactive,
            }
        );
        world.leave_party(leader.id).unwrap();
        assert_eq!(world.player_party_leader(member.id), Ok(None));
        assert_eq!(
            world.party_shared_experience_state(member.id, rules),
            Err(CoreError::PlayerNotInParty(member.id))
        );
    }

    #[test]
    fn party_shared_experience_rejects_deterministic_level_and_range_inputs() {
        let rules = PartySharedExperienceRules::default();
        let mut level_world = WorldState::default();
        let mut leader = party_player(7, "Knight", 100);
        leader.level = 30;
        let mut low_member = party_player(8, "Druid", 101);
        low_member.level = 19;
        level_world.add_player(leader.clone()).unwrap();
        level_world.add_player(low_member.clone()).unwrap();
        level_world
            .invite_to_party(leader.id, low_member.id)
            .unwrap();
        level_world
            .accept_party_invitation(low_member.id, leader.id)
            .unwrap();
        level_world
            .record_party_shared_experience_activity(leader.id)
            .unwrap();
        level_world
            .record_party_shared_experience_activity(low_member.id)
            .unwrap();
        assert_eq!(
            level_world
                .set_party_shared_experience_requested(leader.id, true, rules)
                .unwrap()
                .eligibility,
            PartySharedExperienceEligibility::LevelSpreadTooLarge
        );

        let mut range_world = WorldState::default();
        let range_leader = party_player(17, "Paladin", 100);
        let range_member = party_player(18, "Sorcerer", 131);
        range_world.add_player(range_leader.clone()).unwrap();
        range_world.add_player(range_member.clone()).unwrap();
        range_world
            .invite_to_party(range_leader.id, range_member.id)
            .unwrap();
        range_world
            .accept_party_invitation(range_member.id, range_leader.id)
            .unwrap();
        range_world
            .record_party_shared_experience_activity(range_leader.id)
            .unwrap();
        range_world
            .record_party_shared_experience_activity(range_member.id)
            .unwrap();
        assert_eq!(
            range_world
                .set_party_shared_experience_requested(range_leader.id, true, rules)
                .unwrap()
                .eligibility,
            PartySharedExperienceEligibility::TooFarAway
        );
    }

    #[test]
    fn validated_static_npcs_remain_inert_under_monster_target_and_combat_primitives() {
        let npc_id = 0x4000_0001;
        let npc_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let collection = FeTfsStaticSpawnCollection::with_combat_metadata_and_npc_ids(
            vec![FeTfsStaticEntity {
                id: npc_id,
                name: "Guide".into(),
                name_description: String::new(),
                position: npc_position,
                look_type: 128,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }],
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::from([npc_id]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();
        world.add_player(party_player(7, "Knight", 100)).unwrap();
        let map = WorldMap::new(
            "npc-inert",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );

        assert_eq!(
            world.select_static_creature_target(npc_id, 8).unwrap(),
            StaticCreatureTargetSelection {
                creature_id: npc_id,
                target_player_id: None,
                max_range: 8,
            }
        );
        assert_eq!(world.static_creature_target(npc_id), Ok(None));
        assert_eq!(
            world.step_static_creature_toward_target(npc_id, &map),
            Ok(StaticCreatureTargetStepOutcome::NoTarget)
        );
        assert_eq!(
            world.apply_static_creature_target_damage(npc_id, 10, &map),
            Ok(StaticCreatureTargetAttackOutcome::NoTarget)
        );
        assert_eq!(
            world.apply_static_creature_melee_damage(7, npc_id, 10),
            Err(CoreError::StaticNpcNotAttackable(npc_id))
        );
        assert_eq!(
            world.static_creature(npc_id).unwrap().position,
            npc_position
        );
    }

    #[test]
    fn party_member_leave_preserves_leader_and_disbands_an_empty_party() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Knight", 100);
        let member = party_player(8, "Druid", 101);
        world.add_player(leader.clone()).unwrap();
        world.add_player(member.clone()).unwrap();
        world.invite_to_party(leader.id, member.id).unwrap();
        world.accept_party_invitation(member.id, leader.id).unwrap();

        assert_eq!(world.leave_party(member.id), Ok(()));
        assert_eq!(world.player_party_leader(member.id), Ok(None));
        assert_eq!(world.player_party_leader(leader.id), Ok(None));
        assert_eq!(
            world.player_party_members(leader.id),
            Err(CoreError::PlayerNotInParty(leader.id))
        );
    }

    #[test]
    fn party_leader_leave_transfers_to_lowest_member_and_preserves_invitations() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Knight", 100);
        let first_member = party_player(8, "Druid", 101);
        let second_member = party_player(9, "Paladin", 102);
        let invitee = party_player(10, "Sorcerer", 103);
        for player in [
            leader.clone(),
            first_member.clone(),
            second_member.clone(),
            invitee.clone(),
        ] {
            world.add_player(player).unwrap();
        }
        world.invite_to_party(leader.id, first_member.id).unwrap();
        world
            .accept_party_invitation(first_member.id, leader.id)
            .unwrap();
        world.invite_to_party(leader.id, second_member.id).unwrap();
        world
            .accept_party_invitation(second_member.id, leader.id)
            .unwrap();
        world.invite_to_party(leader.id, invitee.id).unwrap();

        assert_eq!(world.leave_party(leader.id), Ok(()));
        assert_eq!(
            world.player_party_leader(first_member.id),
            Ok(Some(first_member.id))
        );
        assert_eq!(
            world.player_party_leader(second_member.id),
            Ok(Some(first_member.id))
        );
        assert_eq!(
            world.player_party_members(first_member.id),
            Ok(vec![second_member.id])
        );
        assert_eq!(
            world.accept_party_invitation(invitee.id, first_member.id),
            Ok(())
        );
    }

    #[test]
    fn explicit_party_leadership_transfer_reassigns_members_and_invitations() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Knight", 100);
        let first_member = party_player(8, "Druid", 101);
        let new_leader = party_player(9, "Paladin", 102);
        let invitee = party_player(10, "Sorcerer", 103);
        for player in [
            leader.clone(),
            first_member.clone(),
            new_leader.clone(),
            invitee.clone(),
        ] {
            world.add_player(player).unwrap();
        }
        world.invite_to_party(leader.id, first_member.id).unwrap();
        world
            .accept_party_invitation(first_member.id, leader.id)
            .unwrap();
        world.invite_to_party(leader.id, new_leader.id).unwrap();
        world
            .accept_party_invitation(new_leader.id, leader.id)
            .unwrap();
        world.invite_to_party(leader.id, invitee.id).unwrap();

        assert_eq!(
            world.transfer_party_leadership(leader.id, invitee.id),
            Err(CoreError::PartyLeadershipTargetNotMember {
                leader_id: leader.id,
                new_leader_id: invitee.id,
            })
        );
        assert_eq!(
            world.transfer_party_leadership(leader.id, new_leader.id),
            Ok(())
        );
        assert_eq!(
            world.player_party_leader(leader.id),
            Ok(Some(new_leader.id))
        );
        assert_eq!(
            world.player_party_leader(first_member.id),
            Ok(Some(new_leader.id))
        );
        assert_eq!(
            world.player_party_leader(new_leader.id),
            Ok(Some(new_leader.id))
        );
        assert_eq!(
            world.player_party_members(new_leader.id),
            Ok(vec![leader.id, first_member.id])
        );
        assert_eq!(
            world.accept_party_invitation(invitee.id, leader.id),
            Err(CoreError::PartyInvitationNotFound {
                leader_id: leader.id,
                invitee_id: invitee.id,
            })
        );
        assert_eq!(
            world.accept_party_invitation(invitee.id, new_leader.id),
            Ok(())
        );
    }

    #[test]
    fn party_snapshots_round_trip_and_reject_stale_or_invalid_restores() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Snap Leader", 100);
        let first = party_player(8, "Snap One", 101);
        let second = party_player(9, "Snap Two", 102);
        for player in [&leader, &first, &second] {
            world.add_player(player.clone()).unwrap();
        }
        world.invite_to_party(leader.id, first.id).unwrap();
        world.accept_party_invitation(first.id, leader.id).unwrap();
        world.invite_to_party(leader.id, second.id).unwrap();
        world.accept_party_invitation(second.id, leader.id).unwrap();

        let snapshots = world.party_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].0, leader.id);
        assert_eq!(snapshots[0].1, vec![first.id, second.id]);

        // Simulate a restart: fresh world, same players, no live party state.
        let mut restored = WorldState::default();
        for player in [&leader, &first, &second] {
            restored.add_player(player.clone()).unwrap();
        }
        restored
            .restore_party_snapshot(leader.id, &[second.id, first.id])
            .unwrap();
        assert_eq!(restored.player_party_leader(second.id), Ok(Some(leader.id)));
        assert_eq!(
            restored.player_party_members(leader.id),
            Ok(vec![first.id, second.id])
        );
        assert_eq!(restored.party_snapshots(), snapshots);

        // Stale snapshot must never clobber live state formed after it was taken.
        let mut live = WorldState::default();
        for player in [&leader, &first] {
            live.add_player(player.clone()).unwrap();
        }
        live.restore_party_snapshot(leader.id, &[first.id]).unwrap();
        assert!(matches!(
            live.restore_party_snapshot(first.id, &[]),
            Err(CoreError::InvalidPartySnapshot(message))
                if message.contains("already holds")
        ));

        // Structural validation: leaderless and duplicate-member forms are rejected.
        let mut clean = WorldState::default();
        for player in [&leader, &first] {
            clean.add_player(player.clone()).unwrap();
        }
        assert!(matches!(
            clean.restore_party_snapshot(leader.id, &[leader.id]),
            Err(CoreError::InvalidPartySnapshot(message))
                if message.contains("must not list the leader")
        ));
        assert!(matches!(
            clean.restore_party_snapshot(leader.id, &[first.id, first.id]),
            Err(CoreError::InvalidPartySnapshot(message))
                if message.contains("duplicate")
        ));
        // Unknown players cannot be resurrected into a party by a stale snapshot.
        assert_eq!(
            clean.restore_party_snapshot(leader.id, &[999]),
            Err(CoreError::UnknownPlayer(999))
        );
        assert_eq!(clean.player_party_leader(leader.id), Ok(None));
    }

    #[test]
    fn add_existing_party_member_attaches_only_to_live_unaffiliated_players() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Hydra Leader", 100);
        let member = party_player(8, "Hydra One", 101);
        let outsider = party_player(9, "Hydra Two", 102);
        for player in [&leader, &member, &outsider] {
            world.add_player(player.clone()).unwrap();
        }
        // No live party yet: direct attach must fail.
        assert_eq!(
            world.add_existing_party_member(leader.id, member.id),
            Err(CoreError::PlayerNotInParty(leader.id))
        );
        world.invite_to_party(leader.id, outsider.id).unwrap();
        world
            .accept_party_invitation(outsider.id, leader.id)
            .unwrap();
        // Already-affiliated targets are rejected without disturbing the party.
        assert_eq!(
            world.add_existing_party_member(leader.id, outsider.id),
            Err(CoreError::PlayerNotInPartyFree(outsider.id))
        );
        assert_eq!(
            world.party_snapshots(),
            vec![(leader.id, vec![outsider.id])]
        );
        world
            .add_existing_party_member(leader.id, member.id)
            .unwrap();
        assert_eq!(
            world.player_party_members(leader.id),
            Ok(vec![member.id, outsider.id])
        );
        // Self-attach is structurally invalid.
        assert!(matches!(
            world.add_existing_party_member(leader.id, leader.id),
            Err(CoreError::InvalidPartySnapshot(_))
        ));
    }

    #[test]
    fn party_display_relations_are_authoritative_and_deterministic() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Knight", 100);
        let member = party_player(8, "Druid", 101);
        let invitee = party_player(9, "Paladin", 102);
        let unrelated = party_player(10, "Sorcerer", 103);
        for player in [
            leader.clone(),
            member.clone(),
            invitee.clone(),
            unrelated.clone(),
        ] {
            world.add_player(player).unwrap();
        }
        world.invite_to_party(leader.id, member.id).unwrap();
        world.accept_party_invitation(member.id, leader.id).unwrap();
        world.invite_to_party(leader.id, invitee.id).unwrap();

        assert_eq!(
            world.party_display_relations(leader.id),
            Ok(vec![
                (leader.id, PartyDisplayRelation::Leader),
                (member.id, PartyDisplayRelation::Member),
                (invitee.id, PartyDisplayRelation::InvitationToLeader),
                (unrelated.id, PartyDisplayRelation::None),
            ])
        );
        assert_eq!(
            world.party_display_relations(member.id),
            Ok(vec![
                (leader.id, PartyDisplayRelation::Leader),
                (member.id, PartyDisplayRelation::Member),
                (invitee.id, PartyDisplayRelation::InvitationToLeader),
                (unrelated.id, PartyDisplayRelation::None),
            ])
        );
        assert_eq!(
            world.party_display_relations(invitee.id),
            Ok(vec![
                (leader.id, PartyDisplayRelation::InvitationFromLeader),
                (member.id, PartyDisplayRelation::None),
                (invitee.id, PartyDisplayRelation::None),
                (unrelated.id, PartyDisplayRelation::None),
            ])
        );
    }

    #[test]
    fn party_leader_without_members_disbands_and_revocation_cleans_empty_invitation_party() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Knight", 100);
        let invitee = party_player(8, "Druid", 101);
        world.add_player(leader.clone()).unwrap();
        world.add_player(invitee.clone()).unwrap();

        world.invite_to_party(leader.id, invitee.id).unwrap();
        assert_eq!(world.revoke_party_invitation(leader.id, invitee.id), Ok(()));
        assert_eq!(world.player_party_leader(leader.id), Ok(None));
        assert_eq!(
            world.revoke_party_invitation(leader.id, invitee.id),
            Err(CoreError::PlayerNotInParty(leader.id))
        );

        world.invite_to_party(leader.id, invitee.id).unwrap();
        assert_eq!(world.leave_party(leader.id), Ok(()));
        assert_eq!(world.player_party_leader(leader.id), Ok(None));
        assert_eq!(
            world.accept_party_invitation(invitee.id, leader.id),
            Err(CoreError::PartyInvitationNotFound {
                leader_id: leader.id,
                invitee_id: invitee.id,
            })
        );
    }

    #[test]
    fn player_removal_cleans_party_membership_and_pending_invitations() {
        let mut world = WorldState::default();
        let leader = party_player(7, "Knight", 100);
        let member = party_player(8, "Druid", 101);
        let invitee = party_player(9, "Paladin", 102);
        for player in [leader.clone(), member.clone(), invitee.clone()] {
            world.add_player(player).unwrap();
        }
        world.invite_to_party(leader.id, member.id).unwrap();
        world.accept_party_invitation(member.id, leader.id).unwrap();
        world.invite_to_party(leader.id, invitee.id).unwrap();

        world.remove_player(member.id).unwrap();
        assert_eq!(world.player_party_leader(leader.id), Ok(Some(leader.id)));
        world.remove_player(invitee.id).unwrap();
        assert_eq!(world.player_party_leader(leader.id), Ok(None));
        assert_eq!(
            world.remove_player(member.id),
            Err(CoreError::UnknownPlayer(member.id))
        );
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
    fn deterministic_command_batch_orders_worker_handoffs_and_rejects_duplicates() {
        let mut batch = DeterministicWorldCommandBatch::new(3).unwrap();
        let commands = [
            DeterministicWorldCommand {
                key: DeterministicWorldCommandKey {
                    tick: 8,
                    player_id: 9,
                    session_sequence: 2,
                },
                payload: "third",
            },
            DeterministicWorldCommand {
                key: DeterministicWorldCommandKey {
                    tick: 7,
                    player_id: 10,
                    session_sequence: 1,
                },
                payload: "first",
            },
            DeterministicWorldCommand {
                key: DeterministicWorldCommandKey {
                    tick: 8,
                    player_id: 9,
                    session_sequence: 1,
                },
                payload: "second",
            },
        ];
        for command in commands.clone() {
            batch.push(command).unwrap();
        }
        assert_eq!(batch.len(), 3);
        assert_eq!(
            batch.push(commands[1].clone()),
            Err(DeterministicWorldCommandBatchError::DuplicateKey(
                commands[1].key
            ))
        );
        assert_eq!(
            batch
                .drain_sorted()
                .into_iter()
                .map(|command| command.payload)
                .collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
        assert!(batch.is_empty());
    }

    #[test]
    fn deterministic_command_batch_is_bounded_and_external_mutex_handoffs_remain_ordered() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        assert_eq!(
            DeterministicWorldCommandBatch::<u8>::new(0),
            Err(DeterministicWorldCommandBatchError::InvalidLimit(0))
        );
        let batch = Arc::new(Mutex::new(DeterministicWorldCommandBatch::new(2).unwrap()));
        thread::scope(|scope| {
            for (player_id, sequence) in [(9, 2), (8, 1)] {
                let batch = Arc::clone(&batch);
                scope.spawn(move || {
                    batch
                        .lock()
                        .unwrap()
                        .push(DeterministicWorldCommand {
                            key: DeterministicWorldCommandKey {
                                tick: 4,
                                player_id,
                                session_sequence: sequence,
                            },
                            payload: player_id,
                        })
                        .unwrap();
                });
            }
        });
        let mut batch = batch.lock().unwrap();
        assert_eq!(
            batch
                .drain_sorted()
                .into_iter()
                .map(|command| command.payload)
                .collect::<Vec<_>>(),
            vec![8, 9]
        );
        batch
            .push(DeterministicWorldCommand {
                key: DeterministicWorldCommandKey {
                    tick: 5,
                    player_id: 1,
                    session_sequence: 1,
                },
                payload: 1,
            })
            .unwrap();
        batch
            .push(DeterministicWorldCommand {
                key: DeterministicWorldCommandKey {
                    tick: 5,
                    player_id: 2,
                    session_sequence: 1,
                },
                payload: 2,
            })
            .unwrap();
        assert_eq!(
            batch.push(DeterministicWorldCommand {
                key: DeterministicWorldCommandKey {
                    tick: 5,
                    player_id: 3,
                    session_sequence: 1,
                },
                payload: 3,
            }),
            Err(DeterministicWorldCommandBatchError::CapacityExceeded { limit: 2 })
        );
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

        // Nested content foundation: insert/take with bounded slots and depth.
        let mut bag = ItemInstance::new(1988, 1).unwrap();
        assert!(bag.contents().is_empty());
        bag.insert_content(ItemInstance::new(2376, 1).unwrap())
            .unwrap();
        // One nesting level: an EMPTY container may sit inside the bag...
        let inner = ItemInstance::new(1987, 1).unwrap();
        bag.insert_content(inner.clone()).unwrap();
        assert_eq!(bag.contents().len(), 2);
        let mut filled_inner = inner;
        filled_inner
            .insert_content(ItemInstance::new(2666, 1).unwrap())
            .unwrap();
        // ...but a container that already holds content would deepen past level one.
        assert_eq!(
            bag.insert_content(filled_inner),
            Err(CoreError::InvalidItemContentDepth)
        );
        let mut invalid = ItemInstance::new(2376, 1).unwrap();
        invalid.server_id = 0;
        assert_eq!(
            bag.insert_content(invalid),
            Err(CoreError::InvalidItemId(0))
        );
        for _ in 0..ItemInstance::MAX_CONTENT_SLOTS {
            let _ = bag.take_content(0);
        }
        let filler = ItemInstance::new(2376, 1).unwrap();
        for _ in 0..ItemInstance::MAX_CONTENT_SLOTS {
            bag.insert_content(filler.clone()).unwrap();
        }
        assert_eq!(
            bag.insert_content(filler.clone()),
            Err(CoreError::ContainerFull {
                capacity: ItemInstance::MAX_CONTENT_SLOTS as u16,
            })
        );
        assert!(bag.take_content(0).is_some());
        assert!(bag.take_content(usize::MAX).is_none());

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
        assert_eq!(
            catalog.unique_server_id_for_client_thing_id(102),
            Some(4526)
        );
        assert_eq!(catalog.unique_server_id_for_client_thing_id(103), None);
        assert_eq!(catalog.len(), 1);
        catalog
            .insert(
                4527,
                NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        assert_eq!(catalog.unique_server_id_for_client_thing_id(102), None);
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
    fn authoritative_equipment_to_container_transfer_is_atomic_and_bounded() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let sword = ItemInstance::new(2376, 1).unwrap();
        let shield = ItemInstance::new(2512, 1).unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::RightHand, sword.clone());
        world.replace_player_equipment(7, equipment).unwrap();

        let backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 1)
                .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack.clone()).unwrap();
        world.replace_player_containers(7, containers).unwrap();
        let revision_before_transfer = world.revision();

        let outcome = world
            .move_equipment_item_to_container(7, EquipmentSlot::RightHand, 0)
            .unwrap();
        assert_eq!(outcome.item, sword);
        assert!(world
            .player_equipment(7)
            .unwrap()
            .item(EquipmentSlot::RightHand)
            .is_none());
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0),
            Some(&sword)
        );
        assert_eq!(world.revision(), revision_before_transfer + 1);

        let revision_before_empty_source = world.revision();
        assert_eq!(
            world.move_equipment_item_to_container(7, EquipmentSlot::RightHand, 0),
            Err(CoreError::EmptyEquipmentSlot {
                player_id: 7,
                slot: EquipmentSlot::RightHand,
            })
        );
        assert_eq!(world.revision(), revision_before_empty_source);

        let mut equipment = world.player_equipment(7).unwrap().clone();
        equipment.equip(EquipmentSlot::LeftHand, shield.clone());
        world.replace_player_equipment(7, equipment).unwrap();
        let revision_before_full_destination = world.revision();
        assert_eq!(
            world.move_equipment_item_to_container(7, EquipmentSlot::LeftHand, 0),
            Err(CoreError::ContainerFull { capacity: 1 })
        );
        assert_eq!(world.revision(), revision_before_full_destination);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::LeftHand),
            Some(&shield)
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0),
            Some(&sword)
        );
    }

    #[test]
    fn authoritative_container_to_equipment_transfer_is_atomic_and_bounded() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let sword = ItemInstance::new(2376, 1).unwrap();
        let shield = ItemInstance::new(2512, 1).unwrap();
        let mut backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 2)
                .unwrap();
        backpack.items.insert(sword.clone()).unwrap();
        backpack.items.insert(shield.clone()).unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        world.replace_player_containers(7, containers).unwrap();

        let revision_before_transfer = world.revision();
        let outcome = world
            .move_container_item_to_equipment(7, 0, 0, EquipmentSlot::RightHand)
            .unwrap();
        assert_eq!(outcome.item, sword);
        assert_eq!(outcome.to_slot, EquipmentSlot::RightHand);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&sword)
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0),
            Some(&shield)
        );
        assert_eq!(world.revision(), revision_before_transfer + 1);

        let revision_before_occupied = world.revision();
        assert_eq!(
            world.move_container_item_to_equipment(7, 0, 0, EquipmentSlot::RightHand),
            Err(CoreError::OccupiedEquipmentSlot {
                player_id: 7,
                slot: EquipmentSlot::RightHand,
            })
        );
        assert_eq!(world.revision(), revision_before_occupied);

        let revision_before_missing_item = world.revision();
        assert_eq!(
            world.move_container_item_to_equipment(7, 0, 5, EquipmentSlot::LeftHand),
            Err(CoreError::UnknownPlayerContainerItem {
                player_id: 7,
                container_id: 0,
                item_index: 5,
            })
        );
        assert_eq!(world.revision(), revision_before_missing_item);

        let revision_before_missing_container = world.revision();
        assert_eq!(
            world.move_container_item_to_equipment(7, 1, 0, EquipmentSlot::LeftHand),
            Err(CoreError::UnknownPlayerContainer {
                player_id: 7,
                container_id: 1,
            })
        );
        assert_eq!(world.revision(), revision_before_missing_container);
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0),
            Some(&shield)
        );
    }

    #[test]
    fn authoritative_generic_inventory_move_covers_splits_swaps_and_atomic_rejection() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();

        let sword = ItemInstance::new(2376, 1).unwrap();
        let shield = ItemInstance::new(2512, 1).unwrap();
        let mut backpack = PlayerContainer::new(
            0,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        backpack
            .items
            .insert(ItemInstance::new(2544, 60).unwrap())
            .unwrap();
        backpack.items.insert(sword.clone()).unwrap();
        let mut bag =
            PlayerContainer::new(1, ItemInstance::new(1988, 1).unwrap(), "Bag", false, 20).unwrap();
        bag.items
            .insert(ItemInstance::new(2544, 30).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        containers.insert(bag).unwrap();
        world.replace_player_containers(7, containers).unwrap();

        let revision_before_transfer = world.revision();
        let outcome = world
            .execute_generic_inventory_move(
                7,
                GenericInventoryMove {
                    source_kind: GenericMoveSourceKind::Container {
                        container_id: 0,
                        index: 1,
                    },
                    source_count: None,
                    target: GenericMoveTargetKind::Equipment {
                        slot: EquipmentSlot::RightHand,
                        allow_swap: false,
                    },
                },
            )
            .unwrap();
        assert_eq!(outcome.placement, GenericPlacement::Placed);
        assert_eq!(outcome.moved_item, sword);
        assert_eq!(outcome.remaining_source_count, 0);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&sword)
        );
        assert_eq!(world.revision(), revision_before_transfer + 1);

        let revision_before_split = world.revision();
        let outcome = world
            .execute_generic_inventory_move(
                7,
                GenericInventoryMove {
                    source_kind: GenericMoveSourceKind::Container {
                        container_id: 0,
                        index: 0,
                    },
                    source_count: Some(10),
                    target: GenericMoveTargetKind::Container { container_id: 1 },
                },
            )
            .unwrap();
        assert_eq!(outcome.placement, GenericPlacement::MergedOrInserted);
        assert_eq!(outcome.moved_item.count, 10);
        assert_eq!(outcome.remaining_source_count, 50);
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0),
            Some(&ItemInstance::new(2544, 50).unwrap())
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(1)
                .unwrap()
                .items
                .item(0),
            Some(&ItemInstance::new(2544, 40).unwrap())
        );
        assert_eq!(world.revision(), revision_before_split + 1);

        let mut equipment = world.player_equipment(7).unwrap().clone();
        equipment.equip(EquipmentSlot::LeftHand, shield.clone());
        world.replace_player_equipment(7, equipment).unwrap();
        let mut containers = world.player_containers(7).unwrap().clone();
        containers
            .container_mut(0)
            .unwrap()
            .items
            .insert(sword.clone())
            .unwrap();
        world.replace_player_containers(7, containers).unwrap();
        let outcome = world
            .execute_generic_inventory_move(
                7,
                GenericInventoryMove {
                    source_kind: GenericMoveSourceKind::Container {
                        container_id: 0,
                        index: 1,
                    },
                    source_count: None,
                    target: GenericMoveTargetKind::Equipment {
                        slot: EquipmentSlot::LeftHand,
                        allow_swap: true,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            outcome.placement,
            GenericPlacement::Swapped {
                displaced: shield.clone()
            }
        );
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::LeftHand),
            Some(&sword)
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(1),
            Some(&shield)
        );

        let revision_before_rejection = world.revision();
        assert_eq!(
            world.execute_generic_inventory_move(
                7,
                GenericInventoryMove {
                    source_kind: GenericMoveSourceKind::Container {
                        container_id: 0,
                        index: 1,
                    },
                    source_count: None,
                    target: GenericMoveTargetKind::Equipment {
                        slot: EquipmentSlot::RightHand,
                        allow_swap: false,
                    },
                },
            ),
            Err(CoreError::OccupiedEquipmentSlot {
                player_id: 7,
                slot: EquipmentSlot::RightHand,
            })
        );
        assert_eq!(
            world.execute_generic_inventory_move(
                7,
                GenericInventoryMove {
                    source_kind: GenericMoveSourceKind::Equipment {
                        slot: EquipmentSlot::RightHand,
                    },
                    source_count: None,
                    target: GenericMoveTargetKind::Equipment {
                        slot: EquipmentSlot::RightHand,
                        allow_swap: true,
                    },
                },
            ),
            Err(CoreError::SameEquipmentSlotTransfer {
                player_id: 7,
                slot: EquipmentSlot::RightHand,
            })
        );
        assert_eq!(
            world.execute_generic_inventory_move(
                99,
                GenericInventoryMove {
                    source_kind: GenericMoveSourceKind::Container {
                        container_id: 0,
                        index: 0,
                    },
                    source_count: None,
                    target: GenericMoveTargetKind::Container { container_id: 1 },
                },
            ),
            Err(CoreError::UnknownPlayer(99))
        );
        assert!(matches!(
            world.execute_generic_inventory_move(
                7,
                GenericInventoryMove {
                    source_kind: GenericMoveSourceKind::Container {
                        container_id: 0,
                        index: 0,
                    },
                    source_count: Some(0),
                    target: GenericMoveTargetKind::Container { container_id: 1 },
                },
            ),
            Err(CoreError::InvalidItemTransferCount { .. })
        ));
        assert_eq!(world.revision(), revision_before_rejection);
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0),
            Some(&ItemInstance::new(2544, 50).unwrap())
        );
    }

    #[test]
    fn authoritative_container_stack_transfer_merges_between_distinct_containers() {
        let gold = 2148;
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let mut source =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 2)
                .unwrap();
        source
            .items
            .insert(ItemInstance::new(gold, 40).unwrap())
            .unwrap();
        let mut destination =
            PlayerContainer::new(1, ItemInstance::new(1988, 1).unwrap(), "Bag", false, 2).unwrap();
        destination
            .items
            .insert(ItemInstance::new(gold, 20).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(source).unwrap();
        containers.insert(destination).unwrap();
        world.replace_player_containers(7, containers).unwrap();

        assert_eq!(
            world
                .move_container_stack_to_container(7, 0, 0, 1, 15)
                .unwrap(),
            PlayerContainerStackToContainerOutcome {
                player_id: 7,
                from_container_id: 0,
                item_index: 0,
                to_container_id: 1,
                destination_index: 0,
                moved_item: ItemInstance::new(gold, 15).unwrap(),
                source_remaining_count: Some(25),
                destination_count: 35,
            }
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0)
                .unwrap()
                .count,
            25
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(1)
                .unwrap()
                .items
                .item(0)
                .unwrap()
                .count,
            35
        );
    }

    #[test]
    fn authoritative_container_to_occupied_equipment_swap_is_atomic_and_bounded() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let sword = ItemInstance::new(2376, 1).unwrap();
        let shield = ItemInstance::new(2512, 1).unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::RightHand, shield.clone());
        world.replace_player_equipment(7, equipment).unwrap();
        let mut backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 1)
                .unwrap();
        backpack.items.insert(sword.clone()).unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        world.replace_player_containers(7, containers).unwrap();
        let revision_before_swap = world.revision();

        let outcome = world
            .swap_container_item_with_equipment(7, 0, 0, EquipmentSlot::RightHand)
            .unwrap();
        assert_eq!(outcome.container_item, sword);
        assert_eq!(outcome.equipped_item, shield);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&sword)
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0),
            Some(&shield)
        );
        assert_eq!(world.revision(), revision_before_swap + 1);

        let revision_before_invalid_source = world.revision();
        assert_eq!(
            world.swap_container_item_with_equipment(7, 0, 1, EquipmentSlot::RightHand),
            Err(CoreError::UnknownPlayerContainerItem {
                player_id: 7,
                container_id: 0,
                item_index: 1,
            })
        );
        assert_eq!(world.revision(), revision_before_invalid_source);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&sword)
        );
    }

    #[test]
    fn authoritative_equipment_slot_swap_is_atomic_and_bounded() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let sword = ItemInstance::new(2376, 1).unwrap();
        let shield = ItemInstance::new(2512, 1).unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::RightHand, sword.clone());
        equipment.equip(EquipmentSlot::LeftHand, shield.clone());
        world.replace_player_equipment(7, equipment).unwrap();
        let revision_before_swap = world.revision();

        let outcome = world
            .swap_equipment_items(7, EquipmentSlot::RightHand, EquipmentSlot::LeftHand)
            .unwrap();

        assert_eq!(outcome.from_item, sword);
        assert_eq!(outcome.to_item, shield);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&shield)
        );
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::LeftHand),
            Some(&sword)
        );
        assert_eq!(world.revision(), revision_before_swap + 1);

        let revision_before_rejections = world.revision();
        assert_eq!(
            world.swap_equipment_items(7, EquipmentSlot::RightHand, EquipmentSlot::RightHand),
            Err(CoreError::SameEquipmentSlotTransfer {
                player_id: 7,
                slot: EquipmentSlot::RightHand,
            })
        );
        assert_eq!(
            world.swap_equipment_items(7, EquipmentSlot::Head, EquipmentSlot::RightHand),
            Err(CoreError::EmptyEquipmentSlot {
                player_id: 7,
                slot: EquipmentSlot::Head,
            })
        );
        assert_eq!(world.revision(), revision_before_rejections);
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&shield)
        );
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::LeftHand),
            Some(&sword)
        );
    }

    #[test]
    fn authoritative_item_stack_transfers_split_merge_and_reject_invalid_state() {
        let gold = 2148;
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(gold, 80).unwrap(),
        );
        world.replace_player_equipment(7, equipment).unwrap();
        let mut backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 2)
                .unwrap();
        backpack
            .items
            .insert(ItemInstance::new(gold, 10).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        world.replace_player_containers(7, containers).unwrap();

        assert_eq!(
            world
                .move_equipment_stack_to_container(7, EquipmentSlot::RightHand, 0, 15)
                .unwrap(),
            PlayerEquipmentStackToContainerOutcome {
                player_id: 7,
                from_slot: EquipmentSlot::RightHand,
                container_id: 0,
                destination_index: 0,
                moved_item: ItemInstance::new(gold, 15).unwrap(),
                source_remaining_count: Some(65),
                destination_count: 25,
            }
        );
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .unwrap()
                .count,
            65
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0)
                .unwrap()
                .count,
            25
        );

        let revision_before_invalid_count = world.revision();
        assert_eq!(
            world.move_equipment_stack_to_container(7, EquipmentSlot::RightHand, 0, 0),
            Err(CoreError::InvalidItemTransferCount {
                requested: 0,
                available: 65,
            })
        );
        assert_eq!(world.revision(), revision_before_invalid_count);

        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::LeftHand,
            ItemInstance::new(gold, 70).unwrap(),
        );
        world.replace_player_equipment(7, equipment).unwrap();
        let mut backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 2)
                .unwrap();
        backpack
            .items
            .insert(ItemInstance::new(gold, 20).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        world.replace_player_containers(7, containers).unwrap();

        assert_eq!(
            world
                .move_container_stack_to_equipment(7, 0, 0, EquipmentSlot::LeftHand, 15)
                .unwrap(),
            PlayerContainerStackToEquipmentOutcome {
                player_id: 7,
                container_id: 0,
                item_index: 0,
                to_slot: EquipmentSlot::LeftHand,
                moved_item: ItemInstance::new(gold, 15).unwrap(),
                source_remaining_count: Some(5),
                destination_count: 85,
            }
        );
        assert_eq!(
            world
                .player_equipment(7)
                .unwrap()
                .item(EquipmentSlot::LeftHand)
                .unwrap()
                .count,
            85
        );
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0)
                .unwrap()
                .count,
            5
        );

        let revision_before_incompatible = world.revision();
        let equipment_before_incompatible = world.player_equipment(7).unwrap().clone();
        let containers_before_incompatible = world.player_containers(7).unwrap().clone();
        let mut equipment = equipment_before_incompatible.clone();
        equipment.equip(EquipmentSlot::LeftHand, ItemInstance::new(2376, 1).unwrap());
        world.replace_player_equipment(7, equipment).unwrap();
        assert_eq!(
            world.move_container_stack_to_equipment(7, 0, 0, EquipmentSlot::LeftHand, 5),
            Err(CoreError::IncompatibleItemStacks)
        );
        assert_eq!(
            world.player_containers(7).unwrap(),
            &containers_before_incompatible
        );
        assert_eq!(world.revision(), revision_before_incompatible + 1);

        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::LeftHand,
            ItemInstance::new(gold, 96).unwrap(),
        );
        world.replace_player_equipment(7, equipment).unwrap();
        let revision_before_overflow = world.revision();
        assert_eq!(
            world.move_container_stack_to_equipment(7, 0, 0, EquipmentSlot::LeftHand, 5),
            Err(CoreError::ItemStackCountOverflow {
                existing: 96,
                incoming: 5,
            })
        );
        assert_eq!(world.revision(), revision_before_overflow);
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0)
                .unwrap()
                .count,
            5
        );
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
        assert_eq!(
            world
                .teleport_player(
                    7,
                    Position {
                        x: 250,
                        y: 300,
                        z: 7,
                    },
                )
                .unwrap(),
            (
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                Position {
                    x: 250,
                    y: 300,
                    z: 7,
                },
            )
        );
        assert_eq!(
            world.player(7).unwrap().position,
            Position {
                x: 250,
                y: 300,
                z: 7,
            }
        );
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
    fn authoritative_world_clock_batches_elapsed_seconds_without_zero_tick_mutation() {
        let mut world = WorldState::default();
        assert_eq!(world.advance_ticks(0), 0);
        assert_eq!(world.tick(), 0);
        assert_eq!(world.revision(), 0);

        assert_eq!(world.advance_ticks(3), 3);
        assert_eq!(world.tick(), 3);
        assert_eq!(world.revision(), 1);

        assert_eq!(world.advance_tick(), 4);
        assert_eq!(world.revision(), 2);
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
                health_percent: 100,
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
                target_static_creature_id: None,
                follow_player_id: None,
            })
        );
        assert_eq!(
            world.set_player_follow(source.id, Some(selected.id)),
            Ok(PlayerInteractionIntent {
                target_player_id: Some(selected.id),
                target_static_creature_id: None,
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

        world
            .hydrate_player_respawn_state(
                source.id,
                PlayerRespawnState {
                    dead: true,
                    respawn_at: Some(Position {
                        x: 110,
                        y: 120,
                        z: 7,
                    }),
                    death_time: Some(0),
                    loss_applied: false,
                },
            )
            .unwrap();
        let dead_revision = world.revision();
        assert_eq!(
            world.set_player_target(source.id, Some(selected.id)),
            Err(CoreError::PlayerIsDead(source.id))
        );
        assert_eq!(
            world.set_player_follow(source.id, Some(selected.id)),
            Err(CoreError::PlayerIsDead(source.id))
        );
        assert_eq!(world.revision(), dead_revision);
        assert!(world.set_player_target(source.id, None).is_ok());

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
    fn player_follow_step_is_single_deterministic_and_does_not_route_around_occupancy() {
        let mut source = player();
        source.position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let target = Player {
            id: 8,
            account_id: 3,
            name: "Druid".into(),
            position: Position {
                x: 103,
                y: 100,
                z: 7,
            },
            ..source.clone()
        };
        let mut map = WorldMap::new("player-follow", source.position);
        for x in 100..=103 {
            map.set_tile(
                Position { x, y: 100, z: 7 },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: true,
                },
            )
            .unwrap();
        }
        let mut world = WorldState::default();
        world.add_player(source.clone()).unwrap();
        world.add_player(target.clone()).unwrap();
        world.set_player_follow(source.id, Some(target.id)).unwrap();

        assert_eq!(
            world.follow_player_targets_once(&map).unwrap(),
            BTreeSet::from([source.id])
        );
        assert_eq!(
            world.player(source.id).unwrap().position,
            Position {
                x: 101,
                y: 100,
                z: 7,
            }
        );

        let blocker = Player {
            id: 9,
            account_id: 4,
            name: "Blocker".into(),
            position: Position {
                x: 102,
                y: 100,
                z: 7,
            },
            ..source
        };
        world.add_player(blocker).unwrap();
        assert!(world.follow_player_targets_once(&map).unwrap().is_empty());
        assert_eq!(
            world.player(7).unwrap().position,
            Position {
                x: 101,
                y: 100,
                z: 7,
            }
        );
    }

    #[test]
    fn player_death_clears_other_players_target_and_follow_intents() {
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
        world
            .set_player_target(source.id, Some(selected.id))
            .unwrap();
        world
            .set_player_follow(source.id, Some(selected.id))
            .unwrap();

        let temple = Position {
            x: 110,
            y: 120,
            z: 7,
        };
        let mut map = WorldMap::new("dead-interaction-target", temple);
        map.set_town(WorldMapTown {
            id: 42,
            name: "Thais".to_owned(),
            temple_position: temple,
        })
        .unwrap();

        world.apply_player_death(selected.id, 42, &map).unwrap();
        assert!(world.player_respawn_state(selected.id).unwrap().dead);
        assert_eq!(
            world.player_interaction_intent(source.id),
            Ok(PlayerInteractionIntent::default())
        );
    }

    #[test]
    fn static_creature_target_intent_requires_an_active_authoritative_entity() {
        let source = player();
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
        world.add_player(source.clone()).unwrap();
        assert_eq!(
            world.set_player_static_target(source.id, Some(creature_id)),
            Ok(PlayerInteractionIntent {
                target_player_id: None,
                target_static_creature_id: Some(creature_id),
                follow_player_id: None,
            })
        );
        assert_eq!(
            world.set_player_target(source.id, None),
            Ok(PlayerInteractionIntent::default())
        );
        world
            .set_player_static_target(source.id, Some(creature_id))
            .unwrap();
        assert!(world.deactivate_static_creature(creature_id).unwrap());
        assert_eq!(
            world.player_interaction_intent(source.id),
            Ok(PlayerInteractionIntent::default())
        );
        assert_eq!(
            world.set_player_static_target(source.id, Some(creature_id)),
            Err(CoreError::InactiveStaticCreature(creature_id))
        );
        assert_eq!(
            world.set_player_static_target(source.id, Some(creature_id + 1)),
            Err(CoreError::UnknownStaticCreature(creature_id + 1))
        );
    }

    #[test]
    fn static_creature_display_health_is_bounded_runtime_state_without_combat_behavior() {
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
            health_percent: 75,
            direction: 2,
        };
        let mut invalid_creature = creature.clone();
        invalid_creature.health_percent = 101;
        assert_eq!(
            FeTfsStaticSpawnCollection::new(vec![invalid_creature]),
            Err(CoreError::InvalidStaticCreatureHealthPercent(101))
        );

        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        assert_eq!(world.static_creature_health_percent(creature_id), Ok(75));
        let revision = world.revision();
        assert!(world
            .set_static_creature_health_percent(creature_id, 40)
            .unwrap());
        assert_eq!(world.static_creature_health_percent(creature_id), Ok(40));
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(
            world.active_static_spawn_collection().entities[0].health_percent,
            40
        );
        assert!(!world
            .set_static_creature_health_percent(creature_id, 40)
            .unwrap());
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(
            world.set_static_creature_health_percent(creature_id, 101),
            Err(CoreError::InvalidStaticCreatureHealthPercent(101))
        );
        assert!(world
            .set_static_creature_health_percent(creature_id, 0)
            .unwrap());
        assert!(world.static_creature_lifecycle(creature_id).unwrap().active);
        assert_eq!(world.static_creature_health_percent(creature_id), Ok(0));
        assert!(world.deactivate_static_creature(creature_id).unwrap());
        assert_eq!(
            world.set_static_creature_health_percent(creature_id, 1),
            Err(CoreError::InactiveStaticCreature(creature_id))
        );
        assert_eq!(world.reset_static_creatures().reactivated, 1);
        assert_eq!(world.static_creature_health_percent(creature_id), Ok(75));
    }

    #[test]
    fn static_creature_melee_uses_percentage_damage_and_deactivates_only_on_a_real_zero_hit() {
        let attacker = player();
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
            health_percent: 15,
            direction: 2,
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        world.add_player(attacker.clone()).unwrap();
        assert_eq!(
            world.apply_static_creature_melee_damage(attacker.id, creature_id, 0),
            Err(CoreError::InvalidCombatEvent)
        );
        world
            .set_static_creature_health_percent(creature_id, 0)
            .unwrap();
        let no_op = world
            .apply_static_creature_melee_damage(attacker.id, creature_id, 10)
            .unwrap();
        assert_eq!(no_op.applied_damage, 0);
        assert!(!no_op.deactivated);
        assert!(world.static_creature_lifecycle(creature_id).unwrap().active);

        world
            .set_static_creature_health_percent(creature_id, 15)
            .unwrap();
        world
            .set_player_static_target(attacker.id, Some(creature_id))
            .unwrap();
        let first = world
            .apply_static_creature_melee_damage(attacker.id, creature_id, 10)
            .unwrap();
        assert_eq!(first.applied_damage, 10);
        assert_eq!(first.remaining_health_percent, 5);
        assert!(!first.deactivated);
        assert_eq!(
            world
                .player_combat_cooldown(attacker.id)
                .unwrap()
                .next_attack_tick,
            1
        );
        assert_eq!(
            world.apply_static_creature_melee_damage(attacker.id, creature_id, 10),
            Err(CoreError::CombatCooldownActive {
                attacker_id: attacker.id,
                current_tick: 0,
                next_attack_tick: 1,
            })
        );
        assert_eq!(world.static_creature_health_percent(creature_id), Ok(5));
        world.advance_tick();
        let final_hit = world
            .apply_static_creature_melee_damage(attacker.id, creature_id, 10)
            .unwrap();
        assert_eq!(final_hit.applied_damage, 5);
        assert_eq!(final_hit.remaining_health_percent, 0);
        assert!(final_hit.deactivated);
        assert!(!world.static_creature_lifecycle(creature_id).unwrap().active);
        assert_eq!(
            world.player_interaction_intent(attacker.id),
            Ok(PlayerInteractionIntent::default())
        );

        world.reset_static_creatures();
        let mut distant = attacker;
        distant.id = 8;
        distant.position.x = 103;
        world.add_player(distant).unwrap();
        assert_eq!(
            world.apply_static_creature_melee_damage(8, creature_id, 1),
            Err(CoreError::StaticCreatureCombatOutOfRange {
                attacker_id: 8,
                target_id: creature_id,
            })
        );
    }

    #[test]
    fn authoritative_map_item_use_is_bounded_side_effect_free_and_position_validated() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let spawn = player().position;
        let adjacent = Position {
            x: spawn.x + 1,
            y: spawn.y,
            z: spawn.z,
        };
        let far = Position {
            x: spawn.x + 2,
            y: spawn.y,
            z: spawn.z,
        };
        let mut map = WorldMap::new("item-use", spawn);
        for position in [spawn, adjacent, far] {
            map.set_tile(
                position,
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: true,
                },
            )
            .unwrap();
        }
        map.set_tile_items(
            spawn,
            vec![WorldMapItem {
                server_id: 1945,
                client_thing_id: Some(1945),
                count: 1,
                action_id: Some(7),
                unique_id: Some(42),
                text: Some("Read me".into()),
                description: None,
                teleport_destination: Some(far),
                duration: None,
                charges: Some(3),
                children: Vec::new(),
            }],
        )
        .unwrap();
        map.set_tile_items(
            adjacent,
            vec![WorldMapItem {
                server_id: 2376,
                client_thing_id: Some(2376),
                count: 2,
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
        let revision = world.revision();
        assert_eq!(
            world
                .validate_player_item_use(
                    &map,
                    PlayerItemUseIntent::new(7, spawn, 0, 1945).unwrap(),
                )
                .unwrap(),
            PlayerItemUseOutcome {
                player_id: 7,
                position: spawn,
                stack_index: 0,
                server_id: 1945,
                count: 1,
                action_id: Some(7),
                unique_id: Some(42),
                has_text: true,
                charges: Some(3),
                teleport_destination: Some(far),
            }
        );
        assert_eq!(
            world
                .validate_player_item_use(
                    &map,
                    PlayerItemUseIntent::new(7, adjacent, 0, 2376).unwrap(),
                )
                .unwrap()
                .count,
            2
        );
        assert_eq!(
            world
                .validate_player_item_use_ex(
                    &map,
                    PlayerItemUseExIntent::new(7, spawn, 0, 1945, adjacent, 0, 2376).unwrap(),
                )
                .unwrap(),
            PlayerItemUseExOutcome {
                source: PlayerItemUseOutcome {
                    player_id: 7,
                    position: spawn,
                    stack_index: 0,
                    server_id: 1945,
                    count: 1,
                    action_id: Some(7),
                    unique_id: Some(42),
                    has_text: true,
                    charges: Some(3),
                    teleport_destination: Some(far),
                },
                target: PlayerItemUseOutcome {
                    player_id: 7,
                    position: adjacent,
                    stack_index: 0,
                    server_id: 2376,
                    count: 2,
                    action_id: None,
                    unique_id: None,
                    has_text: false,
                    charges: None,
                    teleport_destination: None,
                },
            }
        );
        assert_eq!(
            PlayerItemUseExIntent::new(7, spawn, 0, 1945, adjacent, 0, 0),
            Err(CoreError::InvalidItemUseIntent)
        );
        assert_eq!(
            world.validate_player_item_use_ex(
                &map,
                PlayerItemUseExIntent::new(7, spawn, 0, 1945, far, 0, 2376).unwrap(),
            ),
            Err(CoreError::ItemUseOutOfRange {
                player_id: 7,
                from: spawn,
                to: far,
            })
        );
        assert_eq!(world.revision(), revision);
        assert_eq!(
            PlayerItemUseIntent::new(7, adjacent, 0, 0),
            Err(CoreError::InvalidItemUseIntent)
        );
        assert_eq!(
            world.validate_player_item_use(
                &map,
                PlayerItemUseIntent::new(7, far, 0, 2376).unwrap(),
            ),
            Err(CoreError::ItemUseOutOfRange {
                player_id: 7,
                from: spawn,
                to: far,
            })
        );
        let missing = Position {
            x: spawn.x,
            y: spawn.y + 1,
            z: spawn.z,
        };
        assert_eq!(
            world.validate_player_item_use(
                &map,
                PlayerItemUseIntent::new(7, missing, 0, 2376).unwrap(),
            ),
            Err(CoreError::MissingMapTile(missing))
        );
        assert_eq!(
            world.validate_player_item_use(
                &map,
                PlayerItemUseIntent::new(7, adjacent, 1, 2376).unwrap(),
            ),
            Err(CoreError::UnknownMapItem {
                position: adjacent,
                stack_index: 1,
                expected_server_id: 2376,
            })
        );
    }

    #[test]
    fn authoritative_map_item_use_creature_is_bounded_side_effect_free_and_target_validated() {
        let mut world = WorldState::default();
        let source_player = player();
        let spawn = source_player.position;
        let adjacent = Position {
            x: spawn.x + 1,
            y: spawn.y,
            z: spawn.z,
        };
        let far = Position {
            x: spawn.x + 2,
            y: spawn.y,
            z: spawn.z,
        };
        world.add_player(source_player).unwrap();
        world
            .add_player(Player {
                id: 8,
                account_id: 8,
                name: "Druid".into(),
                position: Position {
                    x: spawn.x,
                    y: spawn.y + 1,
                    z: spawn.z,
                },
                level: 8,
                experience: 0,
                skill_points: 0,
            })
            .unwrap();
        let near_static_id = 0x4000_0001;
        let far_static_id = 0x4000_0002;
        world
            .install_static_creatures(
                &FeTfsStaticSpawnCollection::new(vec![
                    FeTfsStaticEntity {
                        id: near_static_id,
                        name: "Rat".into(),
                        name_description: String::new(),
                        position: adjacent,
                        look_type: 21,
                        head: 0,
                        body: 0,
                        legs: 0,
                        feet: 0,
                        addons: 0,
                        speed: 134,
                        health_percent: 75,
                        direction: 2,
                    },
                    FeTfsStaticEntity {
                        id: far_static_id,
                        name: "Snake".into(),
                        name_description: String::new(),
                        position: far,
                        look_type: 21,
                        head: 0,
                        body: 0,
                        legs: 0,
                        feet: 0,
                        addons: 0,
                        speed: 134,
                        health_percent: 100,
                        direction: 2,
                    },
                ])
                .unwrap(),
            )
            .unwrap();
        let mut map = WorldMap::new("item-use-creature", spawn);
        map.set_tile(
            spawn,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile_items(
            spawn,
            vec![WorldMapItem {
                server_id: 1945,
                client_thing_id: Some(1945),
                count: 1,
                action_id: Some(7),
                unique_id: None,
                text: None,
                description: None,
                teleport_destination: None,
                duration: None,
                charges: Some(3),
                children: Vec::new(),
            }],
        )
        .unwrap();
        let revision = world.revision();
        let source = PlayerItemUseIntent::new(7, spawn, 0, 1945).unwrap();
        let static_outcome = world
            .validate_player_item_use_creature(
                &map,
                PlayerItemUseCreatureIntent {
                    source,
                    target: PlayerItemUseCreatureTarget::StaticCreature(near_static_id),
                },
            )
            .unwrap();
        assert_eq!(static_outcome.source.server_id, 1945);
        assert_eq!(
            static_outcome.target,
            PlayerItemUseCreatureTargetOutcome::StaticCreature {
                creature_id: near_static_id,
                position: adjacent,
                health_percent: 75,
            }
        );
        assert_eq!(
            world
                .validate_player_item_use_creature(
                    &map,
                    PlayerItemUseCreatureIntent {
                        source,
                        target: PlayerItemUseCreatureTarget::Player(8),
                    },
                )
                .unwrap()
                .target,
            PlayerItemUseCreatureTargetOutcome::Player {
                player_id: 8,
                position: Position {
                    x: spawn.x,
                    y: spawn.y + 1,
                    z: spawn.z,
                },
            }
        );
        assert_eq!(
            world.validate_player_item_use_creature(
                &map,
                PlayerItemUseCreatureIntent {
                    source,
                    target: PlayerItemUseCreatureTarget::StaticCreature(999),
                },
            ),
            Err(CoreError::UnknownStaticCreature(999))
        );
        assert_eq!(
            world.validate_player_item_use_creature(
                &map,
                PlayerItemUseCreatureIntent {
                    source,
                    target: PlayerItemUseCreatureTarget::StaticCreature(far_static_id),
                },
            ),
            Err(CoreError::ItemUseOutOfRange {
                player_id: 7,
                from: spawn,
                to: far,
            })
        );
        assert_eq!(world.revision(), revision);
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
        assert_eq!(value.level, 5);
    }

    #[test]
    fn classic_experience_thresholds_are_integer_bounded_and_monotonic() {
        assert_eq!(classic_experience_for_level(0), None);
        assert_eq!(classic_experience_for_level(1), Some(0));
        assert_eq!(classic_experience_for_level(2), Some(100));
        assert_eq!(classic_experience_for_level(5), Some(800));
        assert_eq!(classic_experience_for_level(8), Some(4_200));
        assert_eq!(level_for_experience(4_199), 7);
        assert_eq!(level_for_experience(4_200), 8);
        let maximum_representable = level_for_experience(u64::MAX);
        assert!(classic_experience_for_level(maximum_representable).is_some());
        if maximum_representable < u32::MAX {
            assert!(classic_experience_for_level(maximum_representable + 1).is_none());
        }
    }

    #[test]
    fn authoritative_experience_awards_apply_validated_rate_and_level_stages() {
        let stages = vec![
            ExperienceAwardStage::new(1, 1, 2_000).unwrap(),
            ExperienceAwardStage::new(2, u32::MAX, 3_000).unwrap(),
        ];
        let policy = ExperienceAwardPolicy::new(5, stages).unwrap();
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let revision_before_award = world.revision();

        let first = world.award_player_experience(7, 100, &policy).unwrap();
        assert_eq!(first.raw_experience, 100);
        assert_eq!(first.awarded_experience, 1_000);
        assert_eq!(first.experience, 1_000);
        assert_eq!(first.level, 5);
        assert_eq!(first.gained_levels, 4);
        assert_eq!(first.vitals, PlayerVitals::default());
        assert_eq!(world.revision(), revision_before_award + 1);

        let second = world.award_player_experience(7, 100, &policy).unwrap();
        assert_eq!(second.awarded_experience, 1_500);
        assert_eq!(second.experience, 2_500);
        assert_eq!(second.level, 6);

        let disabled = ExperienceAwardPolicy::new(0, Vec::new()).unwrap();
        let revision_before_disabled = world.revision();
        let disabled_outcome = world.award_player_experience(7, 100, &disabled).unwrap();
        assert_eq!(disabled_outcome.awarded_experience, 0);
        assert_eq!(disabled_outcome.experience, 2_500);
        assert_eq!(world.revision(), revision_before_disabled);

        assert_eq!(
            ExperienceAwardPolicy::new(
                1,
                vec![
                    ExperienceAwardStage::new(1, 10, 1_000).unwrap(),
                    ExperienceAwardStage::new(10, u32::MAX, 1_000).unwrap(),
                ],
            ),
            Err(CoreError::InvalidExperienceAwardPolicy)
        );
    }

    #[test]
    fn vocation_aware_experience_awards_apply_gains_only_for_real_level_increases() {
        let policy = ExperienceAwardPolicy::new(10, Vec::new()).unwrap();
        let initial_vitals = PlayerVitals {
            health: 50,
            max_health: 100,
            mana: 20,
            max_mana: 50,
            capacity: 500,
            magic_level: 4,
        };
        let gains = VocationLevelUpGains::new(15, 5, 25);
        let mut world = WorldState::default();
        world
            .add_player_with_vitals_and_progression(
                player(),
                initial_vitals,
                PlayerProgression {
                    vocation: BaseVocation::Knight.id(),
                    ..PlayerProgression::default()
                },
            )
            .unwrap();

        let no_award = world
            .award_player_experience_with_vocation_gains(
                7,
                100,
                &ExperienceAwardPolicy::new(0, Vec::new()).unwrap(),
                gains,
            )
            .unwrap();
        assert_eq!(no_award.gained_levels, 0);
        assert_eq!(no_award.vitals, initial_vitals);

        let advanced = world
            .award_player_experience_with_vocation_gains(7, 100, &policy, gains)
            .unwrap();
        assert_eq!(advanced.gained_levels, 4);
        assert_eq!(advanced.level, 5);
        assert_eq!(
            advanced.vitals,
            PlayerVitals {
                health: 110,
                max_health: 160,
                mana: 40,
                max_mana: 70,
                capacity: 600,
                magic_level: 4,
            }
        );
        assert_eq!(world.player_vitals(7).unwrap(), advanced.vitals);

        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: u16::MAX - 1,
                    max_health: u16::MAX - 1,
                    mana: u16::MAX - 1,
                    max_mana: u16::MAX - 1,
                    capacity: u16::MAX - 1,
                    magic_level: 4,
                },
            )
            .unwrap();
        let saturated = world
            .award_player_experience_with_vocation_gains(
                7,
                u64::MAX,
                &ExperienceAwardPolicy::new(1, Vec::new()).unwrap(),
                gains,
            )
            .unwrap();
        assert!(saturated.gained_levels > 0);
        assert_eq!(saturated.vitals.health, u16::MAX);
        assert_eq!(saturated.vitals.max_health, u16::MAX);
        assert_eq!(saturated.vitals.mana, u16::MAX);
        assert_eq!(saturated.vitals.max_mana, u16::MAX);
        assert_eq!(saturated.vitals.capacity, u16::MAX);
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
    fn source_map_revision_is_stable_and_changes_with_complete_item_content() {
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("revision", spawn);
        map.set_tile(
            spawn,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile_items(
            spawn,
            vec![WorldMapItem {
                server_id: 1988,
                client_thing_id: Some(1988),
                count: 1,
                action_id: Some(7),
                unique_id: Some(8),
                text: Some("read me".into()),
                description: Some("a note".into()),
                teleport_destination: Some(Position {
                    x: 101,
                    y: 100,
                    z: 7,
                }),
                duration: Some(30),
                charges: Some(2),
                children: vec![WorldMapItem {
                    server_id: 2000,
                    client_thing_id: Some(2000),
                    count: 3,
                    action_id: None,
                    unique_id: None,
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            }],
        )
        .unwrap();

        let original = map.source_revision();
        assert_eq!(original, map.clone().source_revision());

        let mut changed = map.clone();
        changed
            .set_tile_items(
                spawn,
                vec![WorldMapItem {
                    server_id: 1988,
                    client_thing_id: Some(1988),
                    count: 2,
                    action_id: Some(7),
                    unique_id: Some(8),
                    text: Some("read me".into()),
                    description: Some("a note".into()),
                    teleport_destination: Some(Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    }),
                    duration: Some(30),
                    charges: Some(2),
                    children: Vec::new(),
                }],
            )
            .unwrap();
        assert_ne!(original, changed.source_revision());
    }

    #[test]
    fn source_item_identity_is_ordered_revision_bound_and_duplicate_safe() {
        let position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("identity", position);
        map.set_tile(
            position,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        let item = |count| WorldMapItem {
            server_id: 1988,
            client_thing_id: Some(1988),
            count,
            action_id: None,
            unique_id: None,
            text: None,
            description: None,
            teleport_destination: None,
            duration: None,
            charges: None,
            children: Vec::new(),
        };
        map.set_tile_items(position, vec![item(1), item(1)])
            .unwrap();

        let first = map.source_item_identity(position, 0).unwrap();
        let second = map.source_item_identity(position, 1).unwrap();
        assert_ne!(first, second);
        assert_eq!(first.position, position);
        assert_eq!(first.item_index, 0);
        assert_eq!(second.item_index, 1);
        assert_eq!(map.source_item_identity(position, 2), None);

        map.set_tile_items(position, vec![item(2), item(1)])
            .unwrap();
        assert_ne!(
            first.map_revision,
            map.source_item_identity(position, 0).unwrap().map_revision
        );
    }

    #[test]
    fn source_item_recovery_rejects_revision_mismatch_without_mutation() {
        let position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("recovery", position);
        map.set_tile(
            position,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile_items(
            position,
            vec![WorldMapItem {
                server_id: 1988,
                client_thing_id: Some(1988),
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
        let mut identity = map.source_item_identity(position, 0).unwrap();
        identity.map_revision = WorldMapSourceRevision(identity.map_revision.0.wrapping_add(1));
        assert!(map.apply_source_item_removals(&[identity]).is_err());
        assert_eq!(map.tile_items(position).unwrap().len(), 1);
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
            name_description: String::new(),
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
            name_description: String::new(),
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
            name_description: String::new(),
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
        let collection = FeTfsStaticSpawnCollection::with_respawn_intervals(
            vec![creature],
            BTreeMap::from([(0x4000_0001, 3)]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();
        assert_eq!(
            world.static_creature_lifecycle(0x4000_0001),
            Some(StaticCreatureLifecycle {
                id: 0x4000_0001,
                spawn_position,
                position: spawn_position,
                active: true,
                health_percent: 100,
                activated_at_tick: 0,
                inactive_since_tick: None,
                reactivation_due_tick: None,
                respawn_interval_seconds: 3,
            })
        );
        assert!(world.deactivate_static_creature(0x4000_0001).unwrap());
        assert!(!world.deactivate_static_creature(0x4000_0001).unwrap());
        assert!(!world.is_static_creature_occupied(spawn_position));
        assert_eq!(world.active_static_creature_count(), 0);
        assert_eq!(
            world
                .static_creature_lifecycle(0x4000_0001)
                .unwrap()
                .inactive_since_tick,
            Some(0)
        );

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
    fn static_creature_due_reactivation_uses_per_spawn_interval_and_occupancy() {
        let spawn_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
        let collection = FeTfsStaticSpawnCollection::with_respawn_intervals(
            vec![creature],
            BTreeMap::from([(creature_id, 3)]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();
        assert!(world.deactivate_static_creature(creature_id).unwrap());
        world.advance_ticks(2);
        assert_eq!(
            world.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 0,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        world.advance_tick();
        assert_eq!(
            world.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 1,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        assert_eq!(
            world
                .static_creature_lifecycle(creature_id)
                .unwrap()
                .activated_at_tick,
            3
        );

        assert!(world.deactivate_static_creature(creature_id).unwrap());
        world.add_player(player()).unwrap();
        world
            .move_player_cardinal(7, CardinalDirection::East)
            .unwrap();
        world.advance_ticks(3);
        assert_eq!(
            world.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 0,
                deferred_by_player_occupancy: 1,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        assert_eq!(
            world
                .static_creature_lifecycle(creature_id)
                .unwrap()
                .reactivation_due_tick,
            Some(9)
        );
        world
            .move_player_cardinal(7, CardinalDirection::West)
            .unwrap();
        assert_eq!(
            world.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 0,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        world.advance_ticks(2);
        assert_eq!(
            world.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 0,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        world.advance_tick();
        assert_eq!(
            world.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 1,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );
    }

    #[test]
    fn monster_due_reactivation_is_blocked_by_a_player_inside_its_retained_spawn_area() {
        let creature_id = 0x4000_0001;
        let spawn_position = Position {
            x: 105,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
        let collection = FeTfsStaticSpawnCollection::with_respawn_intervals(
            vec![creature],
            BTreeMap::from([(creature_id, 3)]),
        )
        .unwrap()
        .with_monster_spawn_areas(BTreeMap::from([(
            creature_id,
            StaticCreatureSpawnArea {
                center: spawn_position,
                radius: 5,
            },
        )]))
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();
        world.add_player(player()).unwrap();
        world.deactivate_static_creature(creature_id).unwrap();
        world.advance_ticks(3);

        assert_eq!(
            world.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 0,
                deferred_by_player_occupancy: 1,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        assert_eq!(
            world
                .static_creature_lifecycle(creature_id)
                .unwrap()
                .reactivation_due_tick,
            Some(6)
        );
    }

    #[test]
    fn static_creature_runtime_restores_only_the_remaining_reactivation_delay() {
        let creature_id = 0x4000_0001;
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let collection = FeTfsStaticSpawnCollection::with_respawn_intervals(
            vec![FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
            }],
            BTreeMap::from([(creature_id, 5)]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.install_static_creatures(&collection).unwrap();
        world.deactivate_static_creature(creature_id).unwrap();
        world.advance_ticks(2);
        let snapshot = world.static_creature_runtime_snapshot();
        assert_eq!(
            snapshot,
            vec![StaticCreatureRuntimeSnapshot {
                id: creature_id,
                position,
                active: false,
                health_percent: 100,
                reactivation_remaining_seconds: Some(3),
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
            }]
        );

        let mut fresh = WorldState::default();
        fresh.install_static_creatures(&collection).unwrap();
        assert_eq!(
            fresh.restore_static_creature_runtime(&snapshot),
            Ok(StaticCreatureRuntimeRestoreSummary {
                restored: 1,
                ignored_unknown: 0,
            })
        );
        fresh.advance_ticks(2);
        assert_eq!(
            fresh.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 0,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );
        fresh.advance_tick();
        assert_eq!(
            fresh.reactivate_due_static_creatures(),
            StaticCreatureResetSummary {
                reactivated: 1,
                deferred_by_player_occupancy: 0,
                deferred_by_static_creature_occupancy: 0,
            }
        );

        let before_invalid = fresh.static_creature_runtime_snapshot();
        assert_eq!(
            fresh.restore_static_creature_runtime(&[StaticCreatureRuntimeSnapshot {
                id: creature_id,
                position,
                active: false,
                health_percent: 100,
                reactivation_remaining_seconds: Some(6),
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
            }]),
            Err(CoreError::InvalidStaticCreatureReactivationDelay {
                id: creature_id,
                remaining_seconds: 6,
                interval_seconds: 5,
            })
        );
        assert_eq!(fresh.static_creature_runtime_snapshot(), before_invalid);
    }

    #[test]
    fn static_creature_target_selection_is_bounded_deterministic_and_cleanup_safe() {
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: Position {
                x: 104,
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
        let mut first = player();
        first.position = Position {
            x: 102,
            y: 100,
            z: 7,
        };
        let mut second = first.clone();
        second.id = 8;
        second.name = "Paladin".into();
        second.position = Position {
            x: 106,
            y: 100,
            z: 7,
        };
        let mut world = WorldState::default();
        world.add_player(first).unwrap();
        world.add_player(second).unwrap();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();

        let revision_before_target = world.revision();
        assert_eq!(
            world.select_static_creature_target(creature_id, 2),
            Ok(StaticCreatureTargetSelection {
                creature_id,
                target_player_id: Some(7),
                max_range: 2,
            })
        );
        assert_eq!(world.static_creature_target(creature_id), Ok(Some(7)));
        assert_eq!(world.revision(), revision_before_target + 1);
        assert_eq!(
            world.select_static_creature_target(creature_id, 2),
            Ok(StaticCreatureTargetSelection {
                creature_id,
                target_player_id: Some(7),
                max_range: 2,
            })
        );
        assert_eq!(world.revision(), revision_before_target + 1);

        assert_eq!(
            world.select_static_creature_target(creature_id, 1),
            Ok(StaticCreatureTargetSelection {
                creature_id,
                target_player_id: None,
                max_range: 1,
            })
        );
        assert_eq!(world.static_creature_target(creature_id), Ok(None));
        assert_eq!(
            world.select_static_creature_target(creature_id, 0),
            Err(CoreError::InvalidStaticCreatureTargetRange(0))
        );
        assert_eq!(
            world.select_static_creature_target(creature_id, MAX_STATIC_CREATURE_TARGET_RANGE + 1,),
            Err(CoreError::InvalidStaticCreatureTargetRange(
                MAX_STATIC_CREATURE_TARGET_RANGE + 1
            ))
        );

        world.select_static_creature_target(creature_id, 2).unwrap();
        world.remove_player(7).unwrap();
        assert_eq!(world.static_creature_target(creature_id), Ok(None));
        assert!(world.deactivate_static_creature(creature_id).unwrap());
        assert_eq!(
            world.static_creature_target(creature_id),
            Err(CoreError::InactiveStaticCreature(creature_id))
        );
    }

    #[test]
    fn static_creature_target_damage_requires_a_live_adjacent_target_and_reuses_death_state() {
        let creature_id = 0x4000_0001;
        let creature_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let target_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let temple_position = Position {
            x: 102,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: creature_position,
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
        let mut target = player();
        target.position = target_position;
        let mut map = WorldMap::new("static-target-attack", creature_position);
        map.set_town(WorldMapTown {
            id: 1,
            name: "Temple".into(),
            temple_position,
        })
        .unwrap();
        let mut world = WorldState::default();
        world.add_player(target).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 5,
                    max_health: 5,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();

        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::NoTarget)
        );
        world.select_static_creature_target(creature_id, 1).unwrap();
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 5, &map),
            Err(CoreError::UnknownTown(0))
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 5);
        assert_eq!(world.static_creature_target(creature_id), Ok(Some(7)));
        world.replace_player_town(7, 1).unwrap();
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 2, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                creature_id,
                target_player_id: 7,
                requested_damage: 2,
                applied_damage: 2,
                remaining_health: 3,
                death_state: None,
            })
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 3);
        assert_eq!(world.static_creature_target(creature_id), Ok(Some(7)));
        world
            .replace_player_combat_defense(7, PlayerCombatDefense::new(3).unwrap())
            .unwrap();
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 3, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                creature_id,
                target_player_id: 7,
                requested_damage: 3,
                applied_damage: 0,
                remaining_health: 3,
                death_state: None,
            })
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 3);
        assert!(!world.player_respawn_state(7).unwrap().dead);
        world
            .replace_player_combat_defense(7, PlayerCombatDefense::default())
            .unwrap();

        let distant_position = Position {
            x: 103,
            y: 100,
            z: 7,
        };
        world.move_player(7, temple_position).unwrap();
        world.move_player(7, distant_position).unwrap();
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 3, &map),
            Ok(StaticCreatureTargetAttackOutcome::TargetNotAdjacent {
                creature_id,
                target_player_id: 7,
            })
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 3);

        world.move_player(7, temple_position).unwrap();
        world.move_player(7, target_position).unwrap();
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 3, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                creature_id,
                target_player_id: 7,
                requested_damage: 3,
                applied_damage: 3,
                remaining_health: 0,
                death_state: Some(PlayerRespawnState {
                    dead: true,
                    respawn_at: Some(temple_position),
                    death_time: Some(0),
                    loss_applied: false,
                }),
            })
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 0);
        assert!(world.player_respawn_state(7).unwrap().dead);
        assert_eq!(world.static_creature_target(creature_id), Ok(None));
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::NoTarget)
        );

        world.deactivate_static_creature(creature_id).unwrap();
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Err(CoreError::InactiveStaticCreature(creature_id))
        );
    }

    #[test]
    fn static_creature_direct_melee_metadata_prevents_early_repeated_attacks() {
        let creature_id = 0x4000_0002;
        let creature_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: creature_position,
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
        let mut target = player();
        target.position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let map = WorldMap::new("direct-melee-cooldown", creature_position);
        let static_spawns = FeTfsStaticSpawnCollection::with_runtime_metadata(
            vec![creature],
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::from([(creature_id, 2_000)]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.add_player(target.clone()).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 20,
                    max_health: 20,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        world.install_static_creatures(&static_spawns).unwrap();
        world.select_static_creature_target(creature_id, 1).unwrap();

        assert!(matches!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 1,
                ..
            })
        ));
        let snapshot = world.static_creature_runtime_snapshot();
        assert_eq!(snapshot[0].direct_melee_cooldown_remaining_ticks, Some(2));

        let mut fresh = WorldState::default();
        fresh.add_player(target).unwrap();
        fresh
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 20,
                    max_health: 20,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        fresh.install_static_creatures(&static_spawns).unwrap();
        assert_eq!(
            fresh.restore_static_creature_runtime(&snapshot),
            Ok(StaticCreatureRuntimeRestoreSummary {
                restored: 1,
                ignored_unknown: 0,
            })
        );
        fresh.select_static_creature_target(creature_id, 1).unwrap();
        assert_eq!(
            fresh.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::CooldownNotDue {
                creature_id,
                due_tick: 2,
            })
        );
        fresh.advance_ticks(2);
        assert!(matches!(
            fresh.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 1,
                ..
            })
        ));
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::CooldownNotDue {
                creature_id,
                due_tick: 2,
            })
        );
        world.advance_tick();
        assert_eq!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::CooldownNotDue {
                creature_id,
                due_tick: 2,
            })
        );
        world.advance_tick();
        assert!(matches!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 1,
                ..
            })
        ));
    }

    #[test]
    fn static_creature_direct_melee_damage_range_cycles_deterministically() {
        let creature_id = 0x4000_0004;
        let creature_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: creature_position,
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
        let mut target = player();
        target.position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let map = WorldMap::new("direct-melee-range", creature_position);
        let mut world = WorldState::default();
        world.add_player(target).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 50,
                    max_health: 50,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        world
            .install_static_creatures(
                &FeTfsStaticSpawnCollection::with_combat_metadata(
                    vec![creature],
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::from([(
                        creature_id,
                        StaticCreatureDirectMeleeDamageRange {
                            min_damage: 2,
                            max_damage: 4,
                        },
                    )]),
                )
                .unwrap(),
            )
            .unwrap();
        world.select_static_creature_target(creature_id, 1).unwrap();

        for requested_damage in [2, 3, 4, 2] {
            assert!(matches!(
                world.apply_static_creature_target_damage(creature_id, 1, &map),
                Ok(StaticCreatureTargetAttackOutcome::Applied {
                    requested_damage: actual,
                    applied_damage: applied,
                    ..
                }) if actual == requested_damage && applied == requested_damage
            ));
        }
        assert_eq!(world.player_vitals(7).unwrap().health, 39);
    }

    #[test]
    fn god_and_invisible_players_are_untargetable_and_invincible() {
        let mut world = WorldState::default();
        let mut spawn_at = |id, x, y| {
            let mut player = player();
            player.id = id;
            player.position = Position { x, y, z: 7 };
            world.add_player(player).unwrap();
        };
        spawn_at(1, 100, 100); // god-mode GM
        spawn_at(2, 102, 100); // invisible scout
        spawn_at(3, 104, 100); // regular player
        let mut creature = FeTfsStaticEntity {
            id: 0x4000_0001,
            name: "Rat".into(),
            name_description: String::new(),
            position: Position {
                x: 106,
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
        let _ = &mut creature;
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        world.set_player_god_mode(1, true).unwrap();
        world.set_player_invisible(2, true).unwrap();
        // The only unflagged player leaves acquisition range: the creature must find nobody.
        world
            .teleport_player(
                3,
                Position {
                    x: 130,
                    y: 100,
                    z: 7,
                },
            )
            .unwrap();

        // Creature targeting skips both flagged players entirely.
        let selection = world.select_static_creature_target(0x4000_0001, 8).unwrap();
        assert_eq!(selection.target_player_id, None);

        // PvP against either flagged player is rejected (attacker walks up to the god first).
        world
            .teleport_player(
                3,
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            )
            .unwrap();
        let event = PlayerCombatEvent::adjacent_melee(
            3,
            1,
            CombatDamageType::Physical,
            5,
            CombatAttackTiming::new(2).unwrap(),
        )
        .unwrap();
        let pvp_result = world.apply_player_combat_event(event);
        assert!(matches!(pvp_result, Err(CoreError::InvalidCombatEvent)));

        // Invincibility zeroes every damage funnel.
        let health_before = world.player_vitals(1).unwrap().health;
        let outcome = world.apply_player_melee_damage(3, 1, 9).unwrap();
        assert_eq!(outcome.applied_damage, 0);
        assert_eq!(world.player_vitals(1).unwrap().health, health_before);
    }

    #[test]
    fn classic_food_windows_feed_once_regen_and_refuse_when_full() {
        let mut world = WorldState::default();
        let mut eater = player();
        eater.id = 5;
        world.add_player(eater).unwrap();
        world
            .update_player_vitals(
                5,
                PlayerVitals {
                    health: 40,
                    max_health: 100,
                    mana: 10,
                    max_mana: 50,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();

        // First bite grants the window; a second while active answers "You are full."
        assert!(world.grant_player_food_window(5, 8).unwrap());
        assert!(!world.grant_player_food_window(5, 8).unwrap());
        assert_eq!(world.player_food_window_remaining_ticks(5), Some(8));

        // Four elapsed seconds: exactly one cadence point of health.
        let rules = PlayerRegenerationRules {
            health: RegenerationRule::new(100, 0).unwrap(),
            mana: RegenerationRule::new(100, 0).unwrap(),
        };
        let outcome = world.apply_player_regeneration(5, rules, 4).unwrap();
        assert_eq!(outcome.health_gained, FOOD_REGENERATION_HEALTH_PER_INTERVAL);

        // Window expiry is authoritative-tick absolute: once the clock passes it, eating works.
        world.advance_ticks(9);
        assert_eq!(world.player_food_window_remaining_ticks(5), None);
        assert!(world.grant_player_food_window(5, 8).unwrap());
    }

    #[test]
    fn static_creature_direct_melee_declares_and_peeks_without_consuming() {
        let creature_id = 0x4000_0007;
        let declared_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let declared = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: declared_position,
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
        let undeclared_id = 0x4000_0008;
        let mut undeclared = declared.clone();
        undeclared.id = undeclared_id;
        undeclared.position = Position {
            x: 102,
            y: 100,
            z: 7,
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(
                &FeTfsStaticSpawnCollection::with_combat_metadata(
                    vec![declared, undeclared],
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::from([(
                        creature_id,
                        StaticCreatureDirectMeleeDamageRange {
                            min_damage: 2,
                            max_damage: 4,
                        },
                    )]),
                )
                .unwrap(),
            )
            .unwrap();

        assert!(world.static_creature_declares_direct_melee(creature_id));
        assert!(!world.static_creature_declares_direct_melee(undeclared_id));
        assert_eq!(
            world.static_creature_declared_damage_for_next_hit(creature_id),
            Some(2)
        );
        // Peeking never advances the deterministic cycling sequence.
        assert_eq!(
            world.static_creature_declared_damage_for_next_hit(creature_id),
            Some(2)
        );
        assert_eq!(
            world.static_creature_declared_damage_for_next_hit(undeclared_id),
            None
        );
    }

    #[test]
    fn distance_shots_validate_range_and_floor_before_applying() {
        let mut map = WorldMap::new(
            "distance",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        for x in 95..=110 {
            for y in 98..=102 {
                map.set_tile(
                    Position { x, y, z: 7 },
                    WorldMapTile {
                        ground_thing_id: 102,
                        walkable: true,
                    },
                )
                .unwrap();
            }
        }
        let mut world = WorldState::default();
        let mut shooter = player();
        shooter.id = 1;
        world.add_player(shooter).unwrap();
        let mut target = player();
        target.id = 2;
        target.position = Position {
            x: 104,
            y: 101,
            z: 7,
        };
        world.add_player(target).unwrap();
        world
            .update_player_vitals(
                2,
                PlayerVitals {
                    health: 50,
                    max_health: 50,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();

        // Within the declared range (Chebyshev 4 of a declared 5): applies.
        let event = PlayerCombatEvent::distance_shot(
            1,
            2,
            CombatDamageType::Physical,
            5,
            CombatAttackTiming::new(2).unwrap(),
            5,
        )
        .unwrap();
        let outcome = world.apply_player_combat_event(event).unwrap();
        assert_eq!(outcome.damage.applied_damage, 5);

        // Beyond any bounded range: rejected.
        world
            .teleport_player(
                2,
                Position {
                    x: 108,
                    y: 101,
                    z: 7,
                },
            )
            .unwrap();
        let event = PlayerCombatEvent::distance_shot(
            1,
            2,
            CombatDamageType::Physical,
            5,
            CombatAttackTiming::new(2).unwrap(),
            5,
        )
        .unwrap();
        assert!(matches!(
            world.apply_player_combat_event(event),
            Err(CoreError::CombatOutOfRange { .. })
        ));

        // Cross-floor shots are rejected even at zero tile distance.
        world
            .teleport_player(
                2,
                Position {
                    x: 100,
                    y: 100,
                    z: 6,
                },
            )
            .unwrap();
        let event = PlayerCombatEvent::distance_shot(
            1,
            2,
            CombatDamageType::Physical,
            5,
            CombatAttackTiming::new(2).unwrap(),
            5,
        )
        .unwrap();
        assert!(matches!(
            world.apply_player_combat_event(event),
            Err(CoreError::CombatOutOfRange { .. })
        ));
    }

    #[test]
    fn rune_stack_charge_consumption_drops_the_final_unit() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let mut backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 4)
                .unwrap();
        backpack
            .items
            .insert(ItemInstance::new(3198, 3).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        world.replace_player_containers(7, containers).unwrap();

        assert!(world.consume_player_container_item_unit(7, 0, 0).unwrap());
        assert_eq!(
            world
                .player_containers(7)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0)
                .map(|item| item.count),
            Some(2)
        );
        assert!(world.consume_player_container_item_unit(7, 0, 0).unwrap());
        assert!(world.consume_player_container_item_unit(7, 0, 0).unwrap());
        // The final consumed unit removes the entry entirely (legacy rune-stack semantics).
        assert!(world
            .player_containers(7)
            .unwrap()
            .container(0)
            .unwrap()
            .items
            .item(0)
            .is_none());

        // Unknown players, containers, and indexes stay typed errors.
        assert!(matches!(
            world.consume_player_container_item_unit(99, 0, 0),
            Err(CoreError::UnknownPlayer(99))
        ));
        assert!(matches!(
            world.consume_player_container_item_unit(7, 5, 0),
            Err(CoreError::UnknownPlayerContainer {
                container_id: 5,
                ..
            })
        ));
        assert!(!world
            .consume_player_container_item_unit(7, 0, 9)
            .unwrap_or(true));
    }

    #[test]
    fn static_creature_direct_melee_damage_sequence_survives_snapshot_restore() {
        let creature_id = 0x4000_0005;
        let creature_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: creature_position,
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
        let mut target = player();
        target.position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let map = WorldMap::new("direct-melee-sequence-restart", creature_position);
        let static_spawns = FeTfsStaticSpawnCollection::with_combat_metadata(
            vec![creature],
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::from([(
                creature_id,
                StaticCreatureDirectMeleeDamageRange {
                    min_damage: 2,
                    max_damage: 4,
                },
            )]),
        )
        .unwrap();
        let mut world = WorldState::default();
        world.add_player(target.clone()).unwrap();
        world
            .update_player_vitals(
                target.id,
                PlayerVitals {
                    health: 50,
                    max_health: 50,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        world.install_static_creatures(&static_spawns).unwrap();
        world.select_static_creature_target(creature_id, 1).unwrap();
        assert!(matches!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                requested_damage: 2,
                ..
            })
        ));
        let snapshot = world.static_creature_runtime_snapshot();
        assert_eq!(snapshot[0].direct_melee_damage_sequence, 1);

        let mut fresh = WorldState::default();
        fresh.add_player(target.clone()).unwrap();
        fresh
            .update_player_vitals(
                target.id,
                PlayerVitals {
                    health: 50,
                    max_health: 50,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        fresh.install_static_creatures(&static_spawns).unwrap();
        assert_eq!(
            fresh.restore_static_creature_runtime(&snapshot),
            Ok(StaticCreatureRuntimeRestoreSummary {
                restored: 1,
                ignored_unknown: 0,
            })
        );
        fresh.select_static_creature_target(creature_id, 1).unwrap();
        assert!(matches!(
            fresh.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                requested_damage: 3,
                ..
            })
        ));
        assert_eq!(
            fresh.static_creature_runtime_snapshot()[0].direct_melee_damage_sequence,
            2
        );
    }

    #[test]
    fn static_creature_without_direct_melee_metadata_attacks_on_each_heartbeat() {
        let creature_id = 0x4000_0003;
        let creature_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: creature_position,
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
        let mut target = player();
        target.position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let map = WorldMap::new("unbounded-direct-melee", creature_position);
        let mut world = WorldState::default();
        world.add_player(target).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 20,
                    max_health: 20,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        world.select_static_creature_target(creature_id, 1).unwrap();

        assert!(matches!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 1,
                ..
            })
        ));
        assert!(matches!(
            world.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 1,
                ..
            })
        ));
    }

    #[test]
    fn static_creature_target_step_is_single_deterministic_and_map_validated() {
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: Position {
                x: 100,
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
        let mut target = player();
        target.position = Position {
            x: 103,
            y: 101,
            z: 7,
        };
        let mut map = WorldMap::new("target-step", creature.position);
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
                x: 100,
                y: 101,
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
                    walkable: position
                        != Position {
                            x: 101,
                            y: 100,
                            z: 7,
                        },
                },
            )
            .unwrap();
        }
        let mut world = WorldState::default();
        world.add_player(target).unwrap();
        world
            .install_static_creatures(
                &FeTfsStaticSpawnCollection::new(vec![creature.clone()]).unwrap(),
            )
            .unwrap();

        let revision_before_selection = world.revision();
        assert_eq!(
            world.step_static_creature_toward_target(creature_id, &map),
            Ok(StaticCreatureTargetStepOutcome::NoTarget)
        );
        assert_eq!(world.revision(), revision_before_selection);
        world.select_static_creature_target(creature_id, 4).unwrap();
        assert_eq!(
            world.step_static_creature_toward_target(creature_id, &map),
            Ok(StaticCreatureTargetStepOutcome::Moved {
                target_player_id: 7,
                direction: CardinalDirection::South,
                from: Position {
                    x: 100,
                    y: 100,
                    z: 7
                },
                to: Position {
                    x: 100,
                    y: 101,
                    z: 7
                },
            })
        );
        assert_eq!(
            world.static_creature(creature_id).unwrap().position,
            Position {
                x: 100,
                y: 101,
                z: 7
            }
        );
        assert_eq!(
            world.step_static_creature_toward_target(creature_id, &map),
            Ok(StaticCreatureTargetStepOutcome::Moved {
                target_player_id: 7,
                direction: CardinalDirection::East,
                from: Position {
                    x: 100,
                    y: 101,
                    z: 7
                },
                to: Position {
                    x: 101,
                    y: 101,
                    z: 7
                },
            })
        );

        let mut blocked_map = WorldMap::new("blocked-target-step", creature.position);
        blocked_map
            .set_tile(
                Position {
                    x: 101,
                    y: 101,
                    z: 7,
                },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: false,
                },
            )
            .unwrap();
        blocked_map
            .set_tile(
                Position {
                    x: 101,
                    y: 102,
                    z: 7,
                },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: false,
                },
            )
            .unwrap();
        assert_eq!(
            world.step_static_creature_toward_target(creature_id, &blocked_map),
            Ok(StaticCreatureTargetStepOutcome::Blocked {
                target_player_id: 7
            })
        );
    }

    #[test]
    fn static_creature_target_detour_is_bounded_and_preserves_direct_default() {
        let creature_id = 0x4000_0001;
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: Position {
                x: 100,
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
        let mut target = player();
        target.position = Position {
            x: 102,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("detour-target-step", creature.position);
        for (position, walkable) in [
            (creature.position, true),
            (
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                false,
            ),
            (
                Position {
                    x: 100,
                    y: 99,
                    z: 7,
                },
                true,
            ),
            (
                Position {
                    x: 101,
                    y: 99,
                    z: 7,
                },
                true,
            ),
            (
                Position {
                    x: 102,
                    y: 99,
                    z: 7,
                },
                true,
            ),
            (target.position, true),
        ] {
            map.set_tile(
                position,
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable,
                },
            )
            .unwrap();
        }
        let mut world = WorldState::default();
        world.add_player(target).unwrap();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        world.select_static_creature_target(creature_id, 4).unwrap();

        assert_eq!(
            world.step_static_creature_toward_target(creature_id, &map),
            Ok(StaticCreatureTargetStepOutcome::Blocked {
                target_player_id: 7
            })
        );
        assert_eq!(
            world.step_static_creature_toward_target_with_detour(creature_id, &map, 3),
            Ok(StaticCreatureTargetStepOutcome::Moved {
                target_player_id: 7,
                direction: CardinalDirection::North,
                from: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                to: Position {
                    x: 100,
                    y: 99,
                    z: 7,
                },
            })
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
            name_description: String::new(),
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
            name_description: String::new(),
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
    fn static_creature_runtime_restore_is_validated_idempotent_and_clears_targets() {
        let creature_id = 0x4000_0001;
        let initial_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let restored_position = Position {
            x: 102,
            y: 100,
            z: 7,
        };
        let creature = FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
            position: initial_position,
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
        let source = player();
        let mut world = WorldState::default();
        world.add_player(source.clone()).unwrap();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        world
            .set_player_static_target(source.id, Some(creature_id))
            .unwrap();
        world.select_static_creature_target(creature_id, 8).unwrap();

        let records = [
            StaticCreatureRuntimeSnapshot {
                id: creature_id,
                position: restored_position,
                active: false,
                health_percent: 70,
                reactivation_remaining_seconds: None,
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
            },
            StaticCreatureRuntimeSnapshot {
                id: 0x4000_9999,
                position: initial_position,
                active: true,
                health_percent: 100,
                reactivation_remaining_seconds: None,
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
            },
        ];
        assert_eq!(
            world.restore_static_creature_runtime(&records),
            Ok(StaticCreatureRuntimeRestoreSummary {
                restored: 1,
                ignored_unknown: 1,
            })
        );
        assert_eq!(
            world.static_creature_runtime_snapshot(),
            vec![StaticCreatureRuntimeSnapshot {
                id: creature_id,
                position: restored_position,
                active: false,
                health_percent: 70,
                reactivation_remaining_seconds: None,
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
            }]
        );
        assert_eq!(
            world.static_creature_target(creature_id),
            Err(CoreError::InactiveStaticCreature(creature_id))
        );
        assert_eq!(
            world.player_interaction_intent(source.id),
            Ok(PlayerInteractionIntent::default())
        );
        assert!(!world.is_static_creature_occupied(initial_position));
        assert!(!world.is_static_creature_occupied(restored_position));

        let snapshot_before_invalid = world.static_creature_runtime_snapshot();
        assert_eq!(
            world.restore_static_creature_runtime(&[StaticCreatureRuntimeSnapshot {
                id: creature_id,
                position: source.position,
                active: true,
                health_percent: 100,
                reactivation_remaining_seconds: None,
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
            }]),
            Err(CoreError::PlayerOccupiesStaticCreaturePosition(
                source.position
            ))
        );
        assert_eq!(
            world.static_creature_runtime_snapshot(),
            snapshot_before_invalid
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
            name_description: String::new(),
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
            name_description: String::new(),
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
    fn typed_adjacent_melee_events_enforce_deterministic_cooldowns() {
        assert_eq!(
            CombatAttackTiming::new(0),
            Err(CoreError::InvalidCombatEvent)
        );
        assert_eq!(
            CombatAttackTiming::new(MAX_COMBAT_INTERVAL_TICKS + 1),
            Err(CoreError::InvalidCombatEvent)
        );
        let timing = CombatAttackTiming::new(2).unwrap();
        assert_eq!(
            PlayerCombatEvent::adjacent_melee(7, 8, CombatDamageType::Physical, 0, timing),
            Err(CoreError::InvalidCombatEvent)
        );
        assert_eq!(
            PlayerCombatEvent::adjacent_melee(
                7,
                8,
                CombatDamageType::Physical,
                MAX_COMBAT_EVENT_DAMAGE + 1,
                timing,
            ),
            Err(CoreError::InvalidCombatEvent)
        );

        let mut world = WorldState::default();
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
                    max_health: 30,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        let event = PlayerCombatEvent::adjacent_melee(7, 8, CombatDamageType::Physical, 10, timing)
            .unwrap();
        let revision = world.revision();
        assert_eq!(
            world.apply_player_combat_event(event).unwrap(),
            PlayerCombatEventOutcome {
                damage: PlayerDamageOutcome {
                    attacker_id: 7,
                    target_id: 8,
                    requested_damage: 10,
                    applied_damage: 10,
                    remaining_health: 20,
                    defeated: false,
                },
                damage_type: CombatDamageType::Physical,
                mitigated_damage: 10,
                next_attack_tick: 2,
            }
        );
        assert_eq!(world.player_combat_cooldown(7).unwrap().next_attack_tick, 2);
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(
            world.apply_player_combat_event(event),
            Err(CoreError::CombatCooldownActive {
                attacker_id: 7,
                current_tick: 0,
                next_attack_tick: 2,
            })
        );
        world.advance_ticks(2);
        assert_eq!(
            world
                .apply_player_combat_event(event)
                .unwrap()
                .damage
                .remaining_health,
            10
        );
        assert_eq!(
            world.apply_player_combat_event(
                PlayerCombatEvent::adjacent_melee(7, 8, CombatDamageType::Fire, 10, timing,)
                    .unwrap()
            ),
            Err(CoreError::InvalidCombatEvent)
        );
        world.remove_player(7).unwrap();
        assert_eq!(
            world.player_combat_cooldown(7),
            Err(CoreError::UnknownPlayer(7))
        );
    }

    #[test]
    fn profile_neutral_physical_defense_mitigates_typed_events_deterministically() {
        assert_eq!(
            PlayerCombatDefense::new(MAX_COMBAT_EVENT_DAMAGE + 1),
            Err(CoreError::InvalidCombatDefense)
        );
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let mut target = player();
        target.id = 8;
        target.name = "Druid".into();
        target.position.x = 101;
        world
            .add_player_with_vitals(
                target,
                PlayerVitals {
                    health: 25,
                    max_health: 25,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        let defense = PlayerCombatDefense::new(3).unwrap();
        assert!(world.replace_player_combat_defense(8, defense).unwrap());
        assert!(!world.replace_player_combat_defense(8, defense).unwrap());
        let event = PlayerCombatEvent::adjacent_melee(
            7,
            8,
            CombatDamageType::Physical,
            10,
            CombatAttackTiming::new(1).unwrap(),
        )
        .unwrap();
        let outcome = world.apply_player_combat_event(event).unwrap();
        assert_eq!(outcome.mitigated_damage, 7);
        assert_eq!(outcome.damage.applied_damage, 7);
        assert_eq!(outcome.damage.remaining_health, 18);

        world.advance_tick();
        world
            .replace_player_combat_defense(8, PlayerCombatDefense::new(10).unwrap())
            .unwrap();
        let absorbed = world.apply_player_combat_event(event).unwrap();
        assert_eq!(absorbed.mitigated_damage, 0);
        assert_eq!(absorbed.damage.applied_damage, 0);
        assert_eq!(absorbed.damage.remaining_health, 18);
        assert_eq!(world.player_combat_cooldown(7).unwrap().next_attack_tick, 2);

        world.remove_player(8).unwrap();
        assert_eq!(
            world.player_combat_defense(8),
            Err(CoreError::UnknownPlayer(8))
        );
    }

    #[test]
    fn fight_mode_state_is_authoritative_idempotent_and_cleared_with_player() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        assert_eq!(
            world.player_fight_mode_state(7).unwrap(),
            PlayerFightModeState {
                mode: PlayerFightMode::Attack,
                chase: false,
                secure: false,
            }
        );
        let revision = world.revision();
        let state = PlayerFightModeState {
            mode: PlayerFightMode::Balanced,
            chase: true,
            secure: true,
        };
        assert!(world.replace_player_fight_mode_state(7, state).unwrap());
        assert_eq!(world.revision(), revision + 1);
        assert!(!world.replace_player_fight_mode_state(7, state).unwrap());
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(world.player_fight_mode_state(7).unwrap(), state);
        world.remove_player(7).unwrap();
        assert_eq!(
            world.player_fight_mode_state(7),
            Err(CoreError::UnknownPlayer(7))
        );
    }

    #[test]
    fn typed_spell_casts_enforce_authoritative_mana_and_cooldown_state() {
        let timing = CombatAttackTiming::new(2).unwrap();
        assert_eq!(
            PlayerSpellCastEvent::new(7, 0, 10, timing),
            Err(CoreError::InvalidSpellCastEvent)
        );
        assert_eq!(
            PlayerSpellCastEvent::new(7, 1, 0, timing),
            Err(CoreError::InvalidSpellCastEvent)
        );
        assert_eq!(
            PlayerSpellCastEvent::new(7, 1, MAX_SPELL_MANA_COST + 1, timing),
            Err(CoreError::InvalidSpellCastEvent)
        );

        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    mana: 40,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        let event = PlayerSpellCastEvent::new(7, 100, 30, timing).unwrap();
        let outcome = world.apply_player_spell_cast_event(event).unwrap();
        assert_eq!(
            outcome,
            PlayerSpellCastOutcome {
                caster_id: 7,
                spell_id: 100,
                mana_spent: 30,
                remaining_mana: 10,
                next_cast_tick: 2,
            }
        );
        assert_eq!(world.player_vitals(7).unwrap().mana, 10);
        assert_eq!(world.player_spell_cooldown(7).unwrap().next_cast_tick, 2);
        assert_eq!(
            world.apply_player_spell_cast_event(event),
            Err(CoreError::SpellCooldownActive {
                caster_id: 7,
                current_tick: 0,
                next_cast_tick: 2,
            })
        );

        world.advance_ticks(2);
        assert_eq!(
            world.apply_player_spell_cast_event(event),
            Err(CoreError::InsufficientMana {
                player_id: 7,
                required_mana: 30,
                available_mana: 10,
            })
        );
        assert_eq!(world.player_spell_cooldown(7).unwrap().next_cast_tick, 2);

        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    mana: 35,
                    ..world.player_vitals(7).unwrap()
                },
            )
            .unwrap();
        assert_eq!(
            world
                .apply_player_spell_cast_event(event)
                .unwrap()
                .remaining_mana,
            5
        );
        assert_eq!(world.player_spell_cooldown(7).unwrap().next_cast_tick, 4);
        world.remove_player(7).unwrap();
        assert_eq!(
            world.player_spell_cooldown(7),
            Err(CoreError::UnknownPlayer(7))
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
    fn progression_attempts_are_multiplier_driven_and_authoritative() {
        assert_eq!(
            ProgressionMultiplier::new(0),
            Err(CoreError::InvalidProgressionMultiplier(0))
        );
        assert_eq!(
            ProgressionMultiplier::new(MAX_PROGRESSION_MULTIPLIER_MILLI + 1),
            Err(CoreError::InvalidProgressionMultiplier(
                MAX_PROGRESSION_MULTIPLIER_MILLI + 1
            ))
        );
        let multiplier = ProgressionMultiplier::new(1_100).unwrap();
        let rules = PlayerProgressionRules {
            magic_level_multiplier: multiplier,
            skill_multipliers: [multiplier; 7],
        };
        assert_eq!(rules.required_skill_tries(PlayerSkill::Sword, 11), 50);
        assert_eq!(rules.required_skill_tries(PlayerSkill::Sword, 12), 55);
        assert_eq!(rules.required_magic_mana(0), 0);
        assert_eq!(rules.required_magic_mana(1), 1_600);
        assert_eq!(rules.required_magic_mana(2), 1_760);

        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let revision = world.revision();
        let partial = world
            .apply_player_skill_tries(7, PlayerSkill::Sword, 49, rules)
            .unwrap();
        assert_eq!(partial.gained_levels, 0);
        assert_eq!(partial.progress, SkillProgress::new(10, 98).unwrap());
        assert_eq!(partial.stored_tries, 49);
        assert_eq!(world.revision(), revision + 1);

        let advanced = world
            .apply_player_skill_tries(7, PlayerSkill::Sword, 1, rules)
            .unwrap();
        assert_eq!(advanced.gained_levels, 1);
        assert_eq!(advanced.progress, SkillProgress::new(11, 0).unwrap());
        assert_eq!(advanced.stored_tries, 0);
        assert_eq!(
            world
                .player_progression_attempts(7)
                .unwrap()
                .skill_tries(PlayerSkill::Sword),
            0
        );

        let magic = world.apply_player_magic_mana(7, 1_600, rules).unwrap();
        assert_eq!(magic.gained_levels, 1);
        assert_eq!(magic.magic_level, 1);
        assert_eq!(magic.stored_mana, 0);
        assert_eq!(world.player_vitals(7).unwrap().magic_level, 1);
        let no_op_revision = world.revision();
        assert_eq!(
            world
                .apply_player_magic_mana(7, 0, rules)
                .unwrap()
                .gained_levels,
            0
        );
        assert_eq!(world.revision(), no_op_revision);

        world.remove_player(7).unwrap();
        assert_eq!(
            world.player_progression_attempts(7),
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
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 0,
                    ..capped.vitals
                },
            )
            .unwrap();
        world
            .hydrate_player_respawn_state(
                7,
                PlayerRespawnState {
                    dead: true,
                    respawn_at: Some(Position {
                        x: 110,
                        y: 120,
                        z: 7,
                    }),
                    death_time: Some(0),
                    loss_applied: false,
                },
            )
            .unwrap();
        let dead_revision = world.revision();
        let dead = world
            .apply_player_regeneration(7, rules, MAX_REGENERATION_ELAPSED_SECONDS)
            .unwrap();
        assert_eq!(dead.health_gained, 0);
        assert_eq!(dead.mana_gained, 0);
        assert_eq!(dead.vitals.health, 0);
        assert_eq!(world.revision(), dead_revision);
        world.remove_player(7).unwrap();
        assert_eq!(
            world.apply_player_regeneration(7, rules, 1),
            Err(CoreError::UnknownPlayer(7))
        );
    }

    #[test]
    fn persisted_respawn_state_hydration_is_strict_and_idempotent() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        let state = PlayerRespawnState {
            dead: true,
            respawn_at: Some(Position {
                x: 110,
                y: 120,
                z: 7,
            }),
            death_time: Some(12),
            loss_applied: true,
        };
        let revision = world.revision();
        assert!(world.hydrate_player_respawn_state(7, state).unwrap());
        assert_eq!(world.player_respawn_state(7).unwrap(), state);
        assert_eq!(world.revision(), revision + 1);
        assert!(!world.hydrate_player_respawn_state(7, state).unwrap());
        assert_eq!(world.revision(), revision + 1);

        assert_eq!(
            world.hydrate_player_respawn_state(
                7,
                PlayerRespawnState {
                    dead: true,
                    respawn_at: None,
                    death_time: Some(12),
                    loss_applied: false,
                },
            ),
            Err(CoreError::InvalidPlayerRespawnState(7))
        );
        assert_eq!(
            world.hydrate_player_respawn_state(
                7,
                PlayerRespawnState {
                    dead: false,
                    respawn_at: Some(Position {
                        x: 110,
                        y: 120,
                        z: 7,
                    }),
                    death_time: None,
                    loss_applied: false,
                },
            ),
            Err(CoreError::InvalidPlayerRespawnState(7))
        );
        assert_eq!(world.player_respawn_state(7).unwrap(), state);
    }

    #[test]
    fn authoritative_death_state_resolves_a_temple_without_respawning() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        assert_eq!(
            world.player_respawn_state(7).unwrap(),
            PlayerRespawnState::default()
        );
        world
            .apply_player_condition(
                7,
                PlayerCondition::new(PlayerConditionKind::Poison, 2, 7, 5).unwrap(),
            )
            .unwrap();

        let temple = Position {
            x: 110,
            y: 120,
            z: 7,
        };
        let mut map = WorldMap::new("death-state", temple);
        map.set_town(WorldMapTown {
            id: 42,
            name: "Thais".to_owned(),
            temple_position: temple,
        })
        .unwrap();

        assert_eq!(
            world.apply_player_death(7, 999, &map),
            Err(CoreError::UnknownTown(999))
        );
        assert_eq!(
            world.player_respawn_state(7).unwrap(),
            PlayerRespawnState::default()
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 150);

        world.advance_tick();
        world.advance_tick();
        let position_before_death = world.player(7).unwrap().position;
        let revision = world.revision();
        let state = world.apply_player_death(7, 42, &map).unwrap();
        assert_eq!(
            state,
            PlayerRespawnState {
                dead: true,
                respawn_at: Some(temple),
                death_time: Some(2),
                loss_applied: false,
            }
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 0);
        assert!(world.player_conditions(7).unwrap().is_empty());
        assert_eq!(world.player(7).unwrap().position, position_before_death);
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(world.apply_player_death(7, 42, &map).unwrap(), state);
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(
            world.move_player(
                7,
                Position {
                    x: position_before_death.x + 1,
                    y: position_before_death.y,
                    z: position_before_death.z,
                },
            ),
            Err(CoreError::PlayerIsDead(7))
        );
        assert_eq!(world.player(7).unwrap().position, position_before_death);
        assert_eq!(world.revision(), revision + 1);

        assert_eq!(
            world.respawn_player(7).unwrap(),
            PlayerRespawnOutcome {
                player_id: 7,
                position: temple,
                vitals: PlayerVitals::default(),
            }
        );
        assert_eq!(world.player(7).unwrap().position, temple);
        assert_eq!(world.player_vitals(7).unwrap(), PlayerVitals::default());
        assert_eq!(
            world.player_respawn_state(7).unwrap(),
            PlayerRespawnState::default()
        );
        assert_eq!(world.revision(), revision + 2);
        assert_eq!(world.respawn_player(7), Err(CoreError::PlayerIsNotDead(7)));
        assert_eq!(world.revision(), revision + 2);

        world
            .move_player(
                7,
                Position {
                    x: temple.x + 1,
                    y: temple.y,
                    z: temple.z,
                },
            )
            .unwrap();
        world.apply_player_death(7, 42, &map).unwrap();
        world
            .add_player(Player {
                id: 8,
                account_id: 3,
                name: "Druid".to_owned(),
                position: temple,
                level: 8,
                experience: 0,
                skill_points: 0,
            })
            .unwrap();
        let blocked_revision = world.revision();
        assert_eq!(
            world.apply_player_damage(7, 8, 1),
            Err(CoreError::PlayerIsDead(7))
        );
        assert!(world.player_respawn_state(7).unwrap().dead);
        assert_eq!(world.revision(), blocked_revision);
        assert_eq!(
            world.respawn_player(7),
            Err(CoreError::PlayerOccupiesPosition(temple))
        );
        assert!(world.player_respawn_state(7).unwrap().dead);
        assert_eq!(world.revision(), blocked_revision);

        world.remove_player(7).unwrap();
        assert_eq!(
            world.player_respawn_state(7),
            Err(CoreError::UnknownPlayer(7))
        );
    }

    #[test]
    fn lethal_condition_damage_enters_validated_authoritative_death_state() {
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    health: 7,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        world
            .apply_player_condition(
                7,
                PlayerCondition::new(PlayerConditionKind::Poison, 1, 7, 1).unwrap(),
            )
            .unwrap();
        let temple = Position {
            x: 110,
            y: 120,
            z: 7,
        };
        let mut map = WorldMap::new("condition-death", temple);
        map.set_town(WorldMapTown {
            id: 42,
            name: "Thais".to_owned(),
            temple_position: temple,
        })
        .unwrap();

        assert_eq!(
            world.apply_player_conditions_with_death(7, 0, &map, 1),
            Err(CoreError::PlayerTownUnassigned(7))
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 7);
        assert!(world
            .player_conditions(7)
            .unwrap()
            .contains_key(&PlayerConditionKind::Poison));

        world.advance_tick();
        let (outcome, death_state) = world
            .apply_player_conditions_with_death(7, 42, &map, 1)
            .unwrap();
        assert_eq!(outcome.applied_damage, 7);
        assert_eq!(outcome.remaining_health, 0);
        assert_eq!(
            death_state,
            Some(PlayerRespawnState {
                dead: true,
                respawn_at: Some(temple),
                death_time: Some(1),
                loss_applied: false,
            })
        );
        assert_eq!(world.player_vitals(7).unwrap().health, 0);
        assert!(world.player_conditions(7).unwrap().is_empty());
        let dead_revision = world.revision();
        assert_eq!(
            world
                .apply_player_conditions_with_death(7, 42, &map, 1)
                .unwrap(),
            (
                PlayerConditionOutcome {
                    player_id: 7,
                    applied_damage: 0,
                    remaining_health: 0,
                    expired_conditions: 0,
                },
                None,
            )
        );
        assert_eq!(world.revision(), dead_revision);
    }

    #[test]
    fn fixed_percent_death_loss_uses_exact_cumulative_progress_once() {
        let multiplier = ProgressionMultiplier::new(1_000).unwrap();
        let rules = PlayerProgressionRules {
            magic_level_multiplier: multiplier,
            skill_multipliers: [multiplier; 7],
        };
        let temple = Position {
            x: 110,
            y: 120,
            z: 7,
        };
        let mut map = WorldMap::new("fixed-loss", temple);
        map.set_town(WorldMapTown {
            id: 42,
            name: "Thais".to_owned(),
            temple_position: temple,
        })
        .unwrap();
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        {
            let player = world.players.get_mut(&7).unwrap();
            player.experience = 100_000;
            player.level = level_for_experience(player.experience);
        }
        let mut skills = PlayerSkills::default();
        skills.set(PlayerSkill::Sword, SkillProgress::new(11, 50).unwrap());
        world
            .replace_player_progression(
                7,
                PlayerProgression {
                    vocation: BaseVocation::Knight.id(),
                    skills,
                },
            )
            .unwrap();
        world
            .replace_player_progression_attempts(
                7,
                PlayerProgressionAttempts::new([0, 0, 25, 0, 0, 0, 0], 800),
            )
            .unwrap();
        world
            .update_player_vitals(
                7,
                PlayerVitals {
                    magic_level: 1,
                    ..PlayerVitals::default()
                },
            )
            .unwrap();
        assert_eq!(
            world.apply_fixed_percent_death_loss(7, 0, rules),
            Err(CoreError::InvalidFixedDeathLossPercent(0))
        );
        assert_eq!(
            world.apply_fixed_percent_death_loss(7, 25, rules),
            Err(CoreError::PlayerIsNotDead(7))
        );
        world.apply_player_death(7, 42, &map).unwrap();
        let revision = world.revision();
        let outcome = world.apply_fixed_percent_death_loss(7, 25, rules).unwrap();
        assert_eq!(outcome.experience_lost, 25_000);
        assert_eq!(
            outcome.skill_tries_lost[PlayerSkill::Sword.code() as usize],
            18
        );
        assert_eq!(outcome.magic_mana_lost, 600);
        assert_eq!(world.player(7).unwrap().experience, 75_000);
        assert_eq!(world.player(7).unwrap().level, level_for_experience(75_000));
        assert_eq!(
            world
                .player_progression(7)
                .unwrap()
                .skills
                .skill(PlayerSkill::Sword),
            SkillProgress::new(11, 14).unwrap()
        );
        assert_eq!(
            world
                .player_progression_attempts(7)
                .unwrap()
                .skill_tries(PlayerSkill::Sword),
            7
        );
        assert_eq!(world.player_vitals(7).unwrap().magic_level, 1);
        assert_eq!(
            world.player_progression_attempts(7).unwrap().magic_mana(),
            200
        );
        assert!(world.player_respawn_state(7).unwrap().loss_applied);
        assert_eq!(world.revision(), revision + 1);
        assert_eq!(
            world.apply_fixed_percent_death_loss(7, 25, rules),
            Err(CoreError::DeathLossAlreadyApplied(7))
        );
        assert_eq!(world.revision(), revision + 1);
        world.respawn_player(7).unwrap();
        assert_eq!(
            world.player_respawn_state(7).unwrap(),
            PlayerRespawnState::default()
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
    fn persisted_condition_elapsed_progress_resumes_exact_tick_timing() {
        assert_eq!(
            PlayerCondition::from_persisted(PlayerConditionKind::Poison, 2, 7, 0, 5, 2),
            Err(CoreError::InvalidPlayerCondition)
        );
        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        world
            .apply_player_condition(
                7,
                PlayerCondition::from_persisted(PlayerConditionKind::Poison, 3, 7, 0, 5, 2)
                    .unwrap(),
            )
            .unwrap();
        let outcome = world.apply_player_conditions(7, 1).unwrap();
        assert_eq!(outcome.applied_damage, 7);
        assert_eq!(
            world
                .player_conditions(7)
                .unwrap()
                .get(&PlayerConditionKind::Poison)
                .unwrap()
                .elapsed_seconds(),
            0
        );
    }

    #[test]
    fn haste_condition_bounds_apply_and_expire_without_damage() {
        // Bounds: zero or oversized bonus rejected, oversized duration rejected.
        assert_eq!(
            PlayerCondition::new_haste(0, 25),
            Err(CoreError::InvalidPlayerCondition)
        );
        assert_eq!(
            PlayerCondition::new_haste(MAX_SPEED_BONUS_PERCENT + 1, 25),
            Err(CoreError::InvalidPlayerCondition)
        );
        assert_eq!(
            PlayerCondition::new_haste(50, MAX_CONDITION_DURATION_SECONDS + 1),
            Err(CoreError::InvalidPlayerCondition)
        );
        assert_eq!(
            PlayerCondition::new(PlayerConditionKind::Haste, 1, 0, 25),
            Err(CoreError::InvalidPlayerCondition)
        );

        let mut world = WorldState::default();
        world.add_player(player()).unwrap();
        world.apply_player_speed_condition(7, 40, 2).unwrap();
        assert_eq!(world.player_speed_bonus_percent(7), 40);

        // One elapsed second leaves the condition active with no damage applied.
        let outcome = world.apply_player_conditions(7, 1).unwrap();
        assert_eq!(outcome.applied_damage, 0);
        assert_eq!(outcome.expired_conditions, 0);
        assert_eq!(world.player_speed_bonus_percent(7), 40);

        // Second elapsed second expires the condition and clears the modifier.
        let outcome = world.apply_player_conditions(7, 1).unwrap();
        assert_eq!(outcome.applied_damage, 0);
        assert_eq!(outcome.expired_conditions, 1);
        assert_eq!(world.player_speed_bonus_percent(7), 0);

        // Persistence round-trip preserves the bonus through the elapsed-remainder path.
        let condition = PlayerCondition::new_haste(30, 10).unwrap();
        world.apply_player_condition(7, condition).unwrap();
        let restored = PlayerCondition::from_persisted(
            PlayerConditionKind::Haste,
            condition.interval_seconds,
            condition.damage,
            condition.speed_bonus_percent,
            condition.remaining_seconds,
            0,
        )
        .unwrap();
        assert_eq!(restored, condition);
        // A persisted haste row with damage must be rejected (per-kind payload discipline).
        assert_eq!(
            PlayerCondition::from_persisted(PlayerConditionKind::Haste, 1, 5, 30, 10, 0),
            Err(CoreError::InvalidPlayerCondition)
        );
        // A persisted DoT row with a speed payload must be rejected.
        assert_eq!(
            PlayerCondition::from_persisted(PlayerConditionKind::Poison, 2, 7, 30, 10, 0),
            Err(CoreError::InvalidPlayerCondition)
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

/// Protection-zone flag semantics: OTBM bit 0x01 detection and the two-player PvP gate.
#[cfg(test)]
mod protection_zone_tests {
    use super::*;

    fn world_with_two_players() -> WorldState {
        let mut world = WorldState::default();
        for (id, x, name) in [(1_u64, 100_u16, "Alice"), (2, 104, "Bob")] {
            world
                .add_player_with_vitals(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position: Position { x, y: 100, z: 7 },
                        level: 8,
                        experience: 4200,
                        skill_points: 0,
                    },
                    PlayerVitals::default(),
                )
                .unwrap();
        }
        world
    }

    #[test]
    fn protection_zone_flag_blocks_pvp_when_either_side_stands_in_it() {
        let mut world = world_with_two_players();
        let mut map = WorldMap::new(
            "pz-test",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        // Alice's tile is a protection zone; Bob's tile is not.
        map.set_tile_flags(
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
            WorldMap::OTBM_TILE_FLAG_PROTECTION_ZONE,
        );
        assert!(map.is_protection_zone(Position {
            x: 100,
            y: 100,
            z: 7
        }));
        assert!(!map.is_protection_zone(Position {
            x: 104,
            y: 100,
            z: 7
        }));

        // Attacker inside PZ -> blocked.
        assert!(world.either_player_in_protection_zone(&map, 1, 2));

        // Both outside PZ -> allowed.
        {
            let alice = world.players.get_mut(&1).unwrap();
            alice.position = Position {
                x: 102,
                y: 100,
                z: 7,
            };
        }
        assert!(!world.either_player_in_protection_zone(&map, 1, 2));

        // Defender inside PZ -> also blocked.
        {
            let bob = world.players.get_mut(&2).unwrap();
            bob.position = Position {
                x: 100,
                y: 100,
                z: 7,
            };
        }
        assert!(world.either_player_in_protection_zone(&map, 1, 2));
    }
}

/// Player-to-player trade state-machine coverage: open gating, staging bounds, atomic swap
/// success, and the anti-dupe abort when a staged reference no longer resolves.
#[cfg(test)]
mod player_trade_tests {
    use super::*;

    fn two_players() -> WorldState {
        let mut world = WorldState::default();
        world
            .add_player_with_vitals(
                Player {
                    id: 1,
                    account_id: 1,
                    name: "Alice".into(),
                    position: Position {
                        x: 100,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 4200,
                    skill_points: 0,
                },
                PlayerVitals::default(),
            )
            .unwrap();
        world
            .add_player_with_vitals(
                Player {
                    id: 2,
                    account_id: 2,
                    name: "Bob".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 4200,
                    skill_points: 0,
                },
                PlayerVitals::default(),
            )
            .unwrap();
        world
    }

    fn give_container(world: &mut WorldState, owner: u64, container_id: u8) {
        let containers = world.player_containers.entry(owner).or_default();
        let backpack = PlayerContainer::new(
            container_id,
            ItemInstance::new(2854, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        containers.insert(backpack).unwrap();
    }

    fn add_item(world: &mut WorldState, owner: u64, container_id: u8, item_id: u16) -> usize {
        let containers = world.player_containers.get_mut(&owner).unwrap();
        let container = containers.container_mut(container_id).unwrap();
        container
            .items
            .merge_or_insert_stack(ItemInstance::new(item_id, 1).unwrap())
            .unwrap()
            .0
    }

    #[test]
    fn trade_open_gates_self_unknown_and_busy_participants() {
        let mut world = two_players();
        assert!(matches!(
            world.open_player_trade(1, 1),
            Err(CoreError::TradeWithSelf)
        ));
        assert!(matches!(
            world.open_player_trade(1, 99),
            Err(CoreError::UnknownPlayer(99))
        ));
        world.open_player_trade(1, 2).unwrap();
    }

    #[test]
    fn staging_resets_acceptance_and_enforces_bounds() {
        let mut world = two_players();
        give_container(&mut world, 1, 0);
        give_container(&mut world, 2, 0);
        world.open_player_trade(1, 2).unwrap();

        let index = add_item(&mut world, 1, 0, 3031);
        world
            .stage_trade_item(
                1,
                TradeItemReference {
                    container_id: 0,
                    item_index: index,
                },
            )
            .unwrap();
        // A single acceptance is not enough; restaging resets both flags.
        assert!(!world.accept_player_trade(1).unwrap());
        assert!(world
            .stage_trade_item(
                1,
                TradeItemReference {
                    container_id: 0,
                    item_index: index
                }
            )
            .is_err()); // duplicate staging is rejected without changing the offer
        let session = world.player_trade(1).unwrap().clone();
        assert_eq!(session.initiator_items.len(), 1);
        // The duplicate rejection left the earlier acceptance intact.
        assert!(session.initiator_accepted);
        assert!(!session.counterparty_accepted);
    }

    #[test]
    fn accepted_trade_swaps_items_atomically() {
        let mut world = two_players();
        give_container(&mut world, 1, 0);
        give_container(&mut world, 2, 0);
        let a_index = add_item(&mut world, 1, 0, 2160); // Alice offers item A
        let b_index = add_item(&mut world, 2, 0, 2392); // Bob offers item B
        world.open_player_trade(1, 2).unwrap();
        world
            .stage_trade_item(
                1,
                TradeItemReference {
                    container_id: 0,
                    item_index: a_index,
                },
            )
            .unwrap();
        world
            .stage_trade_item(
                2,
                TradeItemReference {
                    container_id: 0,
                    item_index: b_index,
                },
            )
            .unwrap();
        assert!(!world.accept_player_trade(1).unwrap());
        assert!(world.accept_player_trade(2).unwrap());

        let execution = world.execute_player_trade(2).unwrap();
        assert_eq!(execution.initiator, 1);
        assert_eq!(execution.counterparty, 2);
        assert_eq!(execution.initiator_gave.len(), 1);
        assert_eq!(execution.counterparty_gave.len(), 1);

        // The offered items changed owners.
        let alice_items = world
            .player_containers
            .get(&1)
            .unwrap()
            .container(0)
            .unwrap();
        assert!(alice_items.items.iter().any(|item| item.server_id == 2392));
        let bob_items = world
            .player_containers
            .get(&2)
            .unwrap()
            .container(0)
            .unwrap();
        assert!(bob_items.items.iter().any(|item| item.server_id == 2160));
        // Trade closed for both sides.
        assert!(world.player_trade(1).is_none());
        assert!(world.player_trade(2).is_none());
    }

    #[test]
    fn missing_staged_item_aborts_swap_without_touching_inventories() {
        let mut world = two_players();
        give_container(&mut world, 1, 0);
        give_container(&mut world, 2, 0);
        let a_index = add_item(&mut world, 1, 0, 2160);
        let b_index = add_item(&mut world, 2, 0, 2392);
        world.open_player_trade(1, 2).unwrap();
        world
            .stage_trade_item(
                1,
                TradeItemReference {
                    container_id: 0,
                    item_index: a_index,
                },
            )
            .unwrap();
        world
            .stage_trade_item(
                2,
                TradeItemReference {
                    container_id: 0,
                    item_index: b_index,
                },
            )
            .unwrap();
        // Bob removes his offered item after staging (e.g. dropped it).
        {
            let containers = world.player_containers.get_mut(&2).unwrap();
            containers.container_mut(0).unwrap().items.remove(b_index);
        }
        world.accept_player_trade(1).unwrap();
        world.accept_player_trade(2).unwrap();
        let error = world.execute_player_trade(1).unwrap_err();
        assert!(matches!(
            error,
            CoreError::TradeItemMissing { player_id: 2, .. }
        ));
        // Anti-dupe: nothing moved on Alice's side either.
        let alice_items = world
            .player_containers
            .get(&1)
            .unwrap()
            .container(0)
            .unwrap();
        assert!(alice_items.items.iter().any(|item| item.server_id == 2160));
        // A failed swap leaves the session open so players can renegotiate or cancel.
        assert!(world.player_trade(1).is_some());
    }
}

/// Frag tracking coverage: kills accumulate per killer, white-skull classification flips at
/// one unjustified kill, and non-killers stay clean.
#[cfg(test)]
mod frag_tests {
    use super::*;

    #[test]
    fn frags_accumulate_per_killer_and_drive_white_skull() {
        let mut world = WorldState::default();
        world
            .add_player(Player {
                id: 1,
                account_id: 1,
                name: "Killer".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4200,
                skill_points: 0,
            })
            .unwrap();

        assert_eq!(world.player_frag_count(1), 0);
        assert!(!world.player_has_white_skull(1));

        assert_eq!(world.record_player_frag(1), 1);
        assert!(world.player_has_white_skull(1));
        assert_eq!(world.record_player_frag(1), 2);
        assert_eq!(world.player_frag_count(1), 2);

        // A bystander has no frags.
        assert_eq!(world.player_frag_count(2), 0);
    }
}
