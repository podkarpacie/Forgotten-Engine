//! Party and social-group management for the authoritative world state: session-local
//! invitations, membership, leadership transfers, shared-experience eligibility and
//! activity tracking, and display-relation resolution for the client party UI.
//! The party model is session-local; persistence and packet delivery are host concerns.

use super::*;

impl WorldState {
    /// Creates one session-local party invitation. A player can lead one party or belong to one
    /// party, while an unjoined player may hold invitations from several leaders. Party packets,
    /// capacity limits, and policy hooks remain host concerns.
    pub fn invite_to_party(&mut self, leader_id: u64, invitee_id: u64) -> Result<(), CoreError> {
        self.ensure_party_player_exists(leader_id)?;
        self.ensure_party_player_exists(invitee_id)?;
        if leader_id == invitee_id {
            return Err(CoreError::SelfInteractionNotAllowed(leader_id));
        }
        if self.party_memberships.contains_key(&leader_id) {
            return Err(CoreError::PlayerAlreadyInParty(leader_id));
        }
        if self.party_leaders.contains(&invitee_id)
            || self.party_memberships.contains_key(&invitee_id)
        {
            return Err(CoreError::PlayerAlreadyInParty(invitee_id));
        }

        let invitations = self.party_invitations.entry(invitee_id).or_default();
        if !invitations.insert(leader_id) {
            return Err(CoreError::DuplicatePartyInvitation {
                leader_id,
                invitee_id,
            });
        }
        self.party_leaders.insert(leader_id);
        self.mark_changed();
        Ok(())
    }

    /// Accepts one pending invitation and clears every competing invitation held by the joining
    /// player. The relationship remains memory-only and is removed when the live player leaves.
    pub fn accept_party_invitation(
        &mut self,
        invitee_id: u64,
        leader_id: u64,
    ) -> Result<(), CoreError> {
        self.ensure_party_player_exists(invitee_id)?;
        self.ensure_party_player_exists(leader_id)?;
        let invitation_exists = self
            .party_invitations
            .get(&invitee_id)
            .is_some_and(|leaders| leaders.contains(&leader_id));
        if !invitation_exists {
            return Err(CoreError::PartyInvitationNotFound {
                leader_id,
                invitee_id,
            });
        }
        if self.party_leaders.contains(&invitee_id)
            || self.party_memberships.contains_key(&invitee_id)
        {
            return Err(CoreError::PlayerAlreadyInParty(invitee_id));
        }
        if !self.party_leaders.contains(&leader_id) {
            return Err(CoreError::PlayerNotInParty(leader_id));
        }

        let inviting_leaders = self
            .party_invitations
            .remove(&invitee_id)
            .unwrap_or_default();
        self.party_memberships.insert(invitee_id, leader_id);
        for invited_by in inviting_leaders {
            if invited_by != leader_id {
                self.disband_party_if_empty(invited_by);
            }
        }
        self.mark_changed();
        Ok(())
    }

    /// Removes one player from a live party. When a leader leaves, the lowest player ID among
    /// the current members becomes the deterministic replacement leader. A leader without members
    /// disbands its invitation-only party.
    pub fn leave_party(&mut self, player_id: u64) -> Result<(), CoreError> {
        self.ensure_party_player_exists(player_id)?;
        if self.party_leaders.contains(&player_id) {
            self.remove_party_leader(player_id);
        } else if let Some(leader_id) = self.party_memberships.remove(&player_id) {
            self.disband_party_if_empty(leader_id);
        } else {
            return Err(CoreError::PlayerNotInParty(player_id));
        }
        self.mark_changed();
        Ok(())
    }

