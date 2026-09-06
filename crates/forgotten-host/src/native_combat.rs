//! Native 740 combat, death-loss, and static-creature defeat processing: player
//! interaction (target/follow selection), declarative spell casts, selected melee,
//! death-loss application, condition state bits, NPC dialogue, and defeat corpse
//! spawning with experience awards.

use super::*;

pub(crate) fn apply_native_player_interaction(
    shared_world: &SharedNativeWorld,
    source_player_id: u64,
    native_selected_id: u32,
    kind: NativePlayerInteractionKind,
    extended_diagnostics: bool,
) -> Result<NativePlayerInteractionOutcome, HostError> {
    if native_selected_id == 0 {
        let result = match kind {
            NativePlayerInteractionKind::Target => {
                shared_world.set_player_target(source_player_id, None)
            }
            NativePlayerInteractionKind::Follow => {
                shared_world.set_player_follow(source_player_id, None)
            }
        };
        return result.map(|_| NativePlayerInteractionOutcome::Applied);
    }
    if let Some(selected_player_id) = native_player_id_to_character_id(native_selected_id) {
        let result = match kind {
            NativePlayerInteractionKind::Target => {
                shared_world.set_player_target(source_player_id, Some(selected_player_id))
            }
            NativePlayerInteractionKind::Follow => {
                shared_world.set_player_follow(source_player_id, Some(selected_player_id))
            }
        };
        return match result {
            Ok(_) => Ok(NativePlayerInteractionOutcome::Applied),
            Err(HostError::Core(forgotten_core::CoreError::UnknownPlayer(_)))
            | Err(HostError::Core(forgotten_core::CoreError::SelfInteractionNotAllowed(_)))
            | Err(HostError::Core(forgotten_core::CoreError::SelectedPlayerIsDead(_))) => {
                if extended_diagnostics {
                    eprintln!(
                        "> Native OTCv8 {:?} selection ignored native-id={native_selected_id}",
                        kind
                    );
                }
                Ok(NativePlayerInteractionOutcome::Rejected)
            }
            Err(error) => Err(error),
        };
    }
    if matches!(kind, NativePlayerInteractionKind::Target) {
        return match shared_world
            .set_player_static_target(source_player_id, Some(native_selected_id))
        {
            Ok(_) => Ok(NativePlayerInteractionOutcome::Applied),
            Err(HostError::Core(
                forgotten_core::CoreError::UnknownStaticCreature(_)
                | forgotten_core::CoreError::InactiveStaticCreature(_),
            )) => {
                if extended_diagnostics {
                    eprintln!(
                        "> Native OTCv8 static target selection ignored native-id={native_selected_id}"
                    );
                }
                Ok(NativePlayerInteractionOutcome::Rejected)
            }
            Err(error) => Err(error),
        };
    }
    {
        if extended_diagnostics {
            eprintln!(
                "> Native OTCv8 {:?} selection deferred native-id={native_selected_id}",
                kind
            );
        }
        Ok(NativePlayerInteractionOutcome::Rejected)
    }
}

/// Clears only the existing authoritative target and follow intents for one native player. This
/// is the bounded host effect of the classic zero-payload cancel-attack/follow control; it does
/// not perform an attack, emit an effect, or change movement/fight-mode state.
pub(crate) fn cancel_native_player_attack_and_follow(
    shared_world: &SharedNativeWorld,
    player_id: u64,
) -> Result<(), HostError> {
    shared_world.set_player_target(player_id, None)?;
    shared_world.set_player_follow(player_id, None)?;
    Ok(())
}

/// Parses only FE's explicit numeric declarative-spell command. This intentionally does not
/// interpret arbitrary player speech, TFS spell words, parameters, rune use, or target text.
pub(crate) fn native_declarative_spell_command_id(message: &str) -> Option<u16> {
    const PREFIX: &str = "!fe cast ";
    let spell_id = message.strip_prefix(PREFIX)?.parse::<u16>().ok()?;
    (spell_id != 0).then_some(spell_id)
}

