//! Guild management on the persistent database: creation, membership, ranks,
//! invitations, ownership transfer, nickname updates, and dissolution.
//! All methods operate within transactions under the shared world lock.

use super::*;

impl EngineDatabase {
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
        let leader_rank_id = leader_rank_id.ok_or_else(|| {
            PersistenceError::InvalidGuildRecord(
                "fixed guild rank provisioning did not produce a leader rank".into(),
            )
        })?;
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
    /// Reads one guild's display name and message-of-the-day for channel-list and login
    /// delivery (plan v49 slice 19). `None` when the guild does not exist.
    pub fn guild_name_and_motd(
        &self,
        guild_id: u64,
    ) -> Result<Option<(String, String)>, PersistenceError> {
        self.connection
            .query_row(
                "SELECT name, motd FROM guilds WHERE id = ?1",
                params![guild_id as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(PersistenceError::Sql)
    }

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

    /// Lists every member of one guild (player ids only), bounded by a sane ceiling so a
    /// malformed membership table cannot explode memory.
    pub fn guild_member_ids(&self, guild_id: u64) -> Result<Vec<u64>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("SELECT player_id FROM guild_membership WHERE guild_id = ?1 LIMIT 500")?;
        let rows = statement.query_map(params![guild_id as i64], |row| row.get::<_, i64>(0))?;
        let mut members = Vec::new();
        for row in rows {
            members.push(row? as u64);
        }
        Ok(members)
    }
}
