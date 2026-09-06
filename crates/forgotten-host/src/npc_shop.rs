//! NPC shop, quest rewards, depot windows, and item-delivery helpers for native
//! sessions. The shop keyword handler supports buy/sell with bounded container
//! insertion; the quest completion grants catalog-declared rewards; the depot
//! window opens the player's home-town depot as a read-only container.

use super::*;

/// Flips one persisted quest to completed and grants the catalog-declared rewards into the
/// player's first owned container (plan v49 slice 15). Returns the granted rewards, or `None`
/// when the player is offline, the quest is unknown to the catalog, or it was already completed.
pub(crate) fn complete_native_player_quest(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
    player_id: u64,
    quest_id: u16,
    quest_catalog: Option<&QuestCatalog>,
) -> Result<Option<Vec<(u16, u16)>>, HostError> {
    let Some(definition) = quest_catalog.and_then(|catalog| catalog.get(quest_id)) else {
        return Ok(None);
    };
    if !shared_world.has_player(player_id)? {
        return Ok(None);
    }
    let mut states = database.player_quests(player_id)?;
    if states
        .iter()
        .any(|(id, completed)| *id == quest_id && *completed)
    {
        return Ok(None);
    }
    match states.iter_mut().find(|(id, _)| *id == quest_id) {
        Some(entry) => entry.1 = true,
        None => states.push((quest_id, true)),
    }
    database.replace_player_quests(player_id, &states)?;

    let mut rewards = Vec::new();
    if !definition.rewards.is_empty() {
        let mut containers = shared_world.player_containers(player_id)?;
        let first_container_id = containers.iter().next().map(|(id, _)| id);
        let Some(container_id) = first_container_id else {
            return Ok(Some(rewards));
        };
        let mut container = match containers.remove(container_id) {
            Some(container) => container,
            None => return Ok(Some(rewards)),
        };
        for (item_id, count) in &definition.rewards {
            let Ok(stack) = forgotten_core::ItemInstance::new(*item_id, (*count).min(100)) else {
                continue;
            };
            // Merge into a matching stack when possible; otherwise start a fresh one.
            let placed = match container.items.merge_or_insert_stack(stack.clone()) {
                Ok(_) => true,
                Err(_) => container.items.insert(stack).is_ok(),
            };
            if placed {
                rewards.push((*item_id, *count));
            }
        }
        containers.insert(container).map_err(HostError::Core)?;
        shared_world.replace_player_containers(player_id, containers)?;
        database
            .replace_player_containers(player_id, &shared_world.player_containers(player_id)?)
            .map_err(HostError::Persistence)?;
        shared_world.mark_containers_changed();
    }
    Ok(Some(rewards))
}

/// Opens the player's home-town depot (depot id 0) as a read-only classic container window,
/// hydrated from SQLite. Window ids for depots live in a dedicated high range so they never
/// collide with owned-container or corpse windows. Item movement out of depots remains a
/// deferred slice; this opens the window and shows contents.
pub fn handle_native_depot_open(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    database: &EngineDatabase,
    player_id: u64,
    item_presentation_catalog: Option<&NativeItemPresentationCatalog>,
) -> Result<bool, HostError> {
    const DEPOT_WINDOW_ID_BASE: u8 = 0xE0;
    let records = database
        .player_depots(player_id)
        .map_err(HostError::Persistence)?;
    let depot_items = records
        .iter()
        .find(|record| record.depot_id == 0)
        .map(|record| record.items.clone())
        .unwrap_or_default();
    let mut items = Vec::new();
    for item in depot_items.iter().take(u8::MAX as usize) {
        if let Some(record) = native_classic_item_record(item_presentation_catalog, item) {
            items.push(record);
        }
    }
    let frame = encode_native_otclient_open_container(
        profile,
        &NativeOtClientClassicOpenContainer {
            container_id: DEPOT_WINDOW_ID_BASE.saturating_sub(1),
            container_item: NativeOtClientClassicItemRecord {
                client_thing_id: items
                    .first()
                    .map(|record| record.client_thing_id)
                    .unwrap_or(1),
                subtype: None,
            },
            name: "Depot".into(),
            capacity: u8::MAX,
            has_parent: false,
            items,
        },
    )
    .map_err(HostError::Protocol)?;
    write_frame(stream, &frame)?;
    Ok(true)
}