/// Resolves one exact, normalized public-speech keyword against the closest active validated NPC
/// in the bounded same-floor dialogue range. The result is stateless: it does not create focus,
/// parameters, quest progress, shops, travel, or a Lua callback.
pub(crate) fn resolve_native_static_npc_dialogue(
    shared_world: &SharedNativeWorld,
    player_id: u64,
    catalog: &DeclarativeNpcDialogueCatalog,
    message: &str,
) -> Result<Option<(u32, String, NativeOtClientPosition, String)>, HostError> {
    let keyword = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if keyword.is_empty() || !keyword.is_ascii() {
        return Ok(None);
    }
    let world = shared_world.lock()?;
    let player = world
        .player(player_id)
        .ok_or(forgotten_core::CoreError::UnknownPlayer(player_id))
        .map_err(HostError::Core)?;
    let active_spawns = world.active_static_spawn_collection();
    Ok(active_spawns
        .entities
        .iter()
        .filter(|entity| active_spawns.is_npc(entity.id))
        .filter(|entity| entity.position.z == player.position.z)
        .filter_map(|entity| {
            let distance = entity
                .position
                .x
                .abs_diff(player.position.x)
                .max(entity.position.y.abs_diff(player.position.y));
            if distance > MAX_NATIVE_STATIC_NPC_DIALOGUE_RANGE {
                return None;
            }
            catalog.get(&entity.name, &keyword).map(|response| {
                (
                    distance,
                    entity.id,
                    entity.name.clone(),
                    native_position(entity.position),
                    response.text.clone(),
                )
            })
        })
        .min_by_key(|(distance, id, _, _, _)| (*distance, *id))
        .map(|(_, id, name, position, text)| (id, name, position, text)))
}

/// Applies a catalog-backed spell cast and rate-scaled magic progression under the same shared
/// world lock, then durably persists the resulting mana, magic level, and exact progression
/// attempts in one SQLite transaction before publishing the vital refresh epoch.
pub(crate) fn apply_and_persist_native_declarative_spell_cast(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    caster_id: u64,
    spell_id: u16,
    catalog: &DeclarativeSpellCatalog,
    rules_by_vocation: &BTreeMap<VocationId, PlayerProgressionRules>,
    magic_rate: u32,
) -> Result<
    (
        PlayerSpellCastOutcome,
        forgotten_core::PlayerMagicAdvanceOutcome,
    ),
    HostError,
> {
    let definition = catalog.get(spell_id).ok_or_else(|| {
        HostError::InvalidConfiguration("declared spell ID is not present in host catalog".into())
    })?;
    let speed_percent = definition.speed_percent;
    let event = definition.cast_event(caster_id).map_err(|_| {
        HostError::InvalidConfiguration(
            "validated declarative spell did not build a cast event".into(),
        )
    })?;
    let mut world = shared_world.lock()?;
    let vocation = world
        .player_progression(caster_id)
        .map_err(HostError::Core)?
        .vocation;
    let rules = rules_by_vocation.get(&vocation).copied().ok_or_else(|| {
        HostError::InvalidConfiguration(format!(
            "declarative spell caster vocation {} has no validated progression rules",
            vocation.value()
        ))
    })?;
    let cast = world
        .apply_player_spell_cast_event(event)
        .map_err(HostError::Core)?;
    let awarded_mana = u64::from(cast.mana_spent).saturating_mul(u64::from(magic_rate));
    let magic = world
        .apply_player_magic_mana(caster_id, awarded_mana, rules)
        .map_err(HostError::Core)?;
    let vitals = world.player_vitals(caster_id).map_err(HostError::Core)?;
    let attempts = world
        .player_progression_attempts(caster_id)
        .map_err(HostError::Core)?;
    database.update_player_vitals_and_progression_attempts(
        caster_id,
        PersistedPlayerVitals {
            health: vitals.health,
            max_health: vitals.max_health,
            mana: vitals.mana,
            max_mana: vitals.max_mana,
            capacity: vitals.capacity,
            magic_level: vitals.magic_level,
        },
        attempts,
    )?;
    // Haste self-effect (plan v49 slice 12): a spell with a speed declaration applies the bounded
    // timed condition to the caster; the regular heartbeat persists and expires it.
    if let Some(percent) = speed_percent {
        world
            .apply_player_speed_condition(caster_id, percent, NATIVE_HASTE_DURATION_SECONDS)
            .map_err(HostError::Core)?;
    }
    drop(world);
    if speed_percent.is_some() {
        let conditions = shared_world.player_conditions(caster_id)?;
        database
            .replace_player_conditions(caster_id, &conditions)
            .map_err(HostError::Persistence)?;
    }
    shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
    Ok((cast, magic))
}

