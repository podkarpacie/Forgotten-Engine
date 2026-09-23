//! Owned-inventory consumable use: eating/drinking declared instant consumables from the
//! caller's own equipment or owned containers. Classic clients address own equipment with
//! x=0xFFFF and a plain slot code in y, and own container items with the container flag plus
//! the child index in z. Consumption applies capped vitals, persists vitals plus inventory,
//! and delivers the self creature-health frame; dead callers are refused without effect.

use super::*;

/// A resolved consumable item location for one UseItem request: the item's server id plus the
/// single owning inventory position it was addressed from (equipment slot or container content).
struct ConsumableSource {
    server_id: u16,
    slot: Option<EquipmentSlot>,
    container_ref: Option<(u8, usize)>,
}

/// Applies one owned-inventory consumable use. Returns `SessionActionOutcome::Handled`
/// when the addressed item is a declared consumable (consumed, refused while dead,
/// or refused while fed); returns `Unhandled` for map-addressed positions AND for
/// owned items with no declared effect, so backpack/corpse/map-item routing below
/// still runs. Swallowing non-consumables here would make every owned USE
/// unreachable to the routers below (notably backpack opening).
pub(crate) fn apply_native_owned_consumable_use(
    ctx: &mut SessionContext<'_>,
    position: NativeOtClientPosition,
) -> Result<SessionActionOutcome, HostError> {
    if position.x != 0xffff {
        return Ok(SessionActionOutcome::Unhandled);
    }
    if ctx.observed_dead {
        native_diagnostic(
            ctx.config.extended_diagnostics,
            ctx.peer,
            "action=use-item outcome=deferred-consume-while-dead",
        );
        return Ok(SessionActionOutcome::Handled);
    }
    let consumable_target: Option<ConsumableSource> =
        if position.x == 0xffff && position.y & 0x40 == 0 {
            EquipmentSlot::from_code(position.y as u8).and_then(|slot| {
                let equipment = ctx.shared_world.player_equipment(ctx.character_id).ok()?;
                let item = equipment.item(slot)?;
                Some(ConsumableSource {
                    server_id: item.server_id,
                    slot: Some(slot),
                    container_ref: None,
                })
            })
        } else if position.x == 0xffff && position.y & 0x40 != 0 {
            let container_id = (position.y & 0x0f) as u8;
            let child_index = usize::from(position.z);
            ctx.shared_world
                .player_containers(ctx.character_id)
                .ok()
                .and_then(|containers| containers.container(container_id).cloned())
                .and_then(|container| container.items.item(child_index).cloned())
                .map(|item| ConsumableSource {
                    server_id: item.server_id,
                    slot: None,
                    container_ref: Some((container_id, child_index)),
                })
        } else {
            None
        };
    let Some(ConsumableSource {
        server_id: consumable_server_id,
        slot,
        container_ref,
    }) = consumable_target
    else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let Some(&effect) = ctx
        .config
        .consumable_effects
        .as_deref()
        .and_then(|effects| effects.get(&consumable_server_id))
    else {
        return Ok(SessionActionOutcome::Unhandled);
    };
    let (heal, mana_restore) = (effect.health, effect.mana);
    // Classic fed state (plan v49 slice 16): eating while a food window is
    // active answers "You are full." and leaves the item untouched.
    if effect.regeneration_seconds > 0 {
        let granted = ctx
            .shared_world
            .lock()?
            .grant_player_food_window(ctx.character_id, effect.regeneration_seconds)
            .map_err(HostError::Core)?;
        if !granted {
            let full_notice =
                encode_native_otclient_status_message(&ctx.config.client_profile, "You are full.")
                    .map_err(HostError::Protocol)?;
            write_frame(&mut *ctx.stream, &full_notice)?;
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!("action=use-item outcome=too-full server-id={consumable_server_id}"),
            );
            return Ok(SessionActionOutcome::Handled);
        }
    }
    let mut vitals = ctx.shared_world.player_vitals(ctx.character_id)?;
    if heal > 0 {
        vitals.health = vitals.health.saturating_add(heal).min(vitals.max_health);
    }
    if mana_restore > 0 {
        vitals.mana = vitals
            .mana
            .saturating_add(mana_restore)
            .min(vitals.max_mana);
    }
    // Consume one unit from the resolved inventory location.
    match (&slot, &container_ref) {
        (Some(slot), _) => {
            let mut equipment = ctx.shared_world.player_equipment(ctx.character_id)?;
            if let Some(item) = equipment.item(*slot).cloned() {
                if item.count > 1 {
                    let mut remaining = item;
                    remaining.count -= 1;
                    equipment.equip(*slot, remaining);
                } else {
                    equipment.unequip(*slot);
                }
            }
            ctx.shared_world
                .replace_player_equipment(ctx.character_id, equipment.clone())?;
            ctx.database
                .replace_player_equipment(ctx.character_id, &equipment)
                .map_err(HostError::Persistence)?;
        }
        (_, Some((container_id, child_index))) => {
            let mut containers = ctx.shared_world.player_containers(ctx.character_id)?;
            let mut container = match containers.remove(*container_id) {
                Some(container) => container,
                None => return Ok(SessionActionOutcome::Handled),
            };
            if !container.items.consume_item_unit(*child_index) {
                return Ok(SessionActionOutcome::Handled);
            }
            containers.insert(container).map_err(HostError::Core)?;
            ctx.database
                .replace_player_containers(ctx.character_id, &containers)
                .map_err(HostError::Persistence)?;
        }
        _ => {}
    }
    ctx.shared_world
        .lock()?
        .update_player_vitals(ctx.character_id, vitals)
        .map_err(HostError::Core)?;
    ctx.shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
    ctx.database
        .update_player_vitals(
            ctx.character_id,
            PersistedPlayerVitals {
                health: vitals.health,
                max_health: vitals.max_health,
                mana: vitals.mana,
                max_mana: vitals.max_mana,
                capacity: vitals.capacity,
                magic_level: vitals.magic_level,
            },
        )
        .map_err(HostError::Persistence)?;
    let self_native_id = native_player_id(ctx.character_id)?;
    let health_update = encode_native_otclient_creature_health(
        &ctx.config.client_profile,
        self_native_id,
        vitals.health,
        vitals.max_health,
    )
    .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &health_update)?;
    *ctx.observed_vitals_epoch = ctx.shared_world.vitals_epoch();
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "action=use-item outcome=consumed server-id={} heal={} mana={} health={} mana={}",
            consumable_server_id, heal, mana_restore, vitals.health, vitals.mana,
        ),
    );
    Ok(SessionActionOutcome::Handled)
}
