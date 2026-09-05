//! Native classic frame builders: equipment delta frames, container window frames,
//! corpse window frames, look/inspection messages, and related protocol record
//! constructors. These are pure encoding helpers - they read shared state through
//! `&SharedNativeWorld` references and produce protocol `Frame` values.

use super::*;
/// Allocates one deterministic free client window ID for a corpse container view. Classic
/// clients address container windows through the four-bit `y & 0x0f` field, so IDs stay within
/// that addressable range while avoiding every currently open player-owned container window and
/// already-open corpse window. The bounded concurrent corpse-window cap still applies.
pub(crate) fn native_corpse_window_id(
    open_player_container_ids: &BTreeSet<u8>,
    open_corpse_window_ids: &BTreeSet<u8>,
) -> Option<u8> {
    if open_corpse_window_ids.len() >= NATIVE_OTCLIENT_MAX_OPEN_CORPSE_WINDOWS {
        return None;
    }
    (0..=NATIVE_OTCLIENT_CONTAINER_ADDRESSABLE_WINDOW_MAX)
        .rev()
        .find(|id| !open_player_container_ids.contains(id) && !open_corpse_window_ids.contains(id))
}

pub(crate) fn native_classic_item_record(
    catalog: Option<&NativeItemPresentationCatalog>,
    item: &ItemInstance,
) -> Option<NativeOtClientClassicItemRecord> {
    let presentation = catalog?.presentation(item.server_id)?;
    Some(NativeOtClientClassicItemRecord {
        client_thing_id: presentation.client_thing_id,
        subtype: presentation
            .requires_classic_740_subtype
            .then_some(item.count.min(u16::from(u8::MAX)) as u8),
    })
}

/// Applies only one bounded compatibility rule. Missing legacy metadata preserves the existing
/// transfer behavior, while a known nonempty `slotType` set must explicitly admit the requested
/// fixed equipment slot. `two-handed` remains unsupported because FE has no atomic dual-slot
/// occupancy contract yet.
pub(crate) fn native_legacy_slot_types_allow_equipment_slot(
    slot_types_by_server_id: Option<&BTreeMap<u16, BTreeSet<LegacyItemSlotType>>>,
    server_id: u16,
    target_slot: EquipmentSlot,
) -> bool {
    let Some(slot_types) = slot_types_by_server_id.and_then(|entries| entries.get(&server_id))
    else {
        return true;
    };
    slot_types.iter().any(|slot_type| {
        matches!(
            (slot_type, target_slot),
            (LegacyItemSlotType::Head, EquipmentSlot::Head)
                | (LegacyItemSlotType::Necklace, EquipmentSlot::Neck)
                | (LegacyItemSlotType::Backpack, EquipmentSlot::Backpack)
                | (LegacyItemSlotType::Body, EquipmentSlot::Armor)
                | (LegacyItemSlotType::RightHand, EquipmentSlot::RightHand)
                | (LegacyItemSlotType::LeftHand, EquipmentSlot::LeftHand)
                | (
                    LegacyItemSlotType::Hand,
                    EquipmentSlot::RightHand | EquipmentSlot::LeftHand
                )
                | (LegacyItemSlotType::Legs, EquipmentSlot::Legs)
                | (LegacyItemSlotType::Feet, EquipmentSlot::Feet)
                | (LegacyItemSlotType::Ring, EquipmentSlot::Ring)
                | (LegacyItemSlotType::Ammo, EquipmentSlot::Ammo)
        )
    })
}