    /// Transfers one active party from its current leader to one of its current live members.
    /// Pending invitations stay attached to the same party under the new leader; hook vetoes,
    /// client shields, shared experience, and gameplay delivery remain host concerns.
    pub fn transfer_party_leadership(
        &mut self,
        leader_id: u64,
        new_leader_id: u64,
    ) -> Result<(), CoreError> {
        self.ensure_party_player_exists(leader_id)?;
        self.ensure_party_player_exists(new_leader_id)?;
        if leader_id == new_leader_id {
            return Err(CoreError::SelfInteractionNotAllowed(leader_id));
        }
        if !self.party_leaders.contains(&leader_id) {
            return Err(CoreError::PlayerNotInParty(leader_id));
        }
        if self.party_memberships.get(&new_leader_id).copied() != Some(leader_id) {
            return Err(CoreError::PartyLeadershipTargetNotMember {
                leader_id,
                new_leader_id,
            });
        }

        self.party_leaders.remove(&leader_id);
        self.party_leaders.insert(new_leader_id);
        if self.party_shared_experience_requested.remove(&leader_id) {
            self.party_shared_experience_requested.insert(new_leader_id);
        }
        self.party_memberships.remove(&new_leader_id);
        for member_leader_id in self.party_memberships.values_mut() {
            if *member_leader_id == leader_id {
                *member_leader_id = new_leader_id;
            }
        }
        self.party_memberships.insert(leader_id, new_leader_id);
        for leaders in self.party_invitations.values_mut() {
            if leaders.remove(&leader_id) {
                leaders.insert(new_leader_id);
            }
        }
        self.mark_changed();
        Ok(())
    }

    /// Revokes exactly one invitation and removes an otherwise empty invitation-only party.
    pub fn revoke_party_invitation(
        &mut self,
        leader_id: u64,
        invitee_id: u64,
    ) -> Result<(), CoreError> {
        self.ensure_party_player_exists(leader_id)?;
        self.ensure_party_player_exists(invitee_id)?;
        if !self.party_leaders.contains(&leader_id) {
            return Err(CoreError::PlayerNotInParty(leader_id));
        }

        let invitations_empty = {
            let invitations = self.party_invitations.get_mut(&invitee_id).ok_or(
                CoreError::PartyInvitationNotFound {
                    leader_id,
                    invitee_id,
                },
            )?;
            if !invitations.remove(&leader_id) {
                return Err(CoreError::PartyInvitationNotFound {
                    leader_id,
                    invitee_id,
                });
            }
            invitations.is_empty()
        };
        if invitations_empty {
            self.party_invitations.remove(&invitee_id);
        }
        self.disband_party_if_empty(leader_id);
        self.mark_changed();
        Ok(())
    }

    /// Returns the current live party leader for either a leader or member, or `None` when the
    /// active player has no party. Invitation-only players intentionally remain outside a party
    /// until they accept one invitation.
    pub fn player_party_leader(&self, player_id: u64) -> Result<Option<u64>, CoreError> {
        self.ensure_party_player_exists(player_id)?;
        if self.party_leaders.contains(&player_id) {
            return Ok(Some(player_id));
        }
        Ok(self.party_memberships.get(&player_id).copied())
    }

    /// Returns member IDs in deterministic ascending order for a known live party leader.
    pub fn player_party_members(&self, leader_id: u64) -> Result<Vec<u64>, CoreError> {
        self.ensure_party_player_exists(leader_id)?;
        if !self.party_leaders.contains(&leader_id) {
            return Err(CoreError::PlayerNotInParty(leader_id));
        }
        Ok(self.party_member_ids(leader_id))
    }

    /// Exports every live party as deterministic (leader, sorted non-leader members) records
    /// for bounded persistence snapshots, matching `player_party_members` semantics. Empty
    /// output means no live parties.
    pub fn party_snapshots(&self) -> Vec<(u64, Vec<u64>)> {
        self.party_leaders
            .iter()
            .map(|leader| (*leader, self.party_member_ids(*leader)))
            .collect()
    }

    /// Restores one persisted party snapshot into the session-local party state. The members
    /// slice lists non-leader members (leadership is implicit), every player must exist,
    /// duplicates are rejected, and rows that would overwrite an existing live party are
    /// rejected so a stale snapshot can never clobber runtime state formed after the snapshot.
    pub fn restore_party_snapshot(
        &mut self,
        leader_id: u64,
        members: &[u64],
    ) -> Result<(), CoreError> {
        let mut unique = members.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != members.len() {
            return Err(CoreError::InvalidPartySnapshot(
                "party snapshot contains duplicate members".into(),
            ));
        }
        if unique.contains(&leader_id) {
            return Err(CoreError::InvalidPartySnapshot(
                "party snapshot members must not list the leader".into(),
            ));
        }
        self.ensure_party_player_exists(leader_id)?;
        if self.party_leaders.contains(&leader_id)
            || self.party_memberships.contains_key(&leader_id)
        {
            return Err(CoreError::InvalidPartySnapshot(
                "leader already holds live party state".into(),
            ));
        }
        for member in &unique {
            self.ensure_party_player_exists(*member)?;
            if self.party_leaders.contains(member) || self.party_memberships.contains_key(member) {
                return Err(CoreError::InvalidPartySnapshot(
                    "member already holds live party state".into(),
                ));
            }
        }
        for member in &unique {
            self.party_memberships.insert(*member, leader_id);
        }
        self.party_leaders.insert(leader_id);
        Ok(())
    }

