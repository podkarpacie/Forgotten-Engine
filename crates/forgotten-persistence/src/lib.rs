//! SQLite persistence, secure account authentication, and backup primitives.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use forgotten_core::{
    classic_experience_for_level, EquipmentSlot, ItemInstance, Player, PlayerCondition,
    PlayerConditionKind, PlayerContainer, PlayerContainers, PlayerEquipment, PlayerProgression,
    PlayerProgressionAttempts, PlayerRespawnState, PlayerSkill, PlayerSkills, Position,
    SkillProgress, VocationId, WorldMapItemSourceIdentity, WorldMapSourceRevision,
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
const SCHEMA_VERSION_MAP_ITEM_JOURNAL: i64 = 14;
const SCHEMA_VERSION_STATIC_CREATURE_DAMAGE_SEQUENCE: i64 = 15;
const SCHEMA_VERSION_STATIC_CREATURE_MELEE_COOLDOWN: i64 = 16;
const SCHEMA_VERSION_ACCOUNT_VIP_ENTRIES: i64 = 17;
const SCHEMA_VERSION_GUILDS: i64 = 18;
const SCHEMA_VERSION_GUILD_INVITATIONS: i64 = 19;
const SCHEMA_VERSION_PLAYER_BANK_BALANCE: i64 = 20;
const SCHEMA_VERSION_PLAYER_DEPOTS: i64 = 21;
const SCHEMA_VERSION_PLAYER_INBOX: i64 = 22;
const SCHEMA_VERSION_HOUSE_OWNERSHIP: i64 = 23;
pub const LATEST_SCHEMA_VERSION: i64 = SCHEMA_VERSION_HOUSE_OWNERSHIP;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_PROVISIONED_PLAYER_LEVEL: u32 = 8;
pub const MAX_VIP_DESCRIPTION_BYTES: usize = 128;
pub const MAX_GUILD_NAME_BYTES: usize = 64;
pub const MAX_GUILD_MOTD_BYTES: usize = 255;
pub const MAX_GUILD_NICK_BYTES: usize = 15;
pub const MAX_GUILD_RANK_NAME_BYTES: usize = 64;
pub const MAX_GUILD_RANKS_PER_GUILD: usize = 20;
pub const MAX_GUILD_INVITATIONS_PER_GUILD: usize = 20;
pub const MAX_PLAYER_BANK_BALANCE: u64 = i64::MAX as u64;
/// The public TFS reference maps one depot box for each index in the inclusive range 0 through 19.
pub const MAX_PLAYER_DEPOT_ID: u8 = 19;
pub const MAX_PLAYER_DEPOTS_PER_PLAYER: usize = 20;
pub const MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS: usize = 1_000;
pub const MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS: usize = 30;

pub struct EngineDatabase {
    connection: Connection,
    path: PathBuf,
}

/// The durable subset of known static-creature runtime state. It includes a bounded restart-relative
/// reactivation delay, direct-melee cooldown remainder, and deterministic direct-melee selection
/// sequence, not an autonomous scheduler. Spawn definitions, appearance, targets, AI cadence,
/// combat formulas, loot, and scripts remain content/runtime concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticCreatureRuntimeRecord {
    pub creature_id: u32,
    pub position: Position,
    pub active: bool,
    pub health_percent: u8,
    pub reactivation_remaining_seconds: Option<u32>,
    pub direct_melee_cooldown_remaining_ticks: Option<u32>,
    pub direct_melee_damage_sequence: u64,
}

/// Revision-bound record of top-level source-map items removed by future authoritative runtime
/// transitions. An incompatible map revision must be treated as a caller-visible recovery state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapItemRemovalJournal {
    pub map_revision: WorldMapSourceRevision,
    pub removed_items: Vec<WorldMapItemSourceIdentity>,
}

/// One bounded account-owned VIP entry. It retains only validated persisted-player identity and
/// metadata; online status, notifications, quotas, client delivery, and policy remain separate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountVipEntry {
    pub target_player_id: u64,
    pub target_player_name: String,
    pub description: String,
    pub icon: u32,
    pub notify: bool,
}

/// Durable bounded guild identity with its exact persisted owner. Invitations, wars, banks,
/// permissions, client packets, and gameplay behavior remain outside this storage boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildRecord {
    pub id: u64,
    pub name: String,
    pub owner_player_id: u64,
    pub motd: String,
}

/// One typed rank belonging to a guild. FE provisions the TFS-style three base rank levels and
/// persistently manages bounded custom ranks, while permissions remain outside this boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildRankRecord {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    pub level: u8,
}

/// One player-owned guild membership. The primary membership key enforces a single guild per
/// persisted player while client and gameplay delivery remain deferred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildMembershipRecord {
    pub player_id: u64,
    pub guild_id: u64,
    pub rank_id: u64,
    pub nick: String,
}

/// One durable guild invitation. Acceptance, authorization, client delivery, and expiry policy
/// remain outside this storage relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuildInvitationRecord {
    pub player_id: u64,
    pub guild_id: u64,
}

/// One bounded durable depot view. The record intentionally retains only ordered complete
/// top-level item instances for a TFS-shaped depot ID; nested item trees and attribute blobs
/// remain outside this first compatibility boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDepotRecord {
    pub depot_id: u8,
    pub items: Vec<ItemInstance>,
}