/// Converts a fully decoded native map-item request into the core's server-ID intent only when
/// the operator-supplied presentation catalog has an unambiguous reverse mapping. The returned
/// intent is still validation-only; it does not execute an action or produce a packet.
pub(crate) fn native_map_item_use_intent(
    catalog: Option<&NativeItemPresentationCatalog>,
    player_id: u64,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
) -> Option<PlayerItemUseIntent> {
    let server_id = catalog?.unique_server_id_for_client_thing_id(client_thing_id)?;
    PlayerItemUseIntent::new(
        player_id,
        Position {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        stack_position,
        server_id,
    )
    .ok()
}

/// Converts a fully decoded native two-target item-use request only when both client thing IDs
/// map uniquely through the operator-supplied presentation catalog. The returned core request is
/// still validation-only and has no item action, persistence, or packet side effects.
pub(crate) fn native_map_item_use_ex_intent(
    catalog: Option<&NativeItemPresentationCatalog>,
    player_id: u64,
    source: (NativeOtClientPosition, u16, u8),
    target: (NativeOtClientPosition, u16, u8),
) -> Option<PlayerItemUseExIntent> {
    let (source_position, source_client_thing_id, source_stack_position) = source;
    let (target_position, target_client_thing_id, target_stack_position) = target;
    let source_server_id = catalog?.unique_server_id_for_client_thing_id(source_client_thing_id)?;
    let target_server_id = catalog?.unique_server_id_for_client_thing_id(target_client_thing_id)?;
    PlayerItemUseExIntent::new(
        player_id,
        Position {
            x: source_position.x,
            y: source_position.y,
            z: source_position.z,
        },
        source_stack_position,
        source_server_id,
        Position {
            x: target_position.x,
            y: target_position.y,
            z: target_position.z,
        },
        target_stack_position,
        target_server_id,
    )
    .ok()
}

/// Converts a parsed native battle-window item request into a source item plus authoritative
/// creature identity. The source still requires a unique catalog mapping; target validity and
/// range remain core-owned validation and no action is executed here.
pub(crate) fn native_map_item_use_creature_intent(
    catalog: Option<&NativeItemPresentationCatalog>,
    player_id: u64,
    source_position: NativeOtClientPosition,
    source_client_thing_id: u16,
    source_stack_position: u8,
    native_target_creature_id: u32,
) -> Option<PlayerItemUseCreatureIntent> {
    let source = native_map_item_use_intent(
        catalog,
        player_id,
        source_position,
        source_client_thing_id,
        source_stack_position,
    )?;
    let target = native_player_id_to_character_id(native_target_creature_id)
        .map(PlayerItemUseCreatureTarget::Player)
        .unwrap_or(PlayerItemUseCreatureTarget::StaticCreature(
            native_target_creature_id,
        ));
    Some(PlayerItemUseCreatureIntent { source, target })
}

/// Decodes one classic flagged owned-container address (`x=0xFFFF`, `y=0x40|container_id`,
/// `z=item index`) as encoded by the client for inventory-borne use targets. Map tiles,
/// equipment slots, and nonzero stack positions stay outside this bounded decode.
pub(crate) fn native_classic_owned_container_address(
    position: NativeOtClientPosition,
    stack_position: u8,
) -> Option<(u8, usize)> {
    if position.x != u16::MAX || position.y & 0x40 == 0 {
        return None;
    }
    if stack_position != 0 {
        return None;
    }
    Some(((position.y & 0x0f) as u8, usize::from(position.z)))
}

/// Consumes one unit of the fired rune from its owned container slot (plan v49 slice 10).
/// Persists the resulting stack and refreshes this session's rendered-window baseline so the
/// container delta layer emits the precise Change/Delete record. Returns false when the source
/// was not an owned container slot or the slot no longer resolves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn consume_declared_rune_charge(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
    character_id: u64,
    source_position: NativeOtClientPosition,
    source_stack_position: u8,
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    sent_container_windows: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
) -> bool {
    let Some((container_id, item_index)) =
        native_classic_owned_container_address(source_position, source_stack_position)
    else {
        return false;
    };
    let consumed = shared_world
        .lock()
        .ok()
        .and_then(|mut world| {
            world
                .consume_player_container_item_unit(character_id, container_id, item_index)
                .ok()
        })
        .unwrap_or(false);
    if !consumed {
        return false;
    }
    if let Ok(containers) = shared_world.player_containers(character_id) {
        let _ = database.replace_player_containers(character_id, &containers);
        match containers.container(container_id) {
            Some(container) => {
                sent_container_windows.insert(
                    container_id,
                    native_rendered_container_window(profile, catalog, container),
                );
            }
            None => {
                sent_container_windows.remove(&container_id);
            }
        }
    }
    true
}

