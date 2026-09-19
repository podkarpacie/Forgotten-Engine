//! Party and social-group management on the shared world: snapshots for persistence flushes,
//! hydration-path membership attachment, invitations/leadership/leave, shared-experience
//! request and activity tracking, and the client party-shield display frames.

use super::*;

impl SharedNativeWorld {
    /// Returns every live party as deterministic (leader, non-leader members) records for
    /// bounded persistence flushes.
    pub fn party_snapshots(&self) -> Result<Vec<(u64, Vec<u64>)>, HostError> {
        Ok(self.lock()?.party_snapshots())
    }

    /// Attaches an unaffiliated player directly to a live leader's party (hydration path).
    pub fn add_existing_party_member(
        &self,
        leader_id: u64,
        player_id: u64,
    ) -> Result<(), HostError> {
        self.lock()?
            .add_existing_party_member(leader_id, player_id)
            .map_err(HostError::Core)?;
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// Rebuilds one persisted party when neither participant holds live party state.
    pub fn restore_party_snapshot(&self, leader_id: u64, members: &[u64]) -> Result<(), HostError> {
        self.lock()?
            .restore_party_snapshot(leader_id, members)
            .map_err(HostError::Core)?;
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn party_display_frames(
        &self,
        profile: &NativeOtClientProfile,
        observer_id: u64,
    ) -> Result<Vec<Frame>, HostError> {
        self.lock()?
            .party_display_relations(observer_id)
            .map_err(HostError::Core)?
            .into_iter()
            .map(|(target_id, relation)| {
                let shield = match relation {
                    PartyDisplayRelation::None => NativeOtClientClassicPartyShield::None,
                    PartyDisplayRelation::InvitationFromLeader => {
                        NativeOtClientClassicPartyShield::InvitationFromLeader
                    }
                    PartyDisplayRelation::InvitationToLeader => {
                        NativeOtClientClassicPartyShield::InvitationToLeader
                    }
                    PartyDisplayRelation::Member => NativeOtClientClassicPartyShield::Member,
                    PartyDisplayRelation::Leader => NativeOtClientClassicPartyShield::Leader,
                };
                encode_native_otclient_creature_party_shield(
                    profile,
                    native_player_id(target_id)?,
                    shield,
                )
                .map_err(HostError::Protocol)
            })
            .collect()
    }

    pub(crate) fn invite_to_party(&self, leader_id: u64, invitee_id: u64) -> Result<(), HostError> {
        self.lock()?
            .invite_to_party(leader_id, invitee_id)
            .map_err(HostError::Core)?;
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn accept_party_invitation(
        &self,
        invitee_id: u64,
        leader_id: u64,
    ) -> Result<(), HostError> {
        self.lock()?
            .accept_party_invitation(invitee_id, leader_id)
            .map_err(HostError::Core)?;
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn revoke_party_invitation(
        &self,
        leader_id: u64,
        invitee_id: u64,
    ) -> Result<(), HostError> {
        self.lock()?
            .revoke_party_invitation(leader_id, invitee_id)
            .map_err(HostError::Core)?;
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn transfer_party_leadership(
        &self,
        leader_id: u64,
        new_leader_id: u64,
    ) -> Result<(), HostError> {
        self.lock()?
            .transfer_party_leadership(leader_id, new_leader_id)
            .map_err(HostError::Core)?;
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn leave_party(&self, player_id: u64) -> Result<(), HostError> {
        self.lock()?
            .leave_party(player_id)
            .map_err(HostError::Core)?;
        self.party_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn set_party_shared_experience_requested(
        &self,
        leader_id: u64,
        requested: bool,
        rules: PartySharedExperienceRules,
    ) -> Result<(), HostError> {
        self.lock()?
            .set_party_shared_experience_requested(leader_id, requested, rules)
            .map_err(HostError::Core)?;
        Ok(())
    }

    pub(crate) fn record_party_shared_experience_activity(
        &self,
        player_id: u64,
    ) -> Result<bool, HostError> {
        let mut world = self.lock()?;
        if world
            .player_party_leader(player_id)
            .map_err(HostError::Core)?
            .is_none()
        {
            return Ok(false);
        }
        world
            .record_party_shared_experience_activity(player_id)
            .map_err(HostError::Core)?;
        Ok(true)
    }
}

/// Applies one party invitation: resolves the native target id, then records the invitation.
/// An unresolvable id emits a diagnostic without effect.
pub(crate) fn apply_native_party_invite_action(
    ctx: &mut SessionContext<'_>,
    native_target_id: u32,
) -> Result<(), HostError> {
    let Some(invitee_id) = native_player_id_to_character_id(native_target_id) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=party-invite outcome=rejected-invalid-native-player-id",
        );
        return Ok(());
    };
    let outcome = ctx
        .shared_world
        .invite_to_party(ctx.character_id, invitee_id);
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        if outcome.is_ok() {
            "action=party-invite outcome=authoritative-invitation-created"
        } else {
            "action=party-invite outcome=rejected-core-invariant"
        },
    );
    Ok(())
}

/// Applies one party join: resolves the leader id, then records membership.
pub(crate) fn apply_native_party_join_action(
    ctx: &mut SessionContext<'_>,
    native_target_id: u32,
) -> Result<(), HostError> {
    let Some(leader_id) = native_player_id_to_character_id(native_target_id) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=party-join outcome=rejected-invalid-native-player-id",
        );
        return Ok(());
    };
    let outcome = ctx
        .shared_world
        .accept_party_invitation(ctx.character_id, leader_id);
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        if outcome.is_ok() {
            "action=party-join outcome=authoritative-membership-created"
        } else {
            "action=party-join outcome=rejected-core-invariant"
        },
    );
    Ok(())
}

/// Applies one invitation revocation.
pub(crate) fn apply_native_party_revoke_invitation_action(
    ctx: &mut SessionContext<'_>,
    native_target_id: u32,
) -> Result<(), HostError> {
    let Some(invitee_id) = native_player_id_to_character_id(native_target_id) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=party-revoke-invitation outcome=rejected-invalid-native-player-id",
        );
        return Ok(());
    };
    let outcome = ctx
        .shared_world
        .revoke_party_invitation(ctx.character_id, invitee_id);
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        if outcome.is_ok() {
            "action=party-revoke-invitation outcome=authoritative-invitation-removed"
        } else {
            "action=party-revoke-invitation outcome=rejected-core-invariant"
        },
    );
    Ok(())
}