/// Builds and delivers the classic NPC shop windows (0x7A catalog + 0x7B player goods) for one
/// player trading with a named NPC whose declarative shop exists. Returns false when the NPC
/// has no shop or the presentation mapping is unavailable, so callers can fall through to the
/// keyword path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn deliver_native_npc_shop_windows(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    shared_world: &SharedNativeWorld,
    database: &EngineDatabase,
    player_id: u64,
    npc_name: &str,
    shop_catalog: &DeclarativeShopCatalog,
    item_presentation_catalog: Option<&NativeItemPresentationCatalog>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
    item_weight_by_server_id: Option<&BTreeMap<u16, u32>>,
    item_name_by_server_id: Option<&BTreeMap<u16, String>>,
) -> Result<bool, HostError> {
    let Some(shop) = shop_catalog.by_npc_name(npc_name) else {
        return Ok(false);
    };
    let Some(presentation) = item_presentation_catalog else {
        return Ok(false);
    };
    // Catalog entries for the 0x7A record.
    let mut shop_items = Vec::new();
    for entry in shop.entries.iter().take(u8::MAX as usize) {
        let Some(presented) = presentation.presentation(entry.server_id) else {
            continue;
        };
        let name = item_name_by_server_id
            .and_then(|names| names.get(&entry.server_id))
            .cloned()
            .unwrap_or_else(|| format!("item {}", entry.server_id));
        let weight = item_weight_by_server_id
            .and_then(|weights| weights.get(&entry.server_id))
            .copied()
            .unwrap_or(0);
        let subtype = if stackable_item_server_ids.is_some_and(|ids| ids.contains(&entry.server_id))
        {
            None // classic stackables carry their count at buy time, not a subtype here
        } else {
            None
        };
        shop_items.push(NativeOtClientShopItem {
            client_thing_id: presented.client_thing_id,
            subtype,
            name,
            weight,
            buy_price: u32::try_from(entry.buy_price_gold.unwrap_or(0)).unwrap_or(u32::MAX),
            sell_price: u32::try_from(entry.sell_price_gold.unwrap_or(0)).unwrap_or(u32::MAX),
        });
    }
    if shop_items.is_empty() {
        return Ok(false);
    }
    let open_record =
        encode_native_otclient_open_npc_trade(profile, &shop_items).map_err(HostError::Protocol)?;
    write_frame(stream, &open_record)?;

    // Player goods: gold balance plus sellable items found in owned containers.
    let gold = database
        .player_bank_balance(player_id)
        .map_err(HostError::Persistence)?;
    let containers = shared_world.player_containers(player_id)?;
    let mut goods: Vec<NativeOtClientPlayerGood> = Vec::new();
    for entry in shop.entries.iter() {
        if entry.sell_price_gold.is_none() {
            continue;
        }
        let Some(presented) = presentation.presentation(entry.server_id) else {
            continue;
        };
        let mut amount = 0_u32;
        for (_, container) in containers.iter() {
            for item in container.items.iter() {
                if item.server_id == entry.server_id {
                    amount += u32::from(item.count);
                }
            }
        }
        if amount > 0 {
            goods.push(NativeOtClientPlayerGood {
                client_thing_id: presented.client_thing_id,
                amount: u8::try_from(amount).unwrap_or(u8::MAX),
            });
        }
    }
    let goods_record =
        encode_native_otclient_player_goods(profile, gold.min(u32::MAX as u64) as u32, &goods)
            .map_err(HostError::Protocol)?;
    write_frame(stream, &goods_record)?;
    Ok(true)
}