    /// Attaches one unaffiliated player directly to a live leader's party without an
    /// invitation round-trip. Used by persisted-party hydration on relog; ordinary gameplay
    /// must keep using invite/accept.
    pub fn add_existing_party_member(
        &mut self,
        leader_id: u64,
        player_id: u64,
    ) -> Result<(), CoreError> {
        if player_id == leader_id {
            return Err(CoreError::InvalidPartySnapshot(
                "leader cannot be added as a member of itself".into(),
            ));
        }
        self.ensure_party_player_exists(leader_id)?;
        self.ensure_party_player_exists(player_id)?;
        if !self.party_leaders.contains(&leader_id) {
            return Err(CoreError::PlayerNotInParty(leader_id));
        }
        if self.party_leaders.contains(&player_id)
            || self.party_memberships.contains_key(&player_id)
        {
            return Err(CoreError::PlayerNotInPartyFree(player_id));
        }
        self.party_memberships.insert(player_id, leader_id);
        Ok(())
    }

    /// Enables or disables the session-local shared-experience request for one current leader.
    /// The result describes eligibility only; it does not distribute experience or emit a packet.
    pub fn set_party_shared_experience_requested(
        &mut self,
        leader_id: u64,
        requested: bool,
        rules: PartySharedExperienceRules,
    ) -> Result<PartySharedExperienceState, CoreError> {
        self.ensure_party_player_exists(leader_id)?;
        if !self.party_leaders.contains(&leader_id) {
            return Err(CoreError::PlayerNotInParty(leader_id));
        }
        let changed = if requested {
            self.party_shared_experience_requested.insert(leader_id)
        } else {
            self.party_shared_experience_requested.remove(&leader_id)
        };
        if changed {
            self.mark_changed();
        }
        self.party_shared_experience_state(leader_id, rules)
    }

    /// Records one explicit bounded participant-activity observation for the current world tick.
    /// Combat source, damage, healing, flags, packet delivery, and persistence stay outside this
    /// first eligibility model.
    pub fn record_party_shared_experience_activity(
        &mut self,
        player_id: u64,
    ) -> Result<(), CoreError> {
        self.ensure_party_player_exists(player_id)?;
        if self.player_party_leader(player_id)?.is_none() {
            return Err(CoreError::PlayerNotInParty(player_id));
        }
        if self
            .party_shared_experience_activity_ticks
            .insert(player_id, self.tick)
            != Some(self.tick)
        {
            self.mark_changed();
        }
        Ok(())
    }

