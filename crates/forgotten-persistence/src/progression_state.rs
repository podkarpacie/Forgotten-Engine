//! Player quest state, blessing/promotion persistence, and party snapshots on the
//! durable database. Quest IDs are bounded nonzero u16 values; blessings are bounded
//! 0-5; party snapshots enforce one leader with sorted non-leader members.

use super::*;

impl EngineDatabase {
    /// Replaces one player's bounded quest-state rows in one SQLite transaction. Quest IDs must
    /// be nonzero and unique; completed flags are stored exactly as given.
    pub fn replace_player_quests(
        &mut self,
        player_id: u64,
        quests: &[(u16, bool)],
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut seen = BTreeSet::new();
        for (quest_id, _) in quests {
            if *quest_id == 0 {
                return Err(PersistenceError::InvalidQuestState(
                    "quest id must be nonzero".into(),
                ));
            }
            if !seen.insert(*quest_id) {
                return Err(PersistenceError::InvalidQuestState(
                    "duplicate quest id".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_quests WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for (quest_id, completed) in quests {
            transaction.execute(
                "INSERT INTO player_quests (player_id, quest_id, completed) VALUES (?1, ?2, ?3)",
                params![
                    player_id as i64,
                    i64::from(*quest_id),
                    i64::from(u8::from(*completed)),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads one player's bounded quest state sorted by quest ID.
    pub fn player_quests(&self, player_id: u64) -> Result<Vec<(u16, bool)>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT quest_id, completed FROM player_quests WHERE player_id = ?1 ORDER BY quest_id",
        )?;
        let rows = statement.query_map(params![player_id as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut quests = Vec::new();
        for row in rows {
            let (quest_id, completed) = row?;
            let quest_id = u16::try_from(quest_id).map_err(|_| {
                PersistenceError::InvalidQuestState("quest id does not fit u16".into())
            })?;
            if quest_id == 0 {
                return Err(PersistenceError::InvalidQuestState(
                    "persisted quest id must be nonzero".into(),
                ));
            }
            let completed = match completed {
                0 => false,
                1 => true,
                _ => {
                    return Err(PersistenceError::InvalidQuestState(
                        "completed flag must be zero or one".into(),
                    ))
                }
            };
            quests.push((quest_id, completed));
        }
        Ok(quests)
    }

    /// Returns the player's persisted blessing count (0 through the classic ceiling of five)
    /// and promotion flag. These are typed foundations for the audited default death-loss
    /// reduction and promoted-vocation behavior; neither formula runs yet.
    pub fn player_blessing_state(&self, player_id: u64) -> Result<(u8, bool), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let row = self.connection.query_row(
            "SELECT bless_count, promoted FROM players WHERE id = ?1",
            params![player_id as i64],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let (bless_raw, promoted_raw) = row;
        let bless_count = u8::try_from(bless_raw).map_err(|_| {
            PersistenceError::InvalidLifecycleRecord("bless count does not fit u8".into())
        })?;
        if bless_count > MAX_PLAYER_BLESSINGS {
            return Err(PersistenceError::InvalidLifecycleRecord(format!(
                "bless count exceeds {MAX_PLAYER_BLESSINGS}"
            )));
        }
        let promoted = match promoted_raw {
            0 => false,
            _ => true,
        };
        Ok((bless_count, promoted))
    }

    /// Persists one player's blessing count within the classic zero-to-five bound.
    pub fn set_player_blessings(
        &mut self,
        player_id: u64,
        bless_count: u8,
    ) -> Result<(), PersistenceError> {
        if bless_count > MAX_PLAYER_BLESSINGS {
            return Err(PersistenceError::InvalidLifecycleRecord(format!(
                "bless count exceeds {MAX_PLAYER_BLESSINGS}"
            )));
        }
        self.connection.execute(
            "UPDATE players SET bless_count = ?1 WHERE id = ?2",
            params![i64::from(bless_count), player_id as i64],
        )?;
        Ok(())
    }

    /// Persists one player's promotion flag.
    pub fn set_player_promoted(
        &mut self,
        player_id: u64,
        promoted: bool,
    ) -> Result<(), PersistenceError> {
        self.connection.execute(
            "UPDATE players SET promoted = ?1 WHERE id = ?2",
            params![i64::from(u8::from(promoted)), player_id as i64],
        )?;
        Ok(())
    }

    /// Replaces every persisted party row with one bounded snapshot of (leader, non-leader
    /// members) records in a single SQLite transaction. Leaders must not appear in member
    /// lists; every referenced player must exist. An empty slice clears all party rows.
    pub fn replace_player_parties(
        &mut self,
        snapshots: &[(u64, Vec<u64>)],
    ) -> Result<(), PersistenceError> {
        let mut seen: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
        for (leader_id, members) in snapshots {
            if members.contains(leader_id) {
                return Err(PersistenceError::InvalidPartySnapshot(format!(
                    "members list for leader {leader_id} contains the leader"
                )));
            }
            if !seen.insert(*leader_id) {
                return Err(PersistenceError::InvalidPartySnapshot(format!(
                    "duplicate leader {leader_id}"
                )));
            }
            for member in members {
                if !seen.insert(*member) {
                    return Err(PersistenceError::InvalidPartySnapshot(format!(
                        "player {member} appears in multiple parties"
                    )));
                }
            }
        }
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM player_parties", [])?;
        for (leader_id, members) in snapshots {
            for member in members {
                tx.execute(
                    "INSERT INTO player_parties (player_id, party_leader_id) VALUES (?1, ?2)",
                    params![*member as i64, *leader_id as i64],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Returns the persisted party leader for one player, if any.
    pub fn party_leader_of(&self, player_id: u64) -> Result<Option<u64>, PersistenceError> {
        let leader = self.connection.query_row(
            "SELECT party_leader_id FROM player_parties WHERE player_id = ?1",
            params![player_id as i64],
            |row| row.get::<_, i64>(0),
        );
        match leader {
            Ok(raw) => Ok(Some(u64::try_from(raw).map_err(|_| {
                PersistenceError::InvalidPartySnapshot("persisted leader does not fit u64".into())
            })?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns every persisted member of one stored leader's party, sorted.
    pub fn party_members_of(&self, leader_id: u64) -> Result<Vec<u64>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT player_id FROM player_parties WHERE party_leader_id = ?1 ORDER BY player_id",
        )?;
        let rows = statement.query_map(params![leader_id as i64], |row| row.get::<_, i64>(0))?;
        let mut members = Vec::new();
        for row in rows {
            let raw = row?;
            members.push(u64::try_from(raw).map_err(|_| {
                PersistenceError::InvalidPartySnapshot("persisted member does not fit u64".into())
            })?);
        }
        Ok(members)
    }
}