pub(crate) fn native_classic_channel_list_entries(
    catalog: Option<&LegacyPublicChannelCatalog>,
) -> Vec<NativeOtClientClassicChannel> {
    catalog
        .into_iter()
        .flat_map(LegacyPublicChannelCatalog::iter)
        .map(|channel| NativeOtClientClassicChannel {
            id: channel.id,
            name: channel.name.clone(),
        })
        .collect()
}

pub(crate) fn native_configured_public_channel(
    catalog: Option<&LegacyPublicChannelCatalog>,
    channel_id: u16,
) -> Option<NativeOtClientClassicChannel> {
    catalog.and_then(|catalog| {
        catalog
            .get(channel_id)
            .map(|channel| NativeOtClientClassicChannel {
                id: channel.id,
                name: channel.name.clone(),
            })
    })
}

pub(crate) fn native_classic_equipment_frames(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    equipment: &PlayerEquipment,
) -> Result<Vec<Frame>, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Ok(Vec::new());
    }
    equipment
        .iter()
        .filter_map(|(slot, item)| {
            native_classic_item_record(catalog, item).map(|record| (slot, record))
        })
        .map(|(slot, record)| encode_native_otclient_set_inventory(profile, slot, record))
        .collect()
}

pub(crate) fn native_classic_mapped_equipment(
    catalog: Option<&NativeItemPresentationCatalog>,
    equipment: &PlayerEquipment,
) -> BTreeMap<EquipmentSlot, NativeOtClientClassicItemRecord> {
    equipment
        .iter()
        .filter_map(|(slot, item)| {
            native_classic_item_record(catalog, item).map(|record| (slot, record))
        })
        .collect()
}

/// Resolves the one classic fixed-inventory form that can safely share the ordinary LookMap
/// packet. Container-view positions and every nonzero stack index remain outside this bounded
/// inspection route.
pub(crate) fn native_classic_equipment_look_item(
    catalog: Option<&NativeItemPresentationCatalog>,
    equipment: &PlayerEquipment,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
) -> Option<(EquipmentSlot, ItemInstance)> {
    if position.x != u16::MAX || position.y & 0x40 != 0 || position.z != 0 || stack_position != 0 {
        return None;
    }
    let slot = EquipmentSlot::from_code(position.y as u8)?;
    let item = equipment.item(slot)?.clone();
    let record = native_classic_item_record(catalog, &item)?;
    (record.client_thing_id == client_thing_id).then_some((slot, item))
}

pub(crate) fn native_equipment_item_inspection_message(
    slot: EquipmentSlot,
    item: &ItemInstance,
    name_by_server_id: Option<&BTreeMap<u16, String>>,
    weight_by_server_id: Option<&BTreeMap<u16, u32>>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
) -> String {
    native_item_inspection_metadata_details(
        format!(
            "Equipment slot {}: item {} (count {}).",
            slot.code(),
            item.server_id,
            item.count
        ),
        item,
        name_by_server_id,
        weight_by_server_id,
        stackable_item_server_ids,
    )
}

/// Resolves one open owned top-level container item from the classic flagged inventory address.
/// Closed views, nested containers, nonzero stack positions, and unmatched client IDs are kept
/// outside this read-only inspection boundary.
pub(crate) fn native_classic_container_look_item(
    catalog: Option<&NativeItemPresentationCatalog>,
    containers: &PlayerContainers,
    closed_container_ids: &BTreeSet<u8>,
    position: NativeOtClientPosition,
    client_thing_id: u16,
    stack_position: u8,
) -> Option<(u8, ItemInstance)> {
    if position.x != u16::MAX || position.y & 0x40 == 0 || stack_position != 0 {
        return None;
    }
    let container_id = (position.y & 0x0f) as u8;
    if closed_container_ids.contains(&container_id) {
        return None;
    }
    let container = containers.container(container_id)?;
    if container.has_parent {
        return None;
    }
    let item = container.items.item(usize::from(position.z))?.clone();
    let record = native_classic_item_record(catalog, &item)?;
    (record.client_thing_id == client_thing_id).then_some((container_id, item))
}

