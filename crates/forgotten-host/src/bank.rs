//! Bounded native banking services: carried-weight accounting, coin valuation, NPC-officer
//! proximity, and the classic bank Say-keyword handler ("balance" / "deposit all" /
//! "withdraw <n>"). Deposits and withdrawals persist atomically with inventory state.

use std::collections::BTreeMap;

use forgotten_core::{
    EquipmentSlot, ItemInstance, PlayerContainers, PlayerEquipment, MAX_ITEM_STACK_COUNT,
};

use super::{EngineDatabase, HostError, SharedNativeWorld};

/// Computes the bounded flat carried weight in hundredths of an ounce across equipment slots,
/// owned container shells, and their top-level items. Recursive nested trees stay outside this
/// first gate because FE's container model remains non-nested.
pub(crate) fn native_carried_weight(
    weight_by_server_id: &BTreeMap<u16, u32>,
    equipment: &PlayerEquipment,
    containers: &PlayerContainers,
) -> u64 {
    let item_weight = |item: &ItemInstance| -> u64 {
        u64::from(*weight_by_server_id.get(&item.server_id).unwrap_or(&0))
            .saturating_mul(u64::from(item.count))
    };
    let mut total = 0_u64;
    for (_, item) in equipment.iter() {
        total = total.saturating_add(item_weight(item));
    }
    for (_, container) in containers.iter() {
        total = total.saturating_add(item_weight(&container.container_item));
        for item in container.items.iter() {
            total = total.saturating_add(item_weight(item));
        }
    }
    total
}

/// Legacy currency coin identities and their gold values. These are factual classic item
/// identifiers, not redistributed client assets.
const NATIVE_CURRENCY_COINS: &[(u16, u64)] = &[(2148, 1), (2152, 100), (2160, 10_000)];
/// Bounded same-floor proximity to an active static NPC for banking keywords.
pub(crate) const NATIVE_BANK_NPC_RANGE_TILES: i32 = 2;

pub(crate) fn native_coin_value(server_id: u16) -> Option<u64> {
    NATIVE_CURRENCY_COINS
        .iter()
        .find(|(id, _)| *id == server_id)
        .map(|(_, value)| *value)
}

