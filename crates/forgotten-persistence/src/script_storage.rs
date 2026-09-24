//! Per-player script storage values: the durable jail behind TFS
//! `setPlayerStorageValue` / `getPlayerStorageValue`. Keys and values are signed
//! integers; absent keys read as `None` (the scripting layer maps that to TFS's
//! conventional `-1`). Writes upsert; a per-player count gate rejects runaway
//! per-tick writers instead of growing the table without bound.

use super::*;

impl EngineDatabase {
    /// Reads one durable script storage value. `None` means never set; the
    /// scripting layer reports that as `-1` per TFS convention.
    pub fn player_storage_value(
        &self,
        player_id: u64,
        key: i64,
    ) -> Result<Option<i64>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        Ok(self
            .connection
            .query_row(
                "SELECT storage_value FROM player_storage_values WHERE player_id = ?1 AND storage_key = ?2",
                params![player_id as i64, key],
                |row| row.get::<_, i64>(0),
            )
            .optional()?)
    }

    /// Upserts one durable script storage value. New keys beyond
    /// [`MAX_PLAYER_STORAGE_VALUES`] fail closed so a runaway script cannot grow
    /// the table without bound; overwrites of existing keys always succeed.
    pub fn set_player_storage_value(
        &self,
        player_id: u64,
        key: i64,
        value: i64,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let is_new = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM player_storage_values WHERE player_id = ?1 AND storage_key = ?2)",
                params![player_id as i64, key],
                |row| row.get::<_, i64>(0),
            )? == 0;
        if is_new {
            let count: i64 = self.connection.query_row(
                "SELECT COUNT(*) FROM player_storage_values WHERE player_id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )?;
            if count as usize >= MAX_PLAYER_STORAGE_VALUES {
                return Err(PersistenceError::InvalidStorageRecord(format!(
                    "player {player_id} exceeds {MAX_PLAYER_STORAGE_VALUES} script storage values"
                )));
            }
        }
        self.connection.execute(
            "INSERT INTO player_storage_values (player_id, storage_key, storage_value) VALUES (?1, ?2, ?3)
             ON CONFLICT (player_id, storage_key) DO UPDATE SET storage_value = excluded.storage_value",
            params![player_id as i64, key, value],
        )?;
        Ok(())
    }

    /// Counts durable script storage values for one player.
    pub fn player_storage_value_count(&self, player_id: u64) -> Result<usize, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM player_storage_values WHERE player_id = ?1",
            params![player_id as i64],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count as usize)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("forgotten-engine-{name}-{}.db", unix_seconds()))
    }

    fn stored_player(database: &EngineDatabase) -> u64 {
        let account_id = database.create_account("operator", "hash").unwrap();
        let player_id = 1;
        database
            .save_player(&Player {
                id: player_id,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        player_id
    }

    #[test]
    fn script_storage_round_trip_with_absent_and_negative_values() {
        let path = temporary_path("script-storage");
        let database = EngineDatabase::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let player_id = stored_player(&database);
        assert_eq!(
            database.player_storage_value(player_id, 1000).unwrap(),
            None
        );
        database
            .set_player_storage_value(player_id, 1000, 3)
            .unwrap();
        assert_eq!(
            database.player_storage_value(player_id, 1000).unwrap(),
            Some(3)
        );
        database
            .set_player_storage_value(player_id, 1000, -1)
            .unwrap();
        assert_eq!(
            database.player_storage_value(player_id, 1000).unwrap(),
            Some(-1)
        );
        assert_eq!(database.player_storage_value_count(player_id).unwrap(), 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn script_storage_rejects_unknown_players_and_runaway_key_counts() {
        let path = temporary_path("script-storage-bounds");
        let database = EngineDatabase::open(&path).unwrap();
        assert!(database.player_storage_value(999, 1).is_err());
        assert!(database.set_player_storage_value(999, 1, 1).is_err());
        let player_id = stored_player(&database);
        for key in 0..MAX_PLAYER_STORAGE_VALUES as i64 {
            database
                .set_player_storage_value(player_id, key, key)
                .unwrap();
        }
        assert_eq!(
            database.player_storage_value_count(player_id).unwrap(),
            MAX_PLAYER_STORAGE_VALUES
        );
        assert!(database
            .set_player_storage_value(player_id, MAX_PLAYER_STORAGE_VALUES as i64, 1)
            .is_err());
        // Overwriting an existing key still succeeds at the cap.
        database.set_player_storage_value(player_id, 0, 42).unwrap();
        assert_eq!(
            database.player_storage_value(player_id, 0).unwrap(),
            Some(42)
        );
        let _ = fs::remove_file(path);
    }
}