/// Inserts bounded single units of one server item into the target's top-level containers,
/// returning `(containers, unplaced_units)`.
pub(crate) fn insert_units_into_containers(
    mut containers: PlayerContainers,
    item_id: u16,
    count: u64,
) -> (PlayerContainers, u64) {
    let mut unplaced = 0_u64;
    for _ in 0..count {
        let Ok(item) = ItemInstance::new(item_id, 1) else {
            break;
        };
        let container_ids: Vec<u8> = containers.iter().map(|(id, _)| id).collect();
        let mut placed = false;
        for container_id in container_ids {
            let Some(mut container) = containers.remove(container_id) else {
                continue;
            };
            let merged = container.items.merge_or_insert_stack(item.clone()).is_ok();
            if containers.insert(container).is_err() && !merged {
                break;
            }
            if merged {
                placed = true;
                break;
            }
        }
        if !placed {
            unplaced += 1;
        }
    }
    (containers, unplaced)
}

/// Delivers items to an online or offline character's first container with space. Shared by the
/// GM `/give` talkaction and the operator bridge.
pub(crate) fn give_items_to_player(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
    target_id: u64,
    item_id: u16,
    count: u64,
) -> Result<Option<String>, HostError> {
    let online = shared_world.has_player(target_id)?;
    if online {
        let containers = shared_world.player_containers(target_id)?;
        let (staged, unplaced) = insert_units_into_containers(containers.clone(), item_id, count);
        if unplaced > 0 {
            return Ok(Some("Target has no container space.".into()));
        }
        database
            .replace_player_containers(target_id, &staged)
            .map_err(HostError::Persistence)?;
        // Quiet replace + containers-epoch bump: the client refreshes open container windows
        // without a full viewport resend, so avatar colors and rotation stay untouched.
        {
            let mut world = shared_world.lock()?;
            world
                .replace_player_containers_quiet(target_id, staged)
                .map_err(HostError::Core)?;
        }
        shared_world.mark_containers_changed();
        return Ok(Some(format!(
            "Delivered {} x item {item_id}.",
            count - unplaced
        )));
    }
    let containers = database
        .player_containers(target_id)
        .map_err(HostError::Persistence)?;
    let (staged, unplaced) = insert_units_into_containers(containers, item_id, count);
    if unplaced > 0 {
        return Ok(Some("Target has no container space.".into()));
    }
    database
        .replace_player_containers(target_id, &staged)
        .map_err(HostError::Persistence)?;
    Ok(Some(format!(
        "Delivered {count} x item {item_id} to offline inventory."
    )))
}