pub(crate) struct NativeSelectedPlayerMeleePolicy<'a> {
    pub(crate) progression_rules: Option<&'a BTreeMap<VocationId, PlayerProgressionRules>>,
    pub(crate) skill_rate: u32,
    pub(crate) death_loss_policy: DeathLossPolicy,
    pub(crate) armor_by_server_id: Option<&'a BTreeMap<u16, u16>>,
    pub(crate) shield_defense_by_server_id: Option<&'a BTreeMap<u16, u16>>,
    pub(crate) armor_multiplier_by_vocation: Option<&'a BTreeMap<VocationId, u32>>,
    pub(crate) declarative_weapon_catalog: Option<&'a DeclarativeWeaponCatalog>,
}

pub(crate) fn apply_native_selected_player_melee(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    attacker_id: u64,
    world_map: &WorldMap,
    policy: NativeSelectedPlayerMeleePolicy<'_>,
) -> Result<
    Option<(
        u32,
        NativeOtClientPlayerVitals,
        forgotten_core::PlayerDamageOutcome,
    )>,
    HostError,
> {
    let Some(target_id) = shared_world
        .player_interaction_intent(attacker_id)?
        .target_player_id
    else {
        return Ok(None);
    };
    // Protection-zone gate: PvP is blocked when either participant stands on an imported
    // protection-zone tile. The target selection itself stays allowed; only damage is gated.
    {
        let world = shared_world.lock()?;
        if world.either_player_in_protection_zone(world_map, attacker_id, target_id) {
            return Ok(None);
        }
    }
    let target_equipment = shared_world.player_equipment(target_id)?;
    sync_native_equipment_armor_defense(
        shared_world,
        target_id,
        policy.armor_by_server_id,
        policy.shield_defense_by_server_id,
        policy.armor_multiplier_by_vocation,
        &target_equipment,
    )?;
    let declared_weapon_skill = if let Some(catalog) = policy.declarative_weapon_catalog {
        shared_world
            .player_equipment(attacker_id)?
            .item(EquipmentSlot::RightHand)
            .and_then(|item| catalog.adjacent_melee_skill(item.server_id))
    } else {
        Some(PlayerSkill::Fist)
    };
    let combat_result = if let Some(catalog) = policy.declarative_weapon_catalog {
        let Some(event) =
            shared_world.equipped_declarative_melee_event(attacker_id, target_id, catalog)?
        else {
            return Ok(None);
        };
        shared_world
            .apply_player_combat_event_with_death(event, world_map)
            .map(|(outcome, vitals, death_state)| (outcome.damage, vitals, death_state))
    } else {
        let event = PlayerCombatEvent::adjacent_melee(
            attacker_id,
            target_id,
            CombatDamageType::Physical,
            NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE,
            CombatAttackTiming::new(1).map_err(HostError::Core)?,
        )
        .map_err(HostError::Core)?;
        shared_world
            .apply_player_combat_event_with_death(event, world_map)
            .map(|(outcome, vitals, death_state)| (outcome.damage, vitals, death_state))
    };
    let (outcome, mut vitals, mut death_state) = match combat_result {
        Ok(result) => result,
        Err(HostError::Core(forgotten_core::CoreError::CombatOutOfRange { .. }))
        | Err(HostError::Core(forgotten_core::CoreError::UnknownPlayer(_)))
        | Err(HostError::Core(forgotten_core::CoreError::SelfInteractionNotAllowed(_)))
        | Err(HostError::Core(
            forgotten_core::CoreError::CombatCooldownActive { .. }
            | forgotten_core::CoreError::TargetAlreadyDefeated(_),
        ))
        | Err(HostError::Core(forgotten_core::CoreError::PlayerTownUnassigned(_)))
        | Err(HostError::Core(forgotten_core::CoreError::UnknownTown(_))) => return Ok(None),
        Err(error) => return Err(error),
    };
    if outcome.applied_damage == 0 {
        return Ok(None);
    }
    let fixed_death_loss_persisted = if death_state.is_some()
        && matches!(policy.death_loss_policy, DeathLossPolicy::FixedPercent(_))
    {
        let Some(rules_by_vocation) = policy.progression_rules else {
            return Err(HostError::InvalidConfiguration(
                "fixed deathLosePercent requires validated vocation progression rules".into(),
            ));
        };
        let vocation = shared_world.player_progression(target_id)?.vocation;
        let rules = rules_by_vocation.get(&vocation).copied().ok_or_else(|| {
            HostError::InvalidConfiguration(format!(
                "fixed deathLosePercent has no validated progression rules for vocation {}",
                vocation.value()
            ))
        })?;
        let DeathLossPolicy::FixedPercent(percent) = policy.death_loss_policy else {
            unreachable!("fixed death-loss branch requires a fixed policy");
        };
        apply_and_persist_native_fixed_death_loss(
            database,
            shared_world,
            target_id,
            percent,
            rules,
        )?;
        vitals = shared_world.player_vitals(target_id)?;
        death_state = Some(shared_world.player_respawn_state(target_id)?);
        true
    } else {
        false
    };
    let persisted_vitals = forgotten_persistence::PlayerVitals {
        health: vitals.health,
        max_health: vitals.max_health,
        mana: vitals.mana,
        max_mana: vitals.max_mana,
        capacity: vitals.capacity,
        magic_level: vitals.magic_level,
    };
    if fixed_death_loss_persisted {
        // The complete post-loss snapshot and marked lifecycle state were committed together.
    } else if let Some(death_state) = death_state {
        database.update_player_vitals_and_respawn_state(
            target_id,
            persisted_vitals,
            death_state,
        )?;
    } else {
        database.update_player_vitals(target_id, persisted_vitals)?;
    }
    if let (Some(rules_by_vocation), Some(skill)) =
        (policy.progression_rules, declared_weapon_skill)
    {
        let vocation = shared_world.player_progression(attacker_id)?.vocation;
        if let Some(rules) = rules_by_vocation.get(&vocation).copied() {
            let awarded_tries = u64::from(policy.skill_rate);
            shared_world.apply_player_skill_tries(attacker_id, skill, awarded_tries, rules)?;
            database.replace_player_progression(
                attacker_id,
                shared_world.player_progression(attacker_id)?,
            )?;
            database.replace_player_progression_attempts(
                attacker_id,
                shared_world.player_progression_attempts(attacker_id)?,
            )?;
        }
    }
    Ok(Some((
        native_player_id(target_id)?,
        NativeOtClientPlayerVitals {
            health: vitals.health,
            max_health: vitals.max_health,
            mana: vitals.mana,
            max_mana: vitals.max_mana,
            capacity: vitals.capacity,
            magic_level: vitals.magic_level,
        },
        outcome,
    )))
}

