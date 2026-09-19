//! Player interaction intents and viewport: item-use validation (map, ex, creature),
//! fight-mode state, declarative melee event resolution, target/follow setters, and
//! visible-player snapshot construction for the native viewport.

use super::*;

impl SharedNativeWorld {
    /// Validates a server-owned map-item use intent under the shared world lock. This exposes no
    /// client route and does not execute an item action, mutate map state, persist data, or claim
    /// doors, switches, container, script, or protocol behavior.
    pub fn validate_player_item_use(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseIntent,
    ) -> Result<PlayerItemUseOutcome, HostError> {
        self.lock()?
            .validate_player_item_use(map, intent)
            .map_err(HostError::Core)
    }

    /// Validates two server-owned map-item references under the same shared-world lock. It does
    /// not execute an item action, mutate map state, persist data, or produce client packets.
    pub fn validate_player_item_use_ex(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseExIntent,
    ) -> Result<PlayerItemUseExOutcome, HostError> {
        self.lock()?
            .validate_player_item_use_ex(map, intent)
            .map_err(HostError::Core)
    }

    /// Validates one server-owned map item and one authoritative creature under the shared-world
    /// lock. It does not select or affect the target, execute an item action, mutate state,
    /// persist data, advance an epoch, or emit client packets.
    pub fn validate_player_item_use_creature(
        &self,
        map: &WorldMap,
        intent: PlayerItemUseCreatureIntent,
    ) -> Result<PlayerItemUseCreatureOutcome, HostError> {
        self.lock()?
            .validate_player_item_use_creature(map, intent)
            .map_err(HostError::Core)
    }

    pub fn player_interaction_intent(
        &self,
        player_id: u64,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .player_interaction_intent(player_id)
            .map_err(HostError::Core)
    }

    pub fn player_fight_mode_state(
        &self,
        player_id: u64,
    ) -> Result<PlayerFightModeState, HostError> {
        self.lock()?
            .player_fight_mode_state(player_id)
            .map_err(HostError::Core)
    }

    /// Replaces one parsed native fight-mode request through the authoritative core boundary.
    /// This does not change combat formulas, pursuit, persistence, or client output.
    pub fn replace_player_fight_mode_state(
        &self,
        player_id: u64,
        state: PlayerFightModeState,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_fight_mode_state(player_id, state)
            .map_err(HostError::Core)
    }

    /// Builds a typed event only when the authoritative right-hand equipment slot contains an
    /// item declared in the operator-owned scriptless catalog. The client never supplies an item
    /// identifier to this path, and missing or unknown items intentionally produce no event.
    pub fn equipped_declarative_melee_event(
        &self,
        attacker_id: u64,
        target_id: u64,
        catalog: &DeclarativeWeaponCatalog,
    ) -> Result<Option<PlayerCombatEvent>, HostError> {
        let world = self.lock()?;
        let Some(item) = world
            .player_equipment(attacker_id)
            .map_err(HostError::Core)?
            .item(EquipmentSlot::RightHand)
        else {
            return Ok(None);
        };
        catalog
            .get(item.server_id)
            .map(|definition| {
                definition
                    .adjacent_melee_event(attacker_id, target_id)
                    .map_err(|_| {
                        HostError::InvalidConfiguration(
                            "validated declarative weapon did not build a combat event".into(),
                        )
                    })
            })
            .transpose()
    }

    pub fn set_player_target(
        &self,
        player_id: u64,
        target_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_target(player_id, target_player_id)
            .map_err(HostError::Core)
    }

    pub fn set_player_static_target(
        &self,
        player_id: u64,
        target_static_creature_id: Option<u32>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_static_target(player_id, target_static_creature_id)
            .map_err(HostError::Core)
    }

    pub fn set_player_follow(
        &self,
        player_id: u64,
        follow_player_id: Option<u64>,
    ) -> Result<PlayerInteractionIntent, HostError> {
        self.lock()?
            .set_player_follow(player_id, follow_player_id)
            .map_err(HostError::Core)
    }

