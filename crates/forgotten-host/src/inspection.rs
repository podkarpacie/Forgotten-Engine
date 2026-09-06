//! Bounded native frame and inspection-message builders: static-creature health frames,
//! shared-world viewport encoding, and validated map-item look replies with operator
//! catalog names, weights, and descriptions.

use super::*;

pub(crate) fn native_static_creature_health_frames(
    profile: &NativeOtClientProfile,
    static_spawns: &FeTfsStaticSpawnCollection,
) -> Result<Vec<Frame>, HostError> {
    static_spawns
        .entities
        .iter()
        .map(|entity| {
            encode_native_otclient_creature_health(
                profile,
                entity.id,
                u16::from(entity.health_percent),
                100,
            )
            .map_err(HostError::Protocol)
        })
        .collect()
}

pub(crate) fn encode_shared_native_world_viewport(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    shared_world: &SharedNativeWorld,
    observer_id: u64,
) -> Result<Frame, HostError> {
    let render_snapshot = shared_world.native_render_snapshot(
        observer_id,
        snapshot.player_look_type,
        snapshot.player_speed,
    )?;
    encode_native_otclient_map_viewport_with_static_spawns_and_players(
        profile,
        snapshot,
        world_map,
        Some(&render_snapshot.static_spawns),
        Some(&render_snapshot.visible_players),
    )
    .map_err(HostError::Protocol)
}

pub(crate) fn native_validated_map_item_text<'a>(
    world_map: &'a WorldMap,
    outcome: &PlayerItemUseOutcome,
) -> Option<&'a str> {
    let item = world_map
        .tile_items(outcome.position)?
        .get(usize::from(outcome.stack_index))?;
    (item.server_id == outcome.server_id && item.count == outcome.count)
        .then_some(item.text.as_deref())
        .flatten()
        .filter(|text| !text.is_empty())
}

pub(crate) fn native_map_item_inspection_message(
    world_map: &WorldMap,
    outcome: &PlayerItemUseOutcome,
    name_by_server_id: Option<&BTreeMap<u16, String>>,
    weight_by_server_id: Option<&BTreeMap<u16, u32>>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
) -> String {
    let mut message = format!(
        "You see item #{} (count: {}).",
        outcome.server_id, outcome.count
    );
    let Some(item) = world_map
        .tile_items(outcome.position)
        .and_then(|items| items.get(usize::from(outcome.stack_index)))
    else {
        return message;
    };
    if item.server_id != outcome.server_id || item.count != outcome.count {
        return message;
    }
    if let Some(name) = name_by_server_id.and_then(|names| names.get(&item.server_id)) {
        let name_detail = format!("Name: {name}.");
        if message.len() + 1 + name_detail.len() <= NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
            message.push(' ');
            message.push_str(&name_detail);
        }
    }
    if let Some(unit_weight) = weight_by_server_id.and_then(|weights| weights.get(&item.server_id))
    {
        let total_weight =
            if stackable_item_server_ids.is_some_and(|ids| ids.contains(&item.server_id)) {
                unit_weight.saturating_mul(u32::from(item.count).max(1))
            } else {
                *unit_weight
            };
        let weight_detail = native_classic_weight_description(total_weight);
        if message.len() + 1 + weight_detail.len() <= NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
            message.push(' ');
            message.push_str(&weight_detail);
        }
    }
    if let Some(description) = item
        .description
        .as_deref()
        .filter(|description| !description.is_empty())
    {
        if message.len() + 1 + description.len() <= NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
            message.push(' ');
            message.push_str(description);
        }
    }
    message
}

pub(crate) fn native_classic_weight_description(weight: u32) -> String {
    let whole = weight / 100;
    let hundredths = weight % 100;
    format!("It weighs {whole}.{hundredths:02} oz.")
}

pub(crate) fn native_item_inspection_metadata_details(
    mut message: String,
    item: &ItemInstance,
    name_by_server_id: Option<&BTreeMap<u16, String>>,
    weight_by_server_id: Option<&BTreeMap<u16, u32>>,
    stackable_item_server_ids: Option<&BTreeSet<u16>>,
) -> String {
    if let Some(name) = name_by_server_id.and_then(|names| names.get(&item.server_id)) {
        let name_detail = format!("Name: {name}.");
        if message.len() + 1 + name_detail.len() <= NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
            message.push(' ');
            message.push_str(&name_detail);
        }
    }
    if let Some(unit_weight) = weight_by_server_id.and_then(|weights| weights.get(&item.server_id))
    {
        let total_weight =
            if stackable_item_server_ids.is_some_and(|ids| ids.contains(&item.server_id)) {
                unit_weight.saturating_mul(u32::from(item.count).max(1))
            } else {
                *unit_weight
            };
        let weight_detail = native_classic_weight_description(total_weight);
        if message.len() + 1 + weight_detail.len() <= NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
            message.push(' ');
            message.push_str(&weight_detail);
        }
    }
    message
}