/// Applies the existing bounded selected-player melee primitive only when the parsed TFS-style
/// world type permits direct player-versus-player combat. `no-pvp` leaves target selection intact
/// but admits no damage, cooldown, death, skill, or client-health transition.
pub(crate) fn apply_native_selected_player_melee_for_world_type(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    attacker_id: u64,
    world_map: &WorldMap,
    world_type: WorldType,
    policy: NativeSelectedPlayerMeleePolicy<'_>,
) -> Result<
    Option<(
        u32,
        NativeOtClientPlayerVitals,
        forgotten_core::PlayerDamageOutcome,
    )>,
    HostError,
> {
    if matches!(world_type, WorldType::NoPvp) {
        return Ok(None);
    }
    apply_native_selected_player_melee(database, shared_world, attacker_id, world_map, policy)
}

/// Applies one accepted explicit fixed-percent loss and commits its complete authoritative result.
/// The caller invokes this only after the existing combat path has entered a validated death state.
/// Default formulas, promotions, blessings, and client-facing lifecycle presentation remain out of
/// scope because the current FE data model does not yet represent their compatibility inputs.
pub(crate) fn apply_configured_native_death_loss(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_id: u64,
    policy: DeathLossPolicy,
    progression_rules: Option<&BTreeMap<VocationId, PlayerProgressionRules>>,
) -> Result<bool, HostError> {
    let DeathLossPolicy::FixedPercent(percent) = policy else {
        return Ok(false);
    };
    let rules_by_vocation = progression_rules.ok_or_else(|| {
        HostError::InvalidConfiguration(
            "fixed deathLosePercent requires validated vocation progression rules".into(),
        )
    })?;
    let vocation = shared_world.player_progression(player_id)?.vocation;
    let rules = rules_by_vocation.get(&vocation).copied().ok_or_else(|| {
        HostError::InvalidConfiguration(format!(
            "fixed deathLosePercent has no validated progression rules for vocation {}",
            vocation.value()
        ))
    })?;
    apply_and_persist_native_fixed_death_loss(database, shared_world, player_id, percent, rules)?;
    Ok(true)
}