pub(crate) fn native_container_item_inspection_message(
    container_id: u8,
    item: &ItemInstance,
    name_by_server_id: Option<&BTreeMap<u16, String>>,
    weight_by_server_id: Option<&BTreeMap<u16, u32>>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
) -> String {
    native_item_inspection_metadata_details(
        format!(
            "Container {}: item {} (count {}).",
            container_id, item.server_id, item.count
        ),
        item,
        name_by_server_id,
        weight_by_server_id,
        stackable_item_server_ids,
    )
}

/// Derives FE's explicit armor-only physical reduction from the six armor slots used by the
/// legacy reference boundary, plus the bounded legacy defense value of the equipped left-hand
/// (shield-hand) item when operator metadata supplies one. This intentionally excludes the right
/// hand, skills, random blocking, resistance, and any claim of TFS formula parity. The sum is
/// capped at the existing bounded combat-event maximum.
pub(crate) fn native_equipment_armor_defense(
    armor_by_server_id: Option<&BTreeMap<u16, u16>>,
    shield_defense_by_server_id: Option<&BTreeMap<u16, u16>>,
    equipment: &PlayerEquipment,
    armor_multiplier_milli: u32,
) -> PlayerCombatDefense {
    if armor_by_server_id.is_none() && shield_defense_by_server_id.is_none() {
        return PlayerCombatDefense::default();
    }
    let empty = BTreeMap::new();
    let armor_by_server_id = armor_by_server_id.unwrap_or(&empty);
    let shield_defense_by_server_id = shield_defense_by_server_id.unwrap_or(&empty);
    let armor_slots = [
        EquipmentSlot::Head,
        EquipmentSlot::Neck,
        EquipmentSlot::Armor,
        EquipmentSlot::Legs,
        EquipmentSlot::Feet,
        EquipmentSlot::Ring,
    ];
    let mut unscaled_armor = armor_slots.into_iter().fold(0_u16, |total, slot| {
        let armor = equipment
            .item(slot)
            .and_then(|item| armor_by_server_id.get(&item.server_id))
            .copied()
            .unwrap_or_default();
        total.saturating_add(armor).min(MAX_COMBAT_EVENT_DAMAGE)
    });
    // Only the left hand contributes legacy defense; shields and defensive weapons alike must
    // sit there for this bounded extension to count them.
    let shield_hand_defense = equipment
        .item(EquipmentSlot::LeftHand)
        .and_then(|item| shield_defense_by_server_id.get(&item.server_id))
        .copied()
        .unwrap_or_default();
    unscaled_armor = unscaled_armor
        .saturating_add(shield_hand_defense)
        .min(MAX_COMBAT_EVENT_DAMAGE);
    PlayerCombatDefense {
        physical_flat_reduction: ((u64::from(unscaled_armor) * u64::from(armor_multiplier_milli))
            / u64::from(DEFAULT_NATIVE_ARMOR_MULTIPLIER_MILLI))
        .min(u64::from(MAX_COMBAT_EVENT_DAMAGE)) as u16,
    }
}

