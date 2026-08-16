//! SQLite persistence, secure account authentication, and backup primitives.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use forgotten_core::{EquipmentSlot, ItemInstance, Player, PlayerEquipment, Position};
use rand::rngs::OsRng;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LATEST_SCHEMA_VERSION: i64 = 3;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct EngineDatabase {
    connection: Connection,
    path: PathBuf,
}

impl EngineDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PersistenceError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&path)?;
        connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
        let mut database = Self { connection, path };
        database.migrate()?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<i64, PersistenceError> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }

    /// Inserts an account whose hash has already been produced by an approved password provider.
    pub fn create_account(&self, name: &str, password_hash: &str) -> Result<i64, PersistenceError> {
        self.connection.execute(
            "INSERT INTO accounts (name, password_hash, created_at) VALUES (?1, ?2, ?3)",
            params![name, password_hash, unix_seconds()],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Creates an account using an Argon2 password hash; plaintext is never persisted.
    pub fn create_account_with_password(
        &self,
        name: &str,
        password: &str,
    ) -> Result<i64, PersistenceError> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| PersistenceError::PasswordHash(error.to_string()))?
            .to_string();
        self.create_account(name, &hash)
    }

    /// Authenticates an account and returns only its own character summaries.
    pub fn authenticate_account(
        &self,
        name: &str,
        password: &str,
    ) -> Result<Option<LoginAccount>, PersistenceError> {
        let record = self
            .connection
            .query_row(
                "SELECT id, password_hash FROM accounts WHERE name = ?1",
                params![name],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((id, password_hash)) = record else {
            return Ok(None);
        };
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|error| PersistenceError::PasswordHash(error.to_string()))?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            return Ok(None);
        }
        Ok(Some(LoginAccount {
            id,
            name: name.to_owned(),
            characters: self.characters_for_account(id)?,
        }))
    }

    /// Authenticates a legacy numeric account identifier for native protocol profiles.
    pub fn authenticate_account_id(
        &self,
        account_id: u32,
        password: &str,
    ) -> Result<Option<LoginAccount>, PersistenceError> {
        let record = self
            .connection
            .query_row(
                "SELECT id, name, password_hash FROM accounts WHERE id = ?1",
                params![account_id as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, name, password_hash)) = record else {
            return Ok(None);
        };
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|error| PersistenceError::PasswordHash(error.to_string()))?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            return Ok(None);
        }
        Ok(Some(LoginAccount {
            id,
            name,
            characters: self.characters_for_account(id)?,
        }))
    }

    pub fn characters_for_account(
        &self,
        account_id: i64,
    ) -> Result<Vec<LoginCharacter>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, level, experience, skill_points, health, max_health, mana, max_mana, capacity, magic_level, x, y, z FROM players WHERE account_id = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        let characters = statement
            .query_map(params![account_id], |row| {
                Ok(LoginCharacter {
                    id: row.get::<_, i64>(0)? as u64,
                    name: row.get(1)?,
                    level: row.get::<_, i64>(2)? as u32,
                    experience: row.get::<_, i64>(3)? as u64,
                    skill_points: row.get::<_, i64>(4)? as u32,
                    vitals: PlayerVitals {
                        health: row.get::<_, i64>(5)? as u16,
                        max_health: row.get::<_, i64>(6)? as u16,
                        mana: row.get::<_, i64>(7)? as u16,
                        max_mana: row.get::<_, i64>(8)? as u16,
                        capacity: row.get::<_, i64>(9)? as u16,
                        magic_level: row.get::<_, i64>(10)? as u8,
                    },
                    position: Position {
                        x: row.get::<_, i64>(11)? as u16,
                        y: row.get::<_, i64>(12)? as u16,
                        z: row.get::<_, i64>(13)? as u8,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(characters)
    }

    pub fn save_player(&self, player: &Player) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO players (id, account_id, name, x, y, z, level, experience, skill_points)\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)\
             ON CONFLICT(id) DO UPDATE SET account_id=excluded.account_id, name=excluded.name,\
             x=excluded.x, y=excluded.y, z=excluded.z, level=excluded.level,\
             experience=excluded.experience, skill_points=excluded.skill_points",
            params![
                player.id as i64,
                player.account_id as i64,
                player.name,
                player.position.x as i64,
                player.position.y as i64,
                player.position.z as i64,
                player.level as i64,
                player.experience as i64,
                player.skill_points as i64,
            ],
        )?;
        Ok(())
    }

    pub fn create_player_for_account(
        &self,
        account_id: u32,
        name: &str,
    ) -> Result<LoginCharacter, PersistenceError> {
        if name.trim().is_empty() || name.len() > 32 {
            return Err(PersistenceError::InvalidPlayerName);
        }
        let account_exists = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = ?1)",
            params![account_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !account_exists {
            return Err(PersistenceError::UnknownAccount(account_id));
        }

        let position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        self.connection.execute(
            "INSERT INTO players (account_id, name, x, y, z, level, experience, skill_points)\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                account_id as i64,
                name.trim(),
                position.x as i64,
                position.y as i64,
                position.z as i64,
                8_i64,
                0_i64,
                0_i64,
            ],
        )?;
        Ok(LoginCharacter {
            id: self.connection.last_insert_rowid() as u64,
            name: name.trim().to_owned(),
            level: 8,
            experience: 0,
            skill_points: 0,
            vitals: PlayerVitals::default(),
            position,
        })
    }

    pub fn update_player_vitals(
        &self,
        player_id: u64,
        vitals: PlayerVitals,
    ) -> Result<(), PersistenceError> {
        if !vitals.is_valid() {
            return Err(PersistenceError::InvalidPlayerVitals);
        }
        let affected = self.connection.execute(
            "UPDATE players SET health = ?1, max_health = ?2, mana = ?3, max_mana = ?4, capacity = ?5, magic_level = ?6 WHERE id = ?7",
            params![
                vitals.health as i64,
                vitals.max_health as i64,
                vitals.mana as i64,
                vitals.max_mana as i64,
                vitals.capacity as i64,
                vitals.magic_level as i64,
                player_id as i64,
            ],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownPlayer(player_id));
        }
        Ok(())
    }

    pub fn update_player_position(
        &self,
        player_id: u64,
        position: Position,
    ) -> Result<(), PersistenceError> {
        let affected = self.connection.execute(
            "UPDATE players SET x = ?1, y = ?2, z = ?3 WHERE id = ?4",
            params![
                position.x as i64,
                position.y as i64,
                position.z as i64,
                player_id as i64,
            ],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownPlayer(player_id));
        }
        Ok(())
    }

    /// Replaces the player's fixed equipment set in one SQLite transaction. Containers, depot,
    /// inbox, and map-item ownership are intentionally outside this first inventory slice.
    pub fn replace_player_equipment(
        &mut self,
        player_id: u64,
        equipment: &PlayerEquipment,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_equipment WHERE player_id = ?1",
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
        transaction.commit()?;
        Ok(())
    }

    /// Loads a player's fixed equipment into the validated core item model. Database values are
    /// never trusted blindly: invalid slot codes, item IDs, counts, or attribute widths are
    /// surfaced as safe persistence errors.
    pub fn player_equipment(&self, player_id: u64) -> Result<PlayerEquipment, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT slot, server_id, count, action_id, unique_id FROM player_equipment WHERE player_id = ?1 ORDER BY slot",
        )?;
        let records = statement
            .query_map(params![player_id as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut equipment = PlayerEquipment::default();
        for (slot_code, server_id, count, action_id, unique_id) in records {
            let slot_code = u8::try_from(slot_code).map_err(|_| {
                PersistenceError::InvalidEquipmentRecord(
                    "slot does not fit an unsigned byte".into(),
                )
            })?;
            let slot = EquipmentSlot::from_code(slot_code).ok_or_else(|| {
                PersistenceError::InvalidEquipmentRecord(format!(
                    "unknown equipment slot {slot_code}"
                ))
            })?;
            let server_id = u16::try_from(server_id).map_err(|_| {
                PersistenceError::InvalidEquipmentRecord("server item ID does not fit u16".into())
            })?;
            let count = u16::try_from(count).map_err(|_| {
                PersistenceError::InvalidEquipmentRecord("item count does not fit u16".into())
            })?;
            let mut item = ItemInstance::new(server_id, count)
                .map_err(|error| PersistenceError::InvalidEquipmentRecord(error.to_string()))?;
            item.action_id = optional_u16_attribute(action_id, "action ID")?;
            item.unique_id = optional_u16_attribute(unique_id, "unique ID")?;
            equipment.equip(slot, item);
        }
        Ok(equipment)
    }

    pub fn record_event(&self, level: &str, message: &str) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO engine_events (level, message, created_at) VALUES (?1, ?2, ?3)",
            params![level, message, unix_seconds()],
        )?;
        Ok(())
    }

    pub fn event_count(&self) -> Result<i64, PersistenceError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM engine_events", [], |row| row.get(0))?)
    }

    fn migrate(&mut self) -> Result<(), PersistenceError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);\
             CREATE TABLE IF NOT EXISTS accounts (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at INTEGER NOT NULL);\
             CREATE TABLE IF NOT EXISTS players (id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, name TEXT NOT NULL UNIQUE, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, level INTEGER NOT NULL, experience INTEGER NOT NULL, skill_points INTEGER NOT NULL, health INTEGER NOT NULL DEFAULT 150, max_health INTEGER NOT NULL DEFAULT 150, mana INTEGER NOT NULL DEFAULT 50, max_mana INTEGER NOT NULL DEFAULT 50, capacity INTEGER NOT NULL DEFAULT 40000, magic_level INTEGER NOT NULL DEFAULT 0);\
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
        if self.schema_version()? < LATEST_SCHEMA_VERSION {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS player_equipment (player_id INTEGER NOT NULL, slot INTEGER NOT NULL, server_id INTEGER NOT NULL, count INTEGER NOT NULL, action_id INTEGER, unique_id INTEGER, PRIMARY KEY (player_id, slot));",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![LATEST_SCHEMA_VERSION, unix_seconds()],
            )?;
        }
        Ok(())
    }

    fn ensure_player_exists(&self, player_id: u64) -> Result<(), PersistenceError> {
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

    fn player_column_exists(&self, column: &str) -> Result<bool, PersistenceError> {
        let mut statement = self.connection.prepare("PRAGMA table_info(players)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(columns.iter().any(|name| name == column))
    }
}

pub fn create_backup(
    database_path: impl AsRef<Path>,
    backups_directory: impl AsRef<Path>,
) -> Result<BackupArtifact, PersistenceError> {
    let source = database_path.as_ref();
    let directory = backups_directory.as_ref();
    fs::create_dir_all(directory)?;
    let timestamp = unix_seconds();
    let database_copy = directory.join(format!("server-{timestamp}.db"));
    fs::copy(source, &database_copy)?;
    let manifest_path = directory.join(format!("server-{timestamp}.manifest.txt"));
    let manifest = format!(
        "forgotten-engine-backup-v1\ncreated_at={timestamp}\ndatabase={}\nincludes=database,player-data,map,config,plugins,scripts\n",
        database_copy.display()
    );
    fs::write(&manifest_path, manifest)?;
    Ok(BackupArtifact {
        database_copy,
        manifest_path,
        created_at: timestamp,
    })
}

#[derive(Debug, Clone)]
pub struct BackupArtifact {
    pub database_copy: PathBuf,
    pub manifest_path: PathBuf,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginAccount {
    pub id: i64,
    pub name: String,
    pub characters: Vec<LoginCharacter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCharacter {
    pub id: u64,
    pub name: String,
    pub level: u32,
    pub experience: u64,
    pub skill_points: u32,
    pub vitals: PlayerVitals,
    pub position: Position,
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
    fn is_valid(self) -> bool {
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

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn optional_u16_attribute(
    value: Option<i64>,
    label: &str,
) -> Result<Option<u16>, PersistenceError> {
    value
        .map(|value| {
            u16::try_from(value).map_err(|_| {
                PersistenceError::InvalidEquipmentRecord(format!("{label} does not fit u16"))
            })
        })
        .transpose()
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    PasswordHash(String),
    InvalidPlayerName,
    InvalidPlayerVitals,
    InvalidEquipmentRecord(String),
    UnknownAccount(u32),
    UnknownPlayer(u64),
}

impl From<std::io::Error> for PersistenceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for PersistenceError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PersistenceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use forgotten_core::Position;
    use std::sync::mpsc;
    use std::thread;

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("forgotten-engine-{name}-{}.db", unix_seconds()))
    }

    #[test]
    fn creates_and_migrates_empty_database() {
        let path = temporary_path("migration");
        let database = EngineDatabase::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn migrates_existing_v1_players_with_safe_native_vital_defaults() {
        let path = temporary_path("v1-vitals-migration");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
                 CREATE TABLE accounts (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at INTEGER NOT NULL);
                 CREATE TABLE players (id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, name TEXT NOT NULL UNIQUE, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, level INTEGER NOT NULL, experience INTEGER NOT NULL, skill_points INTEGER NOT NULL);
                 CREATE TABLE engine_events (id INTEGER PRIMARY KEY, level TEXT NOT NULL, message TEXT NOT NULL, created_at INTEGER NOT NULL);
                 INSERT INTO schema_migrations (version, applied_at) VALUES (1, 0);
                 INSERT INTO accounts (id, name, password_hash, created_at) VALUES (1, 'admin', 'hash', 0);
                 INSERT INTO players (id, account_id, name, x, y, z, level, experience, skill_points) VALUES (7, 1, 'Knight', 100, 100, 7, 8, 4900, 3);",
            )
            .unwrap();
        drop(connection);

        let database = EngineDatabase::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let character = database.characters_for_account(1).unwrap().remove(0);
        assert_eq!(character.experience, 4_900);
        assert_eq!(character.vitals, PlayerVitals::default());
        assert!(database.player_equipment(7).unwrap().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_a_player_and_event() {
        let path = temporary_path("player");
        let database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("admin", "hash").unwrap();
        database
            .save_player(&Player {
                id: 1,
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
        database.record_event("info", "player saved").unwrap();
        assert_eq!(database.event_count().unwrap(), 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_and_replaces_player_equipment_transactionally() {
        let path = temporary_path("equipment");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("admin", "hash").unwrap();
        database
            .save_player(&Player {
                id: 7,
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

        let mut sword = ItemInstance::new(2376, 1).unwrap();
        sword.action_id = Some(4_500);
        let shield = ItemInstance::new(2512, 1).unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::RightHand, sword.clone());
        equipment.equip(EquipmentSlot::LeftHand, shield.clone());
        database.replace_player_equipment(7, &equipment).unwrap();

        let loaded = database.player_equipment(7).unwrap();
        assert_eq!(loaded.item(EquipmentSlot::RightHand), Some(&sword));
        assert_eq!(loaded.item(EquipmentSlot::LeftHand), Some(&shield));

        let mut replacement = PlayerEquipment::default();
        replacement.equip(EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap());
        database.replace_player_equipment(7, &replacement).unwrap();
        let loaded = database.player_equipment(7).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded.item(EquipmentSlot::RightHand).is_none());
        assert_eq!(
            loaded.item(EquipmentSlot::Armor),
            replacement.item(EquipmentSlot::Armor)
        );
        database
            .connection
            .execute(
                "INSERT INTO player_equipment (player_id, slot, server_id, count) VALUES (?1, ?2, ?3, ?4)",
                params![7_i64, 99_i64, 2376_i64, 1_i64],
            )
            .unwrap();
        assert!(matches!(
            database.player_equipment(7),
            Err(PersistenceError::InvalidEquipmentRecord(_))
        ));
        assert!(matches!(
            database.player_equipment(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn authenticates_hashed_passwords_and_lists_own_characters() {
        let path = temporary_path("authentication");
        let database = EngineDatabase::open(&path).unwrap();
        let account_id = database
            .create_account_with_password("admin", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 7,
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
        assert!(database
            .authenticate_account("admin", "wrong")
            .unwrap()
            .is_none());
        let account = database
            .authenticate_account("admin", "correct horse battery staple")
            .unwrap()
            .unwrap();
        assert_eq!(account.id, account_id);
        assert_eq!(
            database
                .authenticate_account_id(account_id as u32, "correct horse battery staple")
                .unwrap()
                .unwrap()
                .name,
            "admin"
        );
        assert_eq!(account.characters[0].name, "Knight");
        assert_eq!(account.characters[0].experience, 4_900);
        assert_eq!(account.characters[0].skill_points, 3);
        assert_eq!(account.characters[0].vitals, PlayerVitals::default());
        assert_eq!(
            account.characters[0].position,
            Position {
                x: 100,
                y: 100,
                z: 7,
            }
        );
        database
            .update_player_position(
                7,
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            )
            .unwrap();
        let vitals = PlayerVitals {
            health: 95,
            max_health: 150,
            mana: 42,
            max_mana: 50,
            capacity: 32_000,
            magic_level: 4,
        };
        database.update_player_vitals(7, vitals).unwrap();
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].vitals,
            vitals
        );
        assert!(matches!(
            database.update_player_vitals(
                7,
                PlayerVitals {
                    health: 151,
                    ..vitals
                }
            ),
            Err(PersistenceError::InvalidPlayerVitals)
        ));
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            101
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn provisions_a_default_native_test_character_for_an_existing_account() {
        let path = temporary_path("provisioning");
        let database = EngineDatabase::open(&path).unwrap();
        assert!(matches!(
            database.create_player_for_account(42, "Knight"),
            Err(PersistenceError::UnknownAccount(42))
        ));
        let account_id = database
            .create_account_with_password("test-account", "test-password")
            .unwrap();
        let character = database
            .create_player_for_account(account_id.try_into().unwrap(), "Knight")
            .unwrap();
        assert_eq!(character.name, "Knight");
        assert_eq!(character.level, 8);
        assert_eq!(
            character.position,
            Position {
                x: 100,
                y: 100,
                z: 7,
            }
        );
        assert!(database
            .authenticate_account_id(account_id.try_into().unwrap(), "test-password")
            .unwrap()
            .unwrap()
            .characters
            .iter()
            .any(|entry| entry.name == "Knight"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn waits_for_a_brief_concurrent_sqlite_write_lock() {
        let path = temporary_path("sqlite-busy-timeout");
        let database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("admin", "hash").unwrap();
        database
            .save_player(&Player {
                id: 7,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 0,
                skill_points: 0,
            })
            .unwrap();
        let updater = EngineDatabase::open(&path).unwrap();
        let (start_sender, start_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let update_thread = thread::spawn(move || {
            start_receiver.recv().unwrap();
            result_sender.send(updater.update_player_position(
                7,
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ))
        });

        database
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .unwrap();
        start_sender.send(()).unwrap();
        thread::sleep(Duration::from_millis(50));
        database.connection.execute_batch("COMMIT").unwrap();
        assert!(result_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_ok());
        assert!(update_thread.join().unwrap().is_ok());
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            101
        );
        let _ = fs::remove_file(path);
    }
}