pub(crate) fn apply_and_persist_native_fixed_death_loss(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_id: u64,
    percent: u8,
    rules: PlayerProgressionRules,
) -> Result<(), HostError> {
    shared_world.apply_fixed_percent_death_loss(player_id, percent, rules)?;
    let (player, vitals) = shared_world.player_and_vitals(player_id)?;
    let progression = shared_world.player_progression(player_id)?;
    let attempts = shared_world.player_progression_attempts(player_id)?;
    let state = shared_world.player_respawn_state(player_id)?;
    database.update_player_fixed_death_loss(PlayerFixedDeathLossSnapshot {
        player_id,
        level: player.level,
        experience: player.experience,
        vitals: PersistedPlayerVitals {
            health: vitals.health,
            max_health: vitals.max_health,
            mana: vitals.mana,
            max_mana: vitals.max_mana,
            capacity: vitals.capacity,
            magic_level: vitals.magic_level,
        },
        progression,
        attempts,
        state,
    })?;
    Ok(())
}

/// Places one deterministic loot corpse on the defeated static creature's tile. The corpse is a
/// runtime-only map item (no source identity, no journal entry) whose children are the rolled
/// loot. The caller owns defeat validation, persistence of the map state, and client delivery.
/// Maps authoritative condition kinds onto the legacy client PlayerState bit flags (plan v49
/// slice 13): poison 0x0001, burning 0x0002, energy 0x0004.
pub(crate) fn native_condition_state_bits(
    conditions: &BTreeMap<forgotten_core::PlayerConditionKind, forgotten_core::PlayerCondition>,
) -> u16 {
    use forgotten_core::PlayerConditionKind as Kind;
    let mut bits = 0_u16;
    for (kind, _) in conditions {
        bits |= match kind {
            Kind::Poison => 0x0001,
            Kind::Burning => 0x0002,
            Kind::Energy => 0x0004,
            // Classic 740 has no haste icon bit; the modifier is felt through walk cadence
            // (plan v49 slice 12). Zero contribution keeps the state record unchanged.
            Kind::Haste => 0x0000,
        };
    }
    bits
}

/// Resolves the guild channel entry and message-of-the-day for one character (plan v49 slice
/// 19). `None` when the character belongs to no guild or the guild row is missing.
pub(crate) fn native_guild_channel_context(
    database: &EngineDatabase,
    character_id: u64,
) -> Option<(NativeOtClientClassicChannel, String)> {
    let membership = database.guild_membership(character_id).ok()??;
    let (name, motd) = database.guild_name_and_motd(membership.guild_id).ok()??;
    Some((
        NativeOtClientClassicChannel {
            id: NATIVE_GUILD_CHAT_CHANNEL_ID,
            name,
        },
        motd,
    ))
}

/// Resolves the client-visible corpse sprite for a defeated creature: the operator-declared
/// corpse item id when the imported definition declares one, otherwise the shared default.
pub(crate) fn native_declared_corpse_server_id(
    corpse_by_creature_name: Option<&BTreeMap<String, u16>>,
    creature_name: Option<String>,
) -> u16 {
    creature_name
        .as_deref()
        .and_then(|name| {
            corpse_by_creature_name.and_then(|map| map.get(&name.to_ascii_lowercase()).copied())
        })
        .unwrap_or(NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID)
}