    /// Returns the session-local request and its deterministic current eligibility. A request is
    /// active only when there is at least one member and every participant satisfies the bounded
    /// level, leader-relative range/floor, and recent-activity inputs.
    pub fn party_shared_experience_state(
        &self,
        leader_id: u64,
        rules: PartySharedExperienceRules,
    ) -> Result<PartySharedExperienceState, CoreError> {
        self.ensure_party_player_exists(leader_id)?;
        if !self.party_leaders.contains(&leader_id) {
            return Err(CoreError::PlayerNotInParty(leader_id));
        }
        let requested = self.party_shared_experience_requested.contains(&leader_id);
        if !requested {
            return Ok(PartySharedExperienceState {
                requested,
                eligibility: PartySharedExperienceEligibility::NotRequested,
            });
        }
        let member_ids = self.party_member_ids(leader_id);
        if member_ids.is_empty() {
            return Ok(PartySharedExperienceState {
                requested,
                eligibility: PartySharedExperienceEligibility::EmptyParty,
            });
        }
        let participant_ids = std::iter::once(leader_id)
            .chain(member_ids)
            .collect::<Vec<_>>();
        let highest_level = participant_ids
            .iter()
            .filter_map(|id| self.players.get(id).map(|player| player.level))
            .max()
            .unwrap_or_default();
        let minimum_level = highest_level.saturating_mul(2).saturating_add(2) / 3;
        // Fail closed: any party member (leader included) missing from the live player map
        // means inconsistent party state, so shared experience is denied rather than guessed.
        let Some(leader) = self.players.get(&leader_id) else {
            return Ok(PartySharedExperienceState {
                requested,
                eligibility: PartySharedExperienceEligibility::LevelSpreadTooLarge,
            });
        };
        for participant_id in participant_ids {
            let Some(participant) = self.players.get(&participant_id) else {
                return Ok(PartySharedExperienceState {
                    requested,
                    eligibility: PartySharedExperienceEligibility::LevelSpreadTooLarge,
                });
            };
            if participant.level < minimum_level {
                return Ok(PartySharedExperienceState {
                    requested,
                    eligibility: PartySharedExperienceEligibility::LevelSpreadTooLarge,
                });
            }
            let range = participant
                .position
                .x
                .abs_diff(leader.position.x)
                .max(participant.position.y.abs_diff(leader.position.y));
            if range > rules.maximum_range
                || participant.position.z.abs_diff(leader.position.z) > rules.maximum_floor_delta
            {
                return Ok(PartySharedExperienceState {
                    requested,
                    eligibility: PartySharedExperienceEligibility::TooFarAway,
                });
            }
            let active = self
                .party_shared_experience_activity_ticks
                .get(&participant_id)
                .is_some_and(|tick| self.tick.saturating_sub(*tick) <= rules.activity_window_ticks);
            if !active {
                return Ok(PartySharedExperienceState {
                    requested,
                    eligibility: PartySharedExperienceEligibility::MemberInactive,
                });
            }
        }
        Ok(PartySharedExperienceState {
            requested,
            eligibility: PartySharedExperienceEligibility::Eligible,
        })
    }

    /// Returns deterministic ascending recipient IDs only when the current player belongs to a
    /// requested and eligible live party. `None` means callers must retain their non-shared reward
    /// behavior. This selector does not award experience or mutate party state.
    pub fn party_shared_experience_recipients(
        &self,
        player_id: u64,
        rules: PartySharedExperienceRules,
    ) -> Result<Option<Vec<u64>>, CoreError> {
        let Some(leader_id) = self.player_party_leader(player_id)? else {
            return Ok(None);
        };
        if self
            .party_shared_experience_state(leader_id, rules)?
            .eligibility
            != PartySharedExperienceEligibility::Eligible
        {
            return Ok(None);
        }
        let mut recipients = self.party_member_ids(leader_id);
        recipients.push(leader_id);
        recipients.sort_unstable();
        Ok(Some(recipients))
    }

    /// Captures the basic party display relation for every current active player in deterministic
    /// player-ID order. The caller owns client visibility, packet framing, and refresh timing.
    pub fn party_display_relations(
        &self,
        observer_id: u64,
    ) -> Result<Vec<(u64, PartyDisplayRelation)>, CoreError> {
        self.ensure_party_player_exists(observer_id)?;
        self.players
            .keys()
            .copied()
            .map(|target_id| {
                Ok((
                    target_id,
                    self.party_display_relation(observer_id, target_id)?,
                ))
            })
            .collect()
    }

    fn party_display_relation(
        &self,
        observer_id: u64,
        target_id: u64,
    ) -> Result<PartyDisplayRelation, CoreError> {
        self.ensure_party_player_exists(observer_id)?;
        self.ensure_party_player_exists(target_id)?;
        let observer_leader = self.player_party_leader(observer_id)?;
        let target_leader = self.player_party_leader(target_id)?;

        if let Some(leader_id) = observer_leader {
            if target_id == leader_id {
                return Ok(PartyDisplayRelation::Leader);
            }
            if target_leader == Some(leader_id) {
                return Ok(PartyDisplayRelation::Member);
            }
            if self
                .party_invitations
                .get(&target_id)
                .is_some_and(|leaders| leaders.contains(&leader_id))
            {
                return Ok(PartyDisplayRelation::InvitationToLeader);
            }
        }

        if self
            .party_invitations
            .get(&observer_id)
            .is_some_and(|leaders| leaders.contains(&target_id))
        {
            return Ok(PartyDisplayRelation::InvitationFromLeader);
        }
        Ok(PartyDisplayRelation::None)
    }