    pub fn visible_players(
        &self,
        observer_id: u64,
        look_type: u8,
        speed: u16,
    ) -> Result<Vec<NativeOtClientVisiblePlayer>, HostError> {
        let (player_snapshots, invisible_ids) = {
            let world = self.lock()?;
            (
                world.player_render_snapshots(),
                world.invisible_player_ids(),
            )
        };
        let invisible_ids: std::collections::BTreeSet<u64> = invisible_ids.into_iter().collect();
        let player_outfits = self
            .player_outfits
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let player_directions = self
            .player_directions
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        player_snapshots
            .into_iter()
            // /invisible players are hidden from every other player's viewport.
            .filter(|player| player.id != observer_id && !invisible_ids.contains(&player.id))
            .map(|player| {
                Ok(NativeOtClientVisiblePlayer {
                    player_id: native_player_id(player.id)?,
                    name: player.name,
                    position: native_position(player.position),
                    health_percent: player.health_percent,
                    outfit: player_outfits.get(&player.id).copied().unwrap_or(
                        NativeOtClientClassicOutfit {
                            look_type,
                            head: 0,
                            body: 0,
                            legs: 0,
                            feet: 0,
                        },
                    ),
                    direction: player_directions
                        .get(&player.id)
                        .copied()
                        .unwrap_or(NativeOtClientCardinalDirection::South.protocol_direction()),
                    speed,
                })
            })
            .collect()
    }
}

/// Validates one map-item UseItemEx (source item onto target item) without executing an action.
/// Unmapped ids, missing world map, and unresolvable references emit diagnostics without
/// effect; unexpected failures propagate.
pub(crate) fn apply_native_use_item_ex_action(
    ctx: &mut SessionContext<'_>,
    source_position: NativeOtClientPosition,
    source_client_thing_id: u16,
    source_stack_position: u8,
    target_position: NativeOtClientPosition,
    target_client_thing_id: u16,
    target_stack_position: u8,
) -> Result<(), HostError> {
    let Some(world_map) = ctx.config.world_map.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item-ex outcome=deferred-no-world-map",
        );
        return Ok(());
    };
    let Some(intent) = native_map_item_use_ex_intent(
        ctx.config.item_presentation_catalog.as_deref(),
        ctx.character_id,
        (
            source_position,
            source_client_thing_id,
            source_stack_position,
        ),
        (
            target_position,
            target_client_thing_id,
            target_stack_position,
        ),
    ) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item-ex outcome=deferred-unmapped-or-ambiguous-client-thing-id",
        );
        return Ok(());
    };
    match ctx
        .shared_world
        .validate_player_item_use_ex(world_map, intent)
    {
        Ok(outcome) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=use-item-ex outcome=validated source-server-id={} source-count={} target-server-id={} target-count={}",
                outcome.source.server_id,
                outcome.source.count,
                outcome.target.server_id,
                outcome.target.count,
            ),
        ),
        Err(HostError::Core(_)) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item-ex outcome=deferred-invalid-server-owned-map-item",
        ),
        Err(error) => return Err(error),
    }
    Ok(())
}

/// Validates one map-item rotation without mutating the item. Same deferred-without-effect
/// contract as the other validation-only item actions.
pub(crate) fn apply_native_rotate_item_action(
    ctx: &mut SessionContext<'_>,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
) -> Result<(), HostError> {
    let Some(world_map) = ctx.config.world_map.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=rotate-item outcome=deferred-no-world-map",
        );
        return Ok(());
    };
    let Some(intent) = native_map_item_use_intent(
        ctx.config.item_presentation_catalog.as_deref(),
        ctx.character_id,
        position,
        client_thing_id,
        stack_position,
    ) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=rotate-item outcome=deferred-unmapped-or-ambiguous-client-thing-id",
        );
        return Ok(());
    };
    match ctx.shared_world.validate_player_item_use(world_map, intent) {
        Ok(outcome) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            &format!(
                "action=rotate-item outcome=validated server-id={} count={}",
                outcome.server_id, outcome.count
            ),
        ),
        Err(HostError::Core(_)) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=rotate-item outcome=deferred-invalid-server-owned-map-item",
        ),
        Err(error) => return Err(error),
    }
    Ok(())
}