/// Replaces only the existing profile-neutral physical reduction after authoritative equipment
/// hydration or an accepted native transfer. It never persists derived state independently.
pub(crate) fn sync_native_equipment_armor_defense(
    shared_world: &SharedNativeWorld,
    player_id: u64,
    armor_by_server_id: Option<&BTreeMap<u16, u16>>,
    shield_defense_by_server_id: Option<&BTreeMap<u16, u16>>,
    armor_multiplier_by_vocation: Option<&BTreeMap<VocationId, u32>>,
    equipment: &PlayerEquipment,
) -> Result<bool, HostError> {
    let vocation = shared_world.player_progression(player_id)?.vocation;
    let armor_multiplier_milli = armor_multiplier_by_vocation
        .and_then(|multipliers| multipliers.get(&vocation))
        .copied()
        .unwrap_or(DEFAULT_NATIVE_ARMOR_MULTIPLIER_MILLI);
    shared_world.replace_player_combat_defense(
        player_id,
        native_equipment_armor_defense(
            armor_by_server_id,
            shield_defense_by_server_id,
            equipment,
            armor_multiplier_milli,
        ),
    )
}

/// Produces only the parser-verified equipment delta for one native session. An item without a
/// current catalog mapping is not shown; if it replaced a previously mapped item the old visual
/// slot is explicitly deleted so the client cannot retain stale equipment state.
pub(crate) fn native_classic_equipment_delta_frames(
    profile: &NativeOtClientProfile,
    previous: &BTreeMap<EquipmentSlot, NativeOtClientClassicItemRecord>,
    current: &BTreeMap<EquipmentSlot, NativeOtClientClassicItemRecord>,
) -> Result<Vec<Frame>, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Ok(Vec::new());
    }
    let slots: BTreeSet<_> = previous.keys().chain(current.keys()).copied().collect();
    slots
        .into_iter()
        .filter_map(|slot| match (previous.get(&slot), current.get(&slot)) {
            (Some(previous), Some(current)) if previous == current => None,
            (_, Some(current)) => Some(encode_native_otclient_set_inventory(
                profile, slot, *current,
            )),
            (Some(_), None) => Some(encode_native_otclient_delete_inventory(profile, slot)),
            (None, None) => None,
        })
        .collect()
}

/// Produces the sole client-control frame associated with FE's bounded selected-static-target
/// deactivation. It does not cover generic creature removal, loot, corpses, effects, or AI.
pub(crate) fn native_static_target_deactivation_frames(
    profile: &NativeOtClientProfile,
    deactivated: bool,
) -> Result<Vec<Frame>, ProtocolError> {
    if !deactivated {
        return Ok(Vec::new());
    }
    Ok(vec![encode_native_otclient_clear_target(profile)?])
}

/// Produces the sole client-control frame associated with the current native session defeating
/// its selected player target. It does not notify other sessions, alter death presentation, or
/// cover generic combat cancellation.
pub(crate) fn native_selected_player_death_target_frames(
    profile: &NativeOtClientProfile,
    defeated: bool,
) -> Result<Vec<Frame>, ProtocolError> {
    if !defeated {
        return Ok(Vec::new());
    }
    Ok(vec![encode_native_otclient_clear_target(profile)?])
}

pub(crate) fn native_classic_container_frame(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    container: &PlayerContainer,
) -> Result<Option<Frame>, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || container.has_parent {
        return Ok(None);
    }
    let Some(container_item) = native_classic_item_record(catalog, &container.container_item)
    else {
        return Ok(None);
    };
    let Some(items) = container
        .items
        .iter()
        .map(|item| native_classic_item_record(catalog, item))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    let frame = encode_native_otclient_open_container(
        profile,
        &NativeOtClientClassicOpenContainer {
            container_id: container.container_id,
            container_item,
            name: container.name.clone(),
            capacity: container.items.capacity() as u8,
            has_parent: false,
            items,
        },
    )?;
    Ok(Some(frame))
}

