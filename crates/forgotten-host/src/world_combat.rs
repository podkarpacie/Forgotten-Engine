//! Combat and death handling on the shared world: melee damage application, declarative
//! spell casting, fixed-percent death loss, and combat event processing. All methods hold
//! the shared lock and persist through the caller-provided database where required.

use super::*;

impl SharedNativeWorld {
    pub(crate) fn apply_player_melee_damage(
        &self,
        attacker_id: u64,
        target_id: u64,
        damage: u16,
    ) -> Result<(forgotten_core::PlayerDamageOutcome, PlayerVitals), HostError> {
        let mut world = self.lock()?;
        let outcome = world
            .apply_player_melee_damage(attacker_id, target_id, damage)
            .map_err(HostError::Core)?;
        let vitals = world.player_vitals(target_id).map_err(HostError::Core)?;
        if outcome.applied_damage > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok((outcome, vitals))
    }

    /// Applies one already-validated fixed loss percentage to a player with an accepted death
    /// state. The caller owns policy selection and persistence; this synchronized boundary keeps
    /// the core transition authoritative and refreshes only the state epochs affected by its
    /// level, skill, magic-level, and vitality changes.
    pub(crate) fn apply_fixed_percent_death_loss(
        &self,
        player_id: u64,
        percent: u8,
        rules: PlayerProgressionRules,
    ) -> Result<forgotten_core::PlayerDeathLossOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_fixed_percent_death_loss(player_id, percent, rules)
            .map_err(HostError::Core)?;
        self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies one bounded player melee hit and, only when it would defeat a target with a
    /// validated hydrated town, enters the authoritative death state in the same world lock.
    /// Client death screens, loss application, persistence of death state, and respawn packets
    /// remain outside this transition.
    pub(crate) fn apply_player_melee_damage_with_death(
        &self,
        attacker_id: u64,
        target_id: u64,
        damage: u16,
        world_map: &WorldMap,
    ) -> Result<
        (
            forgotten_core::PlayerDamageOutcome,
            PlayerVitals,
            Option<forgotten_core::PlayerRespawnState>,
        ),
        HostError,
    > {
        let event = PlayerCombatEvent::adjacent_melee(
            attacker_id,
            target_id,
            CombatDamageType::Physical,
            damage,
            CombatAttackTiming::new(1).map_err(HostError::Core)?,
        )
        .map_err(HostError::Core)?;
        let (outcome, vitals, death_state) =
            self.apply_player_combat_event_with_death(event, world_map)?;
        Ok((outcome.damage, vitals, death_state))
    }

    /// Applies one typed bounded event and enters the existing server-side death state only for a
    /// validated potentially lethal target. The precheck keeps invalid temple assignment from
    /// partially mutating combat state; client delivery remains a separate responsibility.
    pub(crate) fn apply_player_combat_event_with_death(
        &self,
        event: PlayerCombatEvent,
        world_map: &WorldMap,
    ) -> Result<
        (
            PlayerCombatEventOutcome,
            PlayerVitals,
            Option<forgotten_core::PlayerRespawnState>,
        ),
        HostError,
    > {
        let mut world = self.lock()?;
        let vitals_before = world
            .player_vitals(event.target_id)
            .map_err(HostError::Core)?;
        let town_id = world
            .player_town(event.target_id)
            .map_err(HostError::Core)?;
        if event.requested_damage > 0 && vitals_before.health <= event.requested_damage {
            if town_id == 0 {
                return Err(HostError::Core(
                    forgotten_core::CoreError::PlayerTownUnassigned(event.target_id),
                ));
            }
            if world_map.temple_position_for_town(town_id).is_none() {
                return Err(HostError::Core(forgotten_core::CoreError::UnknownTown(
                    town_id,
                )));
            }
        }
        let outcome = world
            .apply_player_combat_event(event)
            .map_err(HostError::Core)?;
        let death_state = if outcome.damage.defeated {
            // PvP lethal transition: record an unjustified kill against the attacker when the
            // combat event was player-caused (attacker != target). Monster and condition deaths
            // route through different transitions and never increment frags.
            let frags = if event.attacker_id != event.target_id {
                world.record_player_frag(event.attacker_id)
            } else {
                world.player_frag_count(event.target_id)
            };
            let mut state = world
                .apply_player_death(event.target_id, town_id, world_map)
                .map_err(HostError::Core)?;
            let _ = frags;
            Some(state)
        } else {
            None
        };
        let vitals = world
            .player_vitals(event.target_id)
            .map_err(HostError::Core)?;
        if outcome.damage.applied_damage > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok((outcome, vitals, death_state))
    }

    /// Resolves one scriptless declared spell into the core's resource-and-cooldown event. This
    /// method has no protocol route and makes no target, formula, effect, persistence, or Lua
    /// claim; it is a synchronized host boundary for later profile-approved invocation paths.
    pub(crate) fn apply_declarative_spell_cast(
        &self,
        caster_id: u64,
        spell_id: u16,
        catalog: &DeclarativeSpellCatalog,
    ) -> Result<PlayerSpellCastOutcome, HostError> {
        let definition = catalog.get(spell_id).ok_or_else(|| {
            HostError::InvalidConfiguration(
                "declared spell ID is not present in host catalog".into(),
            )
        })?;
        let event = definition.cast_event(caster_id).map_err(|_| {
            HostError::InvalidConfiguration(
                "validated declarative spell did not build a cast event".into(),
            )
        })?;
        let outcome = self
            .lock()?
            .apply_player_spell_cast_event(event)
            .map_err(HostError::Core)?;
        self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }
}
