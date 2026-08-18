//! SQLite persistence, secure account authentication, and backup primitives.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use forgotten_core::{
    EquipmentSlot, ItemInstance, Player, PlayerCondition, PlayerConditionKind, PlayerContainer,
    PlayerContainers, PlayerEquipment, PlayerProgression, PlayerProgressionAttempts,
    PlayerRespawnState, PlayerSkill, PlayerSkills, Position, SkillProgress, VocationId,
};
use rand::rngs::OsRng;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION_EQUIPMENT: i64 = 3;
const SCHEMA_VERSION_CONTAINERS: i64 = 4;
const SCHEMA_VERSION_PROGRESSION: i64 = 5;
const SCHEMA_VERSION_TOWNS: i64 = 6;
const SCHEMA_VERSION_CONDITIONS: i64 = 7;
const SCHEMA_VERSION_PROGRESSION_ATTEMPTS: i64 = 8;
const SCHEMA_VERSION_LIFECYCLE: i64 = 9;
const SCHEMA_VERSION_CONDITION_ELAPSED: i64 = 10;
const SCHEMA_VERSION_OUTFIT: i64 = 11;
const SCHEMA_VERSION_STATIC_CREATURE_RUNTIME: i64 = 12;
const SCHEMA_VERSION_STATIC_CREATURE_REACTIVATION: i64 = 13;
pub const LATEST_SCHEMA_VERSION: i64 = SCHEMA_VERSION_STATIC_CREATURE_REACTIVATION;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct EngineDatabase {
    connection: Connection,
    path: PathBuf,
}

/// The durable subset of known static-creature runtime state. The optional delay is a bounded,
/// restart-relative reactivation value, not an autonomous scheduler. Spawn definitions,
/// appearance, targets, AI cadence, combat, loot, and scripts remain content/runtime concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureRuntimeRecord {
    pub creature_id: u32,
    pub position: Position,
    pub active: bool,
    pub health_percent: u8,
    pub reactivation_remaining_seconds: Option<u32>,
}

