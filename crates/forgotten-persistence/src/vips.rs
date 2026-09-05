//! Account VIP entry management on the persistent database: add, list, edit, and remove
//! VIP entries for an account's watched player list. Each entry stores a player name,
//! description, and optional notification flag.

use super::*;

impl EngineDatabase {
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
}
