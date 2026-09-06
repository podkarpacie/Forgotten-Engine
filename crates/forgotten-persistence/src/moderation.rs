//! Account moderation persistence: mutes, bans, and player freeze flags.
//! Mutes and bans are account-scoped with bounded durations; freeze flags are
//! player-scoped and persist across relogs.

use super::*;

impl EngineDatabase {
    pub fn clear_account_mute(&self, account_id: u64) -> Result<usize, PersistenceError> {
        let affected = self.connection.execute(
            "DELETE FROM account_mutes WHERE account_id = ?1",
            params![account_id as i64],
        )?;
        Ok(affected)
    }

    /// Sets the operator freeze flag for one character (plan v49 slice 18). Frozen characters
    /// cannot step until unfrozen; the flag survives relogs.
    pub fn set_player_frozen(&self, player_id: u64, frozen: bool) -> Result<(), PersistenceError> {
        let affected = self.connection.execute(
            "UPDATE players SET frozen = ?1 WHERE id = ?2",
            params![i64::from(frozen), player_id as i64],
        )?;
        if affected == 0 {
            return Err(PersistenceError::UnknownPlayer(player_id));
        }
        Ok(())
    }

    pub fn player_frozen(&self, player_id: u64) -> Result<bool, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("SELECT frozen FROM players WHERE id = ?1")?;
        let mut rows = statement.query(params![player_id as i64])?;
        match rows.next()? {
            Some(row) => Ok(row.get::<_, i64>(0)? != 0),
            None => Ok(false),
        }
    }

    /// Records an account ban (plan v49 slice 17). `duration_seconds` of `None` means
    /// permanent; a bounded positive value expires the ban automatically.
    pub fn record_account_ban(
        &self,
        account_id: u32,
        reason: &str,
        duration_seconds: Option<u64>,
    ) -> Result<(), PersistenceError> {
        let reason = reason.trim();
        if reason.is_empty() || reason.len() > 256 {
            return Err(PersistenceError::InvalidPlayerName);
        }
        self.ensure_account_exists(account_id)?;
        let expires_at = duration_seconds
            .map(|seconds| (unix_seconds().saturating_add(seconds)).min(i64::MAX as u64) as i64);
        self.connection.execute(
            "INSERT INTO account_bans (account_id, reason, expires_at, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![account_id as i64, reason, expires_at, unix_seconds()],
        )?;
        Ok(())
    }

    /// Returns the active ban reason for an account when one exists and has not expired.
    pub fn active_account_ban(&self, account_id: u64) -> Result<Option<String>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT reason FROM account_bans WHERE account_id = ?1 AND (expires_at IS NULL OR expires_at > ?2) ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = statement.query(params![account_id as i64, unix_seconds()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Lifts every ban for an account. Returns the number of rows removed.
    pub fn clear_account_bans(&self, account_id: u64) -> Result<usize, PersistenceError> {
        let affected = self.connection.execute(
            "DELETE FROM account_bans WHERE account_id = ?1",
            params![account_id as i64],
        )?;
        Ok(affected)
    }

    /// Mutes an account until the configured number of seconds elapse. A later mute replaces
    /// any earlier one.
    pub fn record_account_mute(
        &self,
        account_id: u32,
        duration_seconds: u64,
    ) -> Result<(), PersistenceError> {
        if duration_seconds == 0 || duration_seconds > 86_400 * 30 {
            return Err(PersistenceError::InvalidPlayerName);
        }
        self.ensure_account_exists(account_id)?;
        self.connection.execute(
            "INSERT INTO account_mutes (account_id, muted_until) VALUES (?1, ?2)
             ON CONFLICT(account_id) DO UPDATE SET muted_until = excluded.muted_until",
            params![
                account_id as i64,
                (unix_seconds().saturating_add(duration_seconds)).min(i64::MAX as u64) as i64
            ],
        )?;
        Ok(())
    }

    /// Remaining mute seconds for an account, pruning the row once it lapses. `None` means not
    /// muted.
    pub fn account_mute_remaining_seconds(
        &self,
        account_id: u64,
    ) -> Result<Option<u64>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("SELECT muted_until FROM account_mutes WHERE account_id = ?1")?;
        let mut rows = statement.query(params![account_id as i64])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let muted_until: i64 = row.get(0)?;
        let now = unix_seconds();
        if muted_until > now as i64 {
            Ok(Some((muted_until - now as i64) as u64))
        } else {
            self.connection.execute(
                "DELETE FROM account_mutes WHERE account_id = ?1",
                params![account_id as i64],
            )?;
            Ok(None)
        }
    }
}