/// The complete accepted authoritative state that must be persisted together after a bounded
/// explicit fixed-percent death-loss transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerFixedDeathLossSnapshot {
    pub player_id: u64,
    pub level: u32,
    pub experience: u64,
    pub vitals: PlayerVitals,
    pub progression: PlayerProgression,
    pub attempts: PlayerProgressionAttempts,
    pub state: PlayerRespawnState,
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
            "SELECT id, name, level, experience, skill_points, health, max_health, mana, max_mana, capacity, magic_level, x, y, z, town_id, look_type, look_head, look_body, look_legs, look_feet FROM players WHERE account_id = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        let mut characters = statement
            .query_map(params![account_id], |row| {
                Ok(LoginCharacter {
                    id: row.get::<_, i64>(0)? as u64,
                    name: row.get(1)?,
                    level: row.get::<_, i64>(2)? as u32,
                    experience: row.get::<_, i64>(3)? as u64,
                    skill_points: row.get::<_, i64>(4)? as u32,
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
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
                    town_id: row.get::<_, i64>(14)? as u32,
                    outfit: PlayerOutfit {
                        look_type: row.get::<_, i64>(15)? as u8,
                        head: row.get::<_, i64>(16)? as u8,
                        body: row.get::<_, i64>(17)? as u8,
                        legs: row.get::<_, i64>(18)? as u8,
                        feet: row.get::<_, i64>(19)? as u8,
                    },
                    respawn_state: PlayerRespawnState::default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for character in &mut characters {
            character.progression = self.player_progression(character.id)?;
            character.progression_attempts = self.player_progression_attempts(character.id)?;
            character.respawn_state = self.player_respawn_state(character.id)?;
        }
        Ok(characters)
    }

    pub fn player_by_id(&self, player_id: u64) -> Result<LoginCharacter, PersistenceError> {
        let account_id = self
            .connection
            .query_row(
                "SELECT account_id FROM players WHERE id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(PersistenceError::UnknownPlayer(player_id))?;
        self.characters_for_account(account_id)?
            .into_iter()
            .find(|character| character.id == player_id)
            .ok_or(PersistenceError::UnknownPlayer(player_id))
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

    /// Updates the persistent town assignment used by future temple-respawn behavior.
    pub fn update_player_town(&self, player_id: u64, town_id: u32) -> Result<(), PersistenceError> {
        let affected = self.connection.execute(
            "UPDATE players SET town_id = ?1 WHERE id = ?2",
            params![town_id as i64, player_id as i64],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownPlayer(player_id));
        }
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
            progression: PlayerProgression::default(),
            progression_attempts: PlayerProgressionAttempts::default(),
            vitals: PlayerVitals::default(),
            position,
            town_id: 0,
            outfit: PlayerOutfit::default(),
            respawn_state: PlayerRespawnState::default(),
        })
    }

    /// Persists an accepted classic outfit. A zero look type is reserved for the migration
    /// fallback, so live native sessions may only save a concrete appearance.
    pub fn update_player_outfit(
        &self,
        player_id: u64,
        outfit: PlayerOutfit,
    ) -> Result<(), PersistenceError> {
        if !outfit.is_concrete() {
            return Err(PersistenceError::InvalidPlayerOutfit);
        }
        let affected = self.connection.execute(
            "UPDATE players SET look_type = ?1, look_head = ?2, look_body = ?3, look_legs = ?4, look_feet = ?5 WHERE id = ?6",
            params![
                outfit.look_type as i64,
                outfit.head as i64,
                outfit.body as i64,
                outfit.legs as i64,
                outfit.feet as i64,
                player_id as i64,
            ],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownPlayer(player_id));
        }
        Ok(())
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

    /// Commits authoritative vitals and lifecycle state together. The normal nonlethal vitals
    /// path remains lightweight; this combined path is for death/respawn transitions where a
    /// restart must not observe zero health without its matching persisted lifecycle record.
    pub fn update_player_vitals_and_respawn_state(
        &mut self,
        player_id: u64,
        vitals: PlayerVitals,
        state: PlayerRespawnState,
    ) -> Result<(), PersistenceError> {
        if !vitals.is_valid() {
            return Err(PersistenceError::InvalidPlayerVitals);
        }
        self.ensure_player_exists(player_id)?;
        let lifecycle = if state == PlayerRespawnState::default() {
            None
        } else {
            Some(lifecycle_state_fields(player_id, state)?)
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE players SET health = ?1, max_health = ?2, mana = ?3, max_mana = ?4, capacity = ?5, magic_level = ?6 WHERE id = ?7",
            params![
                i64::from(vitals.health),
                i64::from(vitals.max_health),
                i64::from(vitals.mana),
                i64::from(vitals.max_mana),
                i64::from(vitals.capacity),
                i64::from(vitals.magic_level),
                player_id as i64,
            ],
        )?;
        if let Some((position, death_time)) = lifecycle {
            let death_time = i64::try_from(death_time).map_err(|_| {
                PersistenceError::InvalidLifecycleRecord(
                    "death time does not fit SQLite integer".into(),
                )
            })?;
            transaction.execute(
                "INSERT INTO player_lifecycle (player_id, dead, respawn_x, respawn_y, respawn_z, death_time, loss_applied) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(player_id) DO UPDATE SET dead=excluded.dead, respawn_x=excluded.respawn_x, respawn_y=excluded.respawn_y, respawn_z=excluded.respawn_z, death_time=excluded.death_time, loss_applied=excluded.loss_applied",
                params![
                    player_id as i64,
                    i64::from(position.x),
                    i64::from(position.y),
                    i64::from(position.z),
                    death_time,
                    i64::from(u8::from(state.loss_applied)),
                ],
            )?;
        } else {
            transaction.execute(
                "DELETE FROM player_lifecycle WHERE player_id = ?1",
                params![player_id as i64],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Commits a completed authoritative respawn as one transaction. Position, vitals, and the
    /// cleared lifecycle record must become visible together so a restart cannot observe a
    /// temple position with a stale dead state or defeated vitals.
    pub fn update_player_position_vitals_and_respawn_state(
        &mut self,
        player_id: u64,
        position: Position,
        vitals: PlayerVitals,
        state: PlayerRespawnState,
    ) -> Result<(), PersistenceError> {
        if !vitals.is_valid() {
            return Err(PersistenceError::InvalidPlayerVitals);
        }
        self.ensure_player_exists(player_id)?;
        let lifecycle = if state == PlayerRespawnState::default() {
            None
        } else {
            Some(lifecycle_state_fields(player_id, state)?)
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE players SET x = ?1, y = ?2, z = ?3, health = ?4, max_health = ?5, mana = ?6, max_mana = ?7, capacity = ?8, magic_level = ?9 WHERE id = ?10",
            params![
                i64::from(position.x),
                i64::from(position.y),
                i64::from(position.z),
                i64::from(vitals.health),
                i64::from(vitals.max_health),
                i64::from(vitals.mana),
                i64::from(vitals.max_mana),
                i64::from(vitals.capacity),
                i64::from(vitals.magic_level),
                player_id as i64,
            ],
        )?;
        if let Some((respawn_position, death_time)) = lifecycle {
            let death_time = i64::try_from(death_time).map_err(|_| {
                PersistenceError::InvalidLifecycleRecord(
                    "death time does not fit SQLite integer".into(),
                )
            })?;
            transaction.execute(
                "INSERT INTO player_lifecycle (player_id, dead, respawn_x, respawn_y, respawn_z, death_time, loss_applied) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(player_id) DO UPDATE SET dead=excluded.dead, respawn_x=excluded.respawn_x, respawn_y=excluded.respawn_y, respawn_z=excluded.respawn_z, death_time=excluded.death_time, loss_applied=excluded.loss_applied",
                params![
                    player_id as i64,
                    i64::from(respawn_position.x),
                    i64::from(respawn_position.y),
                    i64::from(respawn_position.z),
                    death_time,
                    i64::from(u8::from(state.loss_applied)),
                ],
            )?;
        } else {
            transaction.execute(
                "DELETE FROM player_lifecycle WHERE player_id = ?1",
                params![player_id as i64],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Commits authoritative vitals and exact progression counters together. This prevents a
    /// restart from observing an updated magic level without the matching remaining spent mana.
    pub fn update_player_vitals_and_progression_attempts(
        &mut self,
        player_id: u64,
        vitals: PlayerVitals,
        attempts: PlayerProgressionAttempts,
    ) -> Result<(), PersistenceError> {
        if !vitals.is_valid() {
            return Err(PersistenceError::InvalidPlayerVitals);
        }
        self.ensure_player_exists(player_id)?;
        let values = attempts
            .all_skill_tries()
            .into_iter()
            .map(sqlite_progression_attempt)
            .collect::<Result<Vec<_>, _>>()?;
        let magic_mana = sqlite_progression_attempt(attempts.magic_mana())?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE players SET health = ?1, max_health = ?2, mana = ?3, max_mana = ?4, capacity = ?5, magic_level = ?6 WHERE id = ?7",
            params![
                i64::from(vitals.health),
                i64::from(vitals.max_health),
                i64::from(vitals.mana),
                i64::from(vitals.max_mana),
                i64::from(vitals.capacity),
                i64::from(vitals.magic_level),
                player_id as i64,
            ],
        )?;
        transaction.execute(
            "INSERT INTO player_progression_attempts (player_id, fist_tries, club_tries, sword_tries, axe_tries, distance_tries, shielding_tries, fishing_tries, magic_mana) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(player_id) DO UPDATE SET fist_tries=excluded.fist_tries, club_tries=excluded.club_tries, sword_tries=excluded.sword_tries, axe_tries=excluded.axe_tries, distance_tries=excluded.distance_tries, shielding_tries=excluded.shielding_tries, fishing_tries=excluded.fishing_tries, magic_mana=excluded.magic_mana",
            params![
                player_id as i64,
                values[0], values[1], values[2], values[3], values[4], values[5], values[6],
                magic_mana,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Persists the authoritative player level and experience without replacing unrelated player
    /// state such as vitals, position, equipment, conditions, or progression attempts.
    pub fn update_player_experience(
        &self,
        player_id: u64,
        level: u32,
        experience: u64,
    ) -> Result<(), PersistenceError> {
        let affected = self.connection.execute(
            "UPDATE players SET level = ?1, experience = ?2 WHERE id = ?3",
            params![level as i64, experience as i64, player_id as i64],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownPlayer(player_id));
        }
        Ok(())
    }

    /// Commits an authoritative level/experience transition and matching vitality state together.
    /// This is used for validated vocation-aware level-ups so a restart cannot observe the new
    /// level without its corresponding current/max health, mana, and capacity gains.
    pub fn update_player_experience_and_vitals(
        &mut self,
        player_id: u64,
        level: u32,
        experience: u64,
        vitals: PlayerVitals,
    ) -> Result<(), PersistenceError> {
        if !vitals.is_valid() {
            return Err(PersistenceError::InvalidPlayerVitals);
        }
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE players SET level = ?1, experience = ?2, health = ?3, max_health = ?4, mana = ?5, max_mana = ?6, capacity = ?7, magic_level = ?8 WHERE id = ?9",
            params![
                i64::from(level),
                experience as i64,
                i64::from(vitals.health),
                i64::from(vitals.max_health),
                i64::from(vitals.mana),
                i64::from(vitals.max_mana),
                i64::from(vitals.capacity),
                i64::from(vitals.magic_level),
                player_id as i64,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Commits the bounded fixed-percent death-loss result as one transaction. The authoritative
    /// core has already recalculated level, visible skills, exact progression attempts, and magic
    /// level. This method only persists that complete accepted snapshot with its matching marked
    /// dead lifecycle state; default loss formulas and client lifecycle delivery remain outside
    /// this database boundary.
    pub fn update_player_fixed_death_loss(
        &mut self,
        snapshot: PlayerFixedDeathLossSnapshot,
    ) -> Result<(), PersistenceError> {
        let PlayerFixedDeathLossSnapshot {
            player_id,
            level,
            experience,
            vitals,
            progression,
            attempts,
            state,
        } = snapshot;
        if !vitals.is_valid() {
            return Err(PersistenceError::InvalidPlayerVitals);
        }
        if !state.dead || !state.loss_applied {
            return Err(PersistenceError::InvalidLifecycleRecord(
                "fixed death loss requires a marked dead lifecycle state".into(),
            ));
        }
        self.ensure_player_exists(player_id)?;
        let (respawn_position, death_time) = lifecycle_state_fields(player_id, state)?;
        let death_time = i64::try_from(death_time).map_err(|_| {
            PersistenceError::InvalidLifecycleRecord(
                "death time does not fit SQLite integer".into(),
            )
        })?;
        let progress_values = progression
            .skills
            .iter()
            .flat_map(|(_, progress)| [i64::from(progress.level), i64::from(progress.percent)])
            .collect::<Vec<_>>();
        let attempt_values = attempts
            .all_skill_tries()
            .into_iter()
            .map(sqlite_progression_attempt)
            .collect::<Result<Vec<_>, _>>()?;
        let magic_mana = sqlite_progression_attempt(attempts.magic_mana())?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE players SET level = ?1, experience = ?2, health = ?3, max_health = ?4, mana = ?5, max_mana = ?6, capacity = ?7, magic_level = ?8, vocation = ?9 WHERE id = ?10",
            params![
                i64::from(level),
                experience as i64,
                i64::from(vitals.health),
                i64::from(vitals.max_health),
                i64::from(vitals.mana),
                i64::from(vitals.max_mana),
                i64::from(vitals.capacity),
                i64::from(vitals.magic_level),
                i64::from(progression.vocation.value()),
                player_id as i64,
            ],
        )?;
        transaction.execute(
            "INSERT INTO player_skills (player_id, fist_level, fist_percent, club_level, club_percent, sword_level, sword_percent, axe_level, axe_percent, distance_level, distance_percent, shielding_level, shielding_percent, fishing_level, fishing_percent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) ON CONFLICT(player_id) DO UPDATE SET fist_level=excluded.fist_level, fist_percent=excluded.fist_percent, club_level=excluded.club_level, club_percent=excluded.club_percent, sword_level=excluded.sword_level, sword_percent=excluded.sword_percent, axe_level=excluded.axe_level, axe_percent=excluded.axe_percent, distance_level=excluded.distance_level, distance_percent=excluded.distance_percent, shielding_level=excluded.shielding_level, shielding_percent=excluded.shielding_percent, fishing_level=excluded.fishing_level, fishing_percent=excluded.fishing_percent",
            params![
                player_id as i64,
                progress_values[0], progress_values[1], progress_values[2], progress_values[3],
                progress_values[4], progress_values[5], progress_values[6], progress_values[7],
                progress_values[8], progress_values[9], progress_values[10], progress_values[11],
                progress_values[12], progress_values[13],
            ],
        )?;
        transaction.execute(
            "INSERT INTO player_progression_attempts (player_id, fist_tries, club_tries, sword_tries, axe_tries, distance_tries, shielding_tries, fishing_tries, magic_mana) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(player_id) DO UPDATE SET fist_tries=excluded.fist_tries, club_tries=excluded.club_tries, sword_tries=excluded.sword_tries, axe_tries=excluded.axe_tries, distance_tries=excluded.distance_tries, shielding_tries=excluded.shielding_tries, fishing_tries=excluded.fishing_tries, magic_mana=excluded.magic_mana",
            params![
                player_id as i64,
                attempt_values[0], attempt_values[1], attempt_values[2], attempt_values[3],
                attempt_values[4], attempt_values[5], attempt_values[6], magic_mana,
            ],
        )?;
        transaction.execute(
            "INSERT INTO player_lifecycle (player_id, dead, respawn_x, respawn_y, respawn_z, death_time, loss_applied) VALUES (?1, 1, ?2, ?3, ?4, ?5, 1) ON CONFLICT(player_id) DO UPDATE SET dead=excluded.dead, respawn_x=excluded.respawn_x, respawn_y=excluded.respawn_y, respawn_z=excluded.respawn_z, death_time=excluded.death_time, loss_applied=excluded.loss_applied",
            params![
                player_id as i64,
                i64::from(respawn_position.x),
                i64::from(respawn_position.y),
                i64::from(respawn_position.z),
                death_time,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces all active bounded conditions for one known player in a single transaction. The
    /// core condition type already validates timing and damage; stored values are validated again
    /// when loaded to defend against malformed database records.
    pub fn replace_player_conditions(
        &mut self,
        player_id: u64,
        conditions: &BTreeMap<PlayerConditionKind, PlayerCondition>,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_conditions WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for condition in conditions.values() {
            transaction.execute(
                "INSERT INTO player_conditions (player_id, kind, interval_seconds, damage, remaining_seconds, elapsed_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    player_id as i64,
                    i64::from(condition.kind.code()),
                    i64::from(condition.interval_seconds),
                    i64::from(condition.damage),
                    i64::from(condition.remaining_seconds),
                    i64::from(condition.elapsed_seconds()),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads only validated active condition schedules, including the deterministic remainder of
    /// the current damage interval so restart hydration resumes the same next tick boundary.
    pub fn player_conditions(
        &self,
        player_id: u64,
    ) -> Result<BTreeMap<PlayerConditionKind, PlayerCondition>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT kind, interval_seconds, damage, remaining_seconds, elapsed_seconds FROM player_conditions WHERE player_id = ?1 ORDER BY kind",
        )?;
        let records = statement
            .query_map(params![player_id as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut conditions = BTreeMap::new();
        for (kind, interval_seconds, damage, remaining_seconds, elapsed_seconds) in records {
            let kind = u8::try_from(kind).map_err(|_| {
                PersistenceError::InvalidConditionRecord("kind does not fit u8".into())
            })?;
            let kind = PlayerConditionKind::from_code(kind).ok_or_else(|| {
                PersistenceError::InvalidConditionRecord(format!("unknown condition kind {kind}"))
            })?;
            let interval_seconds = u16::try_from(interval_seconds).map_err(|_| {
                PersistenceError::InvalidConditionRecord("interval does not fit u16".into())
            })?;
            let damage = u16::try_from(damage).map_err(|_| {
                PersistenceError::InvalidConditionRecord("damage does not fit u16".into())
            })?;
            let remaining_seconds = u16::try_from(remaining_seconds).map_err(|_| {
                PersistenceError::InvalidConditionRecord(
                    "remaining duration does not fit u16".into(),
                )
            })?;
            let elapsed_seconds = u16::try_from(elapsed_seconds).map_err(|_| {
                PersistenceError::InvalidConditionRecord("elapsed interval does not fit u16".into())
            })?;
            let condition = PlayerCondition::from_persisted(
                kind,
                interval_seconds,
                damage,
                remaining_seconds,
                elapsed_seconds,
            )
            .map_err(|error| PersistenceError::InvalidConditionRecord(error.to_string()))?;
            if conditions.insert(kind, condition).is_some() {
                return Err(PersistenceError::InvalidConditionRecord(
                    "duplicate condition kind".into(),
                ));
            }
        }
        Ok(conditions)
    }

    /// Replaces one known player's persisted authoritative lifecycle state. A default living state
    /// removes its row; a dead state must retain the previously validated temple position and
    /// deterministic death tick. Client delivery and automatic timing are intentionally separate.
    pub fn replace_player_respawn_state(
        &mut self,
        player_id: u64,
        state: PlayerRespawnState,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        if state == PlayerRespawnState::default() {
            self.connection.execute(
                "DELETE FROM player_lifecycle WHERE player_id = ?1",
                params![player_id as i64],
            )?;
            return Ok(());
        }
        let (position, death_time) = lifecycle_state_fields(player_id, state)?;
        let death_time = i64::try_from(death_time).map_err(|_| {
            PersistenceError::InvalidLifecycleRecord(
                "death time does not fit SQLite integer".into(),
            )
        })?;
        self.connection.execute(
            "INSERT INTO player_lifecycle (player_id, dead, respawn_x, respawn_y, respawn_z, death_time, loss_applied) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(player_id) DO UPDATE SET dead=excluded.dead, respawn_x=excluded.respawn_x, respawn_y=excluded.respawn_y, respawn_z=excluded.respawn_z, death_time=excluded.death_time, loss_applied=excluded.loss_applied",
            params![
                player_id as i64,
                i64::from(position.x),
                i64::from(position.y),
                i64::from(position.z),
                death_time,
                i64::from(u8::from(state.loss_applied)),
            ],
        )?;
        Ok(())
    }

    /// Loads the persisted authoritative lifecycle state. No row is a default living player;
    /// malformed rows are rejected rather than interpreted as a safe respawn or death state.
    pub fn player_respawn_state(
        &self,
        player_id: u64,
    ) -> Result<PlayerRespawnState, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let record = self
            .connection
            .query_row(
                "SELECT dead, respawn_x, respawn_y, respawn_z, death_time, loss_applied FROM player_lifecycle WHERE player_id = ?1",
                params![player_id as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((dead, x, y, z, death_time, loss_applied)) = record else {
            return Ok(PlayerRespawnState::default());
        };
        if dead != 1 || !(0..=1).contains(&loss_applied) {
            return Err(PersistenceError::InvalidLifecycleRecord(
                "dead and loss-applied values must be encoded as one-bit values".into(),
            ));
        }
        let (x, y, z, death_time) = match (x, y, z, death_time) {
            (Some(x), Some(y), Some(z), Some(death_time)) => (x, y, z, death_time),
            _ => {
                return Err(PersistenceError::InvalidLifecycleRecord(
                    "dead lifecycle state requires respawn coordinates and death time".into(),
                ))
            }
        };
        let position = Position {
            x: u16::try_from(x).map_err(|_| {
                PersistenceError::InvalidLifecycleRecord("respawn x does not fit u16".into())
            })?,
            y: u16::try_from(y).map_err(|_| {
                PersistenceError::InvalidLifecycleRecord("respawn y does not fit u16".into())
            })?,
            z: u8::try_from(z).map_err(|_| {
                PersistenceError::InvalidLifecycleRecord("respawn z does not fit u8".into())
            })?,
        };
        let death_time = u64::try_from(death_time).map_err(|_| {
            PersistenceError::InvalidLifecycleRecord("death time must be non-negative".into())
        })?;
        Ok(PlayerRespawnState {
            dead: true,
            respawn_at: Some(position),
            death_time: Some(death_time),
            loss_applied: loss_applied == 1,
        })
    }

    /// Replaces a player's vocation identity and all typed skill values atomically. The caller
    /// supplies validated core types; SQLite rows are still validated again on every reload.
    pub fn replace_player_progression(
        &mut self,
        player_id: u64,
        progression: PlayerProgression,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE players SET vocation = ?1 WHERE id = ?2",
            params![i64::from(progression.vocation.value()), player_id as i64],
        )?;
        let values = progression
            .skills
            .iter()
            .flat_map(|(_, progress)| [i64::from(progress.level), i64::from(progress.percent)]);
        let values = values.collect::<Vec<_>>();
        transaction.execute(
            "INSERT INTO player_skills (player_id, fist_level, fist_percent, club_level, club_percent, sword_level, sword_percent, axe_level, axe_percent, distance_level, distance_percent, shielding_level, shielding_percent, fishing_level, fishing_percent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) ON CONFLICT(player_id) DO UPDATE SET fist_level=excluded.fist_level, fist_percent=excluded.fist_percent, club_level=excluded.club_level, club_percent=excluded.club_percent, sword_level=excluded.sword_level, sword_percent=excluded.sword_percent, axe_level=excluded.axe_level, axe_percent=excluded.axe_percent, distance_level=excluded.distance_level, distance_percent=excluded.distance_percent, shielding_level=excluded.shielding_level, shielding_percent=excluded.shielding_percent, fishing_level=excluded.fishing_level, fishing_percent=excluded.fishing_percent",
            params![
                player_id as i64,
                values[0], values[1], values[2], values[3], values[4], values[5], values[6],
                values[7], values[8], values[9], values[10], values[11], values[12], values[13]
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces visible progression and exact counters in one transaction. This prevents a
    /// restart from observing an advanced skill percentage without its matching remaining tries.
    pub fn replace_player_progression_and_attempts(
        &mut self,
        player_id: u64,
        progression: PlayerProgression,
        attempts: PlayerProgressionAttempts,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let progress_values = progression
            .skills
            .iter()
            .flat_map(|(_, progress)| [i64::from(progress.level), i64::from(progress.percent)])
            .collect::<Vec<_>>();
        let attempt_values = attempts
            .all_skill_tries()
            .into_iter()
            .map(sqlite_progression_attempt)
            .collect::<Result<Vec<_>, _>>()?;
        let magic_mana = sqlite_progression_attempt(attempts.magic_mana())?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE players SET vocation = ?1 WHERE id = ?2",
            params![i64::from(progression.vocation.value()), player_id as i64],
        )?;
        transaction.execute(
            "INSERT INTO player_skills (player_id, fist_level, fist_percent, club_level, club_percent, sword_level, sword_percent, axe_level, axe_percent, distance_level, distance_percent, shielding_level, shielding_percent, fishing_level, fishing_percent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) ON CONFLICT(player_id) DO UPDATE SET fist_level=excluded.fist_level, fist_percent=excluded.fist_percent, club_level=excluded.club_level, club_percent=excluded.club_percent, sword_level=excluded.sword_level, sword_percent=excluded.sword_percent, axe_level=excluded.axe_level, axe_percent=excluded.axe_percent, distance_level=excluded.distance_level, distance_percent=excluded.distance_percent, shielding_level=excluded.shielding_level, shielding_percent=excluded.shielding_percent, fishing_level=excluded.fishing_level, fishing_percent=excluded.fishing_percent",
            params![
                player_id as i64,
                progress_values[0], progress_values[1], progress_values[2], progress_values[3],
                progress_values[4], progress_values[5], progress_values[6], progress_values[7],
                progress_values[8], progress_values[9], progress_values[10], progress_values[11],
                progress_values[12], progress_values[13],
            ],
        )?;
        transaction.execute(
            "INSERT INTO player_progression_attempts (player_id, fist_tries, club_tries, sword_tries, axe_tries, distance_tries, shielding_tries, fishing_tries, magic_mana) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(player_id) DO UPDATE SET fist_tries=excluded.fist_tries, club_tries=excluded.club_tries, sword_tries=excluded.sword_tries, axe_tries=excluded.axe_tries, distance_tries=excluded.distance_tries, shielding_tries=excluded.shielding_tries, fishing_tries=excluded.fishing_tries, magic_mana=excluded.magic_mana",
            params![
                player_id as i64,
                attempt_values[0], attempt_values[1], attempt_values[2], attempt_values[3],
                attempt_values[4], attempt_values[5], attempt_values[6], magic_mana,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads a player's progression through validated fixed-width database fields. Worlds that
    /// predate schema-v5 have no skill row after migration and are represented by safe defaults.
    pub fn player_progression(
        &self,
        player_id: u64,
    ) -> Result<PlayerProgression, PersistenceError> {
        let vocation = self
            .connection
            .query_row(
                "SELECT vocation FROM players WHERE id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(PersistenceError::UnknownPlayer(player_id))?;
        let vocation = u16::try_from(vocation).map_err(|_| {
            PersistenceError::InvalidProgressionRecord("vocation ID does not fit u16".into())
        })?;
        let record = self.connection.query_row(
            "SELECT fist_level, fist_percent, club_level, club_percent, sword_level, sword_percent, axe_level, axe_percent, distance_level, distance_percent, shielding_level, shielding_percent, fishing_level, fishing_percent FROM player_skills WHERE player_id = ?1",
            params![player_id as i64],
            |row| Ok([
                row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?, row.get::<_, i64>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?, row.get::<_, i64>(9)?, row.get::<_, i64>(10)?, row.get::<_, i64>(11)?,
                row.get::<_, i64>(12)?, row.get::<_, i64>(13)?,
            ]),
        ).optional()?;
        Ok(PlayerProgression {
            vocation: VocationId::new(vocation),
            skills: record
                .map(player_skills_from_record)
                .transpose()?
                .unwrap_or_default(),
        })
    }

    /// Replaces the exact invisible counters that back client-visible skill and magic progress.
    /// Counter ranges are checked before SQLite receives them and every stored value is checked
    /// again during reload.
    pub fn replace_player_progression_attempts(
        &mut self,
        player_id: u64,
        attempts: PlayerProgressionAttempts,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let values = attempts
            .all_skill_tries()
            .into_iter()
            .map(sqlite_progression_attempt)
            .collect::<Result<Vec<_>, _>>()?;
        let magic_mana = sqlite_progression_attempt(attempts.magic_mana())?;
        self.connection.execute(
            "INSERT INTO player_progression_attempts (player_id, fist_tries, club_tries, sword_tries, axe_tries, distance_tries, shielding_tries, fishing_tries, magic_mana) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(player_id) DO UPDATE SET fist_tries=excluded.fist_tries, club_tries=excluded.club_tries, sword_tries=excluded.sword_tries, axe_tries=excluded.axe_tries, distance_tries=excluded.distance_tries, shielding_tries=excluded.shielding_tries, fishing_tries=excluded.fishing_tries, magic_mana=excluded.magic_mana",
            params![
                player_id as i64,
                values[0], values[1], values[2], values[3], values[4], values[5], values[6],
                magic_mana,
            ],
        )?;
        Ok(())
    }

    /// Loads exact progression counters, supplying safe zeros for worlds migrated from a schema
    /// that predates their persistence. Formula-threshold validation belongs to the runtime rules.
    pub fn player_progression_attempts(
        &self,
        player_id: u64,
    ) -> Result<PlayerProgressionAttempts, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let record = self.connection.query_row(
            "SELECT fist_tries, club_tries, sword_tries, axe_tries, distance_tries, shielding_tries, fishing_tries, magic_mana FROM player_progression_attempts WHERE player_id = ?1",
            params![player_id as i64],
            |row| Ok([
                row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?, row.get::<_, i64>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?,
            ]),
        ).optional()?;
        record
            .map(progression_attempts_from_record)
            .transpose()?
            .map_or_else(|| Ok(PlayerProgressionAttempts::default()), Ok)
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

    /// Replaces a player's bounded non-recursive containers and their ordered contents in one
    /// SQLite transaction. Item-use, nested containers, depot, and inbox semantics remain outside
    /// this storage slice.
    pub fn replace_player_containers(
        &mut self,
        player_id: u64,
        containers: &PlayerContainers,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_container_items WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM player_containers WHERE player_id = ?1",
            params![player_id as i64],
        )?;
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
        transaction.commit()?;
        Ok(())
    }

    /// Replaces both fixed equipment and bounded owned containers in one SQLite transaction.
    /// This is the persistence boundary for authoritative transfers between those two collections;
    /// map-ground items, nested containers, and client inventory delivery remain separate.
    pub fn replace_player_inventory(
        &mut self,
        player_id: u64,
        equipment: &PlayerEquipment,
        containers: &PlayerContainers,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
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
        transaction.commit()?;
        Ok(())
    }

    /// Loads player-owned containers in client-window order. Raw database values are never
    /// trusted: invalid IDs, item fields, names, capacity, parent flags, or sparse item slots are
    /// rejected before they can be admitted into authoritative world state.
    pub fn player_containers(&self, player_id: u64) -> Result<PlayerContainers, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT container_id, server_id, count, name, has_parent, capacity FROM player_containers WHERE player_id = ?1 ORDER BY container_id",
        )?;
        let records = statement
            .query_map(params![player_id as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut containers = PlayerContainers::default();
        for (container_id, server_id, count, name, has_parent, capacity) in records {
            let container_id = u8::try_from(container_id).map_err(|_| {
                PersistenceError::InvalidContainerRecord(
                    "container ID does not fit an unsigned byte".into(),
                )
            })?;
            let has_parent = match has_parent {
                0 => false,
                1 => true,
                _ => {
                    return Err(PersistenceError::InvalidContainerRecord(
                        "parent flag must be zero or one".into(),
                    ));
                }
            };
            let capacity = u16::try_from(capacity).map_err(|_| {
                PersistenceError::InvalidContainerRecord("capacity does not fit u16".into())
            })?;
            let mut container = PlayerContainer::new(
                container_id,
                container_item_from_record(server_id, count)?,
                name,
                has_parent,
                capacity,
            )
            .map_err(|error| PersistenceError::InvalidContainerRecord(error.to_string()))?;
            let mut item_statement = self.connection.prepare(
                "SELECT slot, server_id, count, action_id, unique_id FROM player_container_items WHERE player_id = ?1 AND container_id = ?2 ORDER BY slot",
            )?;
            let item_records = item_statement
                .query_map(params![player_id as i64, i64::from(container_id)], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for (expected_slot, (slot, server_id, count, action_id, unique_id)) in
                item_records.into_iter().enumerate()
            {
                let slot = usize::try_from(slot).map_err(|_| {
                    PersistenceError::InvalidContainerRecord("item slot does not fit usize".into())
                })?;
                if slot != expected_slot {
                    return Err(PersistenceError::InvalidContainerRecord(
                        "container item slots must be contiguous from zero".into(),
                    ));
                }
                let mut item = container_item_from_record(server_id, count)?;
                item.action_id = optional_u16_container_attribute(action_id, "action ID")?;
                item.unique_id = optional_u16_container_attribute(unique_id, "unique ID")?;
                container
                    .items
                    .insert(item)
                    .map_err(|error| PersistenceError::InvalidContainerRecord(error.to_string()))?;
            }
            containers
                .insert(container)
                .map_err(|error| PersistenceError::InvalidContainerRecord(error.to_string()))?;
        }
        Ok(containers)
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

    /// Replaces the complete static-creature runtime snapshot atomically. Callers must supply
    /// only known static spawn IDs; identity validation remains in the authoritative world when
    /// this storage record is applied.
    pub fn replace_static_creature_runtime(
        &mut self,
        records: &[StaticCreatureRuntimeRecord],
    ) -> Result<(), PersistenceError> {
        let mut seen = BTreeMap::new();
        for record in records {
            if record.health_percent > 100 {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "health percent must be at most 100".into(),
                ));
            }
            if record.active && record.reactivation_remaining_seconds.is_some() {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "active creatures cannot carry a reactivation delay".into(),
                ));
            }
            if seen.insert(record.creature_id, ()).is_some() {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "duplicate static creature ID".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM static_creature_runtime", [])?;
        for record in records {
            transaction.execute(
                "INSERT INTO static_creature_runtime (creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    i64::from(record.creature_id),
                    i64::from(record.position.x),
                    i64::from(record.position.y),
                    i64::from(record.position.z),
                    i64::from(u8::from(record.active)),
                    i64::from(record.health_percent),
                    record.reactivation_remaining_seconds.map(i64::from),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads the complete bounded static-creature runtime snapshot. Rows are independently
    /// validated so malformed external SQLite edits never enter the authoritative world.
    pub fn static_creature_runtime(
        &self,
    ) -> Result<Vec<StaticCreatureRuntimeRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds FROM static_creature_runtime ORDER BY creature_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds) =
                row?;
            let creature_id = u32::try_from(creature_id).map_err(|_| {
                PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "creature ID does not fit u32".into(),
                )
            })?;
            let position = Position {
                x: u16::try_from(x).map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "x does not fit u16".into(),
                    )
                })?,
                y: u16::try_from(y).map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "y does not fit u16".into(),
                    )
                })?,
                z: u8::try_from(z).map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord("z does not fit u8".into())
                })?,
            };
            let active = match active {
                0 => false,
                1 => true,
                _ => {
                    return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "active flag must be zero or one".into(),
                    ))
                }
            };
            let health_percent = u8::try_from(health_percent).map_err(|_| {
                PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "health percent does not fit u8".into(),
                )
            })?;
            if health_percent > 100 {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "health percent must be at most 100".into(),
                ));
            }
            let reactivation_remaining_seconds = reactivation_remaining_seconds
                .map(|remaining_seconds| {
                    u32::try_from(remaining_seconds).map_err(|_| {
                        PersistenceError::InvalidStaticCreatureRuntimeRecord(
                            "reactivation delay does not fit u32".into(),
                        )
                    })
                })
                .transpose()?;
            if active && reactivation_remaining_seconds.is_some() {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "active creatures cannot carry a reactivation delay".into(),
                ));
            }
            records.push(StaticCreatureRuntimeRecord {
                creature_id,
                position,
                active,
                health_percent,
                reactivation_remaining_seconds,
            });
        }
        Ok(records)
    }

    fn migrate(&mut self) -> Result<(), PersistenceError> {
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
    pub progression: PlayerProgression,
    pub progression_attempts: PlayerProgressionAttempts,
    pub vitals: PlayerVitals,
    pub position: Position,
    /// Imported map town identifier, or zero when no town has been assigned.
    pub town_id: u32,
    /// Persisted appearance values. A zero `look_type` means the host must use its configured
    /// profile fallback because the record predates schema-v11 outfit storage.
    pub outfit: PlayerOutfit,
    /// Persisted authoritative lifecycle state. Client death and respawn delivery remain outside
    /// the storage layer.
    pub respawn_state: PlayerRespawnState,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerOutfit {
    pub look_type: u8,
    pub head: u8,
    pub body: u8,
    pub legs: u8,
    pub feet: u8,
}

impl PlayerOutfit {
    fn is_concrete(self) -> bool {
        self.look_type != 0
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn lifecycle_state_fields(
    player_id: u64,
    state: PlayerRespawnState,
) -> Result<(Position, u64), PersistenceError> {
    if !state.dead {
        return Err(PersistenceError::InvalidLifecycleRecord(format!(
            "player {player_id} has non-default living lifecycle state"
        )));
    }
    let position = state.respawn_at.ok_or_else(|| {
        PersistenceError::InvalidLifecycleRecord(format!(
            "player {player_id} dead lifecycle state has no respawn position"
        ))
    })?;
    let death_time = state.death_time.ok_or_else(|| {
        PersistenceError::InvalidLifecycleRecord(format!(
            "player {player_id} dead lifecycle state has no death time"
        ))
    })?;
    Ok((position, death_time))
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

fn container_item_from_record(
    server_id: i64,
    count: i64,
) -> Result<ItemInstance, PersistenceError> {
    let server_id = u16::try_from(server_id).map_err(|_| {
        PersistenceError::InvalidContainerRecord("server item ID does not fit u16".into())
    })?;
    let count = u16::try_from(count).map_err(|_| {
        PersistenceError::InvalidContainerRecord("item count does not fit u16".into())
    })?;
    ItemInstance::new(server_id, count)
        .map_err(|error| PersistenceError::InvalidContainerRecord(error.to_string()))
}

fn optional_u16_container_attribute(
    value: Option<i64>,
    label: &str,
) -> Result<Option<u16>, PersistenceError> {
    value
        .map(|value| {
            u16::try_from(value).map_err(|_| {
                PersistenceError::InvalidContainerRecord(format!("{label} does not fit u16"))
            })
        })
        .transpose()
}

fn player_skills_from_record(record: [i64; 14]) -> Result<PlayerSkills, PersistenceError> {
    let mut skills = PlayerSkills::default();
    for skill in PlayerSkill::ALL {
        let offset = usize::from(skill.code()) * 2;
        let level = u16::try_from(record[offset]).map_err(|_| {
            PersistenceError::InvalidProgressionRecord(format!(
                "{} skill level does not fit u16",
                skill.code()
            ))
        })?;
        let percent = u8::try_from(record[offset + 1]).map_err(|_| {
            PersistenceError::InvalidProgressionRecord(format!(
                "{} skill percent does not fit u8",
                skill.code()
            ))
        })?;
        let progress = SkillProgress::new(level, percent)
            .map_err(|error| PersistenceError::InvalidProgressionRecord(error.to_string()))?;
        skills.set(skill, progress);
    }
    Ok(skills)
}

fn sqlite_progression_attempt(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| {
        PersistenceError::InvalidProgressionAttemptRecord(
            "progression counter exceeds SQLite signed integer range".into(),
        )
    })
}

fn progression_attempts_from_record(
    record: [i64; 8],
) -> Result<PlayerProgressionAttempts, PersistenceError> {
    let values = record
        .into_iter()
        .map(|value| {
            u64::try_from(value).map_err(|_| {
                PersistenceError::InvalidProgressionAttemptRecord(
                    "progression counters must be nonnegative".into(),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PlayerProgressionAttempts::new(
        [
            values[0], values[1], values[2], values[3], values[4], values[5], values[6],
        ],
        values[7],
    ))
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    PasswordHash(String),
    InvalidPlayerName,
    InvalidPlayerVitals,
    InvalidPlayerOutfit,
    InvalidEquipmentRecord(String),
    InvalidContainerRecord(String),
    InvalidConditionRecord(String),
    InvalidProgressionRecord(String),
    InvalidProgressionAttemptRecord(String),
    InvalidLifecycleRecord(String),
    InvalidStaticCreatureRuntimeRecord(String),
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
        assert_eq!(character.progression, PlayerProgression::default());
        assert_eq!(
            character.progression_attempts,
            PlayerProgressionAttempts::default()
        );
        assert_eq!(character.town_id, 0);
        assert_eq!(character.outfit, PlayerOutfit::default());
        assert!(database.player_equipment(7).unwrap().is_empty());
        assert!(database.player_conditions(7).unwrap().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_player_town_assignment_with_a_safe_default() {
        let path = temporary_path("town-assignment");
        let database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("admin", "hash").unwrap();
        let character = database
            .create_player_for_account(account_id as u32, "Knight")
            .unwrap();
        assert_eq!(character.town_id, 0);

        database.update_player_town(character.id, 42).unwrap();
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].town_id,
            42
        );
        assert!(matches!(
            database.update_player_town(999, 1),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_player_outfit_with_a_safe_migration_default() {
        let path = temporary_path("outfit");
        let database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("admin", "hash").unwrap();
        let character = database
            .create_player_for_account(account_id as u32, "Knight")
            .unwrap();
        assert_eq!(character.outfit, PlayerOutfit::default());

        let outfit = PlayerOutfit {
            look_type: 128,
            head: 1,
            body: 2,
            legs: 3,
            feet: 4,
        };
        database.update_player_outfit(character.id, outfit).unwrap();
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].outfit,
            outfit
        );
        assert!(matches!(
            database.update_player_outfit(character.id, PlayerOutfit::default()),
            Err(PersistenceError::InvalidPlayerOutfit)
        ));
        assert!(matches!(
            database.update_player_outfit(999, outfit),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_and_validates_static_creature_runtime_snapshot_transactionally() {
        let path = temporary_path("static-creature-runtime");
        let mut database = EngineDatabase::open(&path).unwrap();
        let records = [
            StaticCreatureRuntimeRecord {
                creature_id: 0x1000_0001,
                position: Position {
                    x: 101,
                    y: 102,
                    z: 7,
                },
                active: true,
                health_percent: 100,
                reactivation_remaining_seconds: None,
            },
            StaticCreatureRuntimeRecord {
                creature_id: 0x1000_0002,
                position: Position {
                    x: 103,
                    y: 104,
                    z: 6,
                },
                active: false,
                health_percent: 0,
                reactivation_remaining_seconds: Some(42),
            },
        ];
        database.replace_static_creature_runtime(&records).unwrap();
        assert_eq!(database.static_creature_runtime().unwrap(), records);

        let invalid_active_delay = [StaticCreatureRuntimeRecord {
            reactivation_remaining_seconds: Some(1),
            ..records[0]
        }];
        assert!(matches!(
            database.replace_static_creature_runtime(&invalid_active_delay),
            Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(_))
        ));
        assert_eq!(database.static_creature_runtime().unwrap(), records);

        let invalid_health = [StaticCreatureRuntimeRecord {
            health_percent: 101,
            ..records[0]
        }];
        assert!(matches!(
            database.replace_static_creature_runtime(&invalid_health),
            Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(_))
        ));
        assert_eq!(database.static_creature_runtime().unwrap(), records);

        let duplicate_ids = [records[0], records[0]];
        assert!(matches!(
            database.replace_static_creature_runtime(&duplicate_ids),
            Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(_))
        ));
        assert_eq!(database.static_creature_runtime().unwrap(), records);

        database
            .connection
            .execute("DELETE FROM static_creature_runtime", [])
            .unwrap();
        database
            .connection
            .execute(
                "INSERT INTO static_creature_runtime (creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![0x1000_0001_i64, 101_i64, 102_i64, 7_i64, 1_i64, 100_i64, 3_i64],
            )
            .unwrap();
        assert!(matches!(
            database.static_creature_runtime(),
            Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(_))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_and_validates_player_conditions_transactionally() {
        let path = temporary_path("conditions");
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

        let poison =
            PlayerCondition::from_persisted(PlayerConditionKind::Poison, 2, 7, 10, 1).unwrap();
        let burning = PlayerCondition::new(PlayerConditionKind::Burning, 3, 4, 9).unwrap();
        let conditions = BTreeMap::from([
            (PlayerConditionKind::Poison, poison),
            (PlayerConditionKind::Burning, burning),
        ]);
        database.replace_player_conditions(7, &conditions).unwrap();
        assert_eq!(database.player_conditions(7).unwrap(), conditions);

        database
            .replace_player_conditions(7, &BTreeMap::new())
            .unwrap();
        assert!(database.player_conditions(7).unwrap().is_empty());

        database
            .connection
            .execute(
                "INSERT INTO player_conditions (player_id, kind, interval_seconds, damage, remaining_seconds) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![7_i64, 99_i64, 1_i64, 1_i64, 1_i64],
            )
            .unwrap();
        assert!(matches!(
            database.player_conditions(7),
            Err(PersistenceError::InvalidConditionRecord(_))
        ));
        assert!(matches!(
            database.player_conditions(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_and_validates_player_respawn_state_transactionally() {
        let path = temporary_path("lifecycle");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("admin", "hash").unwrap();
        let character = database
            .create_player_for_account(account_id as u32, "Knight")
            .unwrap();
        assert_eq!(
            database.player_respawn_state(character.id).unwrap(),
            PlayerRespawnState::default()
        );

        let state = PlayerRespawnState {
            dead: true,
            respawn_at: Some(Position {
                x: 110,
                y: 120,
                z: 7,
            }),
            death_time: Some(42),
            loss_applied: true,
        };
        database
            .replace_player_respawn_state(character.id, state)
            .unwrap();
        assert_eq!(database.player_respawn_state(character.id).unwrap(), state);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].respawn_state,
            state
        );

        let defeated_vitals = PlayerVitals {
            health: 0,
            max_health: 150,
            mana: 50,
            max_mana: 50,
            capacity: 40_000,
            magic_level: 0,
        };
        database
            .update_player_vitals_and_respawn_state(character.id, defeated_vitals, state)
            .unwrap();
        let loaded = database
            .characters_for_account(account_id)
            .unwrap()
            .remove(0);
        assert_eq!(loaded.vitals, defeated_vitals);
        assert_eq!(loaded.respawn_state, state);

        let restored_position = Position {
            x: 110,
            y: 120,
            z: 7,
        };
        let restored_vitals = PlayerVitals {
            health: 150,
            max_health: 150,
            mana: 50,
            max_mana: 50,
            capacity: 40_000,
            magic_level: 0,
        };
        database
            .update_player_position_vitals_and_respawn_state(
                character.id,
                restored_position,
                restored_vitals,
                PlayerRespawnState::default(),
            )
            .unwrap();
        let restored = database
            .characters_for_account(account_id)
            .unwrap()
            .remove(0);
        assert_eq!(restored.position, restored_position);
        assert_eq!(restored.vitals, restored_vitals);
        assert_eq!(restored.respawn_state, PlayerRespawnState::default());

        database
            .replace_player_respawn_state(character.id, PlayerRespawnState::default())
            .unwrap();
        assert_eq!(
            database.player_respawn_state(character.id).unwrap(),
            PlayerRespawnState::default()
        );

        database
            .connection
            .execute(
                "INSERT INTO player_lifecycle (player_id, dead, respawn_x, respawn_y, respawn_z, death_time, loss_applied) VALUES (?1, ?2, NULL, NULL, NULL, NULL, ?3)",
                params![character.id as i64, 1_i64, 0_i64],
            )
            .unwrap();
        assert!(matches!(
            database.player_respawn_state(character.id),
            Err(PersistenceError::InvalidLifecycleRecord(_))
        ));
        assert!(matches!(
            database.player_respawn_state(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_and_validates_player_progression_attempts_transactionally() {
        let path = temporary_path("progression-attempts");
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

        let attempts = PlayerProgressionAttempts::new([1, 2, 3, 4, 5, 6, 7], 8);
        database
            .replace_player_progression_attempts(7, attempts)
            .unwrap();
        assert_eq!(database.player_progression_attempts(7).unwrap(), attempts);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].progression_attempts,
            attempts
        );

        database
            .replace_player_progression_attempts(7, PlayerProgressionAttempts::default())
            .unwrap();
        assert_eq!(
            database.player_progression_attempts(7).unwrap(),
            PlayerProgressionAttempts::default()
        );
        database
            .connection
            .execute(
                "UPDATE player_progression_attempts SET sword_tries = -1 WHERE player_id = ?1",
                params![7_i64],
            )
            .unwrap();
        assert!(matches!(
            database.player_progression_attempts(7),
            Err(PersistenceError::InvalidProgressionAttemptRecord(_))
        ));
        assert!(matches!(
            database.player_progression_attempts(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn atomically_persists_player_vitals_and_progression_attempts() {
        let path = temporary_path("vitals-progression-attempts");
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

        let vitals = PlayerVitals {
            health: 175,
            max_health: 200,
            mana: 80,
            max_mana: 120,
            capacity: 45_000,
            magic_level: 3,
        };
        let attempts = PlayerProgressionAttempts::new([10, 20, 30, 40, 50, 60, 70], 800);
        database
            .update_player_vitals_and_progression_attempts(7, vitals, attempts)
            .unwrap();
        let loaded = database.player_by_id(7).unwrap();
        assert_eq!(loaded.vitals, vitals);
        assert_eq!(loaded.progression_attempts, attempts);

        let rejected_vitals = PlayerVitals {
            magic_level: 4,
            ..vitals
        };
        assert!(matches!(
            database.update_player_vitals_and_progression_attempts(
                7,
                rejected_vitals,
                PlayerProgressionAttempts::new([u64::MAX; 7], 0),
            ),
            Err(PersistenceError::InvalidProgressionAttemptRecord(_))
        ));
        let loaded = database.player_by_id(7).unwrap();
        assert_eq!(loaded.vitals, vitals);
        assert_eq!(loaded.progression_attempts, attempts);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn atomically_persists_player_progression_and_attempts() {
        let path = temporary_path("progression-attempts-transaction");
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

        let mut progression = PlayerProgression::default();
        progression
            .skills
            .set(PlayerSkill::Sword, SkillProgress::new(11, 0).unwrap());
        let attempts = PlayerProgressionAttempts::new([10, 20, 0, 40, 50, 60, 70], 800);
        database
            .replace_player_progression_and_attempts(7, progression, attempts)
            .unwrap();
        let loaded = database.player_by_id(7).unwrap();
        assert_eq!(loaded.progression, progression);
        assert_eq!(loaded.progression_attempts, attempts);

        let mut rejected_progression = progression;
        rejected_progression
            .skills
            .set(PlayerSkill::Sword, SkillProgress::new(12, 0).unwrap());
        assert!(matches!(
            database.replace_player_progression_and_attempts(
                7,
                rejected_progression,
                PlayerProgressionAttempts::new([u64::MAX; 7], 0),
            ),
            Err(PersistenceError::InvalidProgressionAttemptRecord(_))
        ));
        let loaded = database.player_by_id(7).unwrap();
        assert_eq!(loaded.progression, progression);
        assert_eq!(loaded.progression_attempts, attempts);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_and_validates_player_progression_transactionally() {
        let path = temporary_path("progression");
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

        let mut skills = PlayerSkills::default();
        skills.set(PlayerSkill::Sword, SkillProgress::new(65, 42).unwrap());
        skills.set(PlayerSkill::Shielding, SkillProgress::new(61, 99).unwrap());
        let progression = PlayerProgression {
            vocation: VocationId::new(4),
            skills,
        };
        database.replace_player_progression(7, progression).unwrap();
        assert_eq!(database.player_progression(7).unwrap(), progression);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].progression,
            progression
        );

        database
            .connection
            .execute(
                "UPDATE player_skills SET sword_percent = 255 WHERE player_id = ?1",
                params![7_i64],
            )
            .unwrap();
        assert!(matches!(
            database.player_progression(7),
            Err(PersistenceError::InvalidProgressionRecord(_))
        ));
        assert!(matches!(
            database.player_progression(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_a_player_and_event() {
        let path = temporary_path("player");
        let mut database = EngineDatabase::open(&path).unwrap();
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
        database.update_player_experience(1, 12, 14_400).unwrap();
        let character = database
            .characters_for_account(account_id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(character.level, 12);
        assert_eq!(character.experience, 14_400);
        let advanced_vitals = PlayerVitals {
            health: 165,
            max_health: 165,
            mana: 55,
            max_mana: 55,
            capacity: 40_025,
            magic_level: 0,
        };
        database
            .update_player_experience_and_vitals(1, 13, 16_900, advanced_vitals)
            .unwrap();
        let advanced = database.player_by_id(1).unwrap();
        assert_eq!(advanced.level, 13);
        assert_eq!(advanced.experience, 16_900);
        assert_eq!(advanced.vitals, advanced_vitals);
        assert_eq!(database.player_by_id(1).unwrap().name, "Knight");
        assert!(matches!(
            database.update_player_experience(99, 1, 0),
            Err(PersistenceError::UnknownPlayer(99))
        ));
        assert!(matches!(
            database.player_by_id(99),
            Err(PersistenceError::UnknownPlayer(99))
        ));
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
        let mut backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 2)
                .unwrap();
        backpack.items.insert(sword.clone()).unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        database
            .replace_player_inventory(7, &replacement, &containers)
            .unwrap();
        assert_eq!(database.player_equipment(7).unwrap(), replacement);
        assert_eq!(database.player_containers(7).unwrap(), containers);
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
    fn persists_and_replaces_player_containers_transactionally() {
        let path = temporary_path("containers");
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

        let mut backpack =
            PlayerContainer::new(0, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 4)
                .unwrap();
        let mut gold = ItemInstance::new(3031, 25).unwrap();
        gold.unique_id = Some(7_000);
        backpack.items.insert(gold).unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(7, &containers).unwrap();
        assert_eq!(database.player_containers(7).unwrap(), containers);

        database
            .replace_player_containers(7, &PlayerContainers::default())
            .unwrap();
        assert!(database.player_containers(7).unwrap().is_empty());

        database
            .connection
            .execute(
                "INSERT INTO player_containers (player_id, container_id, server_id, count, name, has_parent, capacity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![7_i64, 0_i64, 1988_i64, 1_i64, "Broken", 2_i64, 4_i64],
            )
            .unwrap();
        assert!(matches!(
            database.player_containers(7),
            Err(PersistenceError::InvalidContainerRecord(_))
        ));
        assert!(matches!(
            database.player_containers(999),
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