/// One durable owner assignment for a nonzero TFS-shaped house identity. Map-house binding,
/// rent, access lists, doors, beds, auctions, and tile contents remain outside this storage slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HouseOwnershipRecord {
    pub house_id: u32,
    pub owner_player_id: u64,
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

    /// Returns the exact durable player bank balance. FE retains the TFS-style nonnegative balance
    /// concept but bounds it to SQLite's signed integer range; money items and client bank packets
    /// remain outside this persistence query.
    pub fn player_bank_balance(&self, player_id: u64) -> Result<u64, PersistenceError> {
        let balance = self
            .connection
            .query_row(
                "SELECT bank_balance FROM players WHERE id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(PersistenceError::UnknownPlayer(player_id))?;
        sqlite_bank_balance(balance)
    }

    /// Replaces one player's durable balance within the SQLite-safe FE bound. This is a storage
    /// primitive only; command authorization, money conversion, client delivery, and economy
    /// policy remain separate.
    pub fn set_player_bank_balance(
        &self,
        player_id: u64,
        balance: u64,
    ) -> Result<(), PersistenceError> {
        let balance = sqlite_bank_balance_value(balance)?;
        let affected = self.connection.execute(
            "UPDATE players SET bank_balance = ?1 WHERE id = ?2",
            params![balance, player_id as i64],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownPlayer(player_id));
        }
        Ok(())
    }

    /// Credits one exact nonnegative amount without allowing an SQLite-range overflow.
    pub fn credit_player_bank_balance(
        &mut self,
        player_id: u64,
        amount: u64,
    ) -> Result<u64, PersistenceError> {
        let transaction = self.connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT bank_balance FROM players WHERE id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(PersistenceError::UnknownPlayer(player_id))?;
        let updated = sqlite_bank_balance(current)?
            .checked_add(amount)
            .filter(|balance| *balance <= MAX_PLAYER_BANK_BALANCE)
            .ok_or(PersistenceError::BankBalanceOverflow { player_id })?;
        transaction.execute(
            "UPDATE players SET bank_balance = ?1 WHERE id = ?2",
            params![sqlite_bank_balance_value(updated)?, player_id as i64],
        )?;
        transaction.commit()?;
        Ok(updated)
    }

    /// Debits one exact amount only when the durable balance covers it. Negative balances are never
    /// persisted and a rejected debit leaves durable state unchanged.
    pub fn debit_player_bank_balance(
        &mut self,
        player_id: u64,
        amount: u64,
    ) -> Result<u64, PersistenceError> {
        let transaction = self.connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT bank_balance FROM players WHERE id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(PersistenceError::UnknownPlayer(player_id))?;
        let current = sqlite_bank_balance(current)?;
        let updated =
            current
                .checked_sub(amount)
                .ok_or(PersistenceError::InsufficientBankBalance {
                    player_id,
                    balance: current,
                    requested: amount,
                })?;
        transaction.execute(
            "UPDATE players SET bank_balance = ?1 WHERE id = ?2",
            params![sqlite_bank_balance_value(updated)?, player_id as i64],
        )?;
        transaction.commit()?;
        Ok(updated)
    }

    /// Adds one exact persisted character to an account-owned VIP list. The target name is matched
    /// against persisted character identity, not an online session or account name.
    pub fn add_account_vip_entry(
        &self,
        account_id: u32,
        target_player_name: &str,
        description: &str,
        icon: u32,
        notify: bool,
    ) -> Result<AccountVipEntry, PersistenceError> {
        self.ensure_account_exists(account_id)?;
        let description = validated_vip_description(description)?;
        let target_player_name = validated_vip_target_name(target_player_name)?;
        let (target_player_id, target_player_name) = self
            .connection
            .query_row(
                "SELECT id, name FROM players WHERE name = ?1",
                params![target_player_name],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| PersistenceError::UnknownVipTarget(target_player_name.to_owned()))?;
        let exists = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM account_vip_entries WHERE account_id = ?1 AND player_id = ?2)",
            params![account_id as i64, target_player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if exists {
            return Err(PersistenceError::DuplicateVipEntry {
                account_id,
                target_player_id,
            });
        }
        self.connection.execute(
            "INSERT INTO account_vip_entries (account_id, player_id, description, icon, notify) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                account_id as i64,
                target_player_id as i64,
                description,
                icon as i64,
                if notify { 1_i64 } else { 0_i64 },
            ],
        )?;
        Ok(AccountVipEntry {
            target_player_id,
            target_player_name,
            description: description.to_owned(),
            icon,
            notify,
        })
    }

    /// Lists one account's persisted VIP metadata in deterministic target-name and ID order.
    pub fn account_vip_entries(
        &self,
        account_id: u32,
    ) -> Result<Vec<AccountVipEntry>, PersistenceError> {
        self.ensure_account_exists(account_id)?;
        let mut statement = self.connection.prepare(
            "SELECT vip.player_id, player.name, vip.description, vip.icon, vip.notify \
             FROM account_vip_entries AS vip \
             JOIN players AS player ON player.id = vip.player_id \
             WHERE vip.account_id = ?1 \
             ORDER BY player.name COLLATE NOCASE, vip.player_id",
        )?;
        let entries = statement
            .query_map(params![account_id as i64], |row| {
                Ok(AccountVipEntry {
                    target_player_id: row.get::<_, i64>(0)? as u64,
                    target_player_name: row.get(1)?,
                    description: row.get(2)?,
                    icon: row.get::<_, i64>(3)? as u32,
                    notify: row.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(PersistenceError::Sql)?;
        Ok(entries)
    }

    /// Replaces metadata only for an existing account-owned VIP target.
    pub fn edit_account_vip_entry(
        &self,
        account_id: u32,
        target_player_id: u64,
        description: &str,
        icon: u32,
        notify: bool,
    ) -> Result<(), PersistenceError> {
        self.ensure_account_exists(account_id)?;
        let description = validated_vip_description(description)?;
        let affected = self.connection.execute(
            "UPDATE account_vip_entries SET description = ?1, icon = ?2, notify = ?3 WHERE account_id = ?4 AND player_id = ?5",
            params![
                description,
                icon as i64,
                if notify { 1_i64 } else { 0_i64 },
                account_id as i64,
                target_player_id as i64,
            ],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownVipEntry {
                account_id,
                target_player_id,
            });
        }
        Ok(())
    }

    /// Removes one existing account-owned VIP target without deleting its persisted character.
    pub fn remove_account_vip_entry(
        &self,
        account_id: u32,
        target_player_id: u64,
    ) -> Result<(), PersistenceError> {
        self.ensure_account_exists(account_id)?;
        let affected = self.connection.execute(
            "DELETE FROM account_vip_entries WHERE account_id = ?1 AND player_id = ?2",
            params![account_id as i64, target_player_id as i64],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownVipEntry {
                account_id,
                target_player_id,
            });
        }
        Ok(())
    }

    /// Creates a durable guild and atomically provisions the TFS-style leader, vice-leader, and
    /// member ranks. The owner becomes the leader and cannot already belong to another guild.
    pub fn create_guild(
        &mut self,
        owner_player_id: u64,
        name: &str,
        motd: &str,
    ) -> Result<GuildRecord, PersistenceError> {
        self.ensure_player_exists(owner_player_id)?;
        let name = validated_guild_name(name)?;
        let motd = validated_guild_motd(motd)?;
        let owner_has_membership = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_membership WHERE player_id = ?1)",
            params![owner_player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if owner_has_membership {
            return Err(PersistenceError::GuildOwnerAlreadyMember(owner_player_id));
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO guilds (name, owner_player_id, created_at, motd) VALUES (?1, ?2, ?3, ?4)",
            params![name, owner_player_id as i64, unix_seconds() as i64, motd],
        )?;
        let guild_id = transaction.last_insert_rowid() as u64;
        let mut leader_rank_id = None;
        for (rank_name, rank_level) in
            [("the Leader", 3_i64), ("a Vice-Leader", 2), ("a Member", 1)]
        {
            transaction.execute(
                "INSERT INTO guild_ranks (guild_id, name, level) VALUES (?1, ?2, ?3)",
                params![guild_id as i64, rank_name, rank_level],
            )?;
            if rank_level == 3 {
                leader_rank_id = Some(transaction.last_insert_rowid() as u64);
            }
        }
        let leader_rank_id = leader_rank_id.expect("fixed guild rank provisioning includes leader");
        transaction.execute(
            "INSERT INTO guild_membership (player_id, guild_id, rank_id, nick) VALUES (?1, ?2, ?3, '')",
            params![owner_player_id as i64, guild_id as i64, leader_rank_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildRecord {
            id: guild_id,
            name: name.to_owned(),
            owner_player_id,
            motd: motd.to_owned(),
        })
    }

    /// Updates one existing guild's bounded message of the day. Caller authorization, online
    /// delivery, rank permissions, and Lua behavior remain outside this storage operation.
    pub fn update_guild_motd(
        &mut self,
        guild_id: u64,
        motd: &str,
    ) -> Result<GuildRecord, PersistenceError> {
        let motd = validated_guild_motd(motd)?;
        let transaction = self.connection.transaction()?;
        let (name, owner_player_id) = transaction
            .query_row(
                "SELECT name, owner_player_id FROM guilds WHERE id = ?1",
                params![guild_id as i64],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(PersistenceError::UnknownGuild(guild_id))?;
        transaction.execute(
            "UPDATE guilds SET motd = ?1 WHERE id = ?2",
            params![motd, guild_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildRecord {
            id: guild_id,
            name,
            owner_player_id: owner_player_id as u64,
            motd: motd.to_owned(),
        })
    }

    /// Adds one persisted player to an existing guild at its provisioned member rank. The primary
    /// membership key remains the authoritative one-guild-per-player guard; invitation, online
    /// authorization, and client delivery are intentionally outside this storage operation.
    pub fn add_guild_member(
        &mut self,
        guild_id: u64,
        player_id: u64,
    ) -> Result<GuildMembershipRecord, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let already_member = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_membership WHERE player_id = ?1)",
            params![player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if already_member {
            return Err(PersistenceError::GuildMemberAlreadyAssigned(player_id));
        }
        let member_rank_id = transaction
            .query_row(
                "SELECT id FROM guild_ranks WHERE guild_id = ?1 AND level = 1 ORDER BY id LIMIT 1",
                params![guild_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or_else(|| {
                PersistenceError::InvalidGuildRecord(
                    "guild is missing its required member rank".into(),
                )
            })?;
        transaction.execute(
            "INSERT INTO guild_membership (player_id, guild_id, rank_id, nick) VALUES (?1, ?2, ?3, '')",
            params![player_id as i64, guild_id as i64, member_rank_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM guild_invitations WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildMembershipRecord {
            player_id,
            guild_id,
            rank_id: member_rank_id,
            nick: String::new(),
        })
    }

    /// Creates one durable pending invite for an existing player who is not currently a guild
    /// member. The schema prevents duplicate player/guild pairs and the FE cap bounds each guild's
    /// pending invite set; authorization and client-facing delivery remain outside this operation.
    pub fn invite_player_to_guild(
        &mut self,
        guild_id: u64,
        player_id: u64,
    ) -> Result<GuildInvitationRecord, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let has_membership = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_membership WHERE player_id = ?1)",
            params![player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if has_membership {
            return Err(PersistenceError::GuildInviteeAlreadyMember {
                guild_id,
                player_id,
            });
        }
        let duplicate = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_invitations WHERE guild_id = ?1 AND player_id = ?2)",
            params![guild_id as i64, player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if duplicate {
            return Err(PersistenceError::DuplicateGuildInvitation {
                guild_id,
                player_id,
            });
        }
        let pending_count = transaction.query_row(
            "SELECT COUNT(*) FROM guild_invitations WHERE guild_id = ?1",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? as usize;
        if pending_count >= MAX_GUILD_INVITATIONS_PER_GUILD {
            return Err(PersistenceError::GuildInvitationCapExceeded { guild_id });
        }
        transaction.execute(
            "INSERT INTO guild_invitations (player_id, guild_id) VALUES (?1, ?2)",
            params![player_id as i64, guild_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildInvitationRecord {
            player_id,
            guild_id,
        })
    }

    /// Lists pending invitations for one existing player in deterministic guild-ID order.
    pub fn guild_invitations_for_player(
        &self,
        player_id: u64,
    ) -> Result<Vec<GuildInvitationRecord>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT player_id, guild_id FROM guild_invitations WHERE player_id = ?1 ORDER BY guild_id",
        )?;
        let invitations = statement
            .query_map(params![player_id as i64], |row| {
                Ok(GuildInvitationRecord {
                    player_id: row.get::<_, i64>(0)? as u64,
                    guild_id: row.get::<_, i64>(1)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(invitations)
    }

    /// Lists pending invitations issued by one existing guild in deterministic player-ID order.
    pub fn guild_invitations_for_guild(
        &self,
        guild_id: u64,
    ) -> Result<Vec<GuildInvitationRecord>, PersistenceError> {
        let guild_exists = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let mut statement = self.connection.prepare(
            "SELECT player_id, guild_id FROM guild_invitations WHERE guild_id = ?1 ORDER BY player_id",
        )?;
        let invitations = statement
            .query_map(params![guild_id as i64], |row| {
                Ok(GuildInvitationRecord {
                    player_id: row.get::<_, i64>(0)? as u64,
                    guild_id: row.get::<_, i64>(1)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(invitations)
    }

    /// Revokes one existing pending player/guild invite without changing memberships.
    pub fn revoke_guild_invitation(
        &mut self,
        guild_id: u64,
        player_id: u64,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let affected = transaction.execute(
            "DELETE FROM guild_invitations WHERE guild_id = ?1 AND player_id = ?2",
            params![guild_id as i64, player_id as i64],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownGuildInvitation {
                guild_id,
                player_id,
            });
        }
        transaction.commit()?;
        Ok(())
    }

    /// Accepts one exact pending invitation into the named guild's required member rank. The
    /// membership insert and deletion of every competing pending invitation occur atomically;
    /// authorization, client delivery, and rank-permission policy remain outside this operation.
    pub fn accept_guild_invitation(
        &mut self,
        guild_id: u64,
        player_id: u64,
    ) -> Result<GuildMembershipRecord, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let has_membership = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_membership WHERE player_id = ?1)",
            params![player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if has_membership {
            return Err(PersistenceError::GuildMemberAlreadyAssigned(player_id));
        }
        let invite_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_invitations WHERE guild_id = ?1 AND player_id = ?2)",
            params![guild_id as i64, player_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !invite_exists {
            return Err(PersistenceError::UnknownGuildInvitation {
                guild_id,
                player_id,
            });
        }
        let member_rank_id = transaction
            .query_row(
                "SELECT id FROM guild_ranks WHERE guild_id = ?1 AND level = 1 ORDER BY id LIMIT 1",
                params![guild_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or_else(|| {
                PersistenceError::InvalidGuildRecord(
                    "guild is missing its required member rank".into(),
                )
            })?;
        transaction.execute(
            "INSERT INTO guild_membership (player_id, guild_id, rank_id, nick) VALUES (?1, ?2, ?3, '')",
            params![player_id as i64, guild_id as i64, member_rank_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM guild_invitations WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildMembershipRecord {
            player_id,
            guild_id,
            rank_id: member_rank_id,
            nick: String::new(),
        })
    }

    /// Deletes one durable guild and all FE-owned dependent invitation, membership, and rank
    /// records in a single transaction. Authorization, client state, wars, banking, houses, and
    /// broader gameplay cleanup remain outside this storage operation.
    pub fn delete_guild(&mut self, guild_id: u64) -> Result<GuildRecord, PersistenceError> {
        let transaction = self.connection.transaction()?;
        let guild = transaction
            .query_row(
                "SELECT name, owner_player_id, motd FROM guilds WHERE id = ?1",
                params![guild_id as i64],
                |row| {
                    Ok(GuildRecord {
                        id: guild_id,
                        name: row.get(0)?,
                        owner_player_id: row.get::<_, i64>(1)? as u64,
                        motd: row.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or(PersistenceError::UnknownGuild(guild_id))?;
        transaction.execute(
            "DELETE FROM guild_invitations WHERE guild_id = ?1",
            params![guild_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM guild_membership WHERE guild_id = ?1",
            params![guild_id as i64],
        )?;
        transaction.execute(
            "DELETE FROM guild_ranks WHERE guild_id = ?1",
            params![guild_id as i64],
        )?;
        transaction.execute("DELETE FROM guilds WHERE id = ?1", params![guild_id as i64])?;
        transaction.commit()?;
        Ok(guild)
    }

    /// Transfers durable guild ownership to an existing guild member. The new owner receives the
    /// required leader rank and the former owner receives the required vice-leader rank, preserving
    /// one durable owner and rank consistency without adding authorization or client behavior.
    pub fn transfer_guild_ownership(
        &mut self,
        guild_id: u64,
        new_owner_player_id: u64,
    ) -> Result<GuildRecord, PersistenceError> {
        self.ensure_player_exists(new_owner_player_id)?;
        let transaction = self.connection.transaction()?;
        let (name, current_owner_player_id, motd) = transaction
            .query_row(
                "SELECT name, owner_player_id, motd FROM guilds WHERE id = ?1",
                params![guild_id as i64],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(PersistenceError::UnknownGuild(guild_id))?;
        let current_owner_player_id = current_owner_player_id as u64;
        if current_owner_player_id == new_owner_player_id {
            transaction.commit()?;
            return Ok(GuildRecord {
                id: guild_id,
                name,
                owner_player_id: current_owner_player_id,
                motd,
            });
        }
        let new_owner_guild_id = transaction
            .query_row(
                "SELECT guild_id FROM guild_membership WHERE player_id = ?1",
                params![new_owner_player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or(PersistenceError::GuildOwnershipTargetNotMember {
                guild_id,
                player_id: new_owner_player_id,
            })?;
        if new_owner_guild_id != guild_id {
            return Err(PersistenceError::GuildOwnershipTargetNotMember {
                guild_id,
                player_id: new_owner_player_id,
            });
        }
        let current_owner_guild_id = transaction
            .query_row(
                "SELECT guild_id FROM guild_membership WHERE player_id = ?1",
                params![current_owner_player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or_else(|| {
                PersistenceError::InvalidGuildRecord(
                    "guild owner is missing its required membership".into(),
                )
            })?;
        if current_owner_guild_id != guild_id {
            return Err(PersistenceError::InvalidGuildRecord(
                "guild owner membership belongs to another guild".into(),
            ));
        }
        let leader_rank_id = transaction
            .query_row(
                "SELECT id FROM guild_ranks WHERE guild_id = ?1 AND level = 3",
                params![guild_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or_else(|| {
                PersistenceError::InvalidGuildRecord(
                    "guild is missing its required leader rank".into(),
                )
            })?;
        let vice_leader_rank_id = transaction
            .query_row(
                "SELECT id FROM guild_ranks WHERE guild_id = ?1 AND level = 2",
                params![guild_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or_else(|| {
                PersistenceError::InvalidGuildRecord(
                    "guild is missing its required vice-leader rank".into(),
                )
            })?;
        transaction.execute(
            "UPDATE guild_membership SET rank_id = ?1 WHERE player_id = ?2",
            params![leader_rank_id as i64, new_owner_player_id as i64],
        )?;
        transaction.execute(
            "UPDATE guild_membership SET rank_id = ?1 WHERE player_id = ?2",
            params![vice_leader_rank_id as i64, current_owner_player_id as i64],
        )?;
        transaction.execute(
            "UPDATE guilds SET owner_player_id = ?1 WHERE id = ?2",
            params![new_owner_player_id as i64, guild_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildRecord {
            id: guild_id,
            name,
            owner_player_id: new_owner_player_id,
            motd,
        })
    }

    /// Removes one non-owner player from exactly the named guild. Guild deletion remains a separate
    /// future transition, so the current owner cannot leave this bounded model.
    pub fn remove_guild_member(
        &mut self,
        guild_id: u64,
        player_id: u64,
    ) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        let owner_player_id = transaction
            .query_row(
                "SELECT owner_player_id FROM guilds WHERE id = ?1",
                params![guild_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or(PersistenceError::UnknownGuild(guild_id))?;
        if owner_player_id == player_id {
            return Err(PersistenceError::GuildOwnerCannotLeave {
                guild_id,
                player_id,
            });
        }
        let member_guild_id = transaction
            .query_row(
                "SELECT guild_id FROM guild_membership WHERE player_id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or(PersistenceError::UnknownGuildMember {
                guild_id,
                player_id,
            })?;
        if member_guild_id != guild_id {
            return Err(PersistenceError::UnknownGuildMember {
                guild_id,
                player_id,
            });
        }
        transaction.execute(
            "DELETE FROM guild_membership WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Assigns one existing member to one existing rank of the same guild. It cannot create ranks,
    /// transfer ownership, or change nicknames; those policies remain explicit future work.
    pub fn assign_guild_member_rank(
        &mut self,
        guild_id: u64,
        player_id: u64,
        rank_id: u64,
    ) -> Result<GuildMembershipRecord, PersistenceError> {
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let member_guild_id = transaction
            .query_row(
                "SELECT guild_id FROM guild_membership WHERE player_id = ?1",
                params![player_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or(PersistenceError::UnknownGuildMember {
                guild_id,
                player_id,
            })?;
        if member_guild_id != guild_id {
            return Err(PersistenceError::UnknownGuildMember {
                guild_id,
                player_id,
            });
        }
        let rank_guild_id = transaction
            .query_row(
                "SELECT guild_id FROM guild_ranks WHERE id = ?1",
                params![rank_id as i64],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|id| id as u64)
            .ok_or(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id })?;
        if rank_guild_id != guild_id {
            return Err(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id });
        }
        transaction.execute(
            "UPDATE guild_membership SET rank_id = ?1 WHERE player_id = ?2",
            params![rank_id as i64, player_id as i64],
        )?;
        let nick = transaction.query_row(
            "SELECT nick FROM guild_membership WHERE player_id = ?1",
            params![player_id as i64],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(GuildMembershipRecord {
            player_id,
            guild_id,
            rank_id,
            nick,
        })
    }

    /// Replaces the bounded nick of one current guild member. Nicknames are durable member
    /// metadata only; rank permissions, client display, and online authorization remain separate.
    pub fn update_guild_member_nick(
        &mut self,
        guild_id: u64,
        player_id: u64,
        nick: &str,
    ) -> Result<GuildMembershipRecord, PersistenceError> {
        let nick = validated_guild_nick(nick)?;
        let transaction = self.connection.transaction()?;
        let member = transaction
            .query_row(
                "SELECT guild_id, rank_id FROM guild_membership WHERE player_id = ?1",
                params![player_id as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(PersistenceError::UnknownGuildMember {
                guild_id,
                player_id,
            })?;
        if member.0 as u64 != guild_id {
            return Err(PersistenceError::UnknownGuildMember {
                guild_id,
                player_id,
            });
        }
        transaction.execute(
            "UPDATE guild_membership SET nick = ?1 WHERE player_id = ?2",
            params![nick, player_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildMembershipRecord {
            player_id,
            guild_id,
            rank_id: member.1 as u64,
            nick: nick.to_owned(),
        })
    }

    /// Adds one bounded custom rank to an existing guild. Rank names and levels must remain unique
    /// within that guild; authorization, client packets, and permission semantics remain outside
    /// this transactional storage operation.
    pub fn add_guild_rank(
        &mut self,
        guild_id: u64,
        name: &str,
        level: u8,
    ) -> Result<GuildRankRecord, PersistenceError> {
        let name = validated_guild_rank_name(name)?;
        if level == 0 {
            return Err(PersistenceError::InvalidGuildRecord(
                "guild rank level must be between 1 and 255".into(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let rank_count = transaction.query_row(
            "SELECT COUNT(*) FROM guild_ranks WHERE guild_id = ?1",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? as usize;
        if rank_count >= MAX_GUILD_RANKS_PER_GUILD {
            return Err(PersistenceError::GuildRankCapExceeded { guild_id });
        }
        let duplicate = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_ranks WHERE guild_id = ?1 AND (name = ?2 OR level = ?3))",
            params![guild_id as i64, name, level as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if duplicate {
            return Err(PersistenceError::DuplicateGuildRank { guild_id });
        }
        transaction.execute(
            "INSERT INTO guild_ranks (guild_id, name, level) VALUES (?1, ?2, ?3)",
            params![guild_id as i64, name, level as i64],
        )?;
        let rank_id = transaction.last_insert_rowid() as u64;
        transaction.commit()?;
        Ok(GuildRankRecord {
            id: rank_id,
            guild_id,
            name: name.to_owned(),
            level,
        })
    }

    /// Renames one rank owned by the named guild without changing its level or member assignment.
    /// Authorization and rank-permission checks remain outside this storage operation.
    pub fn rename_guild_rank(
        &mut self,
        guild_id: u64,
        rank_id: u64,
        name: &str,
    ) -> Result<GuildRankRecord, PersistenceError> {
        let name = validated_guild_rank_name(name)?;
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let (rank_guild_id, level) = transaction
            .query_row(
                "SELECT guild_id, level FROM guild_ranks WHERE id = ?1",
                params![rank_id as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id })?;
        if rank_guild_id as u64 != guild_id {
            return Err(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id });
        }
        let duplicate_name = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_ranks WHERE guild_id = ?1 AND name = ?2 AND id != ?3)",
            params![guild_id as i64, name, rank_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if duplicate_name {
            return Err(PersistenceError::DuplicateGuildRank { guild_id });
        }
        transaction.execute(
            "UPDATE guild_ranks SET name = ?1 WHERE id = ?2",
            params![name, rank_id as i64],
        )?;
        transaction.commit()?;
        Ok(GuildRankRecord {
            id: rank_id,
            guild_id,
            name: name.to_owned(),
            level: level as u8,
        })
    }

    /// Deletes one unreferenced custom rank owned by the named guild. The three required
    /// provisioned rank levels remain protected so later member creation retains its invariant.
    pub fn remove_guild_rank(
        &mut self,
        guild_id: u64,
        rank_id: u64,
    ) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        let guild_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
            params![guild_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !guild_exists {
            return Err(PersistenceError::UnknownGuild(guild_id));
        }
        let (rank_guild_id, level) = transaction
            .query_row(
                "SELECT guild_id, level FROM guild_ranks WHERE id = ?1",
                params![rank_id as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id })?;
        if rank_guild_id as u64 != guild_id {
            return Err(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id });
        }
        if (1..=3).contains(&level) {
            return Err(PersistenceError::InvalidGuildRecord(
                "guild required rank levels cannot be deleted".into(),
            ));
        }
        let in_use = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM guild_membership WHERE rank_id = ?1)",
            params![rank_id as i64],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if in_use {
            return Err(PersistenceError::GuildRankInUse { guild_id, rank_id });
        }
        transaction.execute(
            "DELETE FROM guild_ranks WHERE id = ?1",
            params![rank_id as i64],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn guild_ranks(&self, guild_id: u64) -> Result<Vec<GuildRankRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT id, guild_id, name, level FROM guild_ranks WHERE guild_id = ?1 ORDER BY level DESC, id",
        )?;
        let ranks = statement
            .query_map(params![guild_id as i64], |row| {
                Ok(GuildRankRecord {
                    id: row.get::<_, i64>(0)? as u64,
                    guild_id: row.get::<_, i64>(1)? as u64,
                    name: row.get(2)?,
                    level: row.get::<_, i64>(3)? as u8,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if ranks.is_empty() {
            let exists = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM guilds WHERE id = ?1)",
                params![guild_id as i64],
                |row| row.get::<_, i64>(0),
            )? != 0;
            if !exists {
                return Err(PersistenceError::UnknownGuild(guild_id));
            }
        }
        Ok(ranks)
    }

    pub fn guild_membership(
        &self,
        player_id: u64,
    ) -> Result<Option<GuildMembershipRecord>, PersistenceError> {
        self.connection
            .query_row(
                "SELECT player_id, guild_id, rank_id, nick FROM guild_membership WHERE player_id = ?1",
                params![player_id as i64],
                |row| {
                    Ok(GuildMembershipRecord {
                        player_id: row.get::<_, i64>(0)? as u64,
                        guild_id: row.get::<_, i64>(1)? as u64,
                        rank_id: row.get::<_, i64>(2)? as u64,
                        nick: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(PersistenceError::Sql)
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
        self.create_player_for_account_with_vocation(account_id, name, VocationId::default())
    }

    /// Creates an account-owned character with a validated typed vocation identity. The caller
    /// chooses the existing default vocation by passing `VocationId::default()`, preserving the
    /// stable legacy provisioning behavior.
    pub fn create_player_for_account_with_vocation(
        &self,
        account_id: u32,
        name: &str,
        vocation: VocationId,
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
        let experience = classic_experience_for_level(DEFAULT_PROVISIONED_PLAYER_LEVEL)
            .expect("the fixed local provisioning level must have a representable threshold");
        self.connection.execute(
            "INSERT INTO players (account_id, name, x, y, z, level, experience, skill_points, vocation)\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                account_id as i64,
                name.trim(),
                position.x as i64,
                position.y as i64,
                position.z as i64,
                i64::from(DEFAULT_PROVISIONED_PLAYER_LEVEL),
                experience as i64,
                0_i64,
                i64::from(vocation.value()),
            ],
        )?;
        Ok(LoginCharacter {
            id: self.connection.last_insert_rowid() as u64,
            name: name.trim().to_owned(),
            level: DEFAULT_PROVISIONED_PLAYER_LEVEL,
            experience,
            skill_points: 0,
            progression: PlayerProgression {
                vocation,
                ..PlayerProgression::default()
            },
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

    /// Replaces a player's complete bounded inventory and the complete revision-bound source-map
    /// removal journal in one SQLite transaction. Callers use this only after validating a
    /// composite authoritative map-to-inventory transition; a failed commit leaves both durable
    /// collections unchanged.
    pub fn replace_player_inventory_and_map_item_removal_journal(
        &mut self,
        player_id: u64,
        equipment: &PlayerEquipment,
        containers: &PlayerContainers,
        journal: &MapItemRemovalJournal,
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut seen = BTreeMap::new();
        for item in &journal.removed_items {
            if item.map_revision != journal.map_revision {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "every item must use the journal map revision".into(),
                ));
            }
            if seen.insert((item.position, item.item_index), ()).is_some() {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "duplicate source item identity".into(),
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

    /// Replaces all durable top-level depot items owned by one player in one transaction. FE
    /// validates the audited 0–19 TFS-shaped depot ID range and ordered complete items, but does
    /// not yet serialize nested containers, arbitrary attribute blobs, capacity, or client views.
    pub fn replace_player_depots(
        &mut self,
        player_id: u64,
        depots: &[PlayerDepotRecord],
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        validate_player_depot_records(depots)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_depot_items WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for depot in depots {
            for (slot, item) in depot.items.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO player_depot_items (player_id, depot_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        player_id as i64,
                        i64::from(depot.depot_id),
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

    /// Loads one player's durable depots in deterministic depot and top-level item order. Raw
    /// database fields are validated before entering FE's authoritative item representation.
    pub fn player_depots(
        &self,
        player_id: u64,
    ) -> Result<Vec<PlayerDepotRecord>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT depot_id, slot, server_id, count, action_id, unique_id FROM player_depot_items WHERE player_id = ?1 ORDER BY depot_id, slot",
        )?;
        let records = statement
            .query_map(params![player_id as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut depots: BTreeMap<u8, Vec<ItemInstance>> = BTreeMap::new();
        for (depot_id, slot, server_id, count, action_id, unique_id) in records {
            let depot_id = u8::try_from(depot_id).map_err(|_| {
                PersistenceError::InvalidDepotRecord("depot ID does not fit u8".into())
            })?;
            if depot_id > MAX_PLAYER_DEPOT_ID {
                return Err(PersistenceError::InvalidDepotRecord(format!(
                    "depot ID exceeds bounded maximum of {MAX_PLAYER_DEPOT_ID}"
                )));
            }
            let items = depots.entry(depot_id).or_default();
            if items.len() >= MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS {
                return Err(PersistenceError::InvalidDepotRecord(format!(
                    "depot exceeds {MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS} top-level items"
                )));
            }
            let expected_slot = i64::try_from(items.len()).map_err(|_| {
                PersistenceError::InvalidDepotRecord("depot item slot does not fit i64".into())
            })?;
            if slot != expected_slot {
                return Err(PersistenceError::InvalidDepotRecord(
                    "depot item slots must be contiguous from zero".into(),
                ));
            }
            let server_id = u16::try_from(server_id).map_err(|_| {
                PersistenceError::InvalidDepotRecord("server item ID does not fit u16".into())
            })?;
            let count = u16::try_from(count).map_err(|_| {
                PersistenceError::InvalidDepotRecord("item count does not fit u16".into())
            })?;
            let mut item = ItemInstance::new(server_id, count)
                .map_err(|error| PersistenceError::InvalidDepotRecord(error.to_string()))?;
            item.action_id = optional_u16_depot_attribute(action_id, "action ID")?;
            item.unique_id = optional_u16_depot_attribute(unique_id, "unique ID")?;
            items.push(item);
        }
        let records = depots
            .into_iter()
            .map(|(depot_id, items)| PlayerDepotRecord { depot_id, items })
            .collect::<Vec<_>>();
        validate_player_depot_records(&records)?;
        Ok(records)
    }

    /// Replaces a player's complete bounded inbox contents in one transaction. This TFS-shaped
    /// storage boundary retains only ordered top-level items; nesting, attributes beyond the
    /// bounded IDs, client windows, capacity policy, and inbox routing remain outside it.
    pub fn replace_player_inbox(
        &mut self,
        player_id: u64,
        items: &[ItemInstance],
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        validate_player_inbox_items(items)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_inbox_items WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for (slot, item) in items.iter().enumerate() {
            transaction.execute(
                "INSERT INTO player_inbox_items (player_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    player_id as i64,
                    slot as i64,
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

    /// Loads a player's bounded inbox in deterministic top-level item order and rejects malformed
    /// raw SQLite fields before they reach the authoritative item representation.
    pub fn player_inbox(&self, player_id: u64) -> Result<Vec<ItemInstance>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT slot, server_id, count, action_id, unique_id FROM player_inbox_items WHERE player_id = ?1 ORDER BY slot",
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
        if records.len() > MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS {
            return Err(PersistenceError::InvalidInboxRecord(format!(
                "inbox exceeds {MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS} top-level items"
            )));
        }
        let mut items = Vec::with_capacity(records.len());
        for (expected_slot, (slot, server_id, count, action_id, unique_id)) in
            records.into_iter().enumerate()
        {
            let expected_slot = i64::try_from(expected_slot).map_err(|_| {
                PersistenceError::InvalidInboxRecord("inbox item slot does not fit i64".into())
            })?;
            if slot != expected_slot {
                return Err(PersistenceError::InvalidInboxRecord(
                    "inbox item slots must be contiguous from zero".into(),
                ));
            }
            let server_id = u16::try_from(server_id).map_err(|_| {
                PersistenceError::InvalidInboxRecord("server item ID does not fit u16".into())
            })?;
            let count = u16::try_from(count).map_err(|_| {
                PersistenceError::InvalidInboxRecord("item count does not fit u16".into())
            })?;
            let mut item = ItemInstance::new(server_id, count)
                .map_err(|error| PersistenceError::InvalidInboxRecord(error.to_string()))?;
            item.action_id = optional_u16_inbox_attribute(action_id, "action ID")?;
            item.unique_id = optional_u16_inbox_attribute(unique_id, "unique ID")?;
            items.push(item);
        }
        validate_player_inbox_items(&items)?;
        Ok(items)
    }

    /// Assigns or clears the durable owner of one nonzero house identity. The selected owner must
    /// be a persisted player. This has no map, rent, access-list, auction, or client side effect.
    pub fn set_house_owner(
        &mut self,
        house_id: u32,
        owner_player_id: Option<u64>,
    ) -> Result<(), PersistenceError> {
        validated_house_id(house_id)?;
        if let Some(owner_player_id) = owner_player_id {
            self.ensure_player_exists(owner_player_id)?;
            self.connection.execute(
                "INSERT INTO house_ownership (house_id, owner_player_id) VALUES (?1, ?2) ON CONFLICT(house_id) DO UPDATE SET owner_player_id=excluded.owner_player_id",
                params![i64::from(house_id), owner_player_id as i64],
            )?;
        } else {
            self.connection.execute(
                "DELETE FROM house_ownership WHERE house_id = ?1",
                params![i64::from(house_id)],
            )?;
        }
        Ok(())
    }

    /// Returns the durable owner assignment for one nonzero house identity. An absent row is the
    /// explicit unowned state; malformed or stale raw owner data is rejected.
    pub fn house_owner(
        &self,
        house_id: u32,
    ) -> Result<Option<HouseOwnershipRecord>, PersistenceError> {
        validated_house_id(house_id)?;
        let owner_player_id = self
            .connection
            .query_row(
                "SELECT owner_player_id FROM house_ownership WHERE house_id = ?1",
                params![i64::from(house_id)],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(owner_player_id) = owner_player_id else {
            return Ok(None);
        };
        let owner_player_id = u64::try_from(owner_player_id).map_err(|_| {
            PersistenceError::InvalidHouseOwnershipRecord(
                "owner player ID must be a nonnegative u64".into(),
            )
        })?;
        self.ensure_player_exists(owner_player_id)?;
        Ok(Some(HouseOwnershipRecord {
            house_id,
            owner_player_id,
        }))
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
            if record.direct_melee_damage_sequence > i64::MAX as u64 {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "direct melee damage sequence does not fit SQLite INTEGER".into(),
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
                "INSERT INTO static_creature_runtime (creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds, direct_melee_cooldown_remaining_ticks, direct_melee_damage_sequence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    i64::from(record.creature_id),
                    i64::from(record.position.x),
                    i64::from(record.position.y),
                    i64::from(record.position.z),
                    i64::from(u8::from(record.active)),
                    i64::from(record.health_percent),
                    record.reactivation_remaining_seconds.map(i64::from),
                    record.direct_melee_cooldown_remaining_ticks.map(i64::from),
                    i64::try_from(record.direct_melee_damage_sequence)
                        .expect("validated sequence fits SQLite INTEGER"),
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
            "SELECT creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds, direct_melee_cooldown_remaining_ticks, direct_melee_damage_sequence FROM static_creature_runtime ORDER BY creature_id",
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
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (
                creature_id,
                x,
                y,
                z,
                active,
                health_percent,
                reactivation_remaining_seconds,
                direct_melee_cooldown_remaining_ticks,
                direct_melee_damage_sequence,
            ) = row?;
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
            let direct_melee_cooldown_remaining_ticks = direct_melee_cooldown_remaining_ticks
                .map(|remaining_ticks| {
                    u32::try_from(remaining_ticks).map_err(|_| {
                        PersistenceError::InvalidStaticCreatureRuntimeRecord(
                            "direct melee cooldown delay does not fit u32".into(),
                        )
                    })
                })
                .transpose()?;
            let direct_melee_damage_sequence = u64::try_from(direct_melee_damage_sequence)
                .map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "direct melee damage sequence must be non-negative".into(),
                    )
                })?;
            records.push(StaticCreatureRuntimeRecord {
                creature_id,
                position,
                active,
                health_percent,
                reactivation_remaining_seconds,
                direct_melee_cooldown_remaining_ticks,
                direct_melee_damage_sequence,
            });
        }
        Ok(records)
    }

    /// Atomically replaces the complete revision-bound removal journal. Future recovery must
    /// compare `map_revision` with the loaded immutable map before applying any removal.
    pub fn replace_map_item_removal_journal(
        &mut self,
        journal: &MapItemRemovalJournal,
    ) -> Result<(), PersistenceError> {
        let mut seen = BTreeMap::new();
        for item in &journal.removed_items {
            if item.map_revision != journal.map_revision {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "every item must use the journal map revision".into(),
                ));
            }
            if seen.insert((item.position, item.item_index), ()).is_some() {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "duplicate source item identity".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
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
        transaction.commit()?;
        Ok(())
    }

    /// Loads the complete journal without applying it to any map. Callers must compare the loaded
    /// revision with the current `WorldMap::source_revision()` before considering recovery.
    pub fn map_item_removal_journal(
        &self,
    ) -> Result<Option<MapItemRemovalJournal>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT map_revision, x, y, z, item_index FROM map_item_removal_journal ORDER BY map_revision, x, y, z, item_index",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut journal: Option<MapItemRemovalJournal> = None;
        for row in rows {
            let (revision, x, y, z, item_index) = row?;
            let revision = u64::from_str_radix(&revision, 16).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "map revision must be hexadecimal u64".into(),
                )
            })?;
            let identity = WorldMapItemSourceIdentity {
                map_revision: WorldMapSourceRevision(revision),
                position: Position {
                    x: u16::try_from(x).map_err(|_| {
                        PersistenceError::InvalidMapItemJournal("x does not fit u16".into())
                    })?,
                    y: u16::try_from(y).map_err(|_| {
                        PersistenceError::InvalidMapItemJournal("y does not fit u16".into())
                    })?,
                    z: u8::try_from(z).map_err(|_| {
                        PersistenceError::InvalidMapItemJournal("z does not fit u8".into())
                    })?,
                },
                item_index: u8::try_from(item_index).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal("item index does not fit u8".into())
                })?,
            };
            match &mut journal {
                Some(existing) if existing.map_revision != identity.map_revision => {
                    return Err(PersistenceError::InvalidMapItemJournal(
                        "journal contains multiple map revisions".into(),
                    ))
                }
                Some(existing) => existing.removed_items.push(identity),
                None => {
                    journal = Some(MapItemRemovalJournal {
                        map_revision: identity.map_revision,
                        removed_items: vec![identity],
                    })
                }
            }
        }
        Ok(journal)
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

    fn ensure_account_exists(&self, account_id: u32) -> Result<(), PersistenceError> {
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

fn optional_u16_depot_attribute(
    value: Option<i64>,
    label: &str,
) -> Result<Option<u16>, PersistenceError> {
    value
        .map(|value| {
            u16::try_from(value).map_err(|_| {
                PersistenceError::InvalidDepotRecord(format!("{label} does not fit u16"))
            })
        })
        .transpose()
}

fn optional_u16_inbox_attribute(
    value: Option<i64>,
    label: &str,
) -> Result<Option<u16>, PersistenceError> {
    value
        .map(|value| {
            u16::try_from(value).map_err(|_| {
                PersistenceError::InvalidInboxRecord(format!("{label} does not fit u16"))
            })
        })
        .transpose()
}

fn validate_player_depot_records(records: &[PlayerDepotRecord]) -> Result<(), PersistenceError> {
    if records.len() > MAX_PLAYER_DEPOTS_PER_PLAYER {
        return Err(PersistenceError::InvalidDepotRecord(format!(
            "player exceeds {MAX_PLAYER_DEPOTS_PER_PLAYER} depots"
        )));
    }
    let mut seen = BTreeMap::new();
    for depot in records {
        if depot.depot_id > MAX_PLAYER_DEPOT_ID {
            return Err(PersistenceError::InvalidDepotRecord(format!(
                "depot ID exceeds bounded maximum of {MAX_PLAYER_DEPOT_ID}"
            )));
        }
        if seen.insert(depot.depot_id, ()).is_some() {
            return Err(PersistenceError::InvalidDepotRecord(
                "duplicate depot ID".into(),
            ));
        }
        if depot.items.is_empty() {
            return Err(PersistenceError::InvalidDepotRecord(
                "empty depots are represented by no durable row".into(),
            ));
        }
        if depot.items.len() > MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS {
            return Err(PersistenceError::InvalidDepotRecord(format!(
                "depot exceeds {MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS} top-level items"
            )));
        }
    }
    Ok(())
}

fn validate_player_inbox_items(items: &[ItemInstance]) -> Result<(), PersistenceError> {
    if items.len() > MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS {
        return Err(PersistenceError::InvalidInboxRecord(format!(
            "inbox exceeds {MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS} top-level items"
        )));
    }
    Ok(())
}

fn validated_house_id(house_id: u32) -> Result<(), PersistenceError> {
    if house_id == 0 {
        return Err(PersistenceError::InvalidHouseOwnershipRecord(
            "house ID must be nonzero".into(),
        ));
    }
    Ok(())
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

fn validated_vip_target_name(value: &str) -> Result<&str, PersistenceError> {
    if value.trim().is_empty() || value.len() > 32 {
        return Err(PersistenceError::InvalidVipEntry(
            "target player name must be nonempty and at most 32 bytes".into(),
        ));
    }
    Ok(value)
}

fn validated_vip_description(value: &str) -> Result<&str, PersistenceError> {
    if value.len() > MAX_VIP_DESCRIPTION_BYTES {
        return Err(PersistenceError::InvalidVipEntry(format!(
            "description exceeds {MAX_VIP_DESCRIPTION_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn validated_guild_name(value: &str) -> Result<&str, PersistenceError> {
    if value.trim().is_empty() || value.len() > MAX_GUILD_NAME_BYTES {
        return Err(PersistenceError::InvalidGuildRecord(format!(
            "guild name must be nonempty and at most {MAX_GUILD_NAME_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn validated_guild_motd(value: &str) -> Result<&str, PersistenceError> {
    if value.len() > MAX_GUILD_MOTD_BYTES {
        return Err(PersistenceError::InvalidGuildRecord(format!(
            "guild motd exceeds {MAX_GUILD_MOTD_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn validated_guild_nick(value: &str) -> Result<&str, PersistenceError> {
    if value.len() > MAX_GUILD_NICK_BYTES {
        return Err(PersistenceError::InvalidGuildRecord(format!(
            "guild nick exceeds {MAX_GUILD_NICK_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn validated_guild_rank_name(value: &str) -> Result<&str, PersistenceError> {
    if value.trim().is_empty() || value.len() > MAX_GUILD_RANK_NAME_BYTES {
        return Err(PersistenceError::InvalidGuildRecord(format!(
            "guild rank name must be nonempty and at most {MAX_GUILD_RANK_NAME_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn sqlite_bank_balance(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| {
        PersistenceError::InvalidBankBalanceRecord(
            "bank balance must be nonnegative and fit SQLite signed range".into(),
        )
    })
}

fn sqlite_bank_balance_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| {
        PersistenceError::InvalidBankBalanceRecord(format!(
            "bank balance exceeds SQLite signed range of {MAX_PLAYER_BANK_BALANCE}"
        ))
    })
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
    InvalidMapItemJournal(String),
    InvalidVipEntry(String),
    InvalidGuildRecord(String),
    InvalidBankBalanceRecord(String),
    InvalidDepotRecord(String),
    InvalidInboxRecord(String),
    InvalidHouseOwnershipRecord(String),
    UnknownAccount(u32),
    UnknownPlayer(u64),
    UnknownGuild(u64),
    UnknownVipTarget(String),
    DuplicateVipEntry {
        account_id: u32,
        target_player_id: u64,
    },
    UnknownVipEntry {
        account_id: u32,
        target_player_id: u64,
    },
    GuildOwnerAlreadyMember(u64),
    GuildMemberAlreadyAssigned(u64),
    UnknownGuildMember {
        guild_id: u64,
        player_id: u64,
    },
    GuildOwnerCannotLeave {
        guild_id: u64,
        player_id: u64,
    },
    GuildOwnershipTargetNotMember {
        guild_id: u64,
        player_id: u64,
    },
    GuildInviteeAlreadyMember {
        guild_id: u64,
        player_id: u64,
    },
    DuplicateGuildInvitation {
        guild_id: u64,
        player_id: u64,
    },
    UnknownGuildInvitation {
        guild_id: u64,
        player_id: u64,
    },
    GuildInvitationCapExceeded {
        guild_id: u64,
    },
    BankBalanceOverflow {
        player_id: u64,
    },
    InsufficientBankBalance {
        player_id: u64,
        balance: u64,
        requested: u64,
    },
    GuildRankOutsideGuild {
        guild_id: u64,
        rank_id: u64,
    },
    GuildRankCapExceeded {
        guild_id: u64,
    },
    GuildRankInUse {
        guild_id: u64,
        rank_id: u64,
    },
    DuplicateGuildRank {
        guild_id: u64,
    },
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
    fn atomically_provisions_bounded_guild_ranks_and_owner_membership() {
        let path = temporary_path("guild-foundation");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("owner", "hash").unwrap();
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
        let guild = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        assert_eq!(guild.name, "Forgotten");
        assert_eq!(guild.owner_player_id, 7);
        let ranks = database.guild_ranks(guild.id).unwrap();
        assert_eq!(
            ranks
                .iter()
                .map(|rank| (rank.name.as_str(), rank.level))
                .collect::<Vec<_>>(),
            vec![("the Leader", 3), ("a Vice-Leader", 2), ("a Member", 1)]
        );
        assert_eq!(
            database.guild_membership(7).unwrap(),
            Some(GuildMembershipRecord {
                player_id: 7,
                guild_id: guild.id,
                rank_id: ranks[0].id,
                nick: String::new(),
            })
        );
        assert!(matches!(
            database.create_guild(7, "Other", ""),
            Err(PersistenceError::GuildOwnerAlreadyMember(7))
        ));
        assert!(matches!(
            database.create_guild(7, "", ""),
            Err(PersistenceError::InvalidGuildRecord(_))
        ));
        assert!(matches!(
            database.guild_ranks(999),
            Err(PersistenceError::UnknownGuild(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_transactional_guild_membership_departure_and_rank_assignment() {
        let path = temporary_path("guild-membership-management");
        let mut database = EngineDatabase::open(&path).unwrap();
        for (id, name) in [(7, "Knight"), (8, "Druid"), (9, "Sorcerer")] {
            database
                .save_player(&Player {
                    id,
                    account_id: 1,
                    name: name.into(),
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
        }
        let guild = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        let ranks = database.guild_ranks(guild.id).unwrap();
        let vice_rank_id = ranks
            .iter()
            .find(|rank| rank.level == 2)
            .expect("guild creates a vice-leader rank")
            .id;
        let member_rank_id = ranks
            .iter()
            .find(|rank| rank.level == 1)
            .expect("guild creates a member rank")
            .id;

        assert_eq!(
            database.add_guild_member(guild.id, 8).unwrap(),
            GuildMembershipRecord {
                player_id: 8,
                guild_id: guild.id,
                rank_id: member_rank_id,
                nick: String::new(),
            }
        );
        assert!(matches!(
            database.add_guild_member(guild.id, 8),
            Err(PersistenceError::GuildMemberAlreadyAssigned(8))
        ));
        assert_eq!(
            database
                .assign_guild_member_rank(guild.id, 8, vice_rank_id)
                .unwrap(),
            GuildMembershipRecord {
                player_id: 8,
                guild_id: guild.id,
                rank_id: vice_rank_id,
                nick: String::new(),
            }
        );

        let other = database.create_guild(9, "Other", "").unwrap();
        let other_rank = database.guild_ranks(other.id).unwrap()[0].id;
        assert!(matches!(
            database.assign_guild_member_rank(guild.id, 8, other_rank),
            Err(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id })
                if guild_id == guild.id && rank_id == other_rank
        ));
        assert!(matches!(
            database.remove_guild_member(guild.id, 7),
            Err(PersistenceError::GuildOwnerCannotLeave { guild_id, player_id })
                if guild_id == guild.id && player_id == 7
        ));
        assert!(matches!(
            database.remove_guild_member(other.id, 8),
            Err(PersistenceError::UnknownGuildMember { guild_id, player_id })
                if guild_id == other.id && player_id == 8
        ));

        database.remove_guild_member(guild.id, 8).unwrap();
        assert_eq!(database.guild_membership(8).unwrap(), None);
        assert!(matches!(
            database.remove_guild_member(guild.id, 8),
            Err(PersistenceError::UnknownGuildMember { guild_id, player_id })
                if guild_id == guild.id && player_id == 8
        ));
        assert!(matches!(
            database.add_guild_member(guild.id, 999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_validated_guild_motd_and_member_nickname_updates() {
        let path = temporary_path("guild-profile-metadata");
        let mut database = EngineDatabase::open(&path).unwrap();
        for (id, name) in [(7, "Knight"), (8, "Druid"), (9, "Sorcerer")] {
            database
                .save_player(&Player {
                    id,
                    account_id: 1,
                    name: name.into(),
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
        }
        let guild = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        database.add_guild_member(guild.id, 8).unwrap();
        let other = database.create_guild(9, "Other", "").unwrap();

        assert_eq!(
            database
                .update_guild_motd(guild.id, "Raid at sunset")
                .unwrap(),
            GuildRecord {
                id: guild.id,
                name: "Forgotten".into(),
                owner_player_id: 7,
                motd: "Raid at sunset".into(),
            }
        );
        assert!(matches!(
            database.update_guild_motd(999, "unknown"),
            Err(PersistenceError::UnknownGuild(999))
        ));
        assert!(matches!(
            database.update_guild_motd(guild.id, &"x".repeat(MAX_GUILD_MOTD_BYTES + 1)),
            Err(PersistenceError::InvalidGuildRecord(_))
        ));

        let member_rank_id = database.guild_membership(8).unwrap().unwrap().rank_id;
        assert_eq!(
            database
                .update_guild_member_nick(guild.id, 8, "Healer")
                .unwrap(),
            GuildMembershipRecord {
                player_id: 8,
                guild_id: guild.id,
                rank_id: member_rank_id,
                nick: "Healer".into(),
            }
        );
        assert_eq!(
            database.guild_membership(8).unwrap().unwrap().nick,
            "Healer"
        );
        assert!(matches!(
            database.update_guild_member_nick(other.id, 8, "wrong"),
            Err(PersistenceError::UnknownGuildMember { guild_id, player_id })
                if guild_id == other.id && player_id == 8
        ));
        assert!(matches!(
            database.update_guild_member_nick(guild.id, 8, &"x".repeat(MAX_GUILD_NICK_BYTES + 1)),
            Err(PersistenceError::InvalidGuildRecord(_))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_bounded_custom_guild_rank_management() {
        let path = temporary_path("guild-custom-ranks");
        let mut database = EngineDatabase::open(&path).unwrap();
        for (id, name) in [(7, "Knight"), (8, "Druid"), (9, "Sorcerer")] {
            database
                .save_player(&Player {
                    id,
                    account_id: 1,
                    name: name.into(),
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
        }
        let guild = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        let other = database.create_guild(9, "Other", "").unwrap();

        let officer = database.add_guild_rank(guild.id, "Officer", 4).unwrap();
        assert_eq!(
            officer,
            GuildRankRecord {
                id: officer.id,
                guild_id: guild.id,
                name: "Officer".into(),
                level: 4,
            }
        );
        assert!(matches!(
            database.add_guild_rank(guild.id, "Officer", 5),
            Err(PersistenceError::DuplicateGuildRank { guild_id }) if guild_id == guild.id
        ));
        assert!(matches!(
            database.add_guild_rank(guild.id, "Veteran", 4),
            Err(PersistenceError::DuplicateGuildRank { guild_id }) if guild_id == guild.id
        ));
        assert!(matches!(
            database.add_guild_rank(guild.id, "", 5),
            Err(PersistenceError::InvalidGuildRecord(_))
        ));
        assert!(matches!(
            database.add_guild_rank(guild.id, "Zero", 0),
            Err(PersistenceError::InvalidGuildRecord(_))
        ));

        assert_eq!(
            database
                .rename_guild_rank(guild.id, officer.id, "Steward")
                .unwrap(),
            GuildRankRecord {
                id: officer.id,
                guild_id: guild.id,
                name: "Steward".into(),
                level: 4,
            }
        );
        assert!(matches!(
            database.rename_guild_rank(guild.id, officer.id, "a Member"),
            Err(PersistenceError::DuplicateGuildRank { guild_id }) if guild_id == guild.id
        ));
        assert!(matches!(
            database.rename_guild_rank(other.id, officer.id, "Wrong guild"),
            Err(PersistenceError::GuildRankOutsideGuild { guild_id, rank_id })
                if guild_id == other.id && rank_id == officer.id
        ));

        database.add_guild_member(guild.id, 8).unwrap();
        database
            .assign_guild_member_rank(guild.id, 8, officer.id)
            .unwrap();
        assert!(matches!(
            database.remove_guild_rank(guild.id, officer.id),
            Err(PersistenceError::GuildRankInUse { guild_id, rank_id })
                if guild_id == guild.id && rank_id == officer.id
        ));
        let member_rank_id = database
            .guild_ranks(guild.id)
            .unwrap()
            .into_iter()
            .find(|rank| rank.level == 1)
            .expect("guild retains its required member rank")
            .id;
        database
            .assign_guild_member_rank(guild.id, 8, member_rank_id)
            .unwrap();
        database.remove_guild_rank(guild.id, officer.id).unwrap();
        assert!(!database
            .guild_ranks(guild.id)
            .unwrap()
            .iter()
            .any(|rank| rank.id == officer.id));
        assert!(matches!(
            database.remove_guild_rank(999, officer.id),
            Err(PersistenceError::UnknownGuild(999))
        ));

        for level in 4..=20 {
            database
                .add_guild_rank(guild.id, &format!("Rank {level}"), level)
                .unwrap();
        }
        assert_eq!(
            database.guild_ranks(guild.id).unwrap().len(),
            MAX_GUILD_RANKS_PER_GUILD
        );
        assert!(matches!(
            database.add_guild_rank(guild.id, "Over cap", 21),
            Err(PersistenceError::GuildRankCapExceeded { guild_id }) if guild_id == guild.id
        ));
        let leader_rank_id = database
            .guild_ranks(guild.id)
            .unwrap()
            .into_iter()
            .find(|rank| rank.level == 3)
            .expect("guild keeps its required leader rank")
            .id;
        assert!(matches!(
            database.remove_guild_rank(guild.id, leader_rank_id),
            Err(PersistenceError::InvalidGuildRecord(_))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_transactional_guild_ownership_transfer() {
        let path = temporary_path("guild-ownership-transfer");
        let mut database = EngineDatabase::open(&path).unwrap();
        for (id, name) in [(7, "Knight"), (8, "Druid"), (9, "Sorcerer")] {
            database
                .save_player(&Player {
                    id,
                    account_id: 1,
                    name: name.into(),
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
        }
        let guild = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        database.add_guild_member(guild.id, 8).unwrap();
        database
            .update_guild_member_nick(guild.id, 8, "Healer")
            .unwrap();
        let other = database.create_guild(9, "Other", "").unwrap();
        let ranks = database.guild_ranks(guild.id).unwrap();
        let leader_rank_id = ranks
            .iter()
            .find(|rank| rank.level == 3)
            .expect("guild creates leader rank")
            .id;
        let vice_leader_rank_id = ranks
            .iter()
            .find(|rank| rank.level == 2)
            .expect("guild creates vice-leader rank")
            .id;

        assert_eq!(
            database.transfer_guild_ownership(guild.id, 8).unwrap(),
            GuildRecord {
                id: guild.id,
                name: "Forgotten".into(),
                owner_player_id: 8,
                motd: "Welcome".into(),
            }
        );
        assert_eq!(
            database.guild_membership(8).unwrap(),
            Some(GuildMembershipRecord {
                player_id: 8,
                guild_id: guild.id,
                rank_id: leader_rank_id,
                nick: "Healer".into(),
            })
        );
        assert_eq!(
            database.guild_membership(7).unwrap(),
            Some(GuildMembershipRecord {
                player_id: 7,
                guild_id: guild.id,
                rank_id: vice_leader_rank_id,
                nick: String::new(),
            })
        );
        assert_eq!(
            database
                .transfer_guild_ownership(guild.id, 8)
                .unwrap()
                .owner_player_id,
            8
        );
        assert!(matches!(
            database.remove_guild_member(guild.id, 8),
            Err(PersistenceError::GuildOwnerCannotLeave { guild_id, player_id })
                if guild_id == guild.id && player_id == 8
        ));
        assert!(matches!(
            database.transfer_guild_ownership(guild.id, 9),
            Err(PersistenceError::GuildOwnershipTargetNotMember { guild_id, player_id })
                if guild_id == guild.id && player_id == 9
        ));
        assert!(matches!(
            database.transfer_guild_ownership(guild.id, 999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        assert!(matches!(
            database.transfer_guild_ownership(999, 8),
            Err(PersistenceError::UnknownGuild(999))
        ));
        assert_eq!(other.owner_player_id, 9);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_bounded_guild_invitations_and_membership_cleanup() {
        let path = temporary_path("guild-invitations");
        let mut database = EngineDatabase::open(&path).unwrap();
        for id in 7..=31 {
            database
                .save_player(&Player {
                    id,
                    account_id: 1,
                    name: format!("Player {id}"),
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
        }
        let guild = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        let initial = database.invite_player_to_guild(guild.id, 8).unwrap();
        assert_eq!(
            initial,
            GuildInvitationRecord {
                player_id: 8,
                guild_id: guild.id,
            }
        );
        assert_eq!(
            database.guild_invitations_for_player(8).unwrap(),
            vec![initial]
        );
        assert_eq!(
            database.guild_invitations_for_guild(guild.id).unwrap(),
            vec![initial]
        );
        assert!(matches!(
            database.invite_player_to_guild(guild.id, 8),
            Err(PersistenceError::DuplicateGuildInvitation { guild_id, player_id })
                if guild_id == guild.id && player_id == 8
        ));
        assert!(matches!(
            database.invite_player_to_guild(guild.id, 7),
            Err(PersistenceError::GuildInviteeAlreadyMember { guild_id, player_id })
                if guild_id == guild.id && player_id == 7
        ));
        assert!(matches!(
            database.invite_player_to_guild(999, 8),
            Err(PersistenceError::UnknownGuild(999))
        ));

        database.add_guild_member(guild.id, 8).unwrap();
        assert_eq!(
            database.guild_invitations_for_player(8).unwrap(),
            Vec::new()
        );
        assert!(matches!(
            database.invite_player_to_guild(guild.id, 8),
            Err(PersistenceError::GuildInviteeAlreadyMember { guild_id, player_id })
                if guild_id == guild.id && player_id == 8
        ));

        database.invite_player_to_guild(guild.id, 9).unwrap();
        database.revoke_guild_invitation(guild.id, 9).unwrap();
        assert!(matches!(
            database.revoke_guild_invitation(guild.id, 9),
            Err(PersistenceError::UnknownGuildInvitation { guild_id, player_id })
                if guild_id == guild.id && player_id == 9
        ));
        assert!(matches!(
            database.guild_invitations_for_guild(999),
            Err(PersistenceError::UnknownGuild(999))
        ));
        assert!(matches!(
            database.guild_invitations_for_player(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));

        for player_id in 10..30 {
            database
                .invite_player_to_guild(guild.id, player_id)
                .unwrap();
        }
        assert_eq!(
            database
                .guild_invitations_for_guild(guild.id)
                .unwrap()
                .len(),
            MAX_GUILD_INVITATIONS_PER_GUILD
        );
        assert!(matches!(
            database.invite_player_to_guild(guild.id, 30),
            Err(PersistenceError::GuildInvitationCapExceeded { guild_id }) if guild_id == guild.id
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn accepts_one_pending_guild_invitation_transactionally() {
        let path = temporary_path("guild-invitation-acceptance");
        let mut database = EngineDatabase::open(&path).unwrap();
        for (id, name) in [
            (7, "Knight"),
            (8, "Druid"),
            (9, "Sorcerer"),
            (10, "Paladin"),
        ] {
            database
                .save_player(&Player {
                    id,
                    account_id: 1,
                    name: name.into(),
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
        }
        let first = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        let second = database.create_guild(9, "Other", "").unwrap();
        database.invite_player_to_guild(first.id, 8).unwrap();
        database.invite_player_to_guild(second.id, 8).unwrap();
        let member_rank_id = database
            .guild_ranks(first.id)
            .unwrap()
            .into_iter()
            .find(|rank| rank.level == 1)
            .expect("guild creates member rank")
            .id;

        assert_eq!(
            database.accept_guild_invitation(first.id, 8).unwrap(),
            GuildMembershipRecord {
                player_id: 8,
                guild_id: first.id,
                rank_id: member_rank_id,
                nick: String::new(),
            }
        );
        assert_eq!(
            database.guild_invitations_for_player(8).unwrap(),
            Vec::new()
        );
        assert!(!database
            .guild_invitations_for_guild(second.id)
            .unwrap()
            .iter()
            .any(|invitation| invitation.player_id == 8));
        assert!(matches!(
            database.accept_guild_invitation(first.id, 8),
            Err(PersistenceError::GuildMemberAlreadyAssigned(8))
        ));
        assert!(matches!(
            database.accept_guild_invitation(first.id, 10),
            Err(PersistenceError::UnknownGuildInvitation { guild_id, player_id })
                if guild_id == first.id && player_id == 10
        ));
        assert!(matches!(
            database.accept_guild_invitation(999, 10),
            Err(PersistenceError::UnknownGuild(999))
        ));
        assert!(matches!(
            database.accept_guild_invitation(first.id, 999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn deletes_guild_and_all_fe_owned_dependent_records_transactionally() {
        let path = temporary_path("guild-deletion");
        let mut database = EngineDatabase::open(&path).unwrap();
        for (id, name) in [(7, "Knight"), (8, "Druid"), (9, "Sorcerer")] {
            database
                .save_player(&Player {
                    id,
                    account_id: 1,
                    name: name.into(),
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
        }
        let guild = database.create_guild(7, "Forgotten", "Welcome").unwrap();
        database.add_guild_member(guild.id, 9).unwrap();
        database.add_guild_rank(guild.id, "Officer", 4).unwrap();
        database.invite_player_to_guild(guild.id, 8).unwrap();

        assert_eq!(database.delete_guild(guild.id).unwrap(), guild);
        assert_eq!(database.guild_membership(7).unwrap(), None);
        assert_eq!(database.guild_membership(9).unwrap(), None);
        assert_eq!(
            database.guild_invitations_for_player(8).unwrap(),
            Vec::new()
        );
        assert!(matches!(
            database.guild_ranks(guild.id),
            Err(PersistenceError::UnknownGuild(id)) if id == guild.id
        ));
        assert!(matches!(
            database.guild_invitations_for_guild(guild.id),
            Err(PersistenceError::UnknownGuild(id)) if id == guild.id
        ));
        assert!(matches!(
            database.delete_guild(guild.id),
            Err(PersistenceError::UnknownGuild(id)) if id == guild.id
        ));
        assert_eq!(
            database
                .create_guild(7, "Forgotten", "Welcome again")
                .unwrap(),
            GuildRecord {
                id: guild.id,
                name: "Forgotten".into(),
                owner_player_id: 7,
                motd: "Welcome again".into(),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_bounded_player_bank_balance_operations() {
        let path = temporary_path("player-bank-balance");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("owner", "hash").unwrap() as u32;
        let player = database
            .create_player_for_account(account_id, "Knight")
            .unwrap();

        assert_eq!(database.player_bank_balance(player.id).unwrap(), 0);
        database.set_player_bank_balance(player.id, 100).unwrap();
        assert_eq!(
            database.credit_player_bank_balance(player.id, 50).unwrap(),
            150
        );
        assert_eq!(
            database.debit_player_bank_balance(player.id, 75).unwrap(),
            75
        );
        assert!(matches!(
            database.debit_player_bank_balance(player.id, 76),
            Err(PersistenceError::InsufficientBankBalance {
                player_id,
                balance: 75,
                requested: 76,
            }) if player_id == player.id
        ));
        assert_eq!(database.player_bank_balance(player.id).unwrap(), 75);

        database
            .set_player_bank_balance(player.id, MAX_PLAYER_BANK_BALANCE)
            .unwrap();
        assert!(matches!(
            database.credit_player_bank_balance(player.id, 1),
            Err(PersistenceError::BankBalanceOverflow { player_id }) if player_id == player.id
        ));
        assert!(matches!(
            database.set_player_bank_balance(player.id, MAX_PLAYER_BANK_BALANCE + 1),
            Err(PersistenceError::InvalidBankBalanceRecord(_))
        ));
        assert!(matches!(
            database.player_bank_balance(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        assert!(matches!(
            database.credit_player_bank_balance(999, 1),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_validates_edits_and_removes_account_vip_entries() {
        let path = temporary_path("account-vip-entries");
        let database = EngineDatabase::open(&path).unwrap();
        let owner_account = database.create_account("owner", "hash").unwrap() as u32;
        let target_account = database.create_account("target", "hash").unwrap() as u32;
        let druid = database
            .create_player_for_account(target_account, "Druid")
            .unwrap();
        let sorcerer = database
            .create_player_for_account(target_account, "Sorcerer")
            .unwrap();

        assert_eq!(
            database.account_vip_entries(owner_account).unwrap(),
            Vec::new()
        );
        let added = database
            .add_account_vip_entry(owner_account, "Druid", "friend", 4, true)
            .unwrap();
        assert_eq!(
            added,
            AccountVipEntry {
                target_player_id: druid.id,
                target_player_name: "Druid".into(),
                description: "friend".into(),
                icon: 4,
                notify: true,
            }
        );
        database
            .add_account_vip_entry(owner_account, "Sorcerer", "trade", 2, false)
            .unwrap();
        assert_eq!(
            database.account_vip_entries(owner_account).unwrap(),
            vec![
                added.clone(),
                AccountVipEntry {
                    target_player_id: sorcerer.id,
                    target_player_name: "Sorcerer".into(),
                    description: "trade".into(),
                    icon: 2,
                    notify: false,
                },
            ]
        );
        assert!(matches!(
            database.add_account_vip_entry(owner_account, "druid", "", 0, false),
            Err(PersistenceError::UnknownVipTarget(name)) if name == "druid"
        ));
        assert!(matches!(
            database.add_account_vip_entry(owner_account, "Druid", "", 0, false),
            Err(PersistenceError::DuplicateVipEntry {
                account_id,
                target_player_id,
            }) if account_id == owner_account && target_player_id == druid.id
        ));
        assert!(matches!(
            database.add_account_vip_entry(
                owner_account,
                "Druid",
                &"x".repeat(MAX_VIP_DESCRIPTION_BYTES + 1),
                0,
                false,
            ),
            Err(PersistenceError::InvalidVipEntry(_))
        ));

        database
            .edit_account_vip_entry(owner_account, druid.id, "best friend", 9, false)
            .unwrap();
        assert_eq!(
            database.account_vip_entries(owner_account).unwrap()[0],
            AccountVipEntry {
                target_player_id: druid.id,
                target_player_name: "Druid".into(),
                description: "best friend".into(),
                icon: 9,
                notify: false,
            }
        );
        assert!(matches!(
            database.edit_account_vip_entry(owner_account, 999, "", 0, false),
            Err(PersistenceError::UnknownVipEntry {
                account_id,
                target_player_id: 999,
            }) if account_id == owner_account
        ));
        database
            .remove_account_vip_entry(owner_account, druid.id)
            .unwrap();
        assert_eq!(
            database.account_vip_entries(owner_account).unwrap().len(),
            1
        );
        assert!(matches!(
            database.remove_account_vip_entry(owner_account, druid.id),
            Err(PersistenceError::UnknownVipEntry {
                account_id,
                target_player_id,
            }) if account_id == owner_account && target_player_id == druid.id
        ));
        assert!(matches!(
            database.account_vip_entries(9_999),
            Err(PersistenceError::UnknownAccount(9_999))
        ));
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
                direct_melee_cooldown_remaining_ticks: Some(2),
                direct_melee_damage_sequence: 5,
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
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 11,
            },
        ];
        database.replace_static_creature_runtime(&records).unwrap();
        assert_eq!(database.static_creature_runtime().unwrap(), records);

        let invalid_sequence = [StaticCreatureRuntimeRecord {
            direct_melee_damage_sequence: u64::MAX,
            ..records[0]
        }];
        assert!(matches!(
            database.replace_static_creature_runtime(&invalid_sequence),
            Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(_))
        ));
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
                "INSERT INTO static_creature_runtime (creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds, direct_melee_damage_sequence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![0x1000_0001_i64, 101_i64, 102_i64, 7_i64, 1_i64, 100_i64, Option::<i64>::None, -1_i64],
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
    fn atomically_replaces_player_inventory_and_map_item_removal_journal() {
        let path = temporary_path("inventory-map-item-journal-transaction");
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

        let mut initial_equipment = PlayerEquipment::default();
        initial_equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(2376, 1).unwrap(),
        );
        database
            .replace_player_inventory(7, &initial_equipment, &PlayerContainers::default())
            .unwrap();
        let initial_journal = MapItemRemovalJournal {
            map_revision: WorldMapSourceRevision(0x1010),
            removed_items: vec![WorldMapItemSourceIdentity {
                map_revision: WorldMapSourceRevision(0x1010),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                item_index: 0,
            }],
        };
        database
            .replace_map_item_removal_journal(&initial_journal)
            .unwrap();

        let mut replacement_equipment = PlayerEquipment::default();
        replacement_equipment.equip(EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap());
        let replacement_journal = MapItemRemovalJournal {
            map_revision: WorldMapSourceRevision(0x2020),
            removed_items: vec![WorldMapItemSourceIdentity {
                map_revision: WorldMapSourceRevision(0x2020),
                position: Position {
                    x: 102,
                    y: 100,
                    z: 7,
                },
                item_index: 1,
            }],
        };
        database
            .replace_player_inventory_and_map_item_removal_journal(
                7,
                &replacement_equipment,
                &PlayerContainers::default(),
                &replacement_journal,
            )
            .unwrap();
        assert_eq!(database.player_equipment(7).unwrap(), replacement_equipment);
        assert_eq!(
            database.player_containers(7).unwrap(),
            PlayerContainers::default()
        );
        assert_eq!(
            database.map_item_removal_journal().unwrap(),
            Some(replacement_journal.clone())
        );

        let duplicated_journal = MapItemRemovalJournal {
            map_revision: replacement_journal.map_revision,
            removed_items: vec![
                replacement_journal.removed_items[0],
                replacement_journal.removed_items[0],
            ],
        };
        assert!(matches!(
            database.replace_player_inventory_and_map_item_removal_journal(
                7,
                &initial_equipment,
                &PlayerContainers::default(),
                &duplicated_journal,
            ),
            Err(PersistenceError::InvalidMapItemJournal(_))
        ));
        assert_eq!(database.player_equipment(7).unwrap(), replacement_equipment);
        assert_eq!(
            database.map_item_removal_journal().unwrap(),
            Some(replacement_journal)
        );

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
    fn persists_bounded_player_depots_with_strict_item_ordering() {
        let path = temporary_path("depots");
        let mut database = EngineDatabase::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
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

        let mut key = ItemInstance::new(2088, 1).unwrap();
        key.action_id = Some(4_242);
        let mut coins = ItemInstance::new(3031, 100).unwrap();
        coins.unique_id = Some(7_000);
        let depots = vec![
            PlayerDepotRecord {
                depot_id: 0,
                items: vec![key, coins],
            },
            PlayerDepotRecord {
                depot_id: MAX_PLAYER_DEPOT_ID,
                items: vec![ItemInstance::new(1987, 1).unwrap()],
            },
        ];
        database.replace_player_depots(7, &depots).unwrap();
        assert_eq!(database.player_depots(7).unwrap(), depots);

        assert!(matches!(
            database.replace_player_depots(
                7,
                &[PlayerDepotRecord {
                    depot_id: MAX_PLAYER_DEPOT_ID.saturating_add(1),
                    items: vec![ItemInstance::new(1987, 1).unwrap()],
                }],
            ),
            Err(PersistenceError::InvalidDepotRecord(_))
        ));
        assert_eq!(database.player_depots(7).unwrap(), depots);
        assert!(matches!(
            database.replace_player_depots(
                7,
                &[PlayerDepotRecord {
                    depot_id: 1,
                    items: Vec::new(),
                }],
            ),
            Err(PersistenceError::InvalidDepotRecord(_))
        ));
        let overfull =
            vec![ItemInstance::new(3031, 1).unwrap(); MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS + 1];
        assert!(matches!(
            database.replace_player_depots(
                7,
                &[PlayerDepotRecord {
                    depot_id: 1,
                    items: overfull,
                }],
            ),
            Err(PersistenceError::InvalidDepotRecord(_))
        ));

        database.replace_player_depots(7, &[]).unwrap();
        assert!(database.player_depots(7).unwrap().is_empty());
        database
            .connection
            .execute(
                "INSERT INTO player_depot_items (player_id, depot_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![7_i64, 0_i64, 1_i64, 1987_i64, 1_i64, Option::<i64>::None, Option::<i64>::None],
            )
            .unwrap();
        assert!(matches!(
            database.player_depots(7),
            Err(PersistenceError::InvalidDepotRecord(_))
        ));
        assert!(matches!(
            database.player_depots(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_bounded_player_inbox_with_strict_item_ordering() {
        let path = temporary_path("inbox");
        let mut database = EngineDatabase::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
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

        let mut letter = ItemInstance::new(2597, 1).unwrap();
        letter.action_id = Some(4_242);
        let mut coins = ItemInstance::new(3031, 50).unwrap();
        coins.unique_id = Some(7_000);
        let inbox = vec![letter, coins];
        database.replace_player_inbox(7, &inbox).unwrap();
        assert_eq!(database.player_inbox(7).unwrap(), inbox);

        let overfull =
            vec![ItemInstance::new(3031, 1).unwrap(); MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS + 1];
        assert!(matches!(
            database.replace_player_inbox(7, &overfull),
            Err(PersistenceError::InvalidInboxRecord(_))
        ));
        assert_eq!(database.player_inbox(7).unwrap(), inbox);

        database.replace_player_inbox(7, &[]).unwrap();
        assert!(database.player_inbox(7).unwrap().is_empty());
        database
            .connection
            .execute(
                "INSERT INTO player_inbox_items (player_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![7_i64, 1_i64, 2597_i64, 1_i64, Option::<i64>::None, Option::<i64>::None],
            )
            .unwrap();
        assert!(matches!(
            database.player_inbox(7),
            Err(PersistenceError::InvalidInboxRecord(_))
        ));
        assert!(matches!(
            database.player_inbox(999),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persists_bounded_house_ownership_assignments() {
        let path = temporary_path("house-ownership");
        let mut database = EngineDatabase::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        let account_id = database.create_account("admin", "hash").unwrap();
        for (id, name) in [(7, "Knight"), (8, "Druid")] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
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
        }

        assert_eq!(database.house_owner(42).unwrap(), None);
        database.set_house_owner(42, Some(7)).unwrap();
        assert_eq!(
            database.house_owner(42).unwrap(),
            Some(HouseOwnershipRecord {
                house_id: 42,
                owner_player_id: 7,
            })
        );
        database.set_house_owner(42, Some(8)).unwrap();
        assert_eq!(
            database.house_owner(42).unwrap(),
            Some(HouseOwnershipRecord {
                house_id: 42,
                owner_player_id: 8,
            })
        );
        database.set_house_owner(42, None).unwrap();
        assert_eq!(database.house_owner(42).unwrap(), None);

        assert!(matches!(
            database.set_house_owner(0, Some(7)),
            Err(PersistenceError::InvalidHouseOwnershipRecord(_))
        ));
        assert!(matches!(
            database.set_house_owner(42, Some(999)),
            Err(PersistenceError::UnknownPlayer(999))
        ));
        database
            .connection
            .execute(
                "INSERT INTO house_ownership (house_id, owner_player_id) VALUES (?1, ?2)",
                params![0_i64, 7_i64],
            )
            .unwrap();
        assert!(matches!(
            database.house_owner(0),
            Err(PersistenceError::InvalidHouseOwnershipRecord(_))
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
        assert_eq!(character.level, DEFAULT_PROVISIONED_PLAYER_LEVEL);
        assert_eq!(
            character.experience,
            classic_experience_for_level(DEFAULT_PROVISIONED_PLAYER_LEVEL).unwrap()
        );
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
        let reloaded = database
            .characters_for_account(account_id)
            .unwrap()
            .remove(0);
        assert_eq!(reloaded.level, character.level);
        assert_eq!(reloaded.experience, character.experience);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn provisions_an_explicit_typed_vocation_with_the_new_character() {
        let path = temporary_path("provisioning-vocation");
        let database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("admin", "hash").unwrap();
        let character = database
            .create_player_for_account_with_vocation(account_id as u32, "Druid", VocationId::new(4))
            .unwrap();
        assert_eq!(character.progression.vocation, VocationId::new(4));
        assert_eq!(
            database
                .player_by_id(character.id)
                .unwrap()
                .progression
                .vocation,
            VocationId::new(4)
        );
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
