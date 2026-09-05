//! SharedNativeMap: the runtime mutable map owner. Wraps an immutable source map with
//! a mutable runtime overlay, a durable removal journal, source-item index tracking,
//! and a revision counter for delta-baseline synchronization.

use super::*;
impl SharedNativeMap {
    /// Startup-only constructor: with no journal supplied, recovery validates against nothing
    /// and cannot fail, so the expect documents a construction invariant rather than a runtime
    /// risk. The network-facing path goes through `recover_from_removal_journal`, which returns
    /// `Result` and fails closed.
    pub fn new(map: WorldMap) -> Self {
        Self::recover_from_removal_journal(map, None)
            .expect("an empty map-item removal journal cannot invalidate a world map")
    }

    /// Restores a mutable runtime map from an immutable loaded source map and an optional durable
    /// source-item removal journal. The core recovery primitive validates every identity against
    /// the source revision and complete ordered source content before removing anything. A stale,
    /// malformed, duplicate, or missing source identity fails closed without constructing an owner.
    pub fn recover_from_removal_journal(
        source_map: WorldMap,
        journal: Option<&MapItemRemovalJournal>,
    ) -> Result<Self, HostError> {
        Self::recover_from_map_item_state(source_map, journal, None)
    }

    /// Restores a mutable runtime map from immutable source content, optional complete removals,
    /// and optional revision-bound remaining-count overrides. Overrides must reduce one existing
    /// source stack and may never overlap a complete removal. The entire state validates before
    /// either mutation is applied.
    pub fn recover_from_map_item_state(
        source_map: WorldMap,
        journal: Option<&MapItemRemovalJournal>,
        count_overrides: Option<&(WorldMapSourceRevision, Vec<MapItemCountOverrideRecord>)>,
    ) -> Result<Self, HostError> {
        Self::recover_complete_map_item_state(source_map, journal, count_overrides, None)
    }