/// Applies one map-item use on a creature: validates the pair, then executes declarative
/// weapon combat (adjacent melee, distance shots with ammo, runes) with hit-effect feedback.
/// Session container windows and the once-per-session white-skull flag travel as specific
/// arguments; everything else rides the shared context.
pub(crate) fn apply_native_use_item_on_creature_action(
    ctx: &mut SessionContext<'_>,
    source_position: NativeOtClientPosition,
    source_client_thing_id: u16,
    source_stack_position: u8,
    target_creature_id: u32,
    sent_container_windows: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
    observed_white_skull_sent: &mut bool,
) -> Result<(), HostError> {
    let Some(world_map) = ctx.config.world_map.as_deref() else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item-on-creature outcome=deferred-no-world-map",
        );
        return Ok(());
    };
    let Some(intent) = native_map_item_use_creature_intent(
        ctx.config.item_presentation_catalog.as_deref(),
        ctx.character_id,
        source_position,
        source_client_thing_id,
        source_stack_position,
        target_creature_id,
    ) else {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item-on-creature outcome=deferred-unmapped-or-ambiguous-client-thing-id",
        );
        return Ok(());
    };
    match ctx
        .shared_world
        .validate_player_item_use_creature(world_map, intent)
    {
        Ok(outcome) => {
            // Declarative weapon use against a creature: adjacent melee for sword/
            // club/axe declarations and runes; ranged distance shots with ammo
            // consumption plus a 0x85 missile record for declared distance weapons
            // (plan v49 slices 9-10).
            if let Some(catalog) = ctx.config.declarative_weapon_catalog.as_deref() {
                if let Some(definition) = catalog.get(outcome.source.server_id) {
                    if let PlayerItemUseCreatureTargetOutcome::Player {
                        player_id: target_id,
                        ..
                    } = outcome.target
                    {
                        let is_distance = definition.distance_range.is_some();
                        // Target tile for slice-11 feedback records (missile,
                        // hit effect, animated damage number).
                        let target_position_feedback =
                            ctx.shared_world.lock().ok().and_then(|world| {
                                world.player(target_id).map(|target| target.position)
                            });
                        let has_ammo = if is_distance {
                            let has =
                                ctx.shared_world
                                    .lock()
                                    .ok()
                                    .and_then(|world| {
                                        world.player_equipment(ctx.character_id).ok().map(
                                            |equipment| {
                                                equipment.item(EquipmentSlot::Ammo).is_some()
                                            },
                                        )
                                    })
                                    .unwrap_or(false);
                            if !has {
                                native_diagnostic(
                                    ctx.config.extended_diagnostics,
                                    ctx.peer,
                                    "action=distance-shot outcome=deferred-no-ammo",
                                );
                                return Ok(());
                            }
                            true
                        } else {
                            true
                        };
                        if !has_ammo {
                            return Ok(());
                        }
                        let event = if is_distance {
                            definition.distance_shot_event(ctx.character_id, target_id)
                        } else {
                            definition.adjacent_melee_event(ctx.character_id, target_id)
                        };
                        let event = match event {
                            Ok(event) => event,
                            Err(error) => {
                                native_diagnostic(
                                    ctx.config.extended_diagnostics,
                                    ctx.peer,
                                    &format!("action=rune-hit outcome=invalid-event error={error}"),
                                );
                                return Ok(());
                            }
                        };
                        match ctx
                            .shared_world
                            .apply_player_combat_event_with_death(event, world_map)
                        {
                            Ok((combat_outcome, _, _)) => {
                                if is_distance {
                                    let ammo_consumed = ctx
                                        .shared_world
                                        .lock()
                                        .ok()
                                        .and_then(|mut world| {
                                            world
                                                .consume_player_equipment_item_unit(
                                                    ctx.character_id,
                                                    EquipmentSlot::Ammo,
                                                )
                                                .ok()
                                        })
                                        .unwrap_or(false);
                                    if let Some(shot_effect) = definition.shot_effect {
                                        let target_position =
                                            ctx.shared_world.lock().ok().and_then(|world| {
                                                world
                                                    .player(target_id)
                                                    .map(|target| target.position)
                                            });
                                        if let Some(target_position) = target_position {
                                            let missile = encode_native_otclient_distance_effect(
                                                &ctx.config.client_profile,
                                                native_position(*ctx.player_position),
                                                native_position(target_position),
                                                shot_effect,
                                            )
                                            .map_err(HostError::Protocol)?;
                                            write_frame(&mut *ctx.stream, &missile)?;
                                        }
                                    }
                                    native_diagnostic(
                                        ctx.config.extended_diagnostics,
                                        ctx.peer,
                                        &format!(
                                            "action=distance-shot item={} target={} damage={} defeated={} ammo-consumed={}",
                                            outcome.source.server_id,
                                            target_id,
                                            combat_outcome.mitigated_damage,
                                            combat_outcome.damage.defeated,
                                            ammo_consumed
                                        ),
                                    );
                                } else {
                                    // Plan v49 slice 10: each fired rune consumes one
                                    // charge from its owned container stack.
                                    let charge_consumed = consume_declared_rune_charge(
                                        ctx.shared_world,
                                        &mut *ctx.database,
                                        ctx.character_id,
                                        source_position,
                                        source_stack_position,
                                        &ctx.config.client_profile,
                                        ctx.config.item_presentation_catalog.as_deref(),
                                        sent_container_windows,
                                    );
                                    native_diagnostic(
                                        ctx.config.extended_diagnostics,
                                        ctx.peer,
                                        &format!(
                                            "action=rune-hit item={} target={} damage={} defeated={} charge-consumed={}",
                                            outcome.source.server_id,
                                            target_id,
                                            combat_outcome.mitigated_damage,
                                            combat_outcome.damage.defeated,
                                            charge_consumed
                                        ),
                                    );
                                }
                                // Plan v49 slice 11 combat feedback: declared hit
                                // effect, optional animated damage number, and the
                                // attacker's white-skull award record.
                                if let (Some(target_position), Some(hit_effect)) =
                                    (target_position_feedback, definition.hit_effect)
                                {
                                    let effect_frame = encode_native_otclient_magic_effect(
                                        &ctx.config.client_profile,
                                        native_position(target_position),
                                        hit_effect,
                                    )
                                    .map_err(HostError::Protocol)?;
                                    write_frame(&mut *ctx.stream, &effect_frame)?;
                                }
                                if ctx.config.animated_damage_text_enabled
                                    && combat_outcome.damage.applied_damage > 0
                                {
                                    if let Some(target_position) = target_position_feedback {
                                        let animated = encode_native_otclient_animated_text(
                                            &ctx.config.client_profile,
                                            native_position(target_position),
                                            180,
                                            &combat_outcome.damage.applied_damage.to_string(),
                                        )
                                        .map_err(HostError::Protocol)?;
                                        write_frame(&mut *ctx.stream, &animated)?;
                                    }
                                }
                                if ctx
                                    .shared_world
                                    .lock()
                                    .map(|world| world.player_has_white_skull(ctx.character_id))
                                    .unwrap_or(false)
                                    && !*observed_white_skull_sent
                                {
                                    *observed_white_skull_sent = true;
                                    if let Ok(native_id) = native_player_id(ctx.character_id) {
                                        let skull = encode_native_otclient_creature_skull(
                                            &ctx.config.client_profile,
                                            native_id,
                                            forgotten_protocol::NATIVE_OTCLIENT_SKULL_WHITE,
                                        )
                                        .map_err(HostError::Protocol)?;
                                        write_frame(&mut *ctx.stream, &skull)?;
                                    }
                                }
                            }
                            Err(HostError::Core(_)) => {}
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!(
                    "action=use-item-on-creature outcome=validated source-server-id={} source-count={} target={:?}",
                    outcome.source.server_id, outcome.source.count, outcome.target
                ),
            );
        }
        Err(HostError::Core(_)) => native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item-on-creature outcome=deferred-invalid-server-owned-item-or-creature",
        ),
        Err(error) => return Err(error),
    }
    Ok(())
}