/// Applies one leadership transfer. Narrow enough for direct parameters.
pub(crate) fn apply_native_party_pass_leadership_action(
    shared_world: &SharedNativeWorld,
    character_id: u64,
    native_target_id: u32,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    let Some(new_leader_id) = native_player_id_to_character_id(native_target_id) else {
        native_diagnostic(
            extended_diagnostics,
            peer,
            "action=party-pass-leadership outcome=rejected-invalid-native-player-id",
        );
        return Ok(());
    };
    let outcome = shared_world.transfer_party_leadership(character_id, new_leader_id);
    native_diagnostic(
        extended_diagnostics,
        peer,
        if outcome.is_ok() {
            "action=party-pass-leadership outcome=authoritative-leadership-transferred"
        } else {
            "action=party-pass-leadership outcome=rejected-core-invariant"
        },
    );
    Ok(())
}

/// Applies one party leave. Narrow enough for direct parameters.
pub(crate) fn apply_native_party_leave_action(
    shared_world: &SharedNativeWorld,
    character_id: u64,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    let outcome = shared_world.leave_party(character_id);
    native_diagnostic(
        extended_diagnostics,
        peer,
        if outcome.is_ok() {
            "action=party-leave outcome=authoritative-membership-removed"
        } else {
            "action=party-leave outcome=rejected-core-invariant"
        },
    );
    Ok(())
}

/// Applies one shared-experience request: honors the operator rules when configured, else
/// records a disabled-configuration diagnostic without effect.
pub(crate) fn apply_native_party_shared_experience_action(
    ctx: &mut SessionContext<'_>,
    requested: bool,
) -> Result<(), HostError> {
    let outcome = ctx.config.party_shared_experience_rules.map_or_else(
        || {
            Err(HostError::InvalidConfiguration(
                "party shared experience is disabled by configuration".into(),
            ))
        },
        |rules| {
            ctx.shared_world.set_party_shared_experience_requested(
                ctx.character_id,
                requested,
                rules,
            )
        },
    );
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        if outcome.is_ok() {
            "action=party-shared-experience outcome=authoritative-request-updated"
        } else {
            "action=party-shared-experience outcome=rejected-disabled-or-core-invariant"
        },
    );
    Ok(())
}