    /// Restores a mutable runtime map from every durable boundary: immutable source content,
    /// optional complete removals, optional remaining-count overrides, and the optional durable
    /// runtime tile-item registry (spawned corpses). Every record must match the loaded source
    /// revision; any inconsistency fails closed without constructing an owner.
    pub fn recover_complete_map_item_state(
        source_map: WorldMap,
        journal: Option<&MapItemRemovalJournal>,
        count_overrides: Option<&(WorldMapSourceRevision, Vec<MapItemCountOverrideRecord>)>,
        runtime_items: Option<&(WorldMapSourceRevision, Vec<RuntimeMapItemRecord>)>,
    ) -> Result<Self, HostError> {
        let source = Arc::new(source_map);
        let mut map = (*source).clone();
        let removed_source_items = journal
            .map(|journal| {
                journal
                    .removed_items
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let source_item_count_overrides = count_overrides
            .map(|(map_revision, overrides)| {
                if *map_revision != source.source_revision() {
                    return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                        "map-item count override source revision does not match the loaded map"
                            .into(),
                    )));
                }
                let mut entries = BTreeMap::new();
                for override_record in overrides {
                    let identity = override_record.source_identity;
                    if identity.map_revision != *map_revision
                        || removed_source_items.contains(&identity)
                    {
                        return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                            "map-item count override conflicts with the loaded map journal".into(),
                        )));
                    }
                    let source_item = source
                        .tile_items(identity.position)
                        .and_then(|items| items.get(usize::from(identity.item_index)))
                        .ok_or_else(|| {
                            HostError::Core(forgotten_core::CoreError::InvalidMap(
                                "map-item count override references a missing ordered source item"
                                    .into(),
                            ))
                        })?;
                    if override_record.remaining_count >= u16::from(source_item.count)
                        || entries
                            .insert(identity, override_record.remaining_count)
                            .is_some()
                    {
                        return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                            "map-item count override must uniquely reduce its source stack".into(),
                        )));
                    }
                }
                Ok(entries)
            })
            .transpose()?
            .unwrap_or_default();
        for (identity, remaining_count) in &source_item_count_overrides {
            let items = map
                .tile_items(identity.position)
                .ok_or_else(|| {
                    HostError::Core(forgotten_core::CoreError::InvalidMap(
                        "validated count override tile list disappeared".into(),
                    ))
                })?
                .to_vec();
            let mut next_items = items;
            next_items[usize::from(identity.item_index)].count = u8::try_from(*remaining_count)
                .map_err(|_| {
                    HostError::Core(forgotten_core::CoreError::InvalidMap(
                        "persisted count override exceeds the map u8 stack bound".into(),
                    ))
                })?;
            map.set_tile_items(identity.position, next_items)
                .map_err(HostError::Core)?;
        }
        if let Some(journal) = journal {
            map.apply_source_item_removals(&journal.removed_items)
                .map_err(HostError::Core)?;
        }
        let runtime_tile_items = runtime_items
            .map(|(map_revision, records)| {
                if *map_revision != source.source_revision() {
                    return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                        "runtime tile-item registry revision does not match the loaded map".into(),
                    )));
                }
                let mut ordered: BTreeMap<Position, Vec<RuntimeMapItemRecord>> = BTreeMap::new();
                for record in records {
                    if record.server_id == 0 || record.count == 0 {
                        return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                            "runtime tile-item record needs a nonzero server id and count".into(),
                        )));
                    }
                    if record.children.len() > forgotten_persistence::MAX_RUNTIME_MAP_ITEM_CHILDREN
                    {
                        return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                            "runtime tile-item record exceeds the supported child bound".into(),
                        )));
                    }
                    if source.tile(record.position).is_none() {
                        return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                            "runtime tile-item record references a missing source tile".into(),
                        )));
                    }
                    let entries = ordered.entry(record.position).or_default();
                    if usize::from(record.ordinal) != entries.len() {
                        return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                            "runtime tile-item ordinals must form one contiguous list per tile"
                                .into(),
                        )));
                    }
                    entries.push(record.clone());
                }
                for (position, position_records) in &ordered {
                    let existing = map
                        .tile_items(*position)
                        .map(<[WorldMapItem]>::len)
                        .unwrap_or(0);
                    if existing + position_records.len()
                        > forgotten_core::MAX_WORLD_MAP_ITEMS_PER_TILE
                    {
                        return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                            "runtime tile-item recovery would exceed the per-tile item limit"
                                .into(),
                        )));
                    }
                    let mut items = map
                        .tile_items(*position)
                        .map(<[WorldMapItem]>::to_vec)
                        .unwrap_or_default();
                    for record in position_records.iter() {
                        items.push(runtime_record_to_world_map_item(record));
                    }
                    map.set_tile_items(*position, items)
                        .map_err(HostError::Core)?;
                }
                Ok(records.clone())
            })
            .transpose()?
            .unwrap_or_default();
        let mut source_item_indices = BTreeMap::new();
        for (position, items) in source.tile_item_entries() {
            source_item_indices.insert(
                position,
                (0..items.len())
                    .filter_map(|index| {
                        let Ok(item_index) = u8::try_from(index) else {
                            return None;
                        };
                        (!removed_source_items.contains(&WorldMapItemSourceIdentity {
                            map_revision: source.source_revision(),
                            position,
                            item_index,
                        }))
                        .then_some(item_index)
                    })
                    .collect(),
            );
        }
        Ok(Self {
            map: Arc::new(Mutex::new(map)),
            source,
            source_item_indices: Arc::new(Mutex::new(source_item_indices)),
            removed_source_items: Arc::new(Mutex::new(removed_source_items)),
            source_item_count_overrides: Arc::new(Mutex::new(source_item_count_overrides)),
            runtime_tile_items: Arc::new(Mutex::new(runtime_tile_items)),
            revision: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Returns an immutable point-in-time render snapshot. Callers never retain the map lock
    /// while encoding or writing protocol frames.
    pub fn render_snapshot(&self) -> Result<Arc<WorldMap>, HostError> {
        Ok(Arc::new(
            self.map
                .lock()
                .map_err(|_| HostError::SharedWorldUnavailable)?
                .clone(),
        ))
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    pub fn source_revision(&self) -> WorldMapSourceRevision {
        self.source.source_revision()
    }

    /// Returns a detached copy of the complete revision-bound removal journal owned by this map.
    /// It never reads or writes SQLite and is primarily useful to recovery/bootstrap code.
    pub fn removal_journal(&self) -> Result<MapItemRemovalJournal, HostError> {
        Ok(MapItemRemovalJournal {
            map_revision: self.source_revision(),
            removed_items: self
                .removed_source_items
                .lock()
                .map_err(|_| HostError::SharedWorldUnavailable)?
                .iter()
                .copied()
                .collect(),
        })
    }

    /// Returns a detached sorted copy of recovered remaining-count overrides. It never reads
    /// SQLite; later composite map-to-inventory transfers reuse it when preparing durable state.
    pub fn count_overrides(&self) -> Result<Vec<MapItemCountOverrideRecord>, HostError> {
        Ok(self
            .source_item_count_overrides
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .iter()
            .map(
                |(source_identity, remaining_count)| MapItemCountOverrideRecord {
                    source_identity: *source_identity,
                    remaining_count: *remaining_count,
                },
            )
            .collect())
    }

    /// Returns a detached copy of the complete durable runtime tile-item registry. It never reads
    /// SQLite; callers persist it through the composite placement boundary.
    pub fn runtime_tile_items(&self) -> Result<Vec<RuntimeMapItemRecord>, HostError> {
        Ok(self
            .runtime_tile_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .clone())
    }

    /// Returns one runtime-added tile item (a spawned corpse) at an exact top-level stack index.
    /// Runtime items always occupy the tail of their tile's ordered list, so only indexes at or
    /// above the surviving source-derived prefix resolve; imported source items never match here.
    pub fn runtime_tile_item(
        &self,
        position: Position,
        item_index: usize,
    ) -> Result<Option<WorldMapItem>, HostError> {
        let map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let registry = self
            .runtime_tile_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let items = map.tile_items(position);
        let runtime_count = registry
            .iter()
            .filter(|record| record.position == position)
            .count();
        let runtime_start = items
            .map(<[WorldMapItem]>::len)
            .unwrap_or(0)
            .saturating_sub(runtime_count);
        if item_index < runtime_start {
            return Ok(None);
        }
        Ok(items.and_then(|items| items.get(item_index)).cloned())
    }

    /// Places one spawned runtime corpse item on one tile, persists the complete registry in one
    /// SQLite transaction while every authoritative lock is held, then commits the staged map
    /// state. A bounded-limit rejection or failed persistence leaves both memory and disk
    /// unchanged. Returns the new map revision only when a corpse was actually placed.
    pub fn add_runtime_tile_item(
        &self,
        database: &mut EngineDatabase,
        position: Position,
        item: WorldMapItem,
        despawn_tick: Option<u64>,
    ) -> Result<Option<u64>, HostError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut registry = self
            .runtime_tile_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let current_items = map
            .tile_items(position)
            .map(<[WorldMapItem]>::to_vec)
            .unwrap_or_default();
        if current_items.len() + 1 > forgotten_core::MAX_WORLD_MAP_ITEMS_PER_TILE {
            return Ok(None);
        }
        if registry.len() >= forgotten_persistence::MAX_RUNTIME_MAP_ITEMS {
            return Ok(None);
        }
        let ordinal = u8::try_from(
            registry
                .iter()
                .filter(|record| record.position == position)
                .count(),
        )
        .map_err(|_| {
            HostError::Core(forgotten_core::CoreError::InvalidMap(
                "runtime tile-item ordinals exhausted for this tile".into(),
            ))
        })?;
        let Some(record) = runtime_world_map_item_to_record(position, ordinal, &item, despawn_tick)
        else {
            return Ok(None);
        };
        let mut next_map = map.clone();
        let mut next_items = current_items;
        next_items.push(item);
        next_map
            .set_tile_items(position, next_items)
            .map_err(HostError::Core)?;
        let mut next_registry = registry.clone();
        next_registry.push(record);
        database.replace_runtime_map_items(self.source_revision(), &next_registry)?;
        *map = next_map;
        *registry = next_registry;
        let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(revision))
    }

    /// Removes every runtime tile item whose durable despawn tick is due at `now_tick`,
    /// persists the surviving registry in one SQLite transaction while authoritative locks are
    /// held, then commits the staged map state. Returns the affected positions so the caller can
    /// advance visibility exactly once after a real change.
    pub fn remove_expired_runtime_items(
        &self,
        database: &mut EngineDatabase,
        now_tick: u64,
    ) -> Result<Vec<Position>, HostError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut registry = self
            .runtime_tile_items
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        if registry.is_empty() {
            return Ok(Vec::new());
        }
        let expired_indexes: Vec<usize> = registry
            .iter()
            .enumerate()
            .filter(|(_, record)| record.despawn_tick.is_some_and(|tick| tick <= now_tick))
            .map(|(index, _)| index)
            .collect();
        if expired_indexes.is_empty() {
            return Ok(Vec::new());
        }
        let expired: BTreeSet<(Position, u8)> = expired_indexes
            .iter()
            .filter_map(|index| {
                registry
                    .get(*index)
                    .map(|record| (record.position, record.ordinal))
            })
            .collect();
        let mut next_registry = registry.clone();
        for index in expired_indexes.iter().rev() {
            next_registry.remove(*index);
        }
        let mut next_map = map.clone();
        for position in expired
            .iter()
            .map(|(position, _)| *position)
            .collect::<BTreeSet<_>>()
        {
            let items = next_map.tile_items(position).map(<[WorldMapItem]>::to_vec);
            let Some(items) = items else {
                return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                    "an expired runtime item references a missing map tile".into(),
                )));
            };
            let runtime_count = registry
                .iter()
                .filter(|record| record.position == position)
                .count();
            let runtime_start = items.len().saturating_sub(runtime_count);
            // Remove the doomed runtime tail indexes in descending order so earlier removals
            // cannot invalidate later ones.
            let mut doomed: Vec<usize> = expired
                .iter()
                .filter(|(expired_position, _)| *expired_position == position)
                .map(|(_, ordinal)| runtime_start + usize::from(*ordinal))
                .collect();
            doomed.sort_unstable();
            doomed.reverse();
            let mut remaining = items;
            for index in &doomed {
                if *index >= remaining.len() {
                    return Err(HostError::Core(forgotten_core::CoreError::InvalidMap(
                        "an expired runtime item index no longer resolves".into(),
                    )));
                }
                remaining.remove(*index);
            }
            next_map
                .set_tile_items(position, remaining)
                .map_err(HostError::Core)?;
        }
        database.replace_runtime_map_items(self.source_revision(), &next_registry)?;
        *map = next_map;
        *registry = next_registry;
        self.revision.fetch_add(1, Ordering::SeqCst);
        Ok(expired
            .into_iter()
            .map(|(position, _)| position)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
}