pub(crate) fn spawn_native_static_defeat_corpse(
    shared_world: &SharedNativeWorld,
    map_owner: &SharedNativeMap,
    database: &mut EngineDatabase,
    creature_id: u32,
    seed: u64,
    corpse_server_id: u16,
    corpse_despawn_seconds: u32,
    loot_split_targets: &[u64],
) -> Result<Option<Position>, HostError> {
    let roll = shared_world.roll_defeated_static_creature_loot(creature_id, seed)?;
    // Plan v49 slice 6: defeated creatures always leave their declared corpse, even when the
    // loot roll is empty; the roll only fills the corpse container's children.
    let lifecycle = {
        let world = shared_world.lock()?;
        world.static_creature_lifecycle(creature_id)
    };
    let Some(lifecycle) = lifecycle else {
        return Ok(None);
    };
    let position = lifecycle.position;
    let despawn_tick = if corpse_despawn_seconds > 0 {
        Some(shared_world.tick()? + u64::from(corpse_despawn_seconds))
    } else {
        None
    };
    // Party loot split (plan v49 slice 14): each rolled stack goes to the next party member
    // (deterministic round-robin) whose owned top-level containers can hold it; stacks that no
    // member can carry stay in the corpse. Persists per member immediately, matching /give.
    let mut leftovers = Vec::new();
    if loot_split_targets.is_empty() {
        leftovers = roll.items.clone();
    } else {
        let mut cursor = 0_usize;
        for (item_id, count) in &roll.items {
            let mut placed = false;
            for attempt in 0..loot_split_targets.len() {
                let member = loot_split_targets[(cursor + attempt) % loot_split_targets.len()];
                let containers = shared_world.player_containers(member)?;
                let (staged, unplaced) =
                    insert_units_into_containers(containers.clone(), *item_id, u64::from(*count));
                if unplaced > 0 {
                    continue;
                }
                shared_world.replace_player_containers(member, staged.clone())?;
                database
                    .replace_player_containers(member, &staged)
                    .map_err(HostError::Persistence)?;
                cursor = (cursor + attempt + 1) % loot_split_targets.len();
                placed = true;
                break;
            }
            if !placed {
                leftovers.push((*item_id, *count));
            }
        }
    };
    let children = leftovers
        .iter()
        .map(|(item_id, count)| WorldMapItem {
            server_id: *item_id,
            client_thing_id: None,
            count: (*count).min(u8::MAX as u16) as u8,
            action_id: None,
            unique_id: None,
            text: None,
            description: None,
            teleport_destination: None,
            duration: None,
            charges: None,
            children: Vec::new(),
        })
        .collect();
    let corpse = WorldMapItem {
        server_id: corpse_server_id,
        client_thing_id: None,
        count: 1,
        action_id: None,
        unique_id: None,
        text: None,
        description: None,
        teleport_destination: None,
        duration: None,
        charges: None,
        children,
    };
    let placed = map_owner.add_runtime_tile_item(database, position, corpse, despawn_tick)?;
    if placed.is_some() {
        shared_world.mark_visibility_changed();
        Ok(Some(position))
    } else {
        Ok(None)
    }
}