/// Handles bounded NPC shop keywords ("buy <server-id> <count>" / "sell <server-id> <count>")
/// near an active static NPC whose declared shop matches. Payments and proceeds flow through the
/// durable bank balance; bought stacks chunk into free owned container slots, sold units leave
/// carried equipment and containers. `Ok(None)` falls through to ordinary routing.
pub(crate) fn handle_native_shop_keyword(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
    player_id: u64,
    message: &str,
    shop_catalog: &DeclarativeShopCatalog,
) -> Result<Option<String>, HostError> {
    let normalized = message.trim().to_ascii_lowercase();
    let mut parts = normalized.split_whitespace();
    let verb = parts.next().unwrap_or("");
    if verb != "buy" && verb != "sell" {
        return Ok(None);
    }
    let (player, _) = shared_world.player_and_vitals(player_id)?;
    let spawns = shared_world.active_static_spawns()?;
    let mut npc_name: Option<String> = None;
    for entity in &spawns.entities {
        if !spawns.is_npc(entity.id) || entity.position.z != player.position.z {
            continue;
        }
        if entity.position.x.abs_diff(player.position.x) as i32 <= NATIVE_BANK_NPC_RANGE_TILES
            && entity.position.y.abs_diff(player.position.y) as i32 <= NATIVE_BANK_NPC_RANGE_TILES
        {
            npc_name = Some(entity.name.clone());
            break;
        }
    }
    let Some(npc_name) = npc_name else {
        return Ok(None);
    };
    let Some(shop) = shop_catalog.by_npc_name(&npc_name) else {
        return Ok(None);
    };
    let server_id = parts
        .next()
        .and_then(|id| id.parse::<u16>().ok())
        .filter(|id| *id != 0);
    let count = parts.next().and_then(|count| count.parse::<u64>().ok());
    let (Some(server_id), Some(count)) = (server_id, count) else {
        return Ok(Some(
            "Usage: buy <item-id> <count> or sell <item-id> <count>.".into(),
        ));
    };
    if count == 0 || count > 100 {
        return Ok(Some("You must trade between 1 and 100 at once.".into()));
    }
    let Some(entry) = shop.entry(server_id) else {
        return Ok(Some("I do not trade that item.".into()));
    };
    let mut equipment = shared_world.player_equipment(player_id)?;
    let mut containers = shared_world.player_containers(player_id)?;

    if verb == "buy" {
        let Some(price) = entry.buy_price_gold else {
            return Ok(Some("I do not sell that item.".into()));
        };
        let total = price.saturating_mul(count);
        let balance = database
            .player_bank_balance(player_id)
            .map_err(HostError::Persistence)?;
        if balance < total {
            return Ok(Some("You do not have enough gold on your account.".into()));
        }
        let mut staged_containers = containers.clone();
        for _ in 0..count {
            let item = ItemInstance::new(server_id, 1).map_err(HostError::Core)?;
            let container_ids: Vec<u8> = staged_containers.iter().map(|(id, _)| id).collect();
            let mut placed = false;
            for container_id in container_ids {
                let Some(mut container) = staged_containers.remove(container_id) else {
                    continue;
                };
                if !container.has_parent
                    && container.items.merge_or_insert_stack(item.clone()).is_ok()
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
                return Ok(Some("You need a container with free space to buy.".into()));
            }
        }
        database.replace_player_inventory_and_bank_balance(
            player_id,
            &equipment,
            &staged_containers,
            balance - total,
        )?;
        shared_world.replace_player_equipment(player_id, equipment)?;
        shared_world.replace_player_containers(player_id, staged_containers)?;
        shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        return Ok(Some(format!("You bought {count} for {total} gold.")));
    }

    // Sell path: gather the requested unit count from carried equipment and container items.
    let Some(unit_price) = entry.sell_price_gold else {
        return Ok(Some("I do not buy that item.".into()));
    };
    let mut remaining_units = count;
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
        if remaining_units == 0 {
            break;
        }
        if let Some(item) = equipment.item(slot).cloned() {
            if item.server_id != server_id {
                continue;
            }
            let take = remaining_units.min(u64::from(item.count)) as u16;
            let mut kept = item;
            kept.count -= take;
            remaining_units -= u64::from(take);
            if kept.count > 0 {
                equipment.equip(slot, kept);
            } else {
                equipment.unequip(slot);
            }
        }
    }
    if remaining_units > 0 {
        let mut next_containers = PlayerContainers::default();
        for (_, container) in containers.iter() {
            let mut next = container.clone();
            while remaining_units > 0 {
                let mut matched = None;
                for index in 0..next.items.len() {
                    if let Some(item) = next.items.item(index) {
                        if item.server_id == server_id {
                            matched = Some(index);
                            break;
                        }
                    }
                }
                let Some(index) = matched else { break };
                let Some(item) = next.items.item(index).cloned() else {
                    break;
                };
                let take = remaining_units.min(u64::from(item.count));
                remaining_units -= take;
                next.items.take_item_units(index, take as u16);
            }
            next_containers.insert(next).map_err(HostError::Core)?;
        }
        containers = next_containers;
    }
    if remaining_units > 0 {
        return Ok(Some("You do not carry enough of that item.".into()));
    }
    let total = unit_price.saturating_mul(count);
    let new_balance = database
        .player_bank_balance(player_id)
        .map_err(HostError::Persistence)?
        .saturating_add(total);
    database.replace_player_inventory_and_bank_balance(
        player_id,
        &equipment,
        &containers,
        new_balance,
    )?;
    shared_world.replace_player_equipment(player_id, equipment)?;
    shared_world.replace_player_containers(player_id, containers)?;
    shared_world.vitals_epoch.fetch_add(1, Ordering::SeqCst);
    Ok(Some(format!("You sold {count} for {total} gold.")))
}