pub(crate) fn native_nested_content_window_frame(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    window_id: u8,
    _parent_container_id: u8,
    item: &ItemInstance,
) -> Result<Option<Frame>, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || item.contents().is_empty() {
        return Ok(None);
    }
    let Some(container_item) = native_classic_item_record(catalog, item) else {
        return Ok(None);
    };
    let Some(items) = item
        .contents()
        .iter()
        .map(|child| native_classic_item_record(catalog, child))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    let mut name = format!("{} contents", item.server_id);
    name.truncate(MAX_LOGIN_STRING_BYTES);
    let frame = encode_native_otclient_open_container(
        profile,
        &NativeOtClientClassicOpenContainer {
            container_id: window_id,
            container_item,
            name,
            capacity: ItemInstance::MAX_CONTENT_SLOTS as u8,
            // Marked as a child so classic clients render the up-arrow back to the parent.
            has_parent: true,
            items,
        },
    )?;
    Ok(Some(frame))
}

/// Re-sends every open nested content window whose parent container was touched. Windows whose
/// parent item vanished (or lost its last content) get an explicit close frame instead.
pub(crate) fn native_refresh_open_content_windows(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    containers: &PlayerContainers,
    open_content_windows: &mut BTreeMap<u8, (u8, usize)>,
) -> Result<(), HostError> {
    let mut stale: Vec<u8> = Vec::new();
    let ids: Vec<u8> = open_content_windows.keys().copied().collect();
    for window_id in ids {
        let &(parent_container_id, parent_item_index) = &open_content_windows[&window_id];
        let Some(item) = containers
            .container(parent_container_id)
            .and_then(|container| container.items.item(parent_item_index))
        else {
            stale.push(window_id);
            if let Ok(frame) = encode_native_otclient_close_container(profile, window_id) {
                write_frame(stream, &frame)?;
            }
            continue;
        };
        match native_nested_content_window_frame(
            profile,
            catalog,
            window_id,
            parent_container_id,
            item,
        ) {
            Ok(Some(frame)) => write_frame(stream, &frame)?,
            _ => stale.push(window_id),
        }
    }
    for window_id in stale {
        open_content_windows.remove(&window_id);
    }
    Ok(())
}

pub(crate) fn native_classic_container_frames(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    containers: &PlayerContainers,
    closed_container_ids: &BTreeSet<u8>,
) -> Result<Vec<Frame>, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Ok(Vec::new());
    }
    containers
        .iter()
        .filter(|(_, container)| !closed_container_ids.contains(&container.container_id))
        .map(|(_, container)| native_classic_container_frame(profile, catalog, container))
        .collect::<Result<Vec<_>, _>>()
        .map(|frames| frames.into_iter().flatten().collect::<Vec<_>>())
}

/// One client-visible slot rendering inside an open top-level container window, exactly as the
/// classic record encoder puts it on the wire. Deltas compare this rendered form, never runtime
/// identity, because the client can only see what the catalog mapped.
pub(crate) type NativeRenderedContainerWindow = Option<(u8, Vec<NativeOtClientClassicItemRecord>)>;

/// Renders one owned top-level container the way the classic encoder presents it. Containers
/// with any unmapped item render as `None` (the classic encoder omits them wholesale), which
/// always forces a fresh full-frame attempt.
pub(crate) fn native_rendered_container_window(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    container: &PlayerContainer,
) -> NativeRenderedContainerWindow {
    if !profile.supports_classic_740_inventory_records() || container.has_parent {
        return None;
    }
    let items = container
        .items
        .iter()
        .map(|item| native_classic_item_record(catalog, item))
        .collect::<Option<Vec<_>>>()
        .map(|items| (container.items.capacity() as u8, items));
    items
}

/// Builds the per-session baseline of what the client currently displays for every open
/// top-level owned container.
pub(crate) fn native_rendered_container_windows(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    containers: &PlayerContainers,
    closed_container_ids: &BTreeSet<u8>,
) -> BTreeMap<u8, NativeRenderedContainerWindow> {
    let mut windows = BTreeMap::new();
    for (_, container) in containers.iter() {
        if closed_container_ids.contains(&container.container_id) {
            continue;
        }
        windows.insert(
            container.container_id,
            native_rendered_container_window(profile, catalog, container),
        );
    }
    windows
}

