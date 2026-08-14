//! SQLite persistence, secure account authentication, and backup primitives.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use forgotten_core::{Player, Position};
use rand::rngs::OsRng;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const LATEST_SCHEMA_VERSION: i64 = 1;

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

    pub fn characters_for_account(
        &self,
        account_id: i64,
    ) -> Result<Vec<LoginCharacter>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, level, x, y, z FROM players WHERE account_id = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        let characters = statement
            .query_map(params![account_id], |row| {
                Ok(LoginCharacter {
                    id: row.get::<_, i64>(0)? as u64,
                    name: row.get(1)?,
                    level: row.get::<_, i64>(2)? as u32,
                    position: Position {
                        x: row.get::<_, i64>(3)? as u16,
                        y: row.get::<_, i64>(4)? as u16,
                        z: row.get::<_, i64>(5)? as u8,
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
             CREATE TABLE IF NOT EXISTS players (id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, name TEXT NOT NULL UNIQUE, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, level INTEGER NOT NULL, experience INTEGER NOT NULL, skill_points INTEGER NOT NULL);\
             CREATE TABLE IF NOT EXISTS engine_events (id INTEGER PRIMARY KEY, level TEXT NOT NULL, message TEXT NOT NULL, created_at INTEGER NOT NULL);",
        )?;
        self.connection.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![LATEST_SCHEMA_VERSION, unix_seconds()],
        )?;
        Ok(())
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
    pub position: Position,
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    PasswordHash(String),
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
        assert_eq!(account.characters[0].name, "Knight");
        assert_eq!(
            account.characters[0].position,
            Position {
                x: 100,
                y: 100,
                z: 7,
            }
        );
        let _ = fs::remove_file(path);
    }
}
