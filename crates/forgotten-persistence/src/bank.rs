//! Player bank balance persistence: lookup, replacement, credit, and debit
//! operations with overflow-safe i64/u64 conversion helpers.

use super::*;

impl EngineDatabase {
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
}