/// Emits the minimal classic CreateInContainer / ChangeInContainer / DeleteInContainer records
/// transforming what the client currently shows (`sent`) into the authoritative rendering
/// (`current`). Windows the session never sent, whose capacity changed, or whose mutation is not
/// representable as bounded slot deltas fall back to one full OpenContainer resend Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă„ÄľÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ă„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ä‚â€žĂ˘â‚¬Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇĂ„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ă„â€šĂ˘â‚¬ĹľÄ‚â€žĂ˘â‚¬Â¦Ă„â€šĂ˘â‚¬Ä…Ä‚ËĂ˘â€šÂ¬Ă‹â€ˇĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă‹â€ˇÄ‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬Ă„â€¦Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚ÂĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬Ä…Ä‚â€šĂ‚ÂÄ‚â€žĂ˘â‚¬ĹˇÄ‚â€ąĂ‚ÂĂ„â€šĂ‹ÂÄ‚ËĂ˘â€šÂ¬ÄąË‡Ä‚â€šĂ‚Â¬Ä‚â€žĂ„â€¦Ä‚â€ąĂ˘â‚¬Ë‡Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąË‡Ă„â€šĂ˘â‚¬ĹˇÄ‚â€šĂ‚Â¬Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ˘â‚¬ĹľÄ‚ËĂ˘â€šÂ¬Ă‚Â¦Ä‚â€žĂ˘â‚¬ĹˇÄ‚ËĂ˘â€šÂ¬ÄąÄľĂ„â€šĂ‹ÂÄ‚ËĂ˘â‚¬ĹˇĂ‚Â¬Ă„Ä…Ă„Äľ the exact
/// frame the previous blanket refresh emitted. Closed and vanished windows drop their baseline.
/// Slot-index semantics respect classic client behavior: DeleteInContainer removes the slot and
/// shifts the remainder, so a contiguous removed block is emitted as descending deletes.
#[allow(clippy::too_many_lines)]
pub(crate) fn native_container_delta_frames(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    containers: &PlayerContainers,
    closed_container_ids: &BTreeSet<u8>,
    sent: &mut BTreeMap<u8, NativeRenderedContainerWindow>,
) -> Result<Vec<Frame>, ProtocolError> {
    let current =
        native_rendered_container_windows(profile, catalog, containers, closed_container_ids);
    let mut frames = Vec::new();
    let stale: Vec<u8> = sent
        .keys()
        .filter(|id| !current.contains_key(id))
        .copied()
        .collect();
    for id in stale {
        sent.remove(&id);
    }
    for (container_id, current_window) in current.clone() {
        let Some(sent_window) = sent.get(&container_id).cloned() else {
            // Newly opened window (or previously unmapped): one full OpenContainer record.
            if let Some((capacity, items)) = current_window.as_ref() {
                if let Some(container) = containers.container(container_id) {
                    if let Some(frame) =
                        native_classic_container_frame(profile, catalog, container)?
                    {
                        frames.push(frame);
                    }
                    sent.insert(container_id, Some((*capacity, items.clone())));
                    continue;
                }
            }
            sent.insert(container_id, current_window);
            continue;
        };
        let (Some((previous_capacity, previous_slots)), Some((capacity, slots))) =
            (sent_window.as_ref(), current_window.as_ref())
        else {
            // Either side unrenderable: nothing expressible as deltas; keep the last known
            // client-visible state untouched and adopt the new baseline.
            sent.insert(container_id, current_window);
            continue;
        };
        if previous_capacity != capacity {
            if let Some(container) = containers.container(container_id) {
                if let Some(frame) = native_classic_container_frame(profile, catalog, container)? {
                    frames.push(frame);
                }
            }
            sent.insert(container_id, current_window);
            continue;
        }
        let mut start = 0;
        while start < previous_slots.len()
            && start < slots.len()
            && previous_slots[start] == slots[start]
        {
            start += 1;
        }
        let mut previous_end = previous_slots.len();
        let mut current_end = slots.len();
        while previous_end > start
            && current_end > start
            && previous_slots[previous_end - 1] == slots[current_end - 1]
        {
            previous_end -= 1;
            current_end -= 1;
        }
        let previous_middle = &previous_slots[start..previous_end];
        let current_middle = &slots[start..current_end];
        if previous_middle.is_empty() && current_middle.is_empty() {
            sent.insert(container_id, current_window);
            continue;
        }
        if current_middle.is_empty() {
            // Contiguous removal block: descending deletes let the client's own slot-shifting
            // converge without touching the preserved prefix or suffix.
            for slot in (start..previous_end).rev() {
                frames.push(encode_native_otclient_delete_in_container(
                    profile,
                    container_id,
                    slot as u8,
                )?);
            }
            sent.insert(container_id, current_window);
            continue;
        }
        if previous_middle.is_empty() {
            // Pure append block after the shared suffix trim: ascending creates.
            for item in current_middle.iter() {
                frames.push(encode_native_otclient_create_in_container(
                    profile,
                    container_id,
                    *item,
                )?);
            }
            sent.insert(container_id, current_window);
            continue;
        }
        if previous_middle.len() == current_middle.len() {
            // Same-length rewrite (stack merges, swaps): targeted changes only.
            for slot in 0..previous_middle.len() {
                if previous_middle[slot] != current_middle[slot] {
                    frames.push(encode_native_otclient_change_in_container(
                        profile,
                        container_id,
                        (start + slot) as u8,
                        current_middle[slot],
                    )?);
                }
            }
            sent.insert(container_id, current_window);
            continue;
        }
        // Mixed insert-and-remove inside one window is not representable as bounded slot
        // deltas under classic shift semantics: fall back to the exact full resend.
        if let Some(container) = containers.container(container_id) {
            if let Some(frame) = native_classic_container_frame(profile, catalog, container)? {
                frames.push(frame);
            }
        }
        sent.insert(container_id, current_window);
    }
    Ok(frames)
}

