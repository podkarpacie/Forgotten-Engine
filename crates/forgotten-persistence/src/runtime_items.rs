//! Runtime tile-item registry and composite inventory/map-item-state persistence:
//! replacement, retrieval, and the composite inventory+map-item-state transaction.
//! All methods operate on the durable database within transactions.

use super::*;

impl EngineDatabase {
    pub fn replace_runtime_map_items(
        &mut self,
        map_revision: WorldMapSourceRevision,
        items: &[RuntimeMapItemRecord],
    ) -> Result<(), PersistenceError> {
        validate_runtime_map_item_records(items)?;
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM runtime_map_item_children", [])?;
        transaction.execute("DELETE FROM runtime_map_items", [])?;
        insert_runtime_map_items(&transaction, map_revision, items)?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces a player's complete bounded inventory and the complete runtime tile-item registry
    /// in one SQLite transaction. Callers use this only after validating a composite
    /// authoritative inventory-to-ground transition; a failed commit leaves both durable
    /// collections unchanged.
    pub fn replace_player_inventory_and_runtime_map_items(
        &mut self,
        player_id: u64,
        equipment: &PlayerEquipment,
        containers: &PlayerContainers,
        map_revision: WorldMapSourceRevision,
        runtime_items: &[RuntimeMapItemRecord],
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        validate_runtime_map_item_records(runtime_items)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_equipment WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM player_container_items WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM player_containers WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for (slot, item) in equipment.iter() {
            transaction.execute(
                "INSERT INTO player_equipment (player_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    player_id as i64,
                    i64::from(slot.code()),
                    i64::from(item.server_id),
                    i64::from(item.count),
                    item.action_id.map(i64::from),
                    item.unique_id.map(i64::from),
                ],
            )?;
        }
        for (container_id, container) in containers.iter() {
            transaction.execute(
                "INSERT INTO player_containers (player_id, container_id, server_id, count, name, has_parent, capacity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    player_id as i64,
                    i64::from(container_id),
                    i64::from(container.container_item.server_id),
                    i64::from(container.container_item.count),
                    container.name,
                    i64::from(u8::from(container.has_parent)),
                    i64::from(container.items.capacity()),
                ],
            )?;
            for (slot, item) in container.items.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO player_container_items (player_id, container_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        player_id as i64,
                        i64::from(container_id),
                        slot as i64,
                        i64::from(item.server_id),
                        i64::from(item.count),
                        item.action_id.map(i64::from),
                        item.unique_id.map(i64::from),
                    ],
                )?;
            }
        }
        transaction.execute("DELETE FROM runtime_map_item_children", [])?;
        transaction.execute("DELETE FROM runtime_map_items", [])?;
        insert_runtime_map_items(&transaction, map_revision, runtime_items)?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads the complete durable runtime tile-item registry without applying it to any map.
    /// Callers must validate it against the current immutable source-map revision before
    /// recovering a runtime map owner.
    pub fn runtime_map_items(
        &self,
    ) -> Result<Option<(WorldMapSourceRevision, Vec<RuntimeMapItemRecord>)>, PersistenceError> {
        let mut item_statement = self.connection.prepare(
            "SELECT map_revision, x, y, z, ordinal, server_id, count, despawn_tick FROM runtime_map_items ORDER BY x, y, z, ordinal",
        )?;
        let item_rows = item_statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<i64>>(7)?,
            ))
        })?;
        let mut map_revision: Option<WorldMapSourceRevision> = None;
        let mut items = Vec::new();
        for row in item_rows {
            let (revision, x, y, z, ordinal, server_id, count, despawn_tick) = row?;
            let parsed_revision = u64::from_str_radix(&revision, 16).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map item revision must be hexadecimal u64".into(),
                )
            })?;
            let parsed_revision = WorldMapSourceRevision(parsed_revision);
            match map_revision {
                Some(existing) if existing != parsed_revision => {
                    return Err(PersistenceError::InvalidMapItemJournal(
                        "runtime map items contain multiple map revisions".into(),
                    ))
                }
                None => map_revision = Some(parsed_revision),
                Some(_) => {}
            }
            let position = Position {
                x: u16::try_from(x).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal(
                        "runtime map item x does not fit u16".into(),
                    )
                })?,
                y: u16::try_from(y).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal(
                        "runtime map item y does not fit u16".into(),
                    )
                })?,
                z: u8::try_from(z).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal(
                        "runtime map item z does not fit u8".into(),
                    )
                })?,
            };
            let ordinal = u8::try_from(ordinal).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map item ordinal does not fit u8".into(),
                )
            })?;
            let server_id = u16::try_from(server_id).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map item server id does not fit u16".into(),
                )
            })?;
            if server_id == 0 {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "runtime map item server id must be nonzero".into(),
                ));
            }
            let count = u8::try_from(count).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map item count does not fit u8".into(),
                )
            })?;
            if count == 0 {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "runtime map item count must be positive".into(),
                ));
            }
            items.push(RuntimeMapItemRecord {
                position,
                ordinal,
                server_id,
                count,
                children: Vec::new(),
                despawn_tick: despawn_tick
                    .map(|tick| {
                        u64::try_from(tick).map_err(|_| {
                            PersistenceError::InvalidMapItemJournal(
                                "runtime item despawn tick must be nonnegative".into(),
                            )
                        })
                    })
                    .transpose()?,
            });
        }
        if items.len() > MAX_RUNTIME_MAP_ITEMS {
            return Err(PersistenceError::InvalidMapItemJournal(
                "runtime map-item registry exceeds the supported bound".into(),
            ));
        }
        let mut child_statement = self.connection.prepare(
            "SELECT x, y, z, ordinal, child_index, server_id, count FROM runtime_map_item_children ORDER BY x, y, z, ordinal, child_index",
        )?;
        let child_rows = child_statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;
        let mut expected_key: Option<(Position, u8)> = None;
        let mut expected_child_index = 0_u8;
        for row in child_rows {
            let (x, y, z, ordinal, child_index, server_id, count) = row?;
            let position = Position {
                x: u16::try_from(x).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal(
                        "runtime map child x does not fit u16".into(),
                    )
                })?,
                y: u16::try_from(y).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal(
                        "runtime map child y does not fit u16".into(),
                    )
                })?,
                z: u8::try_from(z).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal(
                        "runtime map child z does not fit u8".into(),
                    )
                })?,
            };
            let ordinal = u8::try_from(ordinal).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map child ordinal does not fit u8".into(),
                )
            })?;
            let key = (position, ordinal);
            match expected_key {
                Some(existing) if existing == key => {}
                _ => {
                    expected_key = Some(key);
                    expected_child_index = 0;
                }
            }
            let parsed_child_index = u8::try_from(child_index).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map child index does not fit u8".into(),
                )
            })?;
            if parsed_child_index != expected_child_index {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "runtime map children must form one contiguous ordered list per item".into(),
                ));
            }
            expected_child_index += 1;
            let server_id = u16::try_from(server_id).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map child server id does not fit u16".into(),
                )
            })?;
            if server_id == 0 {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "runtime map child server id must be nonzero".into(),
                ));
            }
            let count = u8::try_from(count).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "runtime map child count does not fit u8".into(),
                )
            })?;
            if count == 0 {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "runtime map child count must be positive".into(),
                ));
            }
            let parent = items
                .iter_mut()
                .find(|item| item.position == position && item.ordinal == ordinal)
                .ok_or_else(|| {
                    PersistenceError::InvalidMapItemJournal(
                        "runtime map child references a missing runtime item".into(),
                    )
                })?;
            if parent.children.len() >= MAX_RUNTIME_MAP_ITEM_CHILDREN {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "runtime map item children exceed the supported bound".into(),
                ));
            }
            parent
                .children
                .push(RuntimeMapItemChildRecord { server_id, count });
        }
        Ok(map_revision.map(|revision| (revision, items)))
    }

    /// Replaces a player's complete bounded inventory, the complete revision-bound removal journal,
    /// and the complete remaining-count override collection in one SQLite transaction. A failed
    /// commit leaves all durable inventory and map-source recovery state unchanged.
    pub fn replace_player_inventory_and_map_item_state(
        &mut self,
        player_id: u64,
        equipment: &PlayerEquipment,
        containers: &PlayerContainers,
        journal: &MapItemRemovalJournal,
        overrides: &[MapItemCountOverrideRecord],
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut removed = BTreeMap::new();
        for item in &journal.removed_items {
            if item.map_revision != journal.map_revision {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "every removed item must use the journal map revision".into(),
                ));
            }
            if removed
                .insert((item.position, item.item_index), ())
                .is_some()
            {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "duplicate removed source item identity".into(),
                ));
            }
        }
        let mut overridden = BTreeMap::new();
        for override_record in overrides {
            if override_record.source_identity.map_revision != journal.map_revision {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "every count override must use the journal map revision".into(),
                ));
            }
            if !(1..=MAX_ITEM_STACK_COUNT).contains(&override_record.remaining_count) {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "count override remaining count must stay within the bounded stack range"
                        .into(),
                ));
            }
            let key = (
                override_record.source_identity.position,
                override_record.source_identity.item_index,
            );
            if removed.contains_key(&key) {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "one source item cannot be both removed and count-overridden".into(),
                ));
            }
            if overridden.insert(key, ()).is_some() {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "duplicate source item identity in count overrides".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_equipment WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM player_container_items WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM player_containers WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for (slot, item) in equipment.iter() {
            transaction.execute(
                "INSERT INTO player_equipment (player_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    player_id as i64,
                    i64::from(slot.code()),
                    i64::from(item.server_id),
                    i64::from(item.count),
                    item.action_id.map(i64::from),
                    item.unique_id.map(i64::from),
                ],
            )?;
        }
        for (container_id, container) in containers.iter() {
            transaction.execute(
                "INSERT INTO player_containers (player_id, container_id, server_id, count, name, has_parent, capacity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    player_id as i64,
                    i64::from(container_id),
                    i64::from(container.container_item.server_id),
                    i64::from(container.container_item.count),
                    container.name,
                    i64::from(u8::from(container.has_parent)),
                    i64::from(container.items.capacity()),
                ],
            )?;
            for (slot, item) in container.items.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO player_container_items (player_id, container_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        player_id as i64,
                        i64::from(container_id),
                        slot as i64,
                        i64::from(item.server_id),
                        i64::from(item.count),
                        item.action_id.map(i64::from),
                        item.unique_id.map(i64::from),
                    ],
                )?;
            }
        }
        transaction.execute("DELETE FROM map_item_removal_journal", [])?;
        for item in &journal.removed_items {
            transaction.execute(
                "INSERT INTO map_item_removal_journal (map_revision, x, y, z, item_index) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    format!("{:016x}", journal.map_revision.0),
                    i64::from(item.position.x),
                    i64::from(item.position.y),
                    i64::from(item.position.z),
                    i64::from(item.item_index),
                ],
            )?;
        }
        transaction.execute("DELETE FROM map_item_count_overrides", [])?;
        for override_record in overrides {
            transaction.execute(
                "INSERT INTO map_item_count_overrides (map_revision, x, y, z, item_index, remaining_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format!("{:016x}", journal.map_revision.0),
                    i64::from(override_record.source_identity.position.x),
                    i64::from(override_record.source_identity.position.y),
                    i64::from(override_record.source_identity.position.z),
                    i64::from(override_record.source_identity.item_index),
                    i64::from(override_record.remaining_count),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn migrate(&mut self) -> Result<(), PersistenceError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);\
             CREATE TABLE IF NOT EXISTS accounts (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at INTEGER NOT NULL);\
             CREATE TABLE IF NOT EXISTS players (id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, name TEXT NOT NULL UNIQUE, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, level INTEGER NOT NULL, experience INTEGER NOT NULL, skill_points INTEGER NOT NULL, health INTEGER NOT NULL DEFAULT 150, max_health INTEGER NOT NULL DEFAULT 150, mana INTEGER NOT NULL DEFAULT 50, max_mana INTEGER NOT NULL DEFAULT 50, capacity INTEGER NOT NULL DEFAULT 40000, magic_level INTEGER NOT NULL DEFAULT 0, town_id INTEGER NOT NULL DEFAULT 0);\
             CREATE TABLE IF NOT EXISTS engine_events (id INTEGER PRIMARY KEY, level TEXT NOT NULL, message TEXT NOT NULL, created_at INTEGER NOT NULL);",
        )?;
        self.connection.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![1_i64, unix_seconds()],
        )?;
        if self.schema_version()? < 2 {
            for (name, definition) in [
                ("health", "INTEGER NOT NULL DEFAULT 150"),
                ("max_health", "INTEGER NOT NULL DEFAULT 150"),
                ("mana", "INTEGER NOT NULL DEFAULT 50"),
                ("max_mana", "INTEGER NOT NULL DEFAULT 50"),
                ("capacity", "INTEGER NOT NULL DEFAULT 40000"),
                ("magic_level", "INTEGER NOT NULL DEFAULT 0"),
            ] {
                if !self.player_column_exists(name)? {
                    self.connection.execute_batch(&format!(
                        "ALTER TABLE players ADD COLUMN {name} {definition}"
                    ))?;
                }
            }
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![2_i64, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_EQUIPMENT {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_equipment (player_id INTEGER NOT NULL, slot INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, action_id INTEGER, unique_id INTEGER, PRIMARY KEY (player_id, slot));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_EQUIPMENT, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_CONTAINERS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_containers (player_id INTEGER NOT NULL, container_id INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, name TEXT NOT NULL, has_parent INTEGER NOT NULL, capacity INTEGER NOT NULL, PRIMARY KEY (player_id, container_id));\
                 CREATE TABLE IF NOT EXISTS player_container_items (player_id INTEGER NOT NULL, container_id INTEGER NOT NULL, slot INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, action_id INTEGER, unique_id INTEGER, PRIMARY KEY (player_id, container_id, slot));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_CONTAINERS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PROGRESSION {
            if !self.player_column_exists("vocation")? {
                self.connection.execute_batch(
                    "ALTER TABLE players ADD COLUMN vocation INTEGER NOT NULL DEFAULT 0",
                )?;
            }
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_skills (player_id INTEGER PRIMARY KEY, fist_level INTEGER NOT NULL, fist_percent INTEGER NOT NULL, club_level INTEGER NOT NULL, club_percent INTEGER NOT NULL, sword_level INTEGER NOT NULL, sword_percent INTEGER NOT NULL, axe_level INTEGER NOT NULL, axe_percent INTEGER NOT NULL, distance_level INTEGER NOT NULL, distance_percent INTEGER NOT NULL, shielding_level INTEGER NOT NULL, shielding_percent INTEGER NOT NULL, fishing_level INTEGER NOT NULL, fishing_percent INTEGER NOT NULL);",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PROGRESSION, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_TOWNS {
            if !self.player_column_exists("town_id")? {
                self.connection.execute_batch(
                    "ALTER TABLE players ADD COLUMN town_id INTEGER NOT NULL DEFAULT 0",
                )?;
            }
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_TOWNS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_CONDITIONS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_conditions (player_id INTEGER NOT NULL, kind INTEGER NOT NULL, interval_seconds INTEGER NOT NULL, damage INTEGER NOT NULL, remaining_seconds INTEGER NOT NULL, elapsed_seconds INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (player_id, kind));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_CONDITIONS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PROGRESSION_ATTEMPTS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_progression_attempts (player_id INTEGER PRIMARY KEY, fist_tries INTEGER NOT NULL, club_tries INTEGER NOT NULL, sword_tries INTEGER NOT NULL, axe_tries INTEGER NOT NULL, distance_tries INTEGER NOT NULL, shielding_tries INTEGER NOT NULL, fishing_tries INTEGER NOT NULL, magic_mana INTEGER NOT NULL);",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PROGRESSION_ATTEMPTS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_LIFECYCLE {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_lifecycle (player_id INTEGER PRIMARY KEY, dead INTEGER NOT NULL, respawn_x INTEGER, respawn_y INTEGER, respawn_z INTEGER, death_time INTEGER, loss_applied INTEGER NOT NULL);",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_LIFECYCLE, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_CONDITION_ELAPSED {
            if !self.player_conditions_column_exists("elapsed_seconds")? {
                self.connection.execute_batch(
                    "ALTER TABLE player_conditions ADD COLUMN elapsed_seconds INTEGER NOT NULL DEFAULT 0",
                )?;
            }
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_CONDITION_ELAPSED, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_OUTFIT {
            for (name, definition) in [
                ("look_type", "INTEGER NOT NULL DEFAULT 0"),
                ("look_head", "INTEGER NOT NULL DEFAULT 0"),
                ("look_body", "INTEGER NOT NULL DEFAULT 0"),
                ("look_legs", "INTEGER NOT NULL DEFAULT 0"),
                ("look_feet", "INTEGER NOT NULL DEFAULT 0"),
            ] {
                if !self.player_column_exists(name)? {
                    self.connection.execute_batch(&format!(
                        "ALTER TABLE players ADD COLUMN {name} {definition}"
                    ))?;
                }
            }
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_OUTFIT, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_STATIC_CREATURE_RUNTIME {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS static_creature_runtime (creature_id INTEGER PRIMARY KEY, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, active INTEGER NOT NULL, health_percent INTEGER NOT NULL);",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_STATIC_CREATURE_RUNTIME, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_STATIC_CREATURE_REACTIVATION {
            self.connection.execute_batch(
                "ALTER TABLE static_creature_runtime ADD COLUMN reactivation_remaining_seconds INTEGER NULL;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_STATIC_CREATURE_REACTIVATION, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_MAP_ITEM_JOURNAL {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS map_item_removal_journal (map_revision TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, item_index INTEGER NOT NULL, PRIMARY KEY (map_revision, x, y, z, item_index));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_MAP_ITEM_JOURNAL, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_STATIC_CREATURE_DAMAGE_SEQUENCE {
            self.connection.execute_batch(
                "ALTER TABLE static_creature_runtime ADD COLUMN direct_melee_damage_sequence INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![
                    SCHEMA_VERSION_STATIC_CREATURE_DAMAGE_SEQUENCE,
                    unix_seconds()
                ],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_STATIC_CREATURE_MELEE_COOLDOWN {
            self.connection.execute_batch(
                "ALTER TABLE static_creature_runtime ADD COLUMN direct_melee_cooldown_remaining_ticks INTEGER NULL;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![
                    SCHEMA_VERSION_STATIC_CREATURE_MELEE_COOLDOWN,
                    unix_seconds()
                ],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_ACCOUNT_VIP_ENTRIES {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS account_vip_entries (account_id INTEGER NOT NULL, player_id INTEGER NOT NULL, description TEXT NOT NULL, icon INTEGER NOT NULL, notify INTEGER NOT NULL, PRIMARY KEY (account_id, player_id));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_ACCOUNT_VIP_ENTRIES, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_GUILDS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS guilds (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, owner_player_id INTEGER NOT NULL UNIQUE, created_at INTEGER NOT NULL, motd TEXT NOT NULL DEFAULT '');\
                 CREATE TABLE IF NOT EXISTS guild_ranks (id INTEGER PRIMARY KEY, guild_id INTEGER NOT NULL, name TEXT NOT NULL, level INTEGER NOT NULL, UNIQUE (guild_id, level), UNIQUE (guild_id, name));\
                 CREATE TABLE IF NOT EXISTS guild_membership (player_id INTEGER PRIMARY KEY, guild_id INTEGER NOT NULL, rank_id INTEGER NOT NULL, nick TEXT NOT NULL DEFAULT '');",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_GUILDS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_GUILD_INVITATIONS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS guild_invitations (player_id INTEGER NOT NULL, guild_id INTEGER NOT NULL, PRIMARY KEY (player_id, guild_id));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_GUILD_INVITATIONS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_BANK_BALANCE {
            if !self.player_column_exists("bank_balance")? {
                self.connection.execute_batch(
                    "ALTER TABLE players ADD COLUMN bank_balance INTEGER NOT NULL DEFAULT 0",
                )?;
            }
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_BANK_BALANCE, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_DEPOTS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_depot_items (player_id INTEGER NOT NULL, depot_id INTEGER NOT NULL, slot INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, action_id INTEGER, unique_id INTEGER, PRIMARY KEY (player_id, depot_id, slot));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_DEPOTS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_INBOX {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_inbox_items (player_id INTEGER NOT NULL, slot INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, action_id INTEGER, unique_id INTEGER, PRIMARY KEY (player_id, slot));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_INBOX, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_HOUSE_OWNERSHIP {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS house_ownership (house_id INTEGER PRIMARY KEY, owner_player_id INTEGER NOT NULL);",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_HOUSE_OWNERSHIP, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_HOUSE_ACCESS_LISTS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS house_access_lists (house_id INTEGER NOT NULL, list_id INTEGER NOT NULL, text TEXT NOT NULL, PRIMARY KEY (house_id, list_id));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_HOUSE_ACCESS_LISTS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_MAP_ITEM_COUNT_OVERRIDES {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS map_item_count_overrides (map_revision TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, item_index INTEGER NOT NULL, remaining_count INTEGER NOT NULL, PRIMARY KEY (map_revision, x, y, z, item_index));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_MAP_ITEM_COUNT_OVERRIDES, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_RUNTIME_MAP_ITEMS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS runtime_map_items (map_revision TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, ordinal INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, PRIMARY KEY (x, y, z, ordinal));",
            )?;
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS runtime_map_item_children (x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, ordinal INTEGER NOT NULL, child_index INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, PRIMARY KEY (x, y, z, ordinal, child_index));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_RUNTIME_MAP_ITEMS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_CORPSE_DESPAWN_TICKS {
            self.connection
                .execute_batch("ALTER TABLE runtime_map_items ADD COLUMN despawn_tick INTEGER;")?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_CORPSE_DESPAWN_TICKS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_QUESTS {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_quests (player_id INTEGER NOT NULL, quest_id INTEGER NOT NULL, completed INTEGER NOT NULL, PRIMARY KEY (player_id, quest_id));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_QUESTS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_BLESS_PROMOTION {
            self.connection.execute_batch(
                "ALTER TABLE players ADD COLUMN bless_count INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.connection.execute_batch(
                "ALTER TABLE players ADD COLUMN promoted INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_BLESS_PROMOTION, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_PARTIES {
            self.connection.execute_batch(
                "CREATE TABLE player_parties (
                    player_id INTEGER PRIMARY KEY REFERENCES players(id),
                    party_leader_id INTEGER NOT NULL REFERENCES players(id)
                );",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_PARTIES, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_ITEM_CONTENTS {
            // The old primary key could not host child rows alongside top-level slots, so
            // this migration rebuilds the table with parent_slot inside the key. Existing
            // rows keep their identity as top-level items (parent_slot NULL).
            self.connection.execute_batch(
                "ALTER TABLE player_container_items RENAME TO player_container_items_v30;
                 CREATE TABLE IF NOT EXISTS player_container_items (player_id INTEGER NOT NULL, container_id INTEGER NOT NULL, slot INTEGER NOT NULL, parent_slot INTEGER, server_id INTEGER NOT NULL, count INTEGER NOT NULL, action_id INTEGER, unique_id INTEGER, PRIMARY KEY (player_id, container_id, parent_slot, slot));
                 INSERT INTO player_container_items (player_id, container_id, slot, parent_slot, server_id, count, action_id, unique_id) SELECT player_id, container_id, slot, NULL, server_id, count, action_id, unique_id FROM player_container_items_v30;
                 DROP TABLE player_container_items_v30;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_ITEM_CONTENTS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_GM_LEVEL {
            // Operator-granted gamemaster tier per character. Zero stays the plain-player
            // default so existing worlds upgrade without behavior changes.
            self.connection.execute_batch(
                "ALTER TABLE players ADD COLUMN gm_level INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_GM_LEVEL, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_FACING {
            // Persisted cardinal facing so relog restores the character's rotation together
            // with the saved position. Classic direction bytes: 0 north, 1 east, 2 south,
            // 3 west; two (south) matches the historical login-facing default.
            self.connection.execute_batch(
                "ALTER TABLE players ADD COLUMN facing INTEGER NOT NULL DEFAULT 2;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_FACING, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_ACCOUNT_BANS {
            // Operator moderation state (plan v49 slice 17): account bans with optional
            // expiry plus account mutes for chat suppression. Version-neutral infrastructure.
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS account_bans (
                    id INTEGER PRIMARY KEY,
                    account_id INTEGER NOT NULL REFERENCES accounts(id),
                    reason TEXT NOT NULL,
                    expires_at INTEGER,
                    created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS account_mutes (
                    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
                    muted_until INTEGER NOT NULL
                 );",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_ACCOUNT_BANS, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_PLAYER_FROZEN {
            // Operator freeze flag (plan v49 slice 18): a frozen character cannot step. The
            // flag survives relogs so moderation holds while the operator walks over.
            self.connection.execute_batch(
                "ALTER TABLE players ADD COLUMN frozen INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_PLAYER_FROZEN, unix_seconds()],
            )?;
        }
        if self.schema_version()? < SCHEMA_VERSION_CONDITION_SPEED {
            // Timed speed condition payload (plan v49 slice 12): one percent column per row;
            // zero for damage-over-time kinds, 1..=100 for haste rows.
            self.connection.execute_batch(
                "ALTER TABLE player_conditions ADD COLUMN speed_percent INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION_CONDITION_SPEED, unix_seconds()],
            )?;
        }
        Ok(())
    }

    pub(crate) fn ensure_player_exists(&self, player_id: u64) -> Result<(), PersistenceError> {
        let exists = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM players WHERE id = ?1)",
            params![player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if exists {
            Ok(())
        } else {
            Err(PersistenceError::UnknownPlayer(player_id))
        }
    }

    pub(crate) fn ensure_account_exists(&self, account_id: u32) -> Result<(), PersistenceError> {
        let exists = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = ?1)",
            params![account_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if exists {
            Ok(())
        } else {
            Err(PersistenceError::UnknownAccount(account_id))
        }
    }

    fn player_column_exists(&self, column: &str) -> Result<bool, PersistenceError> {
        let mut statement = self.connection.prepare("PRAGMA table_info(players)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(columns.iter().any(|name| name == column))
    }

    fn player_conditions_column_exists(&self, column: &str) -> Result<bool, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("PRAGMA table_info(player_conditions)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(columns.iter().any(|name| name == column))
    }
}