    fn ensure_party_player_exists(&self, player_id: u64) -> Result<(), CoreError> {
        if self.players.contains_key(&player_id) {
            Ok(())
        } else {
            Err(CoreError::UnknownPlayer(player_id))
        }
    }

    fn party_member_ids(&self, leader_id: u64) -> Vec<u64> {
        self.party_memberships
            .iter()
            .filter_map(|(&member_id, &member_leader_id)| {
                (member_leader_id == leader_id).then_some(member_id)
            })
            .collect()
    }

    fn party_has_invitations(&self, leader_id: u64) -> bool {
        self.party_invitations
            .values()
            .any(|leaders| leaders.contains(&leader_id))
    }

    fn remove_party_leader(&mut self, leader_id: u64) {
        let member_ids = self.party_member_ids(leader_id);
        let Some(new_leader_id) = member_ids.first().copied() else {
            self.disband_party(leader_id);
            return;
        };

        self.party_leaders.remove(&leader_id);
        self.party_leaders.insert(new_leader_id);
        if self.party_shared_experience_requested.remove(&leader_id) {
            self.party_shared_experience_requested.insert(new_leader_id);
        }
        self.party_memberships.remove(&new_leader_id);
        for member_id in member_ids {
            if member_id != new_leader_id {
                self.party_memberships.insert(member_id, new_leader_id);
            }
        }
        for leaders in self.party_invitations.values_mut() {
            if leaders.remove(&leader_id) {
                leaders.insert(new_leader_id);
            }
        }
    }

    fn disband_party_if_empty(&mut self, leader_id: u64) {
        if self.party_leaders.contains(&leader_id)
            && self.party_member_ids(leader_id).is_empty()
            && !self.party_has_invitations(leader_id)
        {
            self.disband_party(leader_id);
        }
    }

    fn disband_party(&mut self, leader_id: u64) {
        let member_ids = self.party_member_ids(leader_id);
        self.party_leaders.remove(&leader_id);
        self.party_shared_experience_requested.remove(&leader_id);
        self.party_shared_experience_activity_ticks
            .remove(&leader_id);
        for member_id in &member_ids {
            self.party_shared_experience_activity_ticks
                .remove(member_id);
        }
        self.party_memberships
            .retain(|_, member_leader_id| *member_leader_id != leader_id);
        self.party_invitations.values_mut().for_each(|leaders| {
            leaders.remove(&leader_id);
        });
        self.party_invitations
            .retain(|_, leaders| !leaders.is_empty());
    }

    pub(crate) fn clear_player_party_state(&mut self, player_id: u64) {
        self.party_shared_experience_activity_ticks
            .remove(&player_id);
        if self.party_leaders.contains(&player_id) {
            self.remove_party_leader(player_id);
        } else if let Some(leader_id) = self.party_memberships.remove(&player_id) {
            self.disband_party_if_empty(leader_id);
        }

        let inviting_leaders = self
            .party_invitations
            .remove(&player_id)
            .unwrap_or_default();
        for leader_id in inviting_leaders {
            self.disband_party_if_empty(leader_id);
        }
    }

    /// Invalidates only target and follow references to a player that is no longer an active
    /// interaction candidate. The player may be removed or remain present with a recorded death
    /// state; packet delivery and broader combat cancellation remain host concerns.
    pub(crate) fn clear_player_interaction_references(&mut self, unavailable_player_id: u64) {
        self.player_interactions.retain(|_, intent| {
            if intent.target_player_id == Some(unavailable_player_id) {
                intent.target_player_id = None;
            }
            if intent.follow_player_id == Some(unavailable_player_id) {
                intent.follow_player_id = None;
            }
            intent.target_player_id.is_some()
                || intent.target_static_creature_id.is_some()
                || intent.follow_player_id.is_some()
        });
    }
}