/// Encodes one read-only native 740 container window for one validated runtime corpse. The
/// container record itself uses the same server-id fallback presentation the client already saw in
/// the map, while loot children render through the validated catalog exactly like equipment
/// records; unmapped children are omitted rather than guessed. The window has no parent.
pub(crate) fn native_corpse_window_frame(
    profile: &NativeOtClientProfile,
    catalog: Option<&NativeItemPresentationCatalog>,
    window_id: u8,
    corpse: &WorldMapItem,
    item_names: Option<&BTreeMap<u16, String>>,
) -> Result<Option<Frame>, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || corpse.server_id == 0 {
        return Ok(None);
    }
    let name = item_names
        .and_then(|names| names.get(&corpse.server_id))
        .cloned()
        .unwrap_or_else(|| NATIVE_OTCLIENT_FALLBACK_CORPSE_NAME.to_string());
    let mut bounded_name = String::new();
    for character in name.chars() {
        if bounded_name.len() + character.len_utf8() > MAX_LOGIN_STRING_BYTES || character == '\0' {
            break;
        }
        bounded_name.push(character);
    }
    let container_item = NativeOtClientClassicItemRecord {
        client_thing_id: corpse.server_id,
        subtype: None,
    };
    let items = corpse
        .children
        .iter()
        .map(|child| {
            let presentation = catalog?.presentation(child.server_id)?;
            Some(NativeOtClientClassicItemRecord {
                client_thing_id: presentation.client_thing_id,
                subtype: presentation
                    .requires_classic_740_subtype
                    .then_some(child.count),
            })
        })
        .collect::<Option<Vec<_>>>();
    let Some(items) = items else {
        return Ok(None);
    };
    let frame = encode_native_otclient_open_container(
        profile,
        &NativeOtClientClassicOpenContainer {
            container_id: window_id,
            container_item,
            name: bounded_name,
            capacity: corpse.children.len().min(u8::MAX as usize) as u8,
            has_parent: false,
            items,
        },
    )?;
    Ok(Some(frame))
}