/// Returns whether an active static NPC stands within the bounded banking range on the player's
/// floor. Banking stays an NPC-adjacent service; remote or offline banking remains deferred.
pub(crate) fn native_bank_officer_nearby(
    shared_world: &SharedNativeWorld,
    player_id: u64,
) -> Result<bool, HostError> {
    let (player, _) = shared_world.player_and_vitals(player_id)?;
    let spawns = shared_world.active_static_spawns()?;
    for entity in &spawns.entities {
        if !spawns.is_npc(entity.id) || entity.position.z != player.position.z {
            continue;
        }
        if entity.position.x.abs_diff(player.position.x) as i32 <= NATIVE_BANK_NPC_RANGE_TILES
            && entity.position.y.abs_diff(player.position.y) as i32 <= NATIVE_BANK_NPC_RANGE_TILES
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Handles bounded NPC banking Say keywords ("balance", "deposit all", "withdraw <n>") for one
/// authenticated player standing near an active static NPC. `Ok(None)` means the message is not
/// a handled banking command and falls through to ordinary routing. Deposit converts every
/// carried coin stack into bank credit atomically with inventory persistence; withdraw debits
/// first and only creates coin stacks that actually fit owned top-level containers.
pub(crate) fn handle_native_bank_keyword(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
    player_id: u64,
    message: &str,
) -> Result<Option<String>, HostError> {
    let normalized = message.trim().to_ascii_lowercase();
    let is_balance = normalized == "balance";
    let is_deposit_all = normalized == "deposit all";
    let withdraw_amount = normalized
        .strip_prefix("withdraw ")
        .and_then(|amount| amount.trim().parse::<u64>().ok())
        .filter(|amount| *amount > 0);
    if !is_balance && !is_deposit_all && withdraw_amount.is_none() {
        return Ok(None);
    }
    if !native_bank_officer_nearby(shared_world, player_id)? {
        return Ok(None);
    }

    let mut equipment = shared_world.player_equipment(player_id)?;
    let mut containers = shared_world.player_containers(player_id)?;

    if is_balance {
        let balance = database
            .player_bank_balance(player_id)
            .map_err(HostError::Persistence)?;
        return Ok(Some(format!("Your account balance is {balance} gold.")));
    }

    if is_deposit_all {
        let mut deposited = 0_u64;
        // Strip every carried coin stack from staged inventory clones while summing value.
        for slot in [
            EquipmentSlot::Head,
            EquipmentSlot::Neck,
            EquipmentSlot::Backpack,
            EquipmentSlot::Armor,
            EquipmentSlot::RightHand,
            EquipmentSlot::LeftHand,
            EquipmentSlot::Legs,
            EquipmentSlot::Feet,
            EquipmentSlot::Ring,
            EquipmentSlot::Ammo,
        ] {
            if let Some(item) = equipment.item(slot).cloned() {
                if let Some(value) = native_coin_value(item.server_id) {
                    deposited += value.saturating_mul(u64::from(item.count));
                    equipment.unequip(slot);
                }
            }
        }
        let mut next_containers = PlayerContainers::default();
        for (_, container) in containers.iter() {
            let mut next = container.clone();
            for index in (0..next.items.len()).rev() {
                if let Some(item) = next.items.item(index).cloned() {
                    if let Some(value) = native_coin_value(item.server_id) {
                        deposited += value.saturating_mul(u64::from(item.count));
                        next.items.remove(index);
                    }
                }
            }
            next_containers.insert(next).map_err(HostError::Core)?;
        }
        containers = next_containers;
        if deposited == 0 {
            return Ok(Some("You have no coins to deposit.".into()));
        }
        let new_balance = database
            .player_bank_balance(player_id)
            .map_err(HostError::Persistence)?
            .saturating_add(deposited);
        database.replace_player_inventory_and_bank_balance(
            player_id,
            &equipment,
            &containers,
            new_balance,
        )?;
        shared_world.replace_player_equipment(player_id, equipment)?;
        shared_world.replace_player_containers(player_id, containers)?;
        return Ok(Some(format!("You deposited {deposited} gold.")));
    }

    // Defensive: the guard above returns early unless a withdrawal amount was parsed, so this
    // branch always has a value; fall back to a no-op instead of panicking on operator text.
    let Some(amount) = withdraw_amount else {
        return Ok(None);
    };
    let balance = database
        .player_bank_balance(player_id)
        .map_err(HostError::Persistence)?;
    if balance < amount {
        return Ok(Some("You do not have enough gold on your account.".into()));
    }
    // Chunk the withdrawal into bounded stacks and place them into free top-level container
    // slots without debiting unless every chunk fits.
    let mut chunks = Vec::new();
    let mut remaining_amount = amount;
    while remaining_amount > 0 {
        let chunk = remaining_amount.min(u64::from(MAX_ITEM_STACK_COUNT));
        let chunk = u16::try_from(chunk)
            .map_err(|_| HostError::InvalidConfiguration("chunk overflow".into()))?;
        chunks.push(chunk);
        remaining_amount -= u64::from(chunk);
        if chunks.len() > 256 {
            return Ok(Some("You cannot withdraw that much at once.".into()));
        }
    }
    let mut staged_containers = containers.clone();
    for chunk in &chunks {
        let item = ItemInstance::new(NATIVE_CURRENCY_COINS[0].0, u16::from(*chunk))
            .map_err(HostError::Core)?;
        let container_ids: Vec<u8> = staged_containers.iter().map(|(id, _)| id).collect();
        let mut placed = false;
        for container_id in container_ids {
            let Some(mut container) = staged_containers.remove(container_id) else {
                continue;
            };
            if !container.has_parent && container.items.merge_or_insert_stack(item.clone()).is_ok()
            {
                placed = true;
            }
            staged_containers
                .insert(container)
                .map_err(HostError::Core)?;
            if placed {
                break;
            }
        }
        if !placed {
            return Ok(Some(
                "You need a container with free space to withdraw.".into(),
            ));
        }
    }
    let new_balance = balance - amount;
    database.replace_player_inventory_and_bank_balance(
        player_id,
        &equipment,
        &staged_containers,
        new_balance,
    )?;
    shared_world.replace_player_equipment(player_id, equipment)?;
    shared_world.replace_player_containers(player_id, staged_containers)?;
    Ok(Some(format!("You withdrew {amount} gold.")))
}