pub(crate) fn apply_native_selected_static_creature_melee(
    shared_world: &SharedNativeWorld,
    attacker_id: u64,
    _world_map: &WorldMap,
) -> Result<Option<StaticCreatureDamageOutcome>, HostError> {
    let Some(target_id) = shared_world
        .player_interaction_intent(attacker_id)?
        .target_static_creature_id
    else {
        return Ok(None);
    };
    match shared_world.apply_static_creature_melee_damage(
        attacker_id,
        target_id,
        NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE,
    ) {
        Ok(outcome) if outcome.applied_damage > 0 => Ok(Some(outcome)),
        Ok(_) => Ok(None),
        Err(HostError::Core(
            forgotten_core::CoreError::StaticCreatureCombatOutOfRange { .. }
            | forgotten_core::CoreError::InactiveStaticCreature(_)
            | forgotten_core::CoreError::StaticNpcNotAttackable(_)
            | forgotten_core::CoreError::UnknownStaticCreature(_)
            | forgotten_core::CoreError::CombatCooldownActive { .. },
        )) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Applies an immutable raw monster reward only after the caller has confirmed an authoritative
/// selected-static defeat. Vocation-specific vital gains remain a separate data-wiring slice.
pub(crate) fn apply_and_persist_native_static_defeat_experience(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_id: u64,
    creature_id: u32,
    policy: Option<&ExperienceAwardPolicy>,
    vocation_level_up_gains: Option<&BTreeMap<VocationId, VocationLevelUpGains>>,
    party_shared_experience_rules: Option<PartySharedExperienceRules>,
) -> Result<Option<forgotten_core::PlayerExperienceAwardOutcome>, HostError> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    let raw_experience = shared_world.static_creature_experience_reward(creature_id)?;
    if raw_experience == 0 {
        return Ok(None);
    }
    let mut world = shared_world.lock()?;
    let recipient_ids = party_shared_experience_rules
        .map(|rules| world.party_shared_experience_recipients(player_id, rules))
        .transpose()
        .map_err(HostError::Core)?
        .flatten()
        .unwrap_or_else(|| vec![player_id]);
    let mut staged_world = world.clone();
    let mut outcomes = Vec::with_capacity(recipient_ids.len());
    for recipient_id in recipient_ids {
        let vocation = staged_world
            .player_progression(recipient_id)
            .map_err(HostError::Core)?
            .vocation;
        let gains = vocation_level_up_gains
            .and_then(|entries| entries.get(&vocation).copied())
            .unwrap_or_default();
        outcomes.push(
            staged_world
                .award_player_experience_with_vocation_gains(
                    recipient_id,
                    raw_experience,
                    policy,
                    gains,
                )
                .map_err(HostError::Core)?,
        );
    }
    let outcome = outcomes
        .iter()
        .copied()
        .find(|outcome| outcome.player_id == player_id)
        .ok_or_else(|| {
            HostError::InvalidConfiguration(
                "shared experience recipient selection omitted the defeating player".into(),
            )
        })?;
    if outcome.awarded_experience == 0 {
        return Ok(None);
    }
    let updates = outcomes
        .iter()
        .map(|outcome| PlayerExperienceVitalsUpdate {
            player_id: outcome.player_id,
            level: outcome.level,
            experience: outcome.experience,
            vitals: PersistedPlayerVitals {
                health: outcome.vitals.health,
                max_health: outcome.vitals.max_health,
                mana: outcome.vitals.mana,
                max_mana: outcome.vitals.max_mana,
                capacity: outcome.vitals.capacity,
                magic_level: outcome.vitals.magic_level,
            },
        })
        .collect::<Vec<_>>();
    database.update_player_experience_and_vitals_batch(&updates)?;
    *world = staged_world;
    drop(world);
    if outcomes.iter().any(|outcome| outcome.gained_levels > 0) {
        shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
    }
    Ok(Some(outcome))
}

pub(crate) fn native_player_id_to_character_id(native_id: u32) -> Option<u64> {
    (NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END)
        .contains(&native_id)
        .then(|| u64::from(native_id - NATIVE_OTCLIENT_PLAYER_ID_START))
}

pub(crate) fn native_position(position: Position) -> NativeOtClientPosition {
    NativeOtClientPosition {
        x: position.x,
        y: position.y,
        z: position.z,
    }
}

/// Rehydrates the authoritative fields emitted by the existing profile-gated classic player-stats
/// record. This is deliberately pure: packet framing, client capability decisions, and all
/// parser-layout assumptions stay in the protocol crate.
pub(crate) fn refresh_native_player_stats_snapshot(
    snapshot: &mut NativeOtClientEmptyWorldSnapshot,
    player: &Player,
    vitals: PlayerVitals,
) {
    snapshot.player_level = player.level.try_into().unwrap_or(u16::MAX);
    snapshot.player_experience = player.experience;
    snapshot.player_vitals = NativeOtClientPlayerVitals {
        health: vitals.health,
        max_health: vitals.max_health,
        mana: vitals.mana,
        max_mana: vitals.max_mana,
        capacity: vitals.capacity,
        magic_level: vitals.magic_level,
    };
}

pub(crate) fn native_cardinal_direction(
    direction: NativeOtClientCardinalDirection,
) -> CardinalDirection {
    match direction {
        NativeOtClientCardinalDirection::North => CardinalDirection::North,
        NativeOtClientCardinalDirection::East => CardinalDirection::East,
        NativeOtClientCardinalDirection::South => CardinalDirection::South,
        NativeOtClientCardinalDirection::West => CardinalDirection::West,
    }
}
