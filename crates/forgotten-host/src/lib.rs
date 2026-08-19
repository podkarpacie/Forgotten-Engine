//! Persistent TCP host and bounded diagnostic session foundation for Forgotten Engine.
//!
//! This crate deliberately exposes an engine probe protocol, not a claimed Tibia wire protocol.

use forgotten_config::{DeclarativeSpellCatalog, DeclarativeWeaponCatalog};
use forgotten_core::{
    CardinalDirection, CombatAttackTiming, CombatDamageType, DeathLossPolicy, EmptyWorldManifest,
    EquipmentSlot, ExperienceAwardPolicy, FeTfsStaticSpawnCollection, ItemInstance,
    NativeItemPresentationCatalog, Player, PlayerCombatEvent, PlayerCombatEventOutcome,
    PlayerCondition, PlayerConditionKind, PlayerConditionOutcome, PlayerContainer,
    PlayerContainerStackToEquipmentOutcome, PlayerContainerToEquipmentOutcome, PlayerContainers,
    PlayerEquipment, PlayerEquipmentStackToContainerOutcome, PlayerEquipmentToContainerOutcome,
    PlayerExperienceAwardOutcome, PlayerFightMode, PlayerFightModeState, PlayerInteractionIntent,
    PlayerItemUseCreatureIntent, PlayerItemUseCreatureOutcome, PlayerItemUseCreatureTarget,
    PlayerItemUseExIntent, PlayerItemUseExOutcome, PlayerItemUseIntent, PlayerItemUseOutcome,
    PlayerProgression, PlayerProgressionAttempts, PlayerProgressionRules,
    PlayerRegenerationOutcome, PlayerRegenerationRules, PlayerRespawnState, PlayerSkill,
    PlayerSkillTryOutcome, PlayerSpellCastOutcome, PlayerVitals, Position,
    StaticCreatureDamageOutcome, StaticCreatureDecisionBatch, StaticCreatureDecisionPolicy,
    StaticCreatureResetSummary, StaticCreatureRuntimeRestoreSummary, StaticCreatureRuntimeSnapshot,
    StaticCreatureTargetAttackOutcome, StaticCreatureTargetStepOutcome, VocationId,
    VocationLevelUpGains, WorldMap, WorldState,
};
use forgotten_persistence::{
    EngineDatabase, PlayerFixedDeathLossSnapshot, PlayerOutfit,
    PlayerVitals as PersistedPlayerVitals, StaticCreatureRuntimeRecord,
};
use forgotten_protocol::{
    decode, decode_fe_otclient_capability_ack, decode_fe_otclient_move_request,
    decode_legacy_74_envelope, decode_legacy_74_game_session_bootstrap_plaintext,
    decode_legacy_74_game_session_envelope, decode_legacy_74_login_plaintext,
    decode_native_otclient_game_action, decode_native_otclient_game_request,
    decode_native_otclient_login_request, decode_status_request, encode,
    encode_fe_otclient_capability_offer, encode_fe_otclient_empty_viewport,
    encode_fe_otclient_initial_world, encode_fe_otclient_movement_ack,
    encode_fe_otclient_world_tick, encode_legacy_74_character_list,
    encode_legacy_74_game_challenge, encode_legacy_74_game_session_error,
    encode_legacy_74_game_session_ready, encode_login_error, encode_native_otclient_character_list,
    encode_native_otclient_choose_outfit, encode_native_otclient_close_container,
    encode_native_otclient_creature_health, encode_native_otclient_creature_outfit,
    encode_native_otclient_delete_inventory, encode_native_otclient_empty_quest_log,
    encode_native_otclient_game_cancel_walk_facing, encode_native_otclient_game_death,
    encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players,
    encode_native_otclient_game_login_error, encode_native_otclient_game_ping,
    encode_native_otclient_game_ping_back, encode_native_otclient_login_error,
    encode_native_otclient_map_step_with_static_spawns_and_players,
    encode_native_otclient_map_viewport_with_static_spawns,
    encode_native_otclient_map_viewport_with_static_spawns_and_players,
    encode_native_otclient_move_creature_at, encode_native_otclient_open_container,
    encode_native_otclient_player_modes, encode_native_otclient_player_skills,
    encode_native_otclient_player_stats, encode_native_otclient_set_inventory,
    encode_native_otclient_status_message, encode_status_binary, encode_status_xml,
    generate_legacy_74_game_challenge, xtea_encrypt_packet, CharacterListEntry,
    CompatibilityProfile, EmptyWorldMovementAck, Frame, InitialWorldSnapshot,
    Legacy74GameSessionState, LegacyRsaPrivateKey, NativeOtClientAutoWalkDirection,
    NativeOtClientCardinalDirection, NativeOtClientClassicItemRecord,
    NativeOtClientClassicOpenContainer, NativeOtClientClassicOutfit,
    NativeOtClientEmptyWorldSnapshot, NativeOtClientFightMode, NativeOtClientFightModeRequest,
    NativeOtClientGameAction, NativeOtClientPlayerVitals, NativeOtClientPosition,
    NativeOtClientProfile, NativeOtClientVisiblePlayer, OtClientEndpoint, ProtocolError,
    StatusPlayer, StatusRequest, StatusSnapshot, MAX_FRAME_SIZE,
    NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES, NATIVE_OTCLIENT_PLAYER_ID_END,
    NATIVE_OTCLIENT_PLAYER_ID_START,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const PROBE_MAGIC: &[u8; 4] = b"FEHS";
pub const PROBE_RESPONSE_MAGIC: &[u8; 4] = b"FEOK";
pub const PROBE_ERROR_MAGIC: &[u8; 4] = b"FEER";
pub const PROBE_VERSION: u8 = 1;
const MAX_EMPTY_WORLD_MOVES_PER_SESSION: usize = 64;
const NATIVE_OTCLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const NATIVE_OTCLIENT_HEARTBEAT_POLL_INTERVAL: Duration = Duration::from_millis(25);
const NATIVE_OTCLIENT_DEFAULT_GROUND_SPEED: u64 = 150;
const NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeWorldHeartbeatOutcome {
    tick: u64,
    reactivated_static_creatures: usize,
    changed_static_targets: usize,
    static_target_attacks: usize,
    static_target_attack_player_ids: BTreeSet<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticTargetAcquisitionPolicy {
    Disabled,
    NearestLivingPlayer { max_range: u8 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticTargetAcquisitionSummary {
    pub examined_static_creatures: usize,
    pub changed_static_targets: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticTargetPursuitPolicy {
    Disabled,
    NearestLivingPlayerOneStep { max_range: u8 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticTargetPursuitSummary {
    pub examined_static_creatures: usize,
    pub changed_static_targets: usize,
    pub moved_static_creatures: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticTargetAttackPolicy {
    Disabled,
    SelectedAdjacentFixedDamage { damage: u16 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticTargetAttackSummary {
    pub examined_static_creatures: usize,
    pub applied_attacks: usize,
    pub total_applied_damage: u64,
    pub affected_player_ids: BTreeSet<u64>,
}

#[cfg(test)]
fn advance_native_shared_world_heartbeat(
    shared_world: &SharedNativeWorld,
    elapsed_seconds: u16,
) -> Result<NativeWorldHeartbeatOutcome, HostError> {
    advance_native_shared_world_heartbeat_with_target_policy(
        shared_world,
        elapsed_seconds,
        StaticTargetAcquisitionPolicy::Disabled,
    )
}

#[cfg(test)]
fn advance_native_shared_world_heartbeat_with_target_policy(
    shared_world: &SharedNativeWorld,
    elapsed_seconds: u16,
    target_policy: StaticTargetAcquisitionPolicy,
) -> Result<NativeWorldHeartbeatOutcome, HostError> {
    advance_native_shared_world_heartbeat_with_static_target_policies(
        shared_world,
        elapsed_seconds,
        target_policy,
        StaticTargetAttackPolicy::Disabled,
        None,
    )
}

fn advance_native_shared_world_heartbeat_with_static_target_policies(
    shared_world: &SharedNativeWorld,
    elapsed_seconds: u16,
    target_policy: StaticTargetAcquisitionPolicy,
    attack_policy: StaticTargetAttackPolicy,
    world_map: Option<&WorldMap>,
) -> Result<NativeWorldHeartbeatOutcome, HostError> {
    let tick = shared_world.advance_ticks(elapsed_seconds)?;
    let reactivated_static_creatures = shared_world.reactivate_due_static_creatures()?.reactivated;
    let changed_static_targets = shared_world
        .acquire_static_creature_targets(target_policy)?
        .changed_static_targets;
    let static_target_attack_summary = match attack_policy {
        StaticTargetAttackPolicy::Disabled => StaticTargetAttackSummary::default(),
        StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { .. } => shared_world
            .attack_static_creature_targets_once(
                attack_policy,
                world_map.ok_or_else(|| {
                    HostError::InvalidConfiguration(
                        "static target attack policy requires a loaded world map".into(),
                    )
                })?,
            )?,
    };
    Ok(NativeWorldHeartbeatOutcome {
        tick,
        reactivated_static_creatures,
        changed_static_targets,
        static_target_attacks: static_target_attack_summary.applied_attacks,
        static_target_attack_player_ids: static_target_attack_summary.affected_player_ids,
    })
}

fn run_native_shared_world_heartbeat(
    shared_world: SharedNativeWorld,
    shutdown: Arc<AtomicBool>,
    attack_policy: StaticTargetAttackPolicy,
    world_map: Option<Arc<WorldMap>>,
    database_path: PathBuf,
    death_loss_policy: DeathLossPolicy,
    progression_rules: Option<Arc<BTreeMap<VocationId, PlayerProgressionRules>>>,
) -> Result<(), HostError> {
    let mut last_tick = Instant::now();
    while !shutdown.load(Ordering::SeqCst) {
        thread::sleep(NATIVE_OTCLIENT_HEARTBEAT_POLL_INTERVAL);
        let now = Instant::now();
        let elapsed_seconds = now
            .saturating_duration_since(last_tick)
            .as_secs()
            .min(u64::from(u16::MAX)) as u16;
        if elapsed_seconds == 0 {
            continue;
        }
        last_tick += Duration::from_secs(u64::from(elapsed_seconds));
        let target_policy = match attack_policy {
            StaticTargetAttackPolicy::Disabled => StaticTargetAcquisitionPolicy::Disabled,
            StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { .. } => {
                StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 1 }
            }
        };
        let outcome = advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared_world,
            elapsed_seconds,
            target_policy,
            attack_policy,
            world_map.as_deref(),
        )?;
        if !outcome.static_target_attack_player_ids.is_empty() {
            let mut database = EngineDatabase::open(&database_path)?;
            persist_static_target_attack_vitals(
                &mut database,
                &shared_world,
                &outcome.static_target_attack_player_ids,
                death_loss_policy,
                progression_rules.as_deref(),
            )?;
        }
    }
    Ok(())
}
const NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY: usize = 64;
const NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE: u16 = 10;

fn native_classic_item_record(
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

/// Converts a fully decoded native map-item request into the core's server-ID intent only when
/// the operator-supplied presentation catalog has an unambiguous reverse mapping. The returned
/// intent is still validation-only; it does not execute an action or produce a packet.
fn native_map_item_use_intent(
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
fn native_map_item_use_ex_intent(
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
fn native_map_item_use_creature_intent(
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

fn native_classic_equipment_frames(
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

fn native_classic_mapped_equipment(
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

/// Produces only the parser-verified equipment delta for one native session. An item without a
/// current catalog mapping is not shown; if it replaced a previously mapped item the old visual
/// slot is explicitly deleted so the client cannot retain stale equipment state.
fn native_classic_equipment_delta_frames(
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

fn native_classic_container_frame(
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

fn native_classic_container_frames(
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

fn truncate_native_chat_text(message: &str) -> String {
    let mut output = String::new();
    for character in message.chars() {
        if output.len() + character.len_utf8() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

fn native_hydrated_classic_outfit(
    configured_look_type: u8,
    outfit_first_look_type: u8,
    outfit_last_look_type: u8,
    persisted: PlayerOutfit,
) -> NativeOtClientClassicOutfit {
    if native_classic_outfit_is_allowed(
        NativeOtClientClassicOutfit {
            look_type: persisted.look_type,
            head: persisted.head,
            body: persisted.body,
            legs: persisted.legs,
            feet: persisted.feet,
        },
        outfit_first_look_type,
        outfit_last_look_type,
    ) {
        NativeOtClientClassicOutfit {
            look_type: persisted.look_type,
            head: persisted.head,
            body: persisted.body,
            legs: persisted.legs,
            feet: persisted.feet,
        }
    } else {
        NativeOtClientClassicOutfit {
            look_type: configured_look_type,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
        }
    }
}

fn native_classic_outfit_is_allowed(
    outfit: NativeOtClientClassicOutfit,
    outfit_first_look_type: u8,
    outfit_last_look_type: u8,
) -> bool {
    outfit.look_type != 0
        && outfit_first_look_type != 0
        && outfit_first_look_type <= outfit_last_look_type
        && (outfit_first_look_type..=outfit_last_look_type).contains(&outfit.look_type)
}

fn native_diagnostic_record(enabled: bool, peer: SocketAddr, event: &str) -> Option<String> {
    enabled.then(|| format!("> Native OTCv8 trace peer={peer} {event}"))
}

fn native_diagnostic(enabled: bool, peer: SocketAddr, event: &str) {
    if let Some(record) = native_diagnostic_record(enabled, peer, event) {
        eprintln!("{record}");
    }
}

fn native_action_diagnostic_summary(action: &NativeOtClientGameAction) -> String {
    match action {
        NativeOtClientGameAction::Ping => "action=ping".into(),
        NativeOtClientGameAction::PingBack => "action=ping-back".into(),
        NativeOtClientGameAction::EnterGame => "action=enter-game".into(),
        NativeOtClientGameAction::LeaveGame => "action=leave-game".into(),
        NativeOtClientGameAction::Stop => "action=stop".into(),
        NativeOtClientGameAction::Turn(direction) => format!("action=turn direction={direction:?}"),
        NativeOtClientGameAction::CardinalMove(direction) => {
            format!("action=cardinal-move direction={direction:?}")
        }
        NativeOtClientGameAction::DiagonalMove(direction) => {
            format!("action=diagonal-move direction={direction:?}")
        }
        NativeOtClientGameAction::AutoWalk(path) => format!(
            "action=auto-walk path-directions={} expanded-steps={}",
            path.len(),
            path.iter()
                .map(|direction| direction.cardinal_steps().len())
                .sum::<usize>()
        ),
        NativeOtClientGameAction::Talk(message) => {
            format!("action=talk text-bytes={}", message.len())
        }
        NativeOtClientGameAction::ChangeFightModes(request) => format!(
            "action=change-fight-modes mode={:?} chase={} secure={}",
            request.mode, request.chase, request.secure
        ),
        NativeOtClientGameAction::CloseContainer(container_id) => {
            format!("action=close-container container-id={container_id}")
        }
        NativeOtClientGameAction::UpArrowContainer(container_id) => {
            format!("action=up-arrow-container container-id={container_id}")
        }
        NativeOtClientGameAction::UpdateContainer(container_id) => {
            format!("action=update-container container-id={container_id}")
        }
        NativeOtClientGameAction::UseItem {
            position,
            client_thing_id,
            stack_position,
            index,
        } => format!(
            "action=use-item position={},{},{} client-thing-id={} stack-position={} index={}",
            position.x, position.y, position.z, client_thing_id, stack_position, index
        ),
        NativeOtClientGameAction::UseItemEx {
            source_position,
            source_client_thing_id,
            source_stack_position,
            target_position,
            target_client_thing_id,
            target_stack_position,
        } => format!(
            "action=use-item-ex source={},{},{} source-client-thing-id={} source-stack-position={} target={},{},{} target-client-thing-id={} target-stack-position={}",
            source_position.x,
            source_position.y,
            source_position.z,
            source_client_thing_id,
            source_stack_position,
            target_position.x,
            target_position.y,
            target_position.z,
            target_client_thing_id,
            target_stack_position,
        ),
        NativeOtClientGameAction::UseItemOnCreature {
            source_position,
            source_client_thing_id,
            source_stack_position,
            target_creature_id,
        } => format!(
            "action=use-item-on-creature source={},{},{} source-client-thing-id={} source-stack-position={} target-creature-id={}",
            source_position.x,
            source_position.y,
            source_position.z,
            source_client_thing_id,
            source_stack_position,
            target_creature_id,
        ),
        NativeOtClientGameAction::RotateItem {
            position,
            client_thing_id,
            stack_position,
        } => format!(
            "action=rotate-item position={},{},{} client-thing-id={} stack-position={}",
            position.x, position.y, position.z, client_thing_id, stack_position
        ),
        NativeOtClientGameAction::LookMap {
            position,
            thing_id,
            stack_position,
        } => format!(
            "action=look-map position={},{},{} thing-id={} stack-position={}",
            position.x, position.y, position.z, thing_id, stack_position
        ),
        NativeOtClientGameAction::LookCreature { creature_id } => {
            format!("action=look-creature creature-id={creature_id}")
        }
        NativeOtClientGameAction::RequestOutfit => "action=request-outfit".into(),
        NativeOtClientGameAction::RequestQuestLog => "action=request-quest-log".into(),
        NativeOtClientGameAction::ChangeOutfit(outfit) => format!(
            "action=change-outfit look-type={} colors={},{},{},{}",
            outfit.look_type, outfit.head, outfit.body, outfit.legs, outfit.feet
        ),
        NativeOtClientGameAction::IgnoredInteraction(opcode) => {
            format!("action=ignored-interaction opcode=0x{opcode:02x}")
        }
        NativeOtClientGameAction::SelectTarget(native_id) => {
            format!("action=select-target native-id={native_id}")
        }
        NativeOtClientGameAction::SelectFollow(native_id) => {
            format!("action=select-follow native-id={native_id}")
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub bind_addr: SocketAddr,
    pub profile: CompatibilityProfile,
    pub max_connections: usize,
    pub session_timeout: Duration,
    pub legacy_login: Option<LegacyLoginConfig>,
}

#[derive(Debug, Clone)]
pub struct LegacyLoginConfig {
    pub rsa_private_key: Arc<LegacyRsaPrivateKey>,
    pub server_name: String,
    pub message_of_the_day: String,
}

#[derive(Debug, Clone)]
pub struct StatusHostConfig {
    pub bind_addr: SocketAddr,
    pub profile: CompatibilityProfile,
    pub server_name: String,
    pub map_name: String,
    pub max_players: u32,
    pub max_connections: usize,
    pub session_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct GameSessionHostConfig {
    pub bind_addr: SocketAddr,
    pub profile: CompatibilityProfile,
    pub rsa_private_key: Arc<LegacyRsaPrivateKey>,
    pub advertised_endpoint: OtClientEndpoint,
    pub max_connections: usize,
    pub session_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct NativeOtClientHostConfig {
    pub bind_addr: SocketAddr,
    pub client_profile: NativeOtClientProfile,
    pub server_name: String,
    pub advertised_game_addr: SocketAddr,
    pub max_connections: usize,
    pub session_timeout: Duration,
    /// Emits bounded session metadata only. Packet bodies and credentials are never logged.
    pub extended_diagnostics: bool,
    pub empty_world: Option<NativeOtClientEmptyWorldConfig>,
    pub world_map: Option<Arc<WorldMap>>,
    /// Validated operator-supplied server-to-client item metadata. It is retained for later
    /// parser-safe inventory delivery and does not itself enable inventory packets.
    pub item_presentation_catalog: Option<Arc<NativeItemPresentationCatalog>>,
    /// Immutable display-only TFS spawn entities. No AI, combat, movement, or Lua behavior is
    /// attached at this host boundary.
    pub static_spawns: Option<Arc<FeTfsStaticSpawnCollection>>,
    /// Disabled by default. When enabled, one heartbeat pass may apply bounded fixed damage from
    /// each active static creature to its already selected adjacent target. Formula, persistence,
    /// packet, loot, corpse, script, and general AI behavior remain separate and deferred.
    pub static_target_attack_policy: StaticTargetAttackPolicy,
    /// Optional validated vocation recovery rules. Without this catalog automatic recovery is
    /// disabled; soul, condition client effects, death activation from conditions, and scripted
    /// lifecycle hooks remain deferred.
    pub regeneration_rules: Option<Arc<BTreeMap<VocationId, PlayerRegenerationRules>>>,
    /// Optional validated vocation progression rules. The host stores these data-driven formula
    /// inputs for explicit authoritative awards; weapons, spells, training, and Lua are not yet
    /// event sources.
    pub progression_rules: Option<Arc<BTreeMap<VocationId, PlayerProgressionRules>>>,
    /// Validated legacy vocation health, mana, and capacity gains for explicit level-up sources.
    pub vocation_level_up_gains: Option<Arc<BTreeMap<VocationId, VocationLevelUpGains>>>,
    /// Validated TFS-style global skill rate used only by the existing fixed selected-player
    /// melee fist-try award. Other combat, weapon, spell, training, and Lua sources remain
    /// separate and deferred.
    pub skill_rate: u32,
    /// Validated configured flat experience rate and optional level-stage policy. Concrete
    /// gameplay reward sources remain separate from this immutable host input.
    pub experience_award_policy: Option<Arc<ExperienceAwardPolicy>>,
    /// Validated `deathLosePercent` mode. The host applies only the bounded explicit fixed-percent
    /// mode when an accepted native death transition has matching vocation progression rules.
    /// Default-formula, promotion, blessing, and client lifecycle semantics remain deferred.
    pub death_loss_policy: DeathLossPolicy,
    /// Optional operator-owned scriptless weapon catalog. It is only eligible for the existing
    /// server-selected adjacent-melee action when a matching main-hand item is equipped.
    pub declarative_weapon_catalog: Option<Arc<DeclarativeWeaponCatalog>>,
    /// Optional operator-owned scriptless spell catalog. It is retained as immutable input for a
    /// future profile-approved cast path and does not enable client spell invocation by itself.
    pub declarative_spell_catalog: Option<Arc<DeclarativeSpellCatalog>>,
}

#[derive(Debug, Clone)]
pub struct NativeOtClientEmptyWorldConfig {
    pub ground_thing_id: u16,
    pub player_look_type: u8,
    pub outfit_first_look_type: u8,
    pub outfit_last_look_type: u8,
    pub player_speed: u16,
    pub server_beat: u16,
}

/// Validated persisted player state admitted to the authoritative native world during session
/// registration. It is a state-transfer payload only; neither client inventory nor condition
/// effects are enabled by constructing it. Native heartbeat scheduling may later advance its
/// already validated condition state authoritatively.
#[derive(Debug, Clone)]
pub struct NativePlayerHydration {
    pub progression: PlayerProgression,
    pub progression_attempts: PlayerProgressionAttempts,
    pub town_id: u32,
    pub respawn_state: PlayerRespawnState,
    pub equipment: PlayerEquipment,
    pub containers: PlayerContainers,
    pub conditions: BTreeMap<PlayerConditionKind, PlayerCondition>,
}

/// One synchronized authoritative world for all native game sessions started by a host. It owns
/// no automatic scheduler: callers advance ticks and apply creature policy explicitly.
#[derive(Debug, Clone)]
pub struct SharedNativeWorld {
    world: Arc<Mutex<WorldState>>,
    visibility_epoch: Arc<AtomicU64>,
    vitals_epoch: Arc<AtomicU64>,
    progression_epoch: Arc<AtomicU64>,
    equipment_epoch: Arc<AtomicU64>,
    containers_epoch: Arc<AtomicU64>,
    chat_recipients: Arc<Mutex<BTreeMap<u64, mpsc::SyncSender<SharedPublicChatEvent>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedPublicChatEvent {
    speaker_name: String,
    speaker_position: NativeOtClientPosition,
    text: String,
}

#[derive(Debug, Clone)]
struct NativeWorldRenderSnapshot {
    static_spawns: FeTfsStaticSpawnCollection,
    visible_players: Vec<NativeOtClientVisiblePlayer>,
}

impl SharedNativeWorld {
    pub fn from_static_spawns(
        static_spawns: Option<&FeTfsStaticSpawnCollection>,
    ) -> Result<Self, HostError> {
        let mut world = WorldState::default();
        if let Some(static_spawns) = static_spawns {
            world
                .install_static_creatures(static_spawns)
                .map_err(HostError::Core)?;
        }
        Ok(Self {
            world: Arc::new(Mutex::new(world)),
            visibility_epoch: Arc::new(AtomicU64::new(0)),
            vitals_epoch: Arc::new(AtomicU64::new(0)),
            progression_epoch: Arc::new(AtomicU64::new(0)),
            equipment_epoch: Arc::new(AtomicU64::new(0)),
            containers_epoch: Arc::new(AtomicU64::new(0)),
            chat_recipients: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    pub fn advance_tick(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.advance_tick())
    }

    pub fn advance_ticks(&self, elapsed_seconds: u16) -> Result<u64, HostError> {
        Ok(self.lock()?.advance_ticks(elapsed_seconds))
    }

    pub fn reactivate_due_static_creatures(&self) -> Result<StaticCreatureResetSummary, HostError> {
        let summary = self.lock()?.reactivate_due_static_creatures();
        if summary.reactivated > 0 {
            self.mark_visibility_changed();
        }
        Ok(summary)
    }

    pub fn tick(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.tick())
    }

    /// Returns the generic authoritative world revision. Protocol paths continue to use their
    /// dedicated visibility and vitals epochs until a typed event stream is introduced.
    pub fn world_revision(&self) -> Result<u64, HostError> {
        Ok(self.lock()?.revision())
    }

    pub fn visibility_epoch(&self) -> u64 {
        self.visibility_epoch.load(Ordering::SeqCst)
    }

    pub fn vitals_epoch(&self) -> u64 {
        self.vitals_epoch.load(Ordering::SeqCst)
    }

    pub fn progression_epoch(&self) -> u64 {
        self.progression_epoch.load(Ordering::SeqCst)
    }

    pub fn equipment_epoch(&self) -> u64 {
        self.equipment_epoch.load(Ordering::SeqCst)
    }

    pub fn containers_epoch(&self) -> u64 {
        self.containers_epoch.load(Ordering::SeqCst)
    }

    pub fn player_vitals(&self, player_id: u64) -> Result<PlayerVitals, HostError> {
        self.lock()?
            .player_vitals(player_id)
            .map_err(HostError::Core)
    }

    fn player_and_vitals(&self, player_id: u64) -> Result<(Player, PlayerVitals), HostError> {
        let world = self.lock()?;
        let player = world.player(player_id).cloned().ok_or(HostError::Core(
            forgotten_core::CoreError::UnknownPlayer(player_id),
        ))?;
        let vitals = world.player_vitals(player_id).map_err(HostError::Core)?;
        Ok((player, vitals))
    }

    pub fn player_progression(&self, player_id: u64) -> Result<PlayerProgression, HostError> {
        self.lock()?
            .player_progression(player_id)
            .map_err(HostError::Core)
    }

    pub fn replace_player_progression(
        &self,
        player_id: u64,
        progression: PlayerProgression,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .replace_player_progression(player_id, progression)
            .map_err(HostError::Core)?;
        if changed {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(changed)
    }

    pub fn player_progression_attempts(
        &self,
        player_id: u64,
    ) -> Result<PlayerProgressionAttempts, HostError> {
        self.lock()?
            .player_progression_attempts(player_id)
            .map_err(HostError::Core)
    }

    pub fn replace_player_progression_attempts(
        &self,
        player_id: u64,
        attempts: PlayerProgressionAttempts,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_progression_attempts(player_id, attempts)
            .map_err(HostError::Core)
    }

    pub fn player_town(&self, player_id: u64) -> Result<u32, HostError> {
        self.lock()?.player_town(player_id).map_err(HostError::Core)
    }

    pub fn replace_player_town(&self, player_id: u64, town_id: u32) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_town(player_id, town_id)
            .map_err(HostError::Core)
    }

    pub fn player_respawn_state(
        &self,
        player_id: u64,
    ) -> Result<forgotten_core::PlayerRespawnState, HostError> {
        self.lock()?
            .player_respawn_state(player_id)
            .map_err(HostError::Core)
    }

    pub fn hydrate_player_respawn_state(
        &self,
        player_id: u64,
        state: PlayerRespawnState,
    ) -> Result<bool, HostError> {
        self.lock()?
            .hydrate_player_respawn_state(player_id, state)
            .map_err(HostError::Core)
    }

    pub fn apply_player_regeneration(
        &self,
        player_id: u64,
        rules: PlayerRegenerationRules,
        elapsed_seconds: u16,
    ) -> Result<PlayerRegenerationOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_player_regeneration(player_id, rules, elapsed_seconds)
            .map_err(HostError::Core)?;
        if outcome.health_gained > 0 || outcome.mana_gained > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    pub fn apply_player_conditions(
        &self,
        player_id: u64,
        elapsed_seconds: u16,
    ) -> Result<PlayerConditionOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_player_conditions(player_id, elapsed_seconds)
            .map_err(HostError::Core)?;
        if outcome.applied_damage > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    /// Advances bounded conditions and enters authoritative death state only when the resulting
    /// damage is lethal at a validated assigned town. Client death effects and packet delivery
    /// remain separate host responsibilities.
    pub fn apply_player_conditions_with_death(
        &self,
        player_id: u64,
        world_map: &WorldMap,
        elapsed_seconds: u16,
    ) -> Result<
        (
            PlayerConditionOutcome,
            PlayerVitals,
            Option<forgotten_core::PlayerRespawnState>,
        ),
        HostError,
    > {
        let mut world = self.lock()?;
        let town_id = world.player_town(player_id).map_err(HostError::Core)?;
        let (outcome, death_state) = world
            .apply_player_conditions_with_death(player_id, town_id, world_map, elapsed_seconds)
            .map_err(HostError::Core)?;
        let vitals = world.player_vitals(player_id).map_err(HostError::Core)?;
        if outcome.applied_damage > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok((outcome, vitals, death_state))
    }

    pub fn award_player_experience(
        &self,
        player_id: u64,
        raw_experience: u64,
        policy: &ExperienceAwardPolicy,
    ) -> Result<PlayerExperienceAwardOutcome, HostError> {
        let outcome = self
            .lock()?
            .award_player_experience(player_id, raw_experience, policy)
            .map_err(HostError::Core)?;
        if outcome.awarded_experience > 0 {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        if outcome.gained_levels > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    /// Additive wrapper for configuration-selected vocation gains. The existing no-gain method
    /// remains available for callers that do not yet hydrate a legacy vocation registry.
    pub fn award_player_experience_with_vocation_gains(
        &self,
        player_id: u64,
        raw_experience: u64,
        policy: &ExperienceAwardPolicy,
        gains: VocationLevelUpGains,
    ) -> Result<PlayerExperienceAwardOutcome, HostError> {
        let outcome = self
            .lock()?
            .award_player_experience_with_vocation_gains(player_id, raw_experience, policy, gains)
            .map_err(HostError::Core)?;
        if outcome.awarded_experience > 0 {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        if outcome.gained_levels > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    pub fn apply_player_skill_tries(
        &self,
        player_id: u64,
        skill: PlayerSkill,
        awarded_tries: u64,
        rules: PlayerProgressionRules,
    ) -> Result<PlayerSkillTryOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_player_skill_tries(player_id, skill, awarded_tries, rules)
            .map_err(HostError::Core)?;
        if awarded_tries > 0 {
            self.progression_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    pub fn player_equipment(&self, player_id: u64) -> Result<PlayerEquipment, HostError> {
        self.lock()?
            .player_equipment(player_id)
            .cloned()
            .map_err(HostError::Core)
    }

    pub fn replace_player_equipment(
        &self,
        player_id: u64,
        equipment: PlayerEquipment,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .replace_player_equipment(player_id, equipment)
            .map_err(HostError::Core)?;
        if changed {
            self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(changed)
    }

    pub fn player_containers(&self, player_id: u64) -> Result<PlayerContainers, HostError> {
        self.lock()?
            .player_containers(player_id)
            .cloned()
            .map_err(HostError::Core)
    }

    pub fn replace_player_containers(
        &self,
        player_id: u64,
        containers: PlayerContainers,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .replace_player_containers(player_id, containers)
            .map_err(HostError::Core)?;
        if changed {
            self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(changed)
    }

    /// Applies the existing authoritative complete-item transfer under the shared world lock.
    /// Native sessions observe the resulting equipment and container state through their separate
    /// epochs; this method neither accepts a client request nor persists the mutation.
    pub fn move_equipment_item_to_container(
        &self,
        player_id: u64,
        from_slot: EquipmentSlot,
        container_id: u8,
    ) -> Result<PlayerEquipmentToContainerOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_equipment_item_to_container(player_id, from_slot, container_id)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies the reverse existing complete-item transfer under the shared world lock. Client
    /// requests, persistence, equipment compatibility, swaps, stack rules, ground transfer, and
    /// recursive containers remain outside this bounded host integration.
    pub fn move_container_item_to_equipment(
        &self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
    ) -> Result<PlayerContainerToEquipmentOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_container_item_to_equipment(player_id, container_id, item_index, to_slot)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies one bounded partial equipment stack movement under the shared world lock. It
    /// advances the two existing inventory epochs only after the core accepts the atomic update;
    /// request decoding, persistence routing, slot rules, and ground/nested inventory remain out
    /// of scope.
    pub fn move_equipment_stack_to_container(
        &self,
        player_id: u64,
        from_slot: EquipmentSlot,
        container_id: u8,
        count: u16,
    ) -> Result<PlayerEquipmentStackToContainerOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_equipment_stack_to_container(player_id, from_slot, container_id, count)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    /// Applies one bounded partial top-level-container stack movement under the shared world
    /// lock. It shares the established inventory refresh contract with full-item transfers and
    /// deliberately does not claim native client request or general inventory semantics.
    pub fn move_container_stack_to_equipment(
        &self,
        player_id: u64,
        container_id: u8,
        item_index: usize,
        to_slot: EquipmentSlot,
        count: u16,
    ) -> Result<PlayerContainerStackToEquipmentOutcome, HostError> {
        let outcome = self
            .lock()?
            .move_container_stack_to_equipment(player_id, container_id, item_index, to_slot, count)
            .map_err(HostError::Core)?;
        self.equipment_epoch.fetch_add(1, Ordering::SeqCst);
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(outcome)
    }

    pub fn player_conditions(
        &self,
        player_id: u64,
    ) -> Result<BTreeMap<PlayerConditionKind, PlayerCondition>, HostError> {
        self.lock()?
            .player_conditions(player_id)
            .cloned()
            .map_err(HostError::Core)
    }

    pub fn replace_player_conditions(
        &self,
        player_id: u64,
        conditions: BTreeMap<PlayerConditionKind, PlayerCondition>,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_conditions(player_id, conditions)
            .map_err(HostError::Core)
    }

    pub fn apply_player_melee_damage(
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
    pub fn apply_fixed_percent_death_loss(
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
    pub fn apply_player_melee_damage_with_death(
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
    pub fn apply_player_combat_event_with_death(
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
            Some(
                world
                    .apply_player_death(event.target_id, town_id, world_map)
                    .map_err(HostError::Core)?,
            )
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
    pub fn apply_declarative_spell_cast(
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

    pub fn active_static_spawns(&self) -> Result<FeTfsStaticSpawnCollection, HostError> {
        Ok(self.lock()?.active_static_spawn_collection())
    }

    /// Applies an explicitly selected, bounded target-acquisition pass to active static creatures.
    /// It records only the core target ID. It does not schedule movement, pursue a target, attack,
    /// send a packet, or change native visibility because target state is not yet client-rendered.
    pub fn acquire_static_creature_targets(
        &self,
        policy: StaticTargetAcquisitionPolicy,
    ) -> Result<StaticTargetAcquisitionSummary, HostError> {
        let StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range } = policy else {
            return Ok(StaticTargetAcquisitionSummary::default());
        };
        if !(1..=forgotten_core::MAX_STATIC_CREATURE_TARGET_RANGE).contains(&max_range) {
            return Err(HostError::Core(
                forgotten_core::CoreError::InvalidStaticCreatureTargetRange(max_range),
            ));
        }
        let mut world = self.lock()?;
        let creature_ids = world
            .active_static_spawn_collection()
            .entities
            .into_iter()
            .map(|entity| entity.id)
            .collect::<Vec<_>>();
        let mut summary = StaticTargetAcquisitionSummary {
            examined_static_creatures: creature_ids.len(),
            changed_static_targets: 0,
        };
        for creature_id in creature_ids {
            let previous = world
                .static_creature_target(creature_id)
                .map_err(HostError::Core)?;
            let selected = world
                .select_static_creature_target(creature_id, max_range)
                .map_err(HostError::Core)?;
            if previous != selected.target_player_id {
                summary.changed_static_targets += 1;
            }
        }
        Ok(summary)
    }

    /// Applies one explicit bounded pursue pass. For every active static creature, it chooses the
    /// nearest living player within the provided range and attempts at most one existing legal
    /// cardinal target step. It does not install a scheduler, retry blocked paths, attack, emit a
    /// target packet, or otherwise implement general creature AI.
    pub fn pursue_static_creature_targets_once(
        &self,
        world_map: &WorldMap,
        policy: StaticTargetPursuitPolicy,
    ) -> Result<StaticTargetPursuitSummary, HostError> {
        let StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range } = policy else {
            return Ok(StaticTargetPursuitSummary::default());
        };
        if !(1..=forgotten_core::MAX_STATIC_CREATURE_TARGET_RANGE).contains(&max_range) {
            return Err(HostError::Core(
                forgotten_core::CoreError::InvalidStaticCreatureTargetRange(max_range),
            ));
        }
        let mut world = self.lock()?;
        let creature_ids = world
            .active_static_spawn_collection()
            .entities
            .into_iter()
            .map(|entity| entity.id)
            .collect::<Vec<_>>();
        let mut summary = StaticTargetPursuitSummary {
            examined_static_creatures: creature_ids.len(),
            changed_static_targets: 0,
            moved_static_creatures: 0,
        };
        for creature_id in creature_ids {
            let previous = world
                .static_creature_target(creature_id)
                .map_err(HostError::Core)?;
            let selected = world
                .select_static_creature_target(creature_id, max_range)
                .map_err(HostError::Core)?;
            if previous != selected.target_player_id {
                summary.changed_static_targets += 1;
            }
            if matches!(
                world
                    .step_static_creature_toward_target(creature_id, world_map)
                    .map_err(HostError::Core)?,
                StaticCreatureTargetStepOutcome::Moved { .. }
            ) {
                summary.moved_static_creatures += 1;
            }
        }
        drop(world);
        if summary.moved_static_creatures > 0 {
            self.mark_visibility_changed();
        }
        Ok(summary)
    }

    /// Applies one explicit bounded static target-attack pass under the shared-world lock. It
    /// does not select targets, install timing beyond the caller, persist state, emit packets, or
    /// claim formulas, loot, corpses, scripts, or general creature AI.
    pub fn attack_static_creature_targets_once(
        &self,
        policy: StaticTargetAttackPolicy,
        world_map: &WorldMap,
    ) -> Result<StaticTargetAttackSummary, HostError> {
        let StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage } = policy else {
            return Ok(StaticTargetAttackSummary::default());
        };
        if !(1..=100).contains(&damage) {
            return Err(HostError::InvalidConfiguration(
                "static target attack damage must be between 1 and 100".into(),
            ));
        }
        let mut world = self.lock()?;
        let creature_ids = world
            .active_static_spawn_collection()
            .entities
            .into_iter()
            .map(|entity| entity.id)
            .collect::<Vec<_>>();
        let mut summary = StaticTargetAttackSummary {
            examined_static_creatures: creature_ids.len(),
            ..StaticTargetAttackSummary::default()
        };
        for creature_id in creature_ids {
            let outcome = world
                .apply_static_creature_target_damage(creature_id, damage, world_map)
                .map_err(HostError::Core)?;
            if let StaticCreatureTargetAttackOutcome::Applied {
                target_player_id,
                applied_damage,
                ..
            } = outcome
            {
                if applied_damage > 0 {
                    summary.applied_attacks += 1;
                    summary.total_applied_damage += u64::from(applied_damage);
                    summary.affected_player_ids.insert(target_player_id);
                }
            }
        }
        drop(world);
        if summary.applied_attacks > 0 {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(summary)
    }

    pub fn set_static_creature_health_percent(
        &self,
        creature_id: u32,
        health_percent: u8,
    ) -> Result<bool, HostError> {
        let changed = self
            .lock()?
            .set_static_creature_health_percent(creature_id, health_percent)
            .map_err(HostError::Core)?;
        if changed {
            self.mark_visibility_changed();
        }
        Ok(changed)
    }

    pub fn static_creature_experience_reward(&self, creature_id: u32) -> Result<u64, HostError> {
        self.lock()?
            .static_creature_experience_reward(creature_id)
            .map_err(HostError::Core)
    }

    pub fn static_creature_runtime_snapshot(
        &self,
    ) -> Result<Vec<StaticCreatureRuntimeSnapshot>, HostError> {
        Ok(self.lock()?.static_creature_runtime_snapshot())
    }

    pub fn restore_static_creature_runtime(
        &self,
        records: &[StaticCreatureRuntimeSnapshot],
    ) -> Result<StaticCreatureRuntimeRestoreSummary, HostError> {
        let summary = self
            .lock()?
            .restore_static_creature_runtime(records)
            .map_err(HostError::Core)?;
        if summary.restored > 0 {
            self.mark_visibility_changed();
        }
        Ok(summary)
    }

    pub fn apply_static_creature_melee_damage(
        &self,
        attacker_id: u64,
        target_id: u32,
        requested_damage: u16,
    ) -> Result<StaticCreatureDamageOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_static_creature_melee_damage(attacker_id, target_id, requested_damage)
            .map_err(HostError::Core)?;
        if outcome.applied_damage > 0 {
            self.mark_visibility_changed();
        }
        Ok(outcome)
    }

    /// Exposes one explicit core-only static target attack under the shared world lock. A real
    /// player-vitals mutation advances the existing refresh epoch, but scheduling, persistence,
    /// native delivery, formulas, loot, corpses, scripts, and creature AI remain caller-owned
    /// deferred concerns.
    pub fn apply_static_creature_target_damage(
        &self,
        creature_id: u32,
        requested_damage: u16,
        world_map: &WorldMap,
    ) -> Result<StaticCreatureTargetAttackOutcome, HostError> {
        let outcome = self
            .lock()?
            .apply_static_creature_target_damage(creature_id, requested_damage, world_map)
            .map_err(HostError::Core)?;
        if matches!(
            outcome,
            StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 1..,
                ..
            }
        ) {
            self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    /// Applies one caller-triggered bounded target step. Only a real movement increments the
    /// shared visibility epoch; target acquisition, scheduling, AI, combat, and packets remain
    /// outside this state transition.
    pub fn step_static_creature_toward_target(
        &self,
        creature_id: u32,
        world_map: &WorldMap,
    ) -> Result<StaticCreatureTargetStepOutcome, HostError> {
        let outcome = self
            .lock()?
            .step_static_creature_toward_target(creature_id, world_map)
            .map_err(HostError::Core)?;
        if matches!(outcome, StaticCreatureTargetStepOutcome::Moved { .. }) {
            self.mark_visibility_changed();
        }
        Ok(outcome)
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
        self.lock()?
            .player_render_snapshots()
            .into_iter()
            .filter(|player| player.id != observer_id)
            .map(|player| {
                Ok(NativeOtClientVisiblePlayer {
                    player_id: native_player_id(player.id)?,
                    name: player.name,
                    position: native_position(player.position),
                    look_type,
                    speed,
                })
            })
            .collect()
    }

    /// Captures all world-owned data needed for native map rendering under one short lock. The
    /// returned snapshot contains owned values so protocol encoding and socket writes can proceed
    /// concurrently without retaining the authoritative-world mutex.
    fn native_render_snapshot(
        &self,
        observer_id: u64,
        look_type: u8,
        speed: u16,
    ) -> Result<NativeWorldRenderSnapshot, HostError> {
        let (static_spawns, player_snapshots) = {
            let world = self.lock()?;
            (
                world.active_static_spawn_collection(),
                world.player_render_snapshots(),
            )
        };
        let visible_players = player_snapshots
            .into_iter()
            .filter(|player| player.id != observer_id)
            .map(|player| {
                Ok(NativeOtClientVisiblePlayer {
                    player_id: native_player_id(player.id)?,
                    name: player.name,
                    position: native_position(player.position),
                    look_type,
                    speed,
                })
            })
            .collect::<Result<Vec<_>, HostError>>()?;
        Ok(NativeWorldRenderSnapshot {
            static_spawns,
            visible_players,
        })
    }

    fn register_public_chat_recipient(
        &self,
        player_id: u64,
    ) -> Result<mpsc::Receiver<SharedPublicChatEvent>, HostError> {
        let (sender, receiver) = mpsc::sync_channel(NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY);
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        if recipients.insert(player_id, sender).is_some() {
            return Err(HostError::InvalidConfiguration(
                "shared chat recipient already registered for player".into(),
            ));
        }
        Ok(receiver)
    }

    fn unregister_public_chat_recipient(&self, player_id: u64) {
        if let Ok(mut recipients) = self.chat_recipients.lock() {
            recipients.remove(&player_id);
        }
    }

    fn broadcast_public_chat(&self, sender_id: u64, message: &str) -> Result<usize, HostError> {
        let sender = self
            .lock()?
            .player(sender_id)
            .cloned()
            .ok_or(forgotten_core::CoreError::UnknownPlayer(sender_id))
            .map_err(HostError::Core)?;
        let body = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if body.is_empty() {
            return Ok(0);
        }
        let event = SharedPublicChatEvent {
            speaker_name: sender.name,
            speaker_position: native_position(sender.position),
            text: truncate_native_chat_text(&body),
        };
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        recipients.retain(|_, recipient| match recipient.try_send(event.clone()) {
            Ok(()) => {
                delivered += 1;
                true
            }
            Err(mpsc::TrySendError::Full(_)) => true,
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        });
        Ok(delivered)
    }

    pub fn register_player_at_available_position(
        &self,
        player: Player,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        self.register_player_at_available_position_with_vitals(
            player,
            PlayerVitals::default(),
            world_map,
        )
    }

    pub fn register_player_at_available_position_with_vitals(
        &self,
        player: Player,
        vitals: PlayerVitals,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        self.register_player_at_available_position_with_vitals_and_progression(
            player,
            vitals,
            PlayerProgression::default(),
            world_map,
        )
    }

    pub fn register_player_at_available_position_with_vitals_and_progression(
        &self,
        mut player: Player,
        vitals: PlayerVitals,
        progression: PlayerProgression,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let mut world = self.lock()?;
        let position = [player.position, world_map.spawn()]
            .into_iter()
            .find(|position| {
                world_map.is_walkable(*position)
                    && !world.is_static_creature_occupied(*position)
                    && !world.is_player_occupied(*position)
            })
            .or_else(|| {
                world_map.tiles().find_map(|(position, tile)| {
                    (tile.walkable
                        && !world.is_static_creature_occupied(position)
                        && !world.is_player_occupied(position))
                    .then_some(position)
                })
            })
            .ok_or_else(|| {
                HostError::InvalidConfiguration(
                    "native map has no walkable tile unoccupied by a player or static creature"
                        .into(),
                )
            })?;
        player.position = position;
        world
            .add_player_with_vitals_and_progression(player, vitals, progression)
            .map_err(HostError::Core)?;
        self.mark_visibility_changed();
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_and_equipment(
        &self,
        player: Player,
        vitals: PlayerVitals,
        equipment: PlayerEquipment,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position =
            self.register_player_at_available_position_with_vitals(player, vitals, world_map)?;
        self.replace_player_equipment(player_id, equipment)?;
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_equipment_and_containers(
        &self,
        player: Player,
        vitals: PlayerVitals,
        equipment: PlayerEquipment,
        containers: PlayerContainers,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position = self.register_player_at_available_position_with_vitals_and_equipment(
            player, vitals, equipment, world_map,
        )?;
        self.replace_player_containers(player_id, containers)?;
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_equipment_containers_and_progression(
        &self,
        player: Player,
        vitals: PlayerVitals,
        progression: PlayerProgression,
        equipment: PlayerEquipment,
        containers: PlayerContainers,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position = self.register_player_at_available_position_with_vitals_and_progression(
            player,
            vitals,
            progression,
            world_map,
        )?;
        self.replace_player_equipment(player_id, equipment)?;
        self.replace_player_containers(player_id, containers)?;
        Ok(position)
    }

    pub fn register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
        &self,
        player: Player,
        vitals: PlayerVitals,
        hydration: NativePlayerHydration,
        world_map: &WorldMap,
    ) -> Result<Position, HostError> {
        let player_id = player.id;
        let position = self
            .register_player_at_available_position_with_vitals_equipment_containers_and_progression(
                player,
                vitals,
                hydration.progression,
                hydration.equipment,
                hydration.containers,
                world_map,
            )?;
        self.replace_player_progression_attempts(player_id, hydration.progression_attempts)?;
        self.replace_player_town(player_id, hydration.town_id)?;
        self.replace_player_conditions(player_id, hydration.conditions)?;
        self.hydrate_player_respawn_state(player_id, hydration.respawn_state)?;
        Ok(position)
    }

    pub fn remove_player(&self, id: u64) -> Result<(), HostError> {
        self.lock()?.remove_player(id).map_err(HostError::Core)?;
        self.mark_visibility_changed();
        Ok(())
    }

    fn mark_visibility_changed(&self) {
        self.visibility_epoch.fetch_add(1, Ordering::SeqCst);
    }

    fn lock(&self) -> Result<MutexGuard<'_, WorldState>, HostError> {
        self.world
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)
    }
}

#[derive(Debug)]
struct SharedNativePlayerRegistration {
    world: SharedNativeWorld,
    player_id: u64,
}

impl Drop for SharedNativePlayerRegistration {
    fn drop(&mut self) {
        self.world.unregister_public_chat_recipient(self.player_id);
        let _ = self.world.remove_player(self.player_id);
    }
}

impl HostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if self.max_connections == 0 {
            return Err(HostError::InvalidConfiguration(
                "max_connections must be greater than zero".into(),
            ));
        }
        if self.session_timeout.is_zero() {
            return Err(HostError::InvalidConfiguration(
                "session_timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

impl StatusHostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if self.max_connections == 0 {
            return Err(HostError::InvalidConfiguration(
                "max_connections must be greater than zero".into(),
            ));
        }
        if self.session_timeout.is_zero() {
            return Err(HostError::InvalidConfiguration(
                "session_timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

impl GameSessionHostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if self.profile.id != "fe-7.4" {
            return Err(HostError::LegacyLoginUnavailable);
        }
        if self.max_connections == 0 {
            return Err(HostError::InvalidConfiguration(
                "max_connections must be greater than zero".into(),
            ));
        }
        if self.session_timeout.is_zero() {
            return Err(HostError::InvalidConfiguration(
                "session_timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

impl NativeOtClientHostConfig {
    pub fn validate(&self) -> Result<(), HostError> {
        if !self.client_profile.supports_current_native_foundation() {
            return Err(HostError::InvalidConfiguration(
                "selected native client profile is not supported by the current foundation".into(),
            ));
        }
        if self.max_connections == 0 {
            return Err(HostError::InvalidConfiguration(
                "max_connections must be greater than zero".into(),
            ));
        }
        if self.session_timeout.is_zero() {
            return Err(HostError::InvalidConfiguration(
                "session_timeout must be greater than zero".into(),
            ));
        }
        if matches!(self.death_loss_policy, DeathLossPolicy::FixedPercent(_))
            && self.progression_rules.is_none()
        {
            return Err(HostError::InvalidConfiguration(
                "fixed deathLosePercent requires validated vocation progression rules".into(),
            ));
        }
        if let Some(empty_world) = &self.empty_world {
            if empty_world.player_speed == 0 || empty_world.server_beat == 0 {
                return Err(HostError::InvalidConfiguration(
                    "native empty-world fixture requires nonzero speed and beat values".into(),
                ));
            }
            if self.world_map.is_none() {
                return Err(HostError::InvalidConfiguration(
                    "native map initialization requires a loaded world map".into(),
                ));
            }
        }
        Ok(())
    }
}

pub struct HostHandle {
    local_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<(), HostError>>>,
}

impl HostHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn shutdown_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }

    pub fn shutdown(mut self) -> Result<(), HostError> {
        self.shutdown.store(true, Ordering::SeqCst);
        match self.thread.take().expect("host thread exists").join() {
            Ok(result) => result,
            Err(_) => Err(HostError::HostThreadPanicked),
        }
    }
}

pub fn start(config: HostConfig, database_path: impl AsRef<Path>) -> Result<HostHandle, HostError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
        )
    });

    Ok(HostHandle {
        local_addr,
        shutdown,
        thread: Some(thread),
    })
}

pub fn start_status(
    config: StatusHostConfig,
    database_path: impl AsRef<Path>,
) -> Result<HostHandle, HostError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve_status(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
            Instant::now(),
        )
    });
    Ok(HostHandle {
        local_addr,
        shutdown,
        thread: Some(thread),
    })
}

pub fn start_game_session(
    config: GameSessionHostConfig,
    database_path: impl AsRef<Path>,
) -> Result<HostHandle, HostError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve_game_session(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
        )
    });
    Ok(HostHandle {
        local_addr,
        shutdown,
        thread: Some(thread),
    })
}

pub fn start_native_otclient_login(
    config: NativeOtClientHostConfig,
    database_path: impl AsRef<Path>,
) -> Result<HostHandle, HostError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve_native_otclient_login(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
        )
    });
    Ok(HostHandle {
        local_addr,
        shutdown,
        thread: Some(thread),
    })
}

pub fn start_native_otclient_game(
    config: NativeOtClientHostConfig,
    database_path: impl AsRef<Path>,
) -> Result<HostHandle, HostError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_connections = Arc::new(AtomicUsize::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let shared_world = SharedNativeWorld::from_static_spawns(config.static_spawns.as_deref())?;
    restore_static_creature_runtime_from_database(&shared_world, &database_path)?;
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::spawn(move || {
        serve_native_otclient_game(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
            shared_world,
        )
    });
    Ok(HostHandle {
        local_addr,
        shutdown,
        thread: Some(thread),
    })
}

fn serve(
    listener: TcpListener,
    config: HostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "network host started addr={} profile={}",
            listener.local_addr()?,
            config.profile.id
        ),
    );

    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let active = active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= config.max_connections {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    let _ = write_frame(&mut stream, &error_frame(b"busy"));
                    record_event(
                        &database_path,
                        "warn",
                        &format!("connection rejected peer={peer} reason=connection-limit"),
                    );
                    continue;
                }

                let session_config = config.clone();
                let session_database_path = database_path.clone();
                let session_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let result =
                        handle_session(&mut stream, peer, &session_config, &session_database_path);
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("session rejected peer={peer} reason={error}"),
                        );
                    }
                    session_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(HostError::Io(error)),
        }
    }

    record_event(&database_path, "info", "network host stopped");
    Ok(())
}

fn serve_status(
    listener: TcpListener,
    config: StatusHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    started_at: Instant,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "status service started addr={} profile={}",
            listener.local_addr()?,
            config.profile.id
        ),
    );
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let active = active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= config.max_connections {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let session_config = config.clone();
                let session_database_path = database_path.clone();
                let session_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let result = handle_status_session(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                        started_at,
                    );
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("status session rejected peer={peer} reason={error}"),
                        );
                    }
                    session_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(HostError::Io(error)),
        }
    }
    record_event(&database_path, "info", "status service stopped");
    Ok(())
}

fn serve_game_session(
    listener: TcpListener,
    config: GameSessionHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "game session foundation started addr={} profile={}",
            listener.local_addr()?,
            config.profile.id
        ),
    );
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let active = active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= config.max_connections {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let session_config = config.clone();
                let session_database_path = database_path.clone();
                let session_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let result = handle_game_session(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                    );
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("game session rejected peer={peer} reason={error}"),
                        );
                    }
                    session_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(HostError::Io(error)),
        }
    }
    record_event(&database_path, "info", "game session foundation stopped");
    Ok(())
}

fn serve_native_otclient_login(
    listener: TcpListener,
    config: NativeOtClientHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "native client login service started addr={} protocol={}",
            listener.local_addr()?,
            config.client_profile.protocol_version
        ),
    );
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let active = active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= config.max_connections {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let session_config = config.clone();
                let session_database_path = database_path.clone();
                let session_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let result = handle_native_otclient_login(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                    );
                    if let Err(error) = result {
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("native login rejected peer={peer} reason={error}"),
                        );
                    }
                    session_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(HostError::Io(error)),
        }
    }
    record_event(
        &database_path,
        "info",
        "native client login service stopped",
    );
    Ok(())
}

/// Tracks one native session's authoritative lifecycle state. A session that joins while already
/// dead does not synthesize a historical death packet; a future authoritative respawn resets the
/// observation so a subsequent real death can be delivered once. Respawn timing, teleportation,
/// loss, and client-side recovery records remain separate deferred work.
fn observe_native_death_transition(
    shared_world: &SharedNativeWorld,
    player_id: u64,
    observed_dead: &mut bool,
) -> Result<bool, HostError> {
    let dead = shared_world.player_respawn_state(player_id)?.dead;
    let should_notify = dead && !*observed_dead;
    *observed_dead = dead;
    Ok(should_notify)
}

fn serve_native_otclient_game(
    listener: TcpListener,
    config: NativeOtClientHostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    shared_world: SharedNativeWorld,
) -> Result<(), HostError> {
    record_event(
        &database_path,
        "info",
        &format!(
            "native client game service started addr={} protocol={}",
            listener.local_addr()?,
            config.client_profile.protocol_version
        ),
    );
    let heartbeat_shutdown = Arc::clone(&shutdown);
    let heartbeat_world = shared_world.clone();
    let heartbeat_attack_policy = config.static_target_attack_policy;
    let heartbeat_map = config.world_map.clone();
    let heartbeat_database_path = database_path.clone();
    let heartbeat_death_loss_policy = config.death_loss_policy;
    let heartbeat_progression_rules = config.progression_rules.clone();
    let heartbeat = thread::spawn(move || {
        run_native_shared_world_heartbeat(
            heartbeat_world,
            heartbeat_shutdown,
            heartbeat_attack_policy,
            heartbeat_map,
            heartbeat_database_path,
            heartbeat_death_loss_policy,
            heartbeat_progression_rules,
        )
    });
    let service_result = loop {
        if shutdown.load(Ordering::SeqCst) {
            break Ok(());
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let active = active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= config.max_connections {
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let session_config = config.clone();
                let session_database_path = database_path.clone();
                let session_connections = Arc::clone(&active_connections);
                let session_world = shared_world.clone();
                thread::spawn(move || {
                    let result = handle_native_otclient_game(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                        &session_world,
                    );
                    if let Err(error) = result {
                        eprintln!("> Native OTCv8 game session ended peer={peer} reason={error}");
                        record_event(
                            &session_database_path,
                            "warn",
                            &format!("native game rejected peer={peer} reason={error}"),
                        );
                    }
                    session_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => break Err(HostError::Io(error)),
        }
    };
    shutdown.store(true, Ordering::SeqCst);
    let heartbeat_result = match heartbeat.join() {
        Ok(result) => result,
        Err(_) => Err(HostError::HostThreadPanicked),
    };
    service_result?;
    heartbeat_result?;
    persist_static_creature_runtime_to_database(&shared_world, &database_path)?;
    record_event(&database_path, "info", "native client game service stopped");
    Ok(())
}

fn restore_static_creature_runtime_from_database(
    shared_world: &SharedNativeWorld,
    database_path: &Path,
) -> Result<StaticCreatureRuntimeRestoreSummary, HostError> {
    let database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    let records = database
        .static_creature_runtime()
        .map_err(HostError::Persistence)?;
    let snapshots = records
        .into_iter()
        .map(|record| StaticCreatureRuntimeSnapshot {
            id: record.creature_id,
            position: record.position,
            active: record.active,
            health_percent: record.health_percent,
            reactivation_remaining_seconds: record.reactivation_remaining_seconds,
        })
        .collect::<Vec<_>>();
    shared_world.restore_static_creature_runtime(&snapshots)
}

fn persist_static_creature_runtime_to_database(
    shared_world: &SharedNativeWorld,
    database_path: &Path,
) -> Result<(), HostError> {
    let mut database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    persist_static_creature_runtime_to_open_database(shared_world, &mut database)
}

fn persist_static_creature_runtime_to_open_database(
    shared_world: &SharedNativeWorld,
    database: &mut EngineDatabase,
) -> Result<(), HostError> {
    let snapshots = shared_world.static_creature_runtime_snapshot()?;
    let records = snapshots
        .into_iter()
        .map(|snapshot| StaticCreatureRuntimeRecord {
            creature_id: snapshot.id,
            position: snapshot.position,
            active: snapshot.active,
            health_percent: snapshot.health_percent,
            reactivation_remaining_seconds: snapshot.reactivation_remaining_seconds,
        })
        .collect::<Vec<_>>();
    database
        .replace_static_creature_runtime(&records)
        .map_err(HostError::Persistence)
}

fn handle_native_otclient_login(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &NativeOtClientHostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let request =
        decode_native_otclient_login_request(&read_frame(stream)?, &config.client_profile)
            .map_err(HostError::Protocol)?;
    let database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    let Some(account) = database
        .authenticate_account_id(request.account_id, &request.password)
        .map_err(HostError::Persistence)?
    else {
        write_frame(
            stream,
            &encode_native_otclient_login_error("Account name or password is not correct."),
        )?;
        return Ok(());
    };
    let IpAddr::V4(address) = config.advertised_game_addr.ip() else {
        write_frame(
            stream,
            &encode_native_otclient_login_error(
                "This native client profile requires an IPv4 game endpoint.",
            ),
        )?;
        return Ok(());
    };
    let entries = account
        .characters
        .iter()
        .map(|character| CharacterListEntry {
            name: character.name.clone(),
            world_name: config.server_name.clone(),
            address: IpAddr::V4(address),
            port: config.advertised_game_addr.port(),
        })
        .collect::<Vec<_>>();
    write_frame(
        stream,
        &encode_native_otclient_character_list(&entries).map_err(HostError::Protocol)?,
    )?;
    record_event(
        database_path,
        "info",
        &format!(
            "native client login accepted peer={peer} account={} protocol={}",
            account.id, request.protocol_version
        ),
    );
    Ok(())
}

fn handle_native_otclient_game(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &NativeOtClientHostConfig,
    database_path: &Path,
    shared_world: &SharedNativeWorld,
) -> Result<(), HostError> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let request = decode_native_otclient_game_request(&read_frame(stream)?, &config.client_profile)
        .map_err(HostError::Protocol)?;
    let mut database = EngineDatabase::open(database_path).map_err(HostError::Persistence)?;
    let Some(account) = database
        .authenticate_account_id(request.account_id, &request.password)
        .map_err(HostError::Persistence)?
    else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error("Account name or password is not correct."),
        )?;
        return Ok(());
    };
    let Some(character) = account
        .characters
        .iter()
        .find(|character| character.name == request.character_name)
    else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error("Character does not belong to this account."),
        )?;
        return Ok(());
    };
    let Some(empty_world) = &config.empty_world else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error(
                "Forgotten Engine native map initialization is not enabled for this selected client profile.",
            ),
        )?;
        return Ok(());
    };
    let Some(world_map) = &config.world_map else {
        write_frame(
            stream,
            &encode_native_otclient_game_login_error(
                "Forgotten Engine native map initialization requires a selected world map.",
            ),
        )?;
        return Ok(());
    };
    let account_id = u64::try_from(account.id).map_err(|_| {
        HostError::InvalidConfiguration("native numeric account IDs must be non-negative".into())
    })?;
    let equipment = database
        .player_equipment(character.id)
        .map_err(HostError::Persistence)?;
    let containers = database
        .player_containers(character.id)
        .map_err(HostError::Persistence)?;
    let conditions = database
        .player_conditions(character.id)
        .map_err(HostError::Persistence)?;
    let bootstrap_equipment = equipment.clone();
    let bootstrap_containers = containers.clone();
    let initial_position = match shared_world
        .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
            Player {
                id: character.id,
                account_id,
                name: character.name.clone(),
                position: character.position,
                level: character.level,
                experience: character.experience,
                skill_points: character.skill_points,
            },
            PlayerVitals {
                health: character.vitals.health,
                max_health: character.vitals.max_health,
                mana: character.vitals.mana,
                max_mana: character.vitals.max_mana,
                capacity: character.vitals.capacity,
                magic_level: character.vitals.magic_level,
            },
            NativePlayerHydration {
                progression: character.progression,
                progression_attempts: character.progression_attempts,
                town_id: character.town_id,
                respawn_state: character.respawn_state,
                equipment,
                containers,
                conditions,
            },
            world_map,
        ) {
        Ok(position) => position,
        Err(HostError::Core(forgotten_core::CoreError::DuplicatePlayer(_))) => {
            write_frame(
                stream,
                &encode_native_otclient_game_login_error(
                    "Character is already active in the shared world.",
                ),
            )?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let _registration = SharedNativePlayerRegistration {
        world: shared_world.clone(),
        player_id: character.id,
    };
    let chat_events = shared_world.register_public_chat_recipient(character.id)?;
    if initial_position != character.position {
        database.update_player_position(character.id, initial_position)?;
    }
    let player_id = native_player_id(character.id)?;
    let (authoritative_player, authoritative_vitals) =
        shared_world.player_and_vitals(character.id)?;
    let mut snapshot = NativeOtClientEmptyWorldSnapshot {
        player_id,
        player_name: character.name.clone(),
        player_position: native_position(initial_position),
        player_level: 1,
        player_experience: 0,
        player_vitals: NativeOtClientPlayerVitals::default(),
        player_skills: character.progression.skills,
        ground_thing_id: empty_world.ground_thing_id,
        player_look_type: empty_world.player_look_type,
        player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
        player_speed: empty_world.player_speed,
        server_beat: empty_world.server_beat,
    };
    refresh_native_player_stats_snapshot(
        &mut snapshot,
        &authoritative_player,
        authoritative_vitals,
    );
    let active_static_spawns = shared_world.active_static_spawns()?;
    let visible_players = shared_world.visible_players(
        character.id,
        empty_world.player_look_type,
        empty_world.player_speed,
    )?;
    let mut initialization =
        encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players(
            &config.client_profile,
            &snapshot,
            world_map,
            Some(&active_static_spawns),
            Some(&visible_players),
        )
        .map_err(HostError::Protocol)?;
    let fight_mode_state = shared_world.player_fight_mode_state(character.id)?;
    let mode = match fight_mode_state.mode {
        PlayerFightMode::Attack => NativeOtClientFightMode::Attack,
        PlayerFightMode::Balanced => NativeOtClientFightMode::Balanced,
        PlayerFightMode::Defense => NativeOtClientFightMode::Defense,
    };
    initialization.0.extend_from_slice(
        &encode_native_otclient_player_modes(
            &config.client_profile,
            NativeOtClientFightModeRequest {
                mode,
                chase: fight_mode_state.chase,
                secure: fight_mode_state.secure,
            },
        )
        .map_err(HostError::Protocol)?
        .0,
    );
    let equipment_frames = native_classic_equipment_frames(
        &config.client_profile,
        config.item_presentation_catalog.as_deref(),
        &bootstrap_equipment,
    )
    .map_err(HostError::Protocol)?;
    let mut observed_mapped_equipment = native_classic_mapped_equipment(
        config.item_presentation_catalog.as_deref(),
        &bootstrap_equipment,
    );
    let container_frames = native_classic_container_frames(
        &config.client_profile,
        config.item_presentation_catalog.as_deref(),
        &bootstrap_containers,
        &BTreeSet::new(),
    )
    .map_err(HostError::Protocol)?;
    let static_health_frames =
        native_static_creature_health_frames(&config.client_profile, &active_static_spawns)?;
    write_frame(stream, &initialization)?;
    let mut player_outfit = native_hydrated_classic_outfit(
        empty_world.player_look_type,
        empty_world.outfit_first_look_type,
        empty_world.outfit_last_look_type,
        character.outfit,
    );
    if character.outfit.look_type == player_outfit.look_type && player_outfit.look_type != 0 {
        let hydrated_outfit = encode_native_otclient_creature_outfit(
            &config.client_profile,
            snapshot.player_id,
            player_outfit,
        )
        .map_err(HostError::Protocol)?;
        write_frame(stream, &hydrated_outfit)?;
        native_diagnostic(
            config.extended_diagnostics,
            peer,
            &format!(
                "outbound=hydrated-creature-outfit opcode=0x8e bytes={} look-type={}",
                hydrated_outfit.0.len(),
                player_outfit.look_type
            ),
        );
    }
    for frame in &equipment_frames {
        write_frame(stream, frame)?;
    }
    for frame in &container_frames {
        write_frame(stream, frame)?;
    }
    for frame in &static_health_frames {
        write_frame(stream, frame)?;
    }
    stream.set_read_timeout(Some(NATIVE_OTCLIENT_HEARTBEAT_INTERVAL))?;
    if config.extended_diagnostics {
        eprintln!(
            "> Native OTCv8 map init sent peer={peer} player={} record-bytes={} equipment-records={}/{} skipped-unmapped={} container-records={}/{} skipped-unmapped-or-nested={} static-health-records={} map={} tiles={} static-spawns={} login-state-opcode=0x0a map-opcode=0x64 asset-free={}",
            character.name,
            initialization.0.len(),
            equipment_frames.len(),
            bootstrap_equipment.len(),
            bootstrap_equipment.len().saturating_sub(equipment_frames.len()),
            container_frames.len(),
            bootstrap_containers.len(),
            bootstrap_containers.len().saturating_sub(container_frames.len()),
            static_health_frames.len(),
            world_map.identifier(),
            world_map.tile_count(),
            active_static_spawns.entities.len(),
            snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0,
        );
    }

    let mut player_position = initial_position;
    let mut facing = NativeOtClientCardinalDirection::South;
    let mut active_click_walk: Option<NativeActiveClickWalk> = None;
    let mut last_regeneration_tick = Instant::now();
    let mut last_condition_tick = Instant::now();
    let mut observed_visibility_epoch = shared_world.visibility_epoch();
    let mut observed_vitals_epoch = shared_world.vitals_epoch();
    let mut observed_progression_epoch = shared_world.progression_epoch();
    let mut observed_equipment_epoch = shared_world.equipment_epoch();
    let mut observed_containers_epoch = shared_world.containers_epoch();
    let mut closed_container_ids = BTreeSet::new();
    let mut observed_dead = shared_world.player_respawn_state(character.id)?.dead;
    loop {
        drain_shared_public_chat(
            stream,
            &config.client_profile,
            &chat_events,
            config.extended_diagnostics,
            peer,
        )?;
        if observe_native_death_transition(shared_world, character.id, &mut observed_dead)? {
            let death = encode_native_otclient_game_death(&config.client_profile)
                .map_err(HostError::Protocol)?;
            write_frame(stream, &death)?;
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                "lifecycle=death-notification profile=740 fields=none source=shared-world-transition",
            );
        }
        let read_timeout = active_click_walk
            .as_ref()
            .map(|task| {
                task.next_step_deadline
                    .saturating_duration_since(Instant::now())
                    .min(NATIVE_OTCLIENT_HEARTBEAT_INTERVAL)
                    .max(Duration::from_millis(1))
            })
            .unwrap_or(NATIVE_OTCLIENT_HEARTBEAT_INTERVAL);
        stream.set_read_timeout(Some(read_timeout))?;
        let action = {
            let request = match read_frame(stream) {
                Ok(request) => request,
                Err(HostError::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    drain_shared_public_chat(
                        stream,
                        &config.client_profile,
                        &chat_events,
                        config.extended_diagnostics,
                        peer,
                    )?;
                    let now = Instant::now();
                    let condition_elapsed_seconds =
                        now.saturating_duration_since(last_condition_tick)
                            .as_secs()
                            .min(u64::from(u16::MAX)) as u16;
                    if condition_elapsed_seconds > 0 {
                        last_condition_tick +=
                            Duration::from_secs(u64::from(condition_elapsed_seconds));
                        let (outcome, mut vitals, death_state) = match shared_world
                            .apply_player_conditions_with_death(
                                character.id,
                                world_map,
                                condition_elapsed_seconds,
                            ) {
                            Ok(result) => result,
                            Err(HostError::Core(
                                forgotten_core::CoreError::PlayerTownUnassigned(_)
                                | forgotten_core::CoreError::UnknownTown(_),
                            )) => {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "lifecycle=conditions outcome=paused-invalid-death-temple",
                                );
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        persist_runtime_player_conditions(
                            &mut database,
                            shared_world,
                            character.id,
                        )?;
                        if outcome.applied_damage > 0 {
                            let died = death_state.is_some();
                            let loss_persisted = if died {
                                apply_configured_native_death_loss(
                                    &mut database,
                                    shared_world,
                                    character.id,
                                    config.death_loss_policy,
                                    config.progression_rules.as_deref(),
                                )?
                            } else {
                                false
                            };
                            if loss_persisted {
                                vitals = shared_world.player_vitals(character.id)?;
                            }
                            let persisted_vitals = PersistedPlayerVitals {
                                health: vitals.health,
                                max_health: vitals.max_health,
                                mana: vitals.mana,
                                max_mana: vitals.max_mana,
                                capacity: vitals.capacity,
                                magic_level: vitals.magic_level,
                            };
                            if loss_persisted {
                                // The complete post-loss snapshot was already committed atomically.
                            } else if let Some(death_state) = death_state {
                                database.update_player_vitals_and_respawn_state(
                                    character.id,
                                    persisted_vitals,
                                    death_state,
                                )?;
                            } else {
                                database.update_player_vitals(character.id, persisted_vitals)?;
                            }
                            if died {
                                let death =
                                    encode_native_otclient_game_death(&config.client_profile)
                                        .map_err(HostError::Protocol)?;
                                write_frame(stream, &death)?;
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    "lifecycle=death-notification profile=740 fields=none",
                                );
                                observed_dead = true;
                            }
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "lifecycle=conditions damage={} health={} expired={} dead={}",
                                    outcome.applied_damage,
                                    outcome.remaining_health,
                                    outcome.expired_conditions,
                                    death_state.is_some(),
                                ),
                            );
                        }
                    }
                    if let Some(rules_by_vocation) = config.regeneration_rules.as_deref() {
                        let elapsed_seconds =
                            now.saturating_duration_since(last_regeneration_tick)
                                .as_secs()
                                .min(u64::from(u16::MAX)) as u16;
                        if elapsed_seconds > 0 {
                            last_regeneration_tick +=
                                Duration::from_secs(u64::from(elapsed_seconds));
                            let vocation = shared_world.player_progression(character.id)?.vocation;
                            if let Some(rules) = rules_by_vocation.get(&vocation).copied() {
                                let outcome = shared_world.apply_player_regeneration(
                                    character.id,
                                    rules,
                                    elapsed_seconds,
                                )?;
                                if outcome.health_gained > 0 || outcome.mana_gained > 0 {
                                    database.update_player_vitals(
                                        character.id,
                                        PersistedPlayerVitals {
                                            health: outcome.vitals.health,
                                            max_health: outcome.vitals.max_health,
                                            mana: outcome.vitals.mana,
                                            max_mana: outcome.vitals.max_mana,
                                            capacity: outcome.vitals.capacity,
                                            magic_level: outcome.vitals.magic_level,
                                        },
                                    )?;
                                    native_diagnostic(
                                        config.extended_diagnostics,
                                        peer,
                                        &format!(
                                            "lifecycle=regeneration vocation={} health-gained={} mana-gained={}",
                                            vocation.value(),
                                            outcome.health_gained,
                                            outcome.mana_gained
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    if let Some((target_native_id, target_vitals, outcome)) =
                        apply_native_selected_player_melee(
                            &mut database,
                            shared_world,
                            character.id,
                            world_map,
                            NativeSelectedPlayerMeleePolicy {
                                progression_rules: config.progression_rules.as_deref(),
                                skill_rate: config.skill_rate,
                                death_loss_policy: config.death_loss_policy,
                                declarative_weapon_catalog: config
                                    .declarative_weapon_catalog
                                    .as_deref(),
                            },
                        )?
                    {
                        let health_update = encode_native_otclient_creature_health(
                            &config.client_profile,
                            target_native_id,
                            target_vitals.health,
                            target_vitals.max_health,
                        )
                        .map_err(HostError::Protocol)?;
                        write_frame(stream, &health_update)?;
                        if outcome.defeated {
                            let death = encode_native_otclient_game_death(&config.client_profile)
                                .map_err(HostError::Protocol)?;
                            write_frame(stream, &death)?;
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "lifecycle=death-notification profile=740 fields=none",
                            );
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "combat=selected-player-melee target={} damage={} health={}/{}",
                                outcome.target_id,
                                outcome.applied_damage,
                                target_vitals.health,
                                target_vitals.max_health
                            ),
                        );
                    }
                    if let Some(outcome) = apply_native_selected_static_creature_melee(
                        shared_world,
                        character.id,
                        world_map,
                    )? {
                        persist_static_creature_runtime_to_open_database(
                            shared_world,
                            &mut database,
                        )?;
                        if outcome.deactivated {
                            apply_and_persist_native_static_defeat_experience(
                                &mut database,
                                shared_world,
                                character.id,
                                outcome.target_id,
                                config.experience_award_policy.as_deref(),
                                config.vocation_level_up_gains.as_deref(),
                            )?;
                        }
                        let health_update = encode_native_otclient_creature_health(
                            &config.client_profile,
                            outcome.target_id,
                            u16::from(outcome.remaining_health_percent),
                            100,
                        )
                        .map_err(HostError::Protocol)?;
                        write_frame(stream, &health_update)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "combat=selected-static-melee target={} damage={} health-percent={} deactivated={} delivery=creature-health persistence=static-runtime",
                                outcome.target_id,
                                outcome.applied_damage,
                                outcome.remaining_health_percent,
                                outcome.deactivated
                            ),
                        );
                    }
                    let vitals_epoch = shared_world.vitals_epoch();
                    if vitals_epoch != observed_vitals_epoch {
                        let (player, vitals) = shared_world.player_and_vitals(character.id)?;
                        refresh_native_player_stats_snapshot(&mut snapshot, &player, vitals);
                        let stats_update =
                            encode_native_otclient_player_stats(&config.client_profile, &snapshot)
                                .map_err(HostError::Protocol)?;
                        write_frame(stream, &stats_update)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=player-stats-refresh epoch={vitals_epoch} bytes={}",
                                stats_update.0.len()
                            ),
                        );
                        observed_vitals_epoch = vitals_epoch;
                    }
                    let progression_epoch = shared_world.progression_epoch();
                    if progression_epoch != observed_progression_epoch {
                        snapshot.player_skills =
                            shared_world.player_progression(character.id)?.skills;
                        let skills_update =
                            encode_native_otclient_player_skills(&config.client_profile, &snapshot)
                                .map_err(HostError::Protocol)?;
                        write_frame(stream, &skills_update)?;
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=player-skills-refresh epoch={progression_epoch} bytes={}",
                                skills_update.0.len()
                            ),
                        );
                        observed_progression_epoch = progression_epoch;
                    }
                    let equipment_epoch = shared_world.equipment_epoch();
                    if equipment_epoch != observed_equipment_epoch {
                        let equipment = shared_world.player_equipment(character.id)?;
                        let current_mapped_equipment = native_classic_mapped_equipment(
                            config.item_presentation_catalog.as_deref(),
                            &equipment,
                        );
                        let equipment_updates = native_classic_equipment_delta_frames(
                            &config.client_profile,
                            &observed_mapped_equipment,
                            &current_mapped_equipment,
                        )
                        .map_err(HostError::Protocol)?;
                        for frame in &equipment_updates {
                            write_frame(stream, frame)?;
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=equipment-refresh epoch={equipment_epoch} records={}",
                                equipment_updates.len()
                            ),
                        );
                        observed_mapped_equipment = current_mapped_equipment;
                        observed_equipment_epoch = equipment_epoch;
                    }
                    let containers_epoch = shared_world.containers_epoch();
                    if containers_epoch != observed_containers_epoch {
                        let containers = shared_world.player_containers(character.id)?;
                        let container_updates = native_classic_container_frames(
                            &config.client_profile,
                            config.item_presentation_catalog.as_deref(),
                            &containers,
                            &closed_container_ids,
                        )
                        .map_err(HostError::Protocol)?;
                        for frame in &container_updates {
                            write_frame(stream, frame)?;
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=container-refresh epoch={containers_epoch} records={}",
                                container_updates.len()
                            ),
                        );
                        observed_containers_epoch = containers_epoch;
                    }
                    let visibility_epoch = shared_world.visibility_epoch();
                    if visibility_epoch != observed_visibility_epoch {
                        let mut refreshed_snapshot = snapshot.clone();
                        refreshed_snapshot.player_position = native_position(player_position);
                        refreshed_snapshot.player_direction = facing.protocol_direction();
                        let refreshed_viewport = encode_shared_native_world_viewport(
                            &config.client_profile,
                            &refreshed_snapshot,
                            world_map,
                            shared_world,
                            character.id,
                        )?;
                        let refreshed_static_spawns = shared_world.active_static_spawns()?;
                        let refreshed_static_health_frames = native_static_creature_health_frames(
                            &config.client_profile,
                            &refreshed_static_spawns,
                        )?;
                        write_frame(stream, &refreshed_viewport)?;
                        for frame in &refreshed_static_health_frames {
                            write_frame(stream, frame)?;
                        }
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            &format!(
                                "outbound=viewport-refresh reason=visibility-epoch epoch={visibility_epoch} bytes={} static-health-records={}",
                                refreshed_viewport.0.len(),
                                refreshed_static_health_frames.len()
                            ),
                        );
                        observed_visibility_epoch = visibility_epoch;
                        continue;
                    }
                    if active_click_walk
                        .as_ref()
                        .is_some_and(|task| task.next_step_deadline <= Instant::now())
                    {
                        let next_step = active_click_walk
                            .as_mut()
                            .and_then(|task| task.queued_steps.pop_front());
                        let Some(direction) = next_step else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                "scheduler=click-walk-complete queued-steps=0",
                            );
                            active_click_walk = None;
                            continue;
                        };
                        if move_native_map_player(
                            stream,
                            &config.client_profile,
                            &snapshot,
                            &database,
                            shared_world,
                            character.id,
                            world_map,
                            &mut player_position,
                            &mut facing,
                            direction,
                        )? {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=moved position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                            observed_visibility_epoch = shared_world.visibility_epoch();
                            if let Some(task) = active_click_walk.as_mut() {
                                task.next_step_deadline = Instant::now()
                                    + native_autowalk_step_delay(
                                        snapshot.player_speed,
                                        snapshot.server_beat,
                                    );
                            }
                        } else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=blocked position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                            active_click_walk = None;
                        }
                        continue;
                    }
                    write_frame(
                        stream,
                        &encode_native_otclient_game_ping(&config.client_profile)
                            .map_err(HostError::Protocol)?,
                    )?;
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "outbound=ping opcode=0x1e",
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            let opcode = request.0.first().copied().unwrap_or_default();
            if config.extended_diagnostics {
                eprintln!(
                    "> Native OTCv8 frame peer={peer} opcode=0x{opcode:02x} len={}",
                    request.0.len()
                );
            }
            let decoded = decode_native_otclient_game_action(&request, &config.client_profile)
                .map_err(HostError::Protocol)?;
            native_diagnostic(
                config.extended_diagnostics,
                peer,
                &native_action_diagnostic_summary(&decoded),
            );
            decoded
        };
        match action {
            NativeOtClientGameAction::Ping => write_frame(
                stream,
                &encode_native_otclient_game_ping_back(&config.client_profile)
                    .map_err(HostError::Protocol)?,
            )?,
            NativeOtClientGameAction::PingBack | NativeOtClientGameAction::EnterGame => {}
            NativeOtClientGameAction::ChangeFightModes(request) => {
                let mode = match request.mode {
                    NativeOtClientFightMode::Attack => PlayerFightMode::Attack,
                    NativeOtClientFightMode::Balanced => PlayerFightMode::Balanced,
                    NativeOtClientFightMode::Defense => PlayerFightMode::Defense,
                };
                let changed = shared_world.replace_player_fight_mode_state(
                    character.id,
                    PlayerFightModeState {
                        mode,
                        chase: request.chase,
                        secure: request.secure,
                    },
                )?;
                if changed {
                    let player_modes =
                        encode_native_otclient_player_modes(&config.client_profile, request)
                            .map_err(HostError::Protocol)?;
                    write_frame(stream, &player_modes)?;
                }
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=change-fight-modes outcome=applied changed={changed} delivery={}",
                        if changed { "player-modes" } else { "unchanged" }
                    ),
                );
            }
            NativeOtClientGameAction::CloseContainer(container_id) => {
                closed_container_ids.insert(container_id);
                let close =
                    encode_native_otclient_close_container(&config.client_profile, container_id)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &close)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=close-container outcome=session-view-closed container-id={container_id}"
                    ),
                );
            }
            NativeOtClientGameAction::UpArrowContainer(container_id) => {
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=up-arrow-container outcome=deferred-no-supported-parent container-id={container_id}"
                    ),
                );
            }
            NativeOtClientGameAction::UpdateContainer(container_id) => {
                let containers = shared_world.player_containers(character.id)?;
                let frame = containers
                    .container(container_id)
                    .map(|container| {
                        native_classic_container_frame(
                            &config.client_profile,
                            config.item_presentation_catalog.as_deref(),
                            container,
                        )
                    })
                    .transpose()
                    .map_err(HostError::Protocol)?
                    .flatten();
                let refreshed = frame.is_some();
                if let Some(frame) = frame {
                    closed_container_ids.remove(&container_id);
                    write_frame(stream, &frame)?;
                }
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "action=update-container outcome={} container-id={container_id}",
                        if refreshed {
                            "session-view-refreshed"
                        } else {
                            "deferred-unavailable-or-unmapped"
                        }
                    ),
                );
            }
            NativeOtClientGameAction::UseItem {
                position,
                client_thing_id,
                stack_position,
                index,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    position,
                    client_thing_id,
                    stack_position,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=use-item outcome=deferred-unmapped-or-ambiguous-client-thing-id client-thing-id={client_thing_id}"
                        ),
                    );
                    continue;
                };
                match shared_world.validate_player_item_use(world_map, intent) {
                    Ok(outcome) => {
                        if let Some(destination) = outcome.teleport_destination {
                            let teleported = activate_native_map_teleport_item(
                                stream,
                                &config.client_profile,
                                &snapshot,
                                &database,
                                shared_world,
                                character.id,
                                world_map,
                                &mut player_position,
                                facing,
                                destination,
                            )?;
                            if teleported {
                                active_click_walk = None;
                                observed_visibility_epoch = shared_world.visibility_epoch();
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=use-item outcome=teleported server-id={} destination={destination:?} index={index}",
                                        outcome.server_id,
                                    ),
                                );
                            } else {
                                native_diagnostic(
                                    config.extended_diagnostics,
                                    peer,
                                    &format!(
                                        "action=use-item outcome=deferred-teleport-destination-blocked server-id={} destination={destination:?} index={index}",
                                        outcome.server_id,
                                    ),
                                );
                            }
                        } else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "action=use-item outcome=validated server-id={} count={} action-id={:?} unique-id={:?} text={} charges={:?} index={index}",
                                    outcome.server_id,
                                    outcome.count,
                                    outcome.action_id,
                                    outcome.unique_id,
                                    outcome.has_text,
                                    outcome.charges,
                                ),
                            );
                        }
                    }
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item outcome=deferred-invalid-server-owned-map-item",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::UseItemEx {
                source_position,
                source_client_thing_id,
                source_stack_position,
                target_position,
                target_client_thing_id,
                target_stack_position,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-ex outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_ex_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
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
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-ex outcome=deferred-unmapped-or-ambiguous-client-thing-id",
                    );
                    continue;
                };
                match shared_world.validate_player_item_use_ex(world_map, intent) {
                    Ok(outcome) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=use-item-ex outcome=validated source-server-id={} source-count={} target-server-id={} target-count={}",
                            outcome.source.server_id,
                            outcome.source.count,
                            outcome.target.server_id,
                            outcome.target.count,
                        ),
                    ),
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-ex outcome=deferred-invalid-server-owned-map-item",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::UseItemOnCreature {
                source_position,
                source_client_thing_id,
                source_stack_position,
                target_creature_id,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-on-creature outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_creature_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    source_position,
                    source_client_thing_id,
                    source_stack_position,
                    target_creature_id,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-on-creature outcome=deferred-unmapped-or-ambiguous-client-thing-id",
                    );
                    continue;
                };
                match shared_world.validate_player_item_use_creature(world_map, intent) {
                    Ok(outcome) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=use-item-on-creature outcome=validated source-server-id={} source-count={} target={:?}",
                            outcome.source.server_id, outcome.source.count, outcome.target
                        ),
                    ),
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=use-item-on-creature outcome=deferred-invalid-server-owned-item-or-creature",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::RotateItem {
                position,
                client_thing_id,
                stack_position,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=rotate-item outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    position,
                    client_thing_id,
                    stack_position,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=rotate-item outcome=deferred-unmapped-or-ambiguous-client-thing-id",
                    );
                    continue;
                };
                match shared_world.validate_player_item_use(world_map, intent) {
                    Ok(outcome) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "action=rotate-item outcome=validated server-id={} count={}",
                            outcome.server_id, outcome.count
                        ),
                    ),
                    Err(HostError::Core(_)) => native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=rotate-item outcome=deferred-invalid-server-owned-map-item",
                    ),
                    Err(error) => return Err(error),
                }
            }
            NativeOtClientGameAction::RequestOutfit => {
                let outfit_window = encode_native_otclient_choose_outfit(
                    &config.client_profile,
                    player_outfit,
                    empty_world.outfit_first_look_type,
                    empty_world.outfit_last_look_type,
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &outfit_window)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=choose-outfit opcode=0xc8 bytes={} look-type={}",
                        outfit_window.0.len(),
                        player_outfit.look_type
                    ),
                );
            }
            NativeOtClientGameAction::RequestQuestLog => {
                let quest_log = encode_native_otclient_empty_quest_log(&config.client_profile)
                    .map_err(HostError::Protocol)?;
                write_frame(stream, &quest_log)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=quest-log-empty opcode=0xf0 bytes={}",
                        quest_log.0.len()
                    ),
                );
            }
            NativeOtClientGameAction::ChangeOutfit(requested_outfit) => {
                let accepted = native_classic_outfit_is_allowed(
                    requested_outfit,
                    empty_world.outfit_first_look_type,
                    empty_world.outfit_last_look_type,
                );
                if accepted {
                    database.update_player_outfit(
                        character.id,
                        PlayerOutfit {
                            look_type: requested_outfit.look_type,
                            head: requested_outfit.head,
                            body: requested_outfit.body,
                            legs: requested_outfit.legs,
                            feet: requested_outfit.feet,
                        },
                    )?;
                    player_outfit = requested_outfit;
                }
                let applied_outfit = encode_native_otclient_creature_outfit(
                    &config.client_profile,
                    snapshot.player_id,
                    player_outfit,
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &applied_outfit)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=creature-outfit opcode=0x8e bytes={} accepted={} look-type={}",
                        applied_outfit.0.len(),
                        accepted,
                        player_outfit.look_type
                    ),
                );
            }
            NativeOtClientGameAction::LookMap {
                position,
                thing_id,
                stack_position,
            } => {
                let Some(world_map) = config.world_map.as_deref() else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=look-map outcome=deferred-no-world-map",
                    );
                    continue;
                };
                let Some(intent) = native_map_item_use_intent(
                    config.item_presentation_catalog.as_deref(),
                    character.id,
                    position,
                    thing_id,
                    stack_position,
                ) else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=look-map outcome=deferred-unmapped-or-ambiguous-client-thing-id",
                    );
                    continue;
                };
                let item = match shared_world.validate_player_item_use(world_map, intent) {
                    Ok(item) => item,
                    Err(HostError::Core(_)) => {
                        native_diagnostic(
                            config.extended_diagnostics,
                            peer,
                            "action=look-map outcome=deferred-invalid-server-owned-map-item",
                        );
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let message = format!("You see item #{} (count: {}).", item.server_id, item.count);
                let response =
                    encode_native_otclient_status_message(&config.client_profile, &message)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &response)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=status-message opcode=0xb4 bytes={} action=look-map server-id={} count={}",
                        response.0.len(), item.server_id, item.count
                    ),
                );
            }
            NativeOtClientGameAction::LookCreature { creature_id } => {
                let Some(message) =
                    native_creature_inspection_message(shared_world, character.id, creature_id)?
                else {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "action=look-creature outcome=deferred-unavailable-or-outside-viewport",
                    );
                    continue;
                };
                let response =
                    encode_native_otclient_status_message(&config.client_profile, &message)
                        .map_err(HostError::Protocol)?;
                write_frame(stream, &response)?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=status-message opcode=0xb4 bytes={} action=look-creature native-id={creature_id}",
                        response.0.len()
                    ),
                );
            }
            NativeOtClientGameAction::IgnoredInteraction(opcode) => {
                if config.extended_diagnostics {
                    eprintln!("> Native OTCv8 compatibility action ignored opcode=0x{opcode:02x}");
                }
            }
            NativeOtClientGameAction::SelectTarget(native_selected_id) => {
                apply_native_player_interaction(
                    shared_world,
                    character.id,
                    native_selected_id,
                    NativePlayerInteractionKind::Target,
                    config.extended_diagnostics,
                )?;
            }
            NativeOtClientGameAction::SelectFollow(native_selected_id) => {
                apply_native_player_interaction(
                    shared_world,
                    character.id,
                    native_selected_id,
                    NativePlayerInteractionKind::Follow,
                    config.extended_diagnostics,
                )?;
            }
            NativeOtClientGameAction::Talk(message) => {
                let recipient_count = shared_world.broadcast_public_chat(character.id, &message)?;
                if config.extended_diagnostics {
                    eprintln!(
                        "> Native OTCv8 public chat received bytes={} recipients={recipient_count}",
                        message.len()
                    );
                }
                drain_shared_public_chat(
                    stream,
                    &config.client_profile,
                    &chat_events,
                    config.extended_diagnostics,
                    peer,
                )?;
            }
            NativeOtClientGameAction::LeaveGame => break,
            NativeOtClientGameAction::Stop => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "scheduler=click-walk-cancel reason=stop active={cancelled_click_walk}"
                    ),
                );
                write_frame(
                    stream,
                    &encode_native_otclient_game_cancel_walk_facing(
                        &config.client_profile,
                        facing.protocol_direction(),
                    )
                    .map_err(HostError::Protocol)?,
                )?;
            }
            NativeOtClientGameAction::Turn(direction) => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "scheduler=click-walk-cancel reason=turn active={cancelled_click_walk} direction={direction:?}"
                    ),
                );
                facing = direction;
                write_frame(
                    stream,
                    &encode_native_otclient_game_cancel_walk_facing(
                        &config.client_profile,
                        facing.protocol_direction(),
                    )
                    .map_err(HostError::Protocol)?,
                )?;
            }
            NativeOtClientGameAction::AutoWalk(path) => {
                if let Some(task) = active_click_walk.as_mut() {
                    let previous_steps = task.queued_steps.len();
                    let replacement_steps = native_click_walk_steps(path.clone()).len();
                    task.replace_path(path);
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "scheduler=click-walk-replace previous-steps={previous_steps} queued-steps={replacement_steps}"
                        ),
                    );
                } else {
                    let step_delay =
                        native_autowalk_step_delay(snapshot.player_speed, snapshot.server_beat);
                    let mut task =
                        NativeActiveClickWalk::from_path(path, Instant::now() + step_delay);
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        &format!(
                            "scheduler=click-walk-create queued-steps={} step-delay-ms={}",
                            task.queued_steps.len(),
                            step_delay.as_millis()
                        ),
                    );
                    if task.queued_steps.is_empty() {
                        continue;
                    }
                    if task.queued_steps.len() == 1 {
                        let direction = task
                            .queued_steps
                            .pop_front()
                            .expect("single queued click-walk step");
                        if move_native_map_player(
                            stream,
                            &config.client_profile,
                            &snapshot,
                            &database,
                            shared_world,
                            character.id,
                            world_map,
                            &mut player_position,
                            &mut facing,
                            direction,
                        )? {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=moved position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                            observed_visibility_epoch = shared_world.visibility_epoch();
                            active_click_walk = Some(task);
                        } else {
                            native_diagnostic(
                                config.extended_diagnostics,
                                peer,
                                &format!(
                                    "scheduler=click-walk-step direction={direction:?} outcome=blocked position={},{},{}",
                                    player_position.x, player_position.y, player_position.z
                                ),
                            );
                        }
                    } else {
                        active_click_walk = Some(task);
                    }
                }
            }
            NativeOtClientGameAction::CardinalMove(direction) => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                let moved = move_native_map_player(
                    stream,
                    &config.client_profile,
                    &snapshot,
                    &database,
                    shared_world,
                    character.id,
                    world_map,
                    &mut player_position,
                    &mut facing,
                    direction,
                )?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "movement=cardinal direction={direction:?} outcome={} position={},{},{} map-update={}",
                        if moved { "moved" } else { "blocked" },
                        player_position.x,
                        player_position.y,
                        player_position.z,
                        if moved { "step" } else { "cancel-walk" }
                    ),
                );
                if cancelled_click_walk {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "scheduler=click-walk-cancel reason=manual-cardinal active=true",
                    );
                }
                if moved {
                    observed_visibility_epoch = shared_world.visibility_epoch();
                }
            }
            NativeOtClientGameAction::DiagonalMove(direction) => {
                let cancelled_click_walk = active_click_walk.take().is_some();
                let moved = move_native_map_player_diagonal(
                    stream,
                    &config.client_profile,
                    &snapshot,
                    &database,
                    shared_world,
                    character.id,
                    world_map,
                    &mut player_position,
                    &mut facing,
                    direction,
                )?;
                native_diagnostic(
                    config.extended_diagnostics,
                    peer,
                    &format!(
                        "movement=diagonal direction={direction:?} outcome={} position={},{},{} map-update={}",
                        if moved { "moved" } else { "blocked" },
                        player_position.x,
                        player_position.y,
                        player_position.z,
                        if moved { "double-step" } else { "cancel-walk" }
                    ),
                );
                if cancelled_click_walk {
                    native_diagnostic(
                        config.extended_diagnostics,
                        peer,
                        "scheduler=click-walk-cancel reason=manual-diagonal active=true",
                    );
                }
                if moved {
                    observed_visibility_epoch = shared_world.visibility_epoch();
                }
            }
        }
    }
    record_event(
        database_path,
        "info",
        &format!(
            "native map session completed peer={peer} account={} character={} protocol={}",
            account.id, request.character_name, request.protocol_version
        ),
    );
    Ok(())
}

fn native_static_creature_health_frames(
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

fn encode_shared_native_world_viewport(
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

fn drain_shared_public_chat(
    _stream: &mut TcpStream,
    _profile: &NativeOtClientProfile,
    events: &mpsc::Receiver<SharedPublicChatEvent>,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    loop {
        match events.try_recv() {
            Ok(event) => {
                native_diagnostic(
                    extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=public-chat-suppressed reason=740-no-message-mode-map text-bytes={}",
                        event.text.len()
                    ),
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// Applies one externally selected static-creature step and returns a full native map refresh.
/// It deliberately makes no AI decision, schedules no autonomous movement, and performs no
/// combat, Lua, spell, or action behavior.
pub fn move_native_static_creature_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world: &mut WorldState,
    world_map: &WorldMap,
    creature_id: u32,
    direction: CardinalDirection,
) -> Result<Frame, HostError> {
    world
        .move_static_creature_cardinal(creature_id, direction, world_map)
        .map_err(HostError::Core)?;
    let active_static_spawns = world.active_static_spawn_collection();
    encode_native_otclient_map_viewport_with_static_spawns(
        profile,
        snapshot,
        world_map,
        Some(&active_static_spawns),
    )
    .map_err(HostError::Protocol)
}

/// Applies a caller-triggered deterministic static creature policy and emits a native map refresh
/// only if that policy made at least one move. It does not create an autonomous scheduler.
pub fn apply_native_static_creature_policy_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world: &mut WorldState,
    world_map: &WorldMap,
    policy: StaticCreatureDecisionPolicy,
) -> Result<(StaticCreatureDecisionBatch, Option<Frame>), HostError> {
    let batch = world
        .apply_static_creature_policy(policy, world_map)
        .map_err(HostError::Core)?;
    if batch.decisions.is_empty() {
        return Ok((batch, None));
    }
    let active_static_spawns = world.active_static_spawn_collection();
    let frame = encode_native_otclient_map_viewport_with_static_spawns(
        profile,
        snapshot,
        world_map,
        Some(&active_static_spawns),
    )
    .map_err(HostError::Protocol)?;
    Ok((batch, Some(frame)))
}

/// Applies one explicitly requested target-directed creature step through the shared world and
/// refreshes the selected native session only after a real move. It creates no autonomous task,
/// protocol-specific target state, combat action, or pathfinding behavior.
pub fn step_shared_native_static_creature_toward_target_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    shared_world: &SharedNativeWorld,
    viewer_player_id: u64,
    world_map: &WorldMap,
    creature_id: u32,
) -> Result<(StaticCreatureTargetStepOutcome, Option<Frame>), HostError> {
    let outcome = shared_world.step_static_creature_toward_target(creature_id, world_map)?;
    if !matches!(outcome, StaticCreatureTargetStepOutcome::Moved { .. }) {
        return Ok((outcome, None));
    }
    let frame = encode_shared_native_world_viewport(
        profile,
        snapshot,
        world_map,
        shared_world,
        viewer_player_id,
    )?;
    Ok((outcome, Some(frame)))
}

/// Reactivates inactive imported static entities at their validated spawn positions and emits a
/// native map refresh only when the active entity set changed. This is caller-triggered and adds
/// no timed respawn scheduler, AI, combat, drops, corpse, Lua, or action behavior.
pub fn reset_native_static_creatures_and_refresh(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world: &mut WorldState,
    world_map: &WorldMap,
) -> Result<(StaticCreatureResetSummary, Option<Frame>), HostError> {
    let summary = world.reset_static_creatures();
    if summary.reactivated == 0 {
        return Ok((summary, None));
    }
    let active_static_spawns = world.active_static_spawn_collection();
    let frame = encode_native_otclient_map_viewport_with_static_spawns(
        profile,
        snapshot,
        world_map,
        Some(&active_static_spawns),
    )
    .map_err(HostError::Protocol)?;
    Ok((summary, Some(frame)))
}

/// Persists the post-heartbeat authoritative condition set. This must run even when a condition
/// has not damaged the player yet, because its elapsed interval remainder is part of deterministic
/// restart behavior; an empty set also removes schedules that expired during the heartbeat.
fn persist_runtime_player_conditions(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_id: u64,
) -> Result<(), HostError> {
    let conditions = shared_world.player_conditions(player_id)?;
    database
        .replace_player_conditions(player_id, &conditions)
        .map_err(HostError::Persistence)
}

/// Persists only the authoritative players actually changed by one static-target attack pass.
/// The caller provides a `BTreeSet`, making write order deterministic. Client combat effects,
/// death packets, loot, corpses, formulas, scripts, and general creature AI remain separate.
fn persist_static_target_attack_vitals(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_ids: &BTreeSet<u64>,
    death_loss_policy: DeathLossPolicy,
    progression_rules: Option<&BTreeMap<VocationId, PlayerProgressionRules>>,
) -> Result<(), HostError> {
    for &player_id in player_ids {
        let loss_persisted = if shared_world.player_respawn_state(player_id)?.dead {
            apply_configured_native_death_loss(
                database,
                shared_world,
                player_id,
                death_loss_policy,
                progression_rules,
            )?
        } else {
            false
        };
        if loss_persisted {
            continue;
        }
        let vitals = shared_world.player_vitals(player_id)?;
        let persisted_vitals = PersistedPlayerVitals {
            health: vitals.health,
            max_health: vitals.max_health,
            mana: vitals.mana,
            max_mana: vitals.max_mana,
            capacity: vitals.capacity,
            magic_level: vitals.magic_level,
        };
        let respawn_state = shared_world.player_respawn_state(player_id)?;
        if respawn_state.dead {
            database
                .update_player_vitals_and_respawn_state(player_id, persisted_vitals, respawn_state)
                .map_err(HostError::Persistence)?;
        } else {
            database
                .update_player_vitals(player_id, persisted_vitals)
                .map_err(HostError::Persistence)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn move_native_map_player(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    database: &EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    world_map: &WorldMap,
    player_position: &mut Position,
    facing: &mut NativeOtClientCardinalDirection,
    direction: NativeOtClientCardinalDirection,
) -> Result<bool, HostError> {
    let moved = {
        let mut world = shared_world.lock()?;
        let source = world
            .player(character_id)
            .ok_or(forgotten_core::CoreError::UnknownPlayer(character_id))
            .map_err(HostError::Core)?
            .position;
        let destination = source
            .step(native_cardinal_direction(direction))
            .map_err(HostError::Core)?;
        if !world_map.is_walkable(destination)
            || world.is_static_creature_occupied(destination)
            || world.is_player_occupied(destination)
        {
            None
        } else {
            let (previous, destination) = world
                .move_player_cardinal(character_id, native_cardinal_direction(direction))
                .map_err(HostError::Core)?;
            let active_static_spawns = world.active_static_spawn_collection();
            Some((previous, destination, active_static_spawns))
        }
    };
    let Some((previous, destination, active_static_spawns)) = moved else {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    };
    shared_world.mark_visibility_changed();
    database.update_player_position(character_id, destination)?;
    *facing = direction;
    write_frame(
        stream,
        &encode_native_otclient_move_creature_at(
            profile,
            native_position(previous),
            1,
            native_position(destination),
        )
        .map_err(HostError::Protocol)?,
    )?;
    let mut refreshed_snapshot = snapshot.clone();
    refreshed_snapshot.player_position = native_position(destination);
    refreshed_snapshot.player_direction = facing.protocol_direction();
    let visible_players = shared_world.visible_players(
        character_id,
        snapshot.player_look_type,
        snapshot.player_speed,
    )?;
    write_frame(
        stream,
        &encode_native_otclient_map_step_with_static_spawns_and_players(
            profile,
            &refreshed_snapshot,
            world_map,
            Some(&active_static_spawns),
            Some(&visible_players),
            direction,
        )
        .map_err(HostError::Protocol)?,
    )?;
    *player_position = destination;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn activate_native_map_teleport_item(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    database: &EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    world_map: &WorldMap,
    player_position: &mut Position,
    facing: NativeOtClientCardinalDirection,
    destination: Position,
) -> Result<bool, HostError> {
    let teleported = {
        let mut world = shared_world.lock()?;
        if !world_map.is_walkable(destination)
            || world.is_static_creature_occupied(destination)
            || world.is_player_occupied(destination)
        {
            None
        } else {
            Some(
                world
                    .teleport_player(character_id, destination)
                    .map_err(HostError::Core)?,
            )
        }
    };
    let Some((_, destination)) = teleported else {
        return Ok(false);
    };

    shared_world.mark_visibility_changed();
    database.update_player_position(character_id, destination)?;
    let mut refreshed_snapshot = snapshot.clone();
    refreshed_snapshot.player_position = native_position(destination);
    refreshed_snapshot.player_direction = facing.protocol_direction();
    write_frame(
        stream,
        &encode_shared_native_world_viewport(
            profile,
            &refreshed_snapshot,
            world_map,
            shared_world,
            character_id,
        )?,
    )?;
    *player_position = destination;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn move_native_map_player_diagonal(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    database: &EngineDatabase,
    shared_world: &SharedNativeWorld,
    character_id: u64,
    world_map: &WorldMap,
    player_position: &mut Position,
    facing: &mut NativeOtClientCardinalDirection,
    direction: NativeOtClientAutoWalkDirection,
) -> Result<bool, HostError> {
    let steps = direction.cardinal_steps();
    debug_assert_eq!(steps.len(), 2);
    let moved = {
        let mut world = shared_world.lock()?;
        let source = world
            .player(character_id)
            .ok_or(forgotten_core::CoreError::UnknownPlayer(character_id))
            .map_err(HostError::Core)?
            .position;
        let intermediate = source
            .step(native_cardinal_direction(steps[0]))
            .map_err(HostError::Core)?;
        let destination = intermediate
            .step(native_cardinal_direction(steps[1]))
            .map_err(HostError::Core)?;
        let blocked = [intermediate, destination].into_iter().any(|position| {
            !world_map.is_walkable(position)
                || world.is_static_creature_occupied(position)
                || world.is_player_occupied(position)
        });
        if blocked {
            None
        } else {
            world
                .move_player(character_id, destination)
                .map_err(HostError::Core)?;
            let active_static_spawns = world.active_static_spawn_collection();
            Some((source, intermediate, destination, active_static_spawns))
        }
    };
    let Some((previous, intermediate, destination, active_static_spawns)) = moved else {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    };
    shared_world.mark_visibility_changed();
    database.update_player_position(character_id, destination)?;
    *facing = steps[1];
    write_frame(
        stream,
        &encode_native_otclient_move_creature_at(
            profile,
            native_position(previous),
            1,
            native_position(destination),
        )
        .map_err(HostError::Protocol)?,
    )?;
    let visible_players = shared_world.visible_players(
        character_id,
        snapshot.player_look_type,
        snapshot.player_speed,
    )?;
    for (step, position) in [(steps[0], intermediate), (steps[1], destination)] {
        let mut refreshed_snapshot = snapshot.clone();
        refreshed_snapshot.player_position = native_position(position);
        refreshed_snapshot.player_direction = step.protocol_direction();
        write_frame(
            stream,
            &encode_native_otclient_map_step_with_static_spawns_and_players(
                profile,
                &refreshed_snapshot,
                world_map,
                Some(&active_static_spawns),
                Some(&visible_players),
                step,
            )
            .map_err(HostError::Protocol)?,
        )?;
    }
    *player_position = destination;
    Ok(true)
}

fn native_autowalk_step_delay(player_speed: u16, server_beat: u16) -> Duration {
    let speed = u64::from(player_speed).max(1);
    let server_beat = u64::from(server_beat).max(1);
    let interval_millis = (1000 * NATIVE_OTCLIENT_DEFAULT_GROUND_SPEED / speed)
        .max(server_beat)
        .min(NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY.as_millis() as u64);
    Duration::from_millis(interval_millis)
}

fn native_player_id(character_id: u64) -> Result<u32, HostError> {
    let character_id = u32::try_from(character_id).map_err(|_| {
        HostError::InvalidConfiguration("character ID exceeds the native player-ID range".into())
    })?;
    let player_id = NATIVE_OTCLIENT_PLAYER_ID_START
        .checked_add(character_id)
        .ok_or_else(|| {
            HostError::InvalidConfiguration(
                "character ID exceeds the native player-ID range".into(),
            )
        })?;
    if player_id >= NATIVE_OTCLIENT_PLAYER_ID_END {
        return Err(HostError::InvalidConfiguration(
            "character ID exceeds the native player-ID range".into(),
        ));
    }
    Ok(player_id)
}

#[derive(Debug, Clone, Copy)]
enum NativePlayerInteractionKind {
    Target,
    Follow,
}

/// A single server-owned native click-walk task. Client paths may replace its queued directions,
/// but never its next-step deadline. This mirrors the classic one-active-event behavior without
/// importing implementation code from another server.
struct NativeActiveClickWalk {
    queued_steps: VecDeque<NativeOtClientCardinalDirection>,
    next_step_deadline: Instant,
}

impl NativeActiveClickWalk {
    fn from_path(path: Vec<NativeOtClientAutoWalkDirection>, next_step_deadline: Instant) -> Self {
        Self {
            queued_steps: native_click_walk_steps(path),
            next_step_deadline,
        }
    }

    fn replace_path(&mut self, path: Vec<NativeOtClientAutoWalkDirection>) {
        self.queued_steps = native_click_walk_steps(path);
    }
}

fn native_click_walk_steps(
    path: Vec<NativeOtClientAutoWalkDirection>,
) -> VecDeque<NativeOtClientCardinalDirection> {
    path.into_iter()
        .flat_map(|direction| direction.cardinal_steps().iter().copied())
        .collect()
}

/// The selected 740 map encoder renders an 18×14 same-floor viewport centered at offset (8, 6),
/// which yields horizontal offsets -8 through +9 and vertical offsets -6 through +7. Creature
/// inspection is intentionally no broader than that already encoded viewport.
fn native_classic_viewport_contains(observer: Position, target: Position) -> bool {
    if observer.z != target.z {
        return false;
    }
    let horizontal_offset = i32::from(target.x) - i32::from(observer.x);
    let vertical_offset = i32::from(target.y) - i32::from(observer.y);
    (-8..=9).contains(&horizontal_offset) && (-6..=7).contains(&vertical_offset)
}

/// Resolves one current native creature ID into a bounded status sentence only when the requested
/// entity is active and already inside the observer's parser-verified classic map viewport. It
/// does not expose off-screen, inactive, absent, or cross-floor state and changes no target,
/// combat, visibility, persistence, or packet state by itself.
fn native_creature_inspection_message(
    shared_world: &SharedNativeWorld,
    observer_id: u64,
    native_creature_id: u32,
) -> Result<Option<String>, HostError> {
    let world = shared_world.lock()?;
    let observer = world.player(observer_id).ok_or(HostError::Core(
        forgotten_core::CoreError::UnknownPlayer(observer_id),
    ))?;
    let message = if let Some(player_id) = native_player_id_to_character_id(native_creature_id) {
        let Some(target) = world.player(player_id) else {
            return Ok(None);
        };
        native_classic_viewport_contains(observer.position, target.position)
            .then(|| format!("You see {}.", target.name))
    } else {
        let Some(lifecycle) = world.static_creature_lifecycle(native_creature_id) else {
            return Ok(None);
        };
        if !lifecycle.active
            || !native_classic_viewport_contains(observer.position, lifecycle.position)
        {
            return Ok(None);
        }
        world
            .static_creature(native_creature_id)
            .map(|creature| format!("You see {}.", creature.name))
    };
    Ok(message.filter(|message| message.len() <= NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES))
}

fn apply_native_player_interaction(
    shared_world: &SharedNativeWorld,
    source_player_id: u64,
    native_selected_id: u32,
    kind: NativePlayerInteractionKind,
    extended_diagnostics: bool,
) -> Result<(), HostError> {
    if native_selected_id == 0 {
        let result = match kind {
            NativePlayerInteractionKind::Target => {
                shared_world.set_player_target(source_player_id, None)
            }
            NativePlayerInteractionKind::Follow => {
                shared_world.set_player_follow(source_player_id, None)
            }
        };
        return result.map(|_| ());
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
            Ok(_) => Ok(()),
            Err(HostError::Core(forgotten_core::CoreError::UnknownPlayer(_)))
            | Err(HostError::Core(forgotten_core::CoreError::SelfInteractionNotAllowed(_))) => {
                if extended_diagnostics {
                    eprintln!(
                        "> Native OTCv8 {:?} selection ignored native-id={native_selected_id}",
                        kind
                    );
                }
                Ok(())
            }
            Err(error) => Err(error),
        };
    }
    if matches!(kind, NativePlayerInteractionKind::Target) {
        return match shared_world
            .set_player_static_target(source_player_id, Some(native_selected_id))
        {
            Ok(_) => Ok(()),
            Err(HostError::Core(
                forgotten_core::CoreError::UnknownStaticCreature(_)
                | forgotten_core::CoreError::InactiveStaticCreature(_),
            )) => {
                if extended_diagnostics {
                    eprintln!(
                        "> Native OTCv8 static target selection ignored native-id={native_selected_id}"
                    );
                }
                Ok(())
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
        Ok(())
    }
}

struct NativeSelectedPlayerMeleePolicy<'a> {
    progression_rules: Option<&'a BTreeMap<VocationId, PlayerProgressionRules>>,
    skill_rate: u32,
    death_loss_policy: DeathLossPolicy,
    declarative_weapon_catalog: Option<&'a DeclarativeWeaponCatalog>,
}

fn apply_native_selected_player_melee(
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
        shared_world.apply_player_melee_damage_with_death(
            attacker_id,
            target_id,
            NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE,
            world_map,
        )
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
    if let Some(rules_by_vocation) = policy.progression_rules {
        let vocation = shared_world.player_progression(attacker_id)?.vocation;
        if let Some(rules) = rules_by_vocation.get(&vocation).copied() {
            let awarded_tries = u64::from(policy.skill_rate);
            shared_world.apply_player_skill_tries(
                attacker_id,
                PlayerSkill::Fist,
                awarded_tries,
                rules,
            )?;
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

/// Applies one accepted explicit fixed-percent loss and commits its complete authoritative result.
/// The caller invokes this only after the existing combat path has entered a validated death state.
/// Default formulas, promotions, blessings, and client-facing lifecycle presentation remain out of
/// scope because the current FE data model does not yet represent their compatibility inputs.
fn apply_configured_native_death_loss(
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

fn apply_and_persist_native_fixed_death_loss(
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

fn apply_native_selected_static_creature_melee(
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
            | forgotten_core::CoreError::UnknownStaticCreature(_)
            | forgotten_core::CoreError::CombatCooldownActive { .. },
        )) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Applies an immutable raw monster reward only after the caller has confirmed an authoritative
/// selected-static defeat. Vocation-specific vital gains remain a separate data-wiring slice.
fn apply_and_persist_native_static_defeat_experience(
    database: &mut EngineDatabase,
    shared_world: &SharedNativeWorld,
    player_id: u64,
    creature_id: u32,
    policy: Option<&ExperienceAwardPolicy>,
    vocation_level_up_gains: Option<&BTreeMap<VocationId, VocationLevelUpGains>>,
) -> Result<Option<forgotten_core::PlayerExperienceAwardOutcome>, HostError> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    let raw_experience = shared_world.static_creature_experience_reward(creature_id)?;
    if raw_experience == 0 {
        return Ok(None);
    }
    let vocation = shared_world.player_progression(player_id)?.vocation;
    let gains = vocation_level_up_gains
        .and_then(|entries| entries.get(&vocation).copied())
        .unwrap_or_default();
    let outcome = shared_world.award_player_experience_with_vocation_gains(
        player_id,
        raw_experience,
        policy,
        gains,
    )?;
    if outcome.awarded_experience == 0 {
        return Ok(None);
    }
    let (player, vitals) = shared_world.player_and_vitals(player_id)?;
    if outcome.gained_levels > 0 {
        database.update_player_experience_and_vitals(
            player_id,
            player.level,
            player.experience,
            PersistedPlayerVitals {
                health: vitals.health,
                max_health: vitals.max_health,
                mana: vitals.mana,
                max_mana: vitals.max_mana,
                capacity: vitals.capacity,
                magic_level: vitals.magic_level,
            },
        )?;
    } else {
        database.update_player_experience(player_id, player.level, player.experience)?;
    }
    Ok(Some(outcome))
}

fn native_player_id_to_character_id(native_id: u32) -> Option<u64> {
    (NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END)
        .contains(&native_id)
        .then(|| u64::from(native_id - NATIVE_OTCLIENT_PLAYER_ID_START))
}

fn native_position(position: Position) -> NativeOtClientPosition {
    NativeOtClientPosition {
        x: position.x,
        y: position.y,
        z: position.z,
    }
}

/// Rehydrates the authoritative fields emitted by the existing profile-gated classic player-stats
/// record. This is deliberately pure: packet framing, client capability decisions, and all
/// parser-layout assumptions stay in the protocol crate.
fn refresh_native_player_stats_snapshot(
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

fn native_cardinal_direction(direction: NativeOtClientCardinalDirection) -> CardinalDirection {
    match direction {
        NativeOtClientCardinalDirection::North => CardinalDirection::North,
        NativeOtClientCardinalDirection::East => CardinalDirection::East,
        NativeOtClientCardinalDirection::South => CardinalDirection::South,
        NativeOtClientCardinalDirection::West => CardinalDirection::West,
    }
}

fn handle_game_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &GameSessionHostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let challenge = generate_legacy_74_game_challenge();
    write_frame(stream, &encode_legacy_74_game_challenge(challenge))?;
    let envelope = decode_legacy_74_game_session_envelope(&read_frame(stream)?)
        .map_err(HostError::Protocol)?;
    let plaintext = config
        .rsa_private_key
        .decrypt_raw_block(&envelope.encrypted_block)
        .map_err(HostError::Protocol)?;
    let bootstrap = decode_legacy_74_game_session_bootstrap_plaintext(
        envelope.client_version,
        &plaintext,
        challenge,
    )
    .map_err(HostError::Protocol)?;
    let database = EngineDatabase::open(database_path)?;
    let Some(account) = database
        .authenticate_account(&bootstrap.request.account_name, &bootstrap.request.password)?
    else {
        return send_game_session_error(
            stream,
            bootstrap.xtea_key,
            "Account name or password is not correct.",
        );
    };
    let Some(character) = account
        .characters
        .iter()
        .find(|character| character.name == bootstrap.request.character_name)
    else {
        return send_game_session_error(
            stream,
            bootstrap.xtea_key,
            "Character is not available on this account.",
        );
    };
    let authenticated = Legacy74GameSessionState::Authenticated {
        account_id: account.id,
        character_name: bootstrap.request.character_name.clone(),
    };
    database.record_event(
        "info",
        &format!("game session state peer={peer} state={authenticated:?}"),
    )?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_legacy_74_game_session_ready(&bootstrap.request.character_name),
    )?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_fe_otclient_capability_offer(&config.advertised_endpoint),
    )?;
    let acknowledgement = read_frame(stream)?;
    let acknowledgement =
        forgotten_protocol::xtea_decrypt_packet(&acknowledgement.0, bootstrap.xtea_key)
            .map_err(HostError::Protocol)?;
    if let Err(error) = decode_fe_otclient_capability_ack(&Frame(acknowledgement)) {
        let _ = send_game_session_error(
            stream,
            bootstrap.xtea_key,
            "A compatible FE OTClient module must acknowledge fe.otclient.v1.",
        );
        return Err(HostError::Protocol(error));
    }
    let custom_client = Legacy74GameSessionState::CustomClientNegotiated {
        character_name: bootstrap.request.character_name.clone(),
    };
    database.record_event(
        "info",
        &format!("game session state peer={peer} state={custom_client:?}"),
    )?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_fe_otclient_initial_world(&InitialWorldSnapshot {
            character_name: bootstrap.request.character_name.clone(),
            start_x: character.position.x,
            start_y: character.position.y,
            start_z: character.position.z,
            endpoint: config.advertised_endpoint.clone(),
        }),
    )?;
    let mut world = WorldState::default();
    world
        .add_player(Player {
            id: character.id,
            account_id: account.id as u64,
            name: character.name.clone(),
            position: character.position,
            level: character.level,
            experience: 0,
            skill_points: 0,
        })
        .map_err(HostError::Core)?;
    let manifest = EmptyWorldManifest::default();
    let viewport = world
        .empty_world_viewport(character.id, manifest.clone())
        .map_err(HostError::Core)?;
    write_game_session_response(
        stream,
        bootstrap.xtea_key,
        &encode_fe_otclient_empty_viewport(&viewport),
    )?;
    for _ in 0..MAX_EMPTY_WORLD_MOVES_PER_SESSION {
        let request = match read_frame(stream) {
            Ok(request) => request,
            Err(HostError::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        };
        let request = forgotten_protocol::xtea_decrypt_packet(&request.0, bootstrap.xtea_key)
            .map_err(HostError::Protocol)?;
        let direction =
            decode_fe_otclient_move_request(&Frame(request)).map_err(HostError::Protocol)?;
        let (from, to) = match world.move_player_cardinal(character.id, direction) {
            Ok(movement) => movement,
            Err(error) => {
                send_game_session_error(
                    stream,
                    bootstrap.xtea_key,
                    "Movement rejected by empty-world bounds.",
                )?;
                return Err(HostError::Core(error));
            }
        };
        let tick = world.advance_tick();
        database.update_player_position(character.id, to)?;
        write_game_session_response(
            stream,
            bootstrap.xtea_key,
            &encode_fe_otclient_movement_ack(&EmptyWorldMovementAck { tick, from, to }),
        )?;
        write_game_session_response(
            stream,
            bootstrap.xtea_key,
            &encode_fe_otclient_world_tick(tick),
        )?;
        let viewport = world
            .empty_world_viewport(character.id, manifest.clone())
            .map_err(HostError::Core)?;
        write_game_session_response(
            stream,
            bootstrap.xtea_key,
            &encode_fe_otclient_empty_viewport(&viewport),
        )?;
    }
    let feature_gate = Legacy74GameSessionState::FeatureGated {
        character_name: bootstrap.request.character_name,
    };
    database.record_event(
        "info",
        &format!("game session state peer={peer} state={feature_gate:?}"),
    )?;
    Ok(())
}

fn send_game_session_error(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    message: &str,
) -> Result<(), HostError> {
    write_game_session_response(stream, key, &encode_legacy_74_game_session_error(message))
}

fn write_game_session_response(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    response: &Frame,
) -> Result<(), HostError> {
    let encrypted = xtea_encrypt_packet(&response.0, key).map_err(HostError::Protocol)?;
    write_frame(stream, &Frame(encrypted))
}

fn handle_status_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &StatusHostConfig,
    database_path: &Path,
    started_at: Instant,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;
    let request = decode_status_request(&read_frame(stream)?).map_err(HostError::Protocol)?;
    let snapshot = StatusSnapshot {
        server_name: config.server_name.clone(),
        bind_ip: config.bind_addr.ip(),
        status_port: config.bind_addr.port(),
        uptime_seconds: started_at.elapsed().as_secs(),
        players_online: 0,
        max_players: config.max_players,
        players_peak: 0,
        map_name: config.map_name.clone(),
        profile: config.profile,
    };
    match request {
        StatusRequest::XmlInfo => {
            stream.write_all(&encode_status_xml(&snapshot))?;
            stream.flush()?;
        }
        StatusRequest::Binary { flags, .. } => {
            let response = encode_status_binary(&snapshot, flags, &[] as &[StatusPlayer], false);
            write_frame(stream, &response)?;
        }
    }
    record_event(
        database_path,
        "info",
        &format!("status query accepted peer={peer}"),
    );
    Ok(())
}

fn handle_session(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &HostConfig,
    database_path: &Path,
) -> Result<(), HostError> {
    stream.set_read_timeout(Some(config.session_timeout))?;
    stream.set_write_timeout(Some(config.session_timeout))?;

    let request = read_frame(stream)?;
    if decode_probe(&request).is_ok() {
        write_frame(stream, &probe_response(config.profile))?;
        record_event(
            database_path,
            "info",
            &format!("probe accepted peer={peer} profile={}", config.profile.id),
        );
        Ok(())
    } else if let Some(login) = &config.legacy_login {
        handle_legacy_login(stream, peer, config, login, database_path, &request)
    } else {
        let error = decode_probe(&request).expect_err("non-probe request must be rejected");
        let _ = write_frame(stream, &error_frame(error.code()));
        Err(error)
    }
}

fn handle_legacy_login(
    stream: &mut TcpStream,
    peer: SocketAddr,
    config: &HostConfig,
    login: &LegacyLoginConfig,
    database_path: &Path,
    request: &Frame,
) -> Result<(), HostError> {
    if config.profile.id != "fe-7.4" {
        return Err(HostError::LegacyLoginUnavailable);
    }
    let envelope = decode_legacy_74_envelope(request).map_err(HostError::Protocol)?;
    let plaintext = login
        .rsa_private_key
        .decrypt_raw_block(&envelope.encrypted_block)
        .map_err(HostError::Protocol)?;
    let request = decode_legacy_74_login_plaintext(envelope.client_version, &plaintext)
        .map_err(HostError::Protocol)?;
    if request.client_version != 740 {
        return send_legacy_login_error(
            stream,
            request.xtea_key,
            "Only clients with protocol 7.4 are allowed.",
        );
    }
    let database = EngineDatabase::open(database_path)?;
    let Some(account) = database.authenticate_account(&request.account_name, &request.password)?
    else {
        return send_legacy_login_error(
            stream,
            request.xtea_key,
            "Account name or password is not correct.",
        );
    };
    let entries = account
        .characters
        .iter()
        .map(|character| CharacterListEntry {
            name: character.name.clone(),
            world_name: login.server_name.clone(),
            address: config.bind_addr.ip(),
            port: config.bind_addr.port(),
        })
        .collect::<Vec<_>>();
    let response = encode_legacy_74_character_list(&login.message_of_the_day, &entries)
        .map_err(HostError::Protocol)?;
    write_legacy_login_response(stream, request.xtea_key, &response)?;
    database.record_event(
        "info",
        &format!(
            "legacy login foundation accepted peer={peer} account={}",
            account.id
        ),
    )?;
    Ok(())
}

fn send_legacy_login_error(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    message: &str,
) -> Result<(), HostError> {
    write_legacy_login_response(stream, key, &encode_login_error(message))
}

fn write_legacy_login_response(
    stream: &mut TcpStream,
    key: forgotten_protocol::XteaKey,
    response: &Frame,
) -> Result<(), HostError> {
    let encrypted = xtea_encrypt_packet(&response.0, key).map_err(HostError::Protocol)?;
    write_frame(stream, &Frame(encrypted))
}

pub fn probe_request() -> Frame {
    Frame([PROBE_MAGIC.as_slice(), &[PROBE_VERSION]].concat())
}

pub fn probe_response(profile: CompatibilityProfile) -> Frame {
    let mut payload = [PROBE_RESPONSE_MAGIC.as_slice(), &[PROBE_VERSION]].concat();
    payload.extend_from_slice(profile.id.as_bytes());
    Frame(payload)
}

pub fn error_frame(reason: &[u8]) -> Frame {
    let mut payload = PROBE_ERROR_MAGIC.to_vec();
    payload.extend_from_slice(reason);
    Frame(payload)
}

pub fn read_frame(stream: &mut TcpStream) -> Result<Frame, HostError> {
    let mut header = [0_u8; 2];
    stream.read_exact(&mut header)?;
    let declared = u16::from_le_bytes(header) as usize;
    if declared == 0 || declared > MAX_FRAME_SIZE {
        return Err(HostError::Protocol(ProtocolError::InvalidLength(declared)));
    }
    let mut encoded = Vec::with_capacity(declared + 2);
    encoded.extend_from_slice(&header);
    encoded.resize(declared + 2, 0);
    stream.read_exact(&mut encoded[2..])?;
    decode(&encoded).map_err(HostError::Protocol)
}

pub fn write_frame(stream: &mut TcpStream, frame: &Frame) -> Result<(), HostError> {
    let encoded = encode(frame).map_err(HostError::Protocol)?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    Ok(())
}

fn decode_probe(frame: &Frame) -> Result<(), HostError> {
    if frame.0.len() != PROBE_MAGIC.len() + 1 {
        return Err(HostError::InvalidProbe("unexpected probe length"));
    }
    if &frame.0[..4] != PROBE_MAGIC {
        return Err(HostError::InvalidProbe("unexpected probe magic"));
    }
    if frame.0[4] != PROBE_VERSION {
        return Err(HostError::InvalidProbe("unsupported probe version"));
    }
    Ok(())
}

fn record_event(database_path: &Path, level: &str, message: &str) {
    let _ = EngineDatabase::open(database_path)
        .and_then(|database| database.record_event(level, message));
}

#[derive(Debug)]
pub enum HostError {
    Core(forgotten_core::CoreError),
    Io(std::io::Error),
    Protocol(ProtocolError),
    Persistence(forgotten_persistence::PersistenceError),
    InvalidConfiguration(String),
    InvalidProbe(&'static str),
    SharedWorldUnavailable,
    LegacyLoginUnavailable,
    HostThreadPanicked,
}

impl HostError {
    fn code(&self) -> &'static [u8] {
        match self {
            Self::Core(_) => b"world-error",
            Self::InvalidProbe(_) => b"invalid-probe",
            Self::Protocol(_) => b"invalid-frame",
            Self::Persistence(_) => b"persistence-error",
            Self::Io(_) => b"io-error",
            Self::InvalidConfiguration(_) => b"invalid-config",
            Self::SharedWorldUnavailable => b"shared-world-unavailable",
            Self::LegacyLoginUnavailable => b"legacy-login-unavailable",
            Self::HostThreadPanicked => b"host-panic",
        }
    }
}

impl From<std::io::Error> for HostError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<forgotten_persistence::PersistenceError> for HostError {
    fn from(value: forgotten_persistence::PersistenceError) -> Self {
        Self::Persistence(value)
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid configuration: {message}")
            }
            Self::InvalidProbe(message) => write!(formatter, "invalid probe: {message}"),
            other => write!(formatter, "{other:?}"),
        }
    }
}

impl std::error::Error for HostError {}

#[cfg(test)]
mod tests {
    use super::*;
    use forgotten_config::{parse_declarative_spells_xml, parse_declarative_weapons_xml};
    use forgotten_core::{Player, Position, WorldMapTile};
    use forgotten_protocol::FE_7_4_PROFILE;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn database_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forgotten-engine-host-{name}-{nonce}.db"))
    }

    fn test_config() -> HostConfig {
        HostConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            profile: FE_7_4_PROFILE,
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
            legacy_login: None,
        }
    }

    fn status_config() -> StatusHostConfig {
        StatusHostConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            profile: FE_7_4_PROFILE,
            server_name: "Forgotten Engine Test".into(),
            map_name: "forgotten".into(),
            max_players: 100,
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
        }
    }

    fn game_session_config(key: Arc<LegacyRsaPrivateKey>) -> GameSessionHostConfig {
        GameSessionHostConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            profile: FE_7_4_PROFILE,
            rsa_private_key: key,
            advertised_endpoint: OtClientEndpoint {
                host: "fe.example.test".into(),
                port: 443,
            },
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
        }
    }

    fn native_otclient_config(bind_addr: SocketAddr) -> NativeOtClientHostConfig {
        NativeOtClientHostConfig {
            bind_addr,
            client_profile: NativeOtClientProfile {
                protocol_version: 740,
                numeric_account_ids: true,
                login_packet_encryption: false,
                protocol_checksum: false,
                challenge_on_login: false,
                max_padding_bytes: 128,
            },
            server_name: "Forgotten Engine Test".into(),
            advertised_game_addr: "127.0.0.1:7265".parse().unwrap(),
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
            extended_diagnostics: false,
            empty_world: None,
            world_map: None,
            item_presentation_catalog: None,
            static_spawns: None,
            static_target_attack_policy: StaticTargetAttackPolicy::Disabled,
            regeneration_rules: None,
            progression_rules: None,
            vocation_level_up_gains: None,
            skill_rate: 1,
            experience_award_policy: None,
            death_loss_policy: DeathLossPolicy::DefaultFormula,
            declarative_weapon_catalog: None,
            declarative_spell_catalog: None,
        }
    }

    fn native_world_map() -> Arc<WorldMap> {
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("native-test", spawn);
        for x in 80..=120 {
            for y in 80..=120 {
                map.set_tile(
                    Position { x, y, z: 7 },
                    WorldMapTile {
                        ground_thing_id: 102,
                        walkable: true,
                    },
                )
                .unwrap();
            }
        }
        map.set_town(forgotten_core::WorldMapTown {
            id: 1,
            name: "Native Temple".into(),
            temple_position: spawn,
        })
        .unwrap();
        map.validate().unwrap();
        Arc::new(map)
    }

    #[test]
    fn native_classic_item_records_require_validated_presentation_metadata() {
        let item = ItemInstance::new(4526, 25).unwrap();
        assert_eq!(native_classic_item_record(None, &item), None);

        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                4526,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: true,
                },
            )
            .unwrap();
        assert_eq!(
            native_classic_item_record(Some(&catalog), &item),
            Some(NativeOtClientClassicItemRecord {
                client_thing_id: 102,
                subtype: Some(25),
            })
        );
        catalog
            .insert(
                2463,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 2463,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();

        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            forgotten_core::EquipmentSlot::Armor,
            ItemInstance::new(2463, 1).unwrap(),
        );
        equipment.equip(forgotten_core::EquipmentSlot::RightHand, item);
        equipment.equip(
            forgotten_core::EquipmentSlot::LeftHand,
            ItemInstance::new(9999, 1).unwrap(),
        );
        let config = native_otclient_config("127.0.0.1:0".parse().unwrap());
        let frames =
            native_classic_equipment_frames(&config.client_profile, Some(&catalog), &equipment)
                .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].0, vec![0x78, 4, 159, 9]);
        assert_eq!(frames[1].0, vec![0x78, 5, 102, 0, 25]);

        let incompatible_profile = NativeOtClientProfile {
            protocol_version: 800,
            ..config.client_profile
        };
        assert!(
            native_classic_equipment_frames(&incompatible_profile, Some(&catalog), &equipment,)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn native_classic_equipment_deltas_are_mapped_ordered_and_delete_stale_slots() {
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                4526,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: true,
                },
            )
            .unwrap();
        catalog
            .insert(
                2463,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 2463,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let mut previous_equipment = PlayerEquipment::default();
        previous_equipment.equip(EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap());
        previous_equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 25).unwrap(),
        );
        let previous = native_classic_mapped_equipment(Some(&catalog), &previous_equipment);
        let config = native_otclient_config("127.0.0.1:0".parse().unwrap());
        assert!(native_classic_equipment_delta_frames(
            &config.client_profile,
            &previous,
            &previous,
        )
        .unwrap()
        .is_empty());

        let mut changed_equipment = previous_equipment.clone();
        changed_equipment.unequip(EquipmentSlot::Armor);
        changed_equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 20).unwrap(),
        );
        let changed = native_classic_mapped_equipment(Some(&catalog), &changed_equipment);
        let frames =
            native_classic_equipment_delta_frames(&config.client_profile, &previous, &changed)
                .unwrap();
        assert_eq!(frames[0].0, vec![0x79, EquipmentSlot::Armor.code()]);
        assert_eq!(
            frames[1].0,
            vec![0x78, EquipmentSlot::RightHand.code(), 102, 0, 20]
        );

        let mut unmapped_equipment = changed_equipment;
        unmapped_equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(9999, 1).unwrap(),
        );
        let unmapped = native_classic_mapped_equipment(Some(&catalog), &unmapped_equipment);
        let frames =
            native_classic_equipment_delta_frames(&config.client_profile, &changed, &unmapped)
                .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0, vec![0x79, EquipmentSlot::RightHand.code()]);

        let incompatible_profile = NativeOtClientProfile {
            protocol_version: 800,
            ..config.client_profile
        };
        assert!(
            native_classic_equipment_delta_frames(&incompatible_profile, &previous, &changed,)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn shared_native_equipment_epoch_advances_only_for_authoritative_changes() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 109,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 1,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 20).unwrap(),
        );
        assert_eq!(shared.equipment_epoch(), 0);
        assert!(shared
            .replace_player_equipment(109, equipment.clone())
            .unwrap());
        assert_eq!(shared.equipment_epoch(), 1);
        assert!(!shared.replace_player_equipment(109, equipment).unwrap());
        assert_eq!(shared.equipment_epoch(), 1);
    }

    #[test]
    fn shared_native_container_epoch_refreshes_only_complete_mapped_windows() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 110,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 1,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let mut container = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        container
            .items
            .insert(ItemInstance::new(4526, 3).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(container).unwrap();
        assert_eq!(shared.containers_epoch(), 0);
        assert!(shared
            .replace_player_containers(110, containers.clone())
            .unwrap());
        assert_eq!(shared.containers_epoch(), 1);
        assert!(!shared.replace_player_containers(110, containers).unwrap());
        assert_eq!(shared.containers_epoch(), 1);

        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id, subtype) in [(1988, 1988, false), (4526, 102, true)] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype: subtype,
                    },
                )
                .unwrap();
        }
        let config = native_otclient_config("127.0.0.1:0".parse().unwrap());
        assert_eq!(
            native_classic_container_frames(
                &config.client_profile,
                Some(&catalog),
                &shared.player_containers(110).unwrap(),
                &BTreeSet::new(),
            )
            .unwrap(),
            vec![Frame(vec![
                0x6e, 2, 196, 7, 8, 0, b'B', b'a', b'c', b'k', b'p', b'a', b'c', b'k', 20, 0, 1,
                102, 0, 3,
            ])]
        );
        let closed_container_ids = BTreeSet::from([2]);
        assert!(native_classic_container_frames(
            &config.client_profile,
            Some(&catalog),
            &shared.player_containers(110).unwrap(),
            &closed_container_ids,
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn shared_complete_item_transfers_advance_both_native_refresh_epochs() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 111,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 1,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let mut equipment = PlayerEquipment::default();
        let item = ItemInstance::new(4526, 3).unwrap();
        equipment.equip(EquipmentSlot::RightHand, item.clone());
        shared.replace_player_equipment(111, equipment).unwrap();
        let container = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(container).unwrap();
        shared.replace_player_containers(111, containers).unwrap();

        let equipment_epoch = shared.equipment_epoch();
        let containers_epoch = shared.containers_epoch();
        let into_container = shared
            .move_equipment_item_to_container(111, EquipmentSlot::RightHand, 2)
            .unwrap();
        assert_eq!(into_container.item, item);
        assert_eq!(shared.equipment_epoch(), equipment_epoch + 1);
        assert_eq!(shared.containers_epoch(), containers_epoch + 1);
        assert!(shared
            .player_equipment(111)
            .unwrap()
            .item(EquipmentSlot::RightHand)
            .is_none());
        let containers_after_move = shared.player_containers(111).unwrap();
        let container_after_move = containers_after_move
            .iter()
            .find_map(|(container_id, container)| (container_id == 2).then_some(container))
            .unwrap();
        assert_eq!(container_after_move.items.item(0), Some(&item));

        let back_to_equipment = shared
            .move_container_item_to_equipment(111, 2, 0, EquipmentSlot::LeftHand)
            .unwrap();
        assert_eq!(back_to_equipment.item, item);
        assert_eq!(shared.equipment_epoch(), equipment_epoch + 2);
        assert_eq!(shared.containers_epoch(), containers_epoch + 2);
        assert_eq!(
            shared
                .player_equipment(111)
                .unwrap()
                .item(EquipmentSlot::LeftHand),
            Some(&item)
        );
        let containers_after_return = shared.player_containers(111).unwrap();
        let container_after_return = &containers_after_return
            .iter()
            .find_map(|(container_id, container)| (container_id == 2).then_some(container))
            .unwrap()
            .items;
        assert!(container_after_return.is_empty());

        assert!(matches!(
            shared.move_equipment_item_to_container(111, EquipmentSlot::RightHand, 2),
            Err(HostError::Core(
                forgotten_core::CoreError::EmptyEquipmentSlot { .. }
            ))
        ));
        assert_eq!(shared.equipment_epoch(), equipment_epoch + 2);
        assert_eq!(shared.containers_epoch(), containers_epoch + 2);
    }

    #[test]
    fn shared_stack_transfers_advance_both_native_refresh_epochs_only_on_success() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 112,
                    account_id: 1,
                    name: "Paladin".into(),
                    position: map.spawn(),
                    level: 1,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(2148, 40).unwrap(),
        );
        shared.replace_player_equipment(112, equipment).unwrap();
        let mut container = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        container
            .items
            .insert(ItemInstance::new(2148, 10).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(container).unwrap();
        shared.replace_player_containers(112, containers).unwrap();

        let equipment_epoch = shared.equipment_epoch();
        let containers_epoch = shared.containers_epoch();
        let moved = shared
            .move_equipment_stack_to_container(112, EquipmentSlot::RightHand, 2, 15)
            .unwrap();
        assert_eq!(moved.source_remaining_count, Some(25));
        assert_eq!(moved.destination_count, 25);
        assert_eq!(shared.equipment_epoch(), equipment_epoch + 1);
        assert_eq!(shared.containers_epoch(), containers_epoch + 1);

        let merged = shared
            .move_container_stack_to_equipment(112, 2, 0, EquipmentSlot::RightHand, 20)
            .unwrap();
        assert_eq!(merged.source_remaining_count, Some(5));
        assert_eq!(merged.destination_count, 45);
        assert_eq!(shared.equipment_epoch(), equipment_epoch + 2);
        assert_eq!(shared.containers_epoch(), containers_epoch + 2);

        assert!(matches!(
            shared.move_container_stack_to_equipment(112, 2, 0, EquipmentSlot::RightHand, 0),
            Err(HostError::Core(
                forgotten_core::CoreError::InvalidItemTransferCount { .. }
            ))
        ));
        assert_eq!(shared.equipment_epoch(), equipment_epoch + 2);
        assert_eq!(shared.containers_epoch(), containers_epoch + 2);
    }

    #[test]
    fn native_classic_container_bootstrap_requires_top_level_mapped_content() {
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id, subtype) in [(1988, 1988, false), (4526, 102, true)] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype: subtype,
                    },
                )
                .unwrap();
        }
        let mut top_level = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        top_level
            .items
            .insert(ItemInstance::new(4526, 3).unwrap())
            .unwrap();
        let nested = forgotten_core::PlayerContainer::new(
            3,
            ItemInstance::new(1988, 1).unwrap(),
            "Nested",
            true,
            20,
        )
        .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(top_level).unwrap();
        containers.insert(nested).unwrap();

        let config = native_otclient_config("127.0.0.1:0".parse().unwrap());
        let frames = native_classic_container_frames(
            &config.client_profile,
            Some(&catalog),
            &containers,
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            frames,
            vec![Frame(vec![
                0x6e, 2, 196, 7, 8, 0, b'B', b'a', b'c', b'k', b'p', b'a', b'c', b'k', 20, 0, 1,
                102, 0, 3,
            ])]
        );

        let incompatible_profile = NativeOtClientProfile {
            protocol_version: 800,
            ..config.client_profile
        };
        assert!(native_classic_container_frames(
            &incompatible_profile,
            Some(&catalog),
            &containers,
            &BTreeSet::new(),
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn shared_native_world_synchronizes_concurrent_player_registration_and_cleanup() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let first_world = shared.clone();
        let first_map = Arc::clone(&map);
        let first = thread::spawn(move || {
            first_world
                .register_player_at_available_position(
                    Player {
                        id: 101,
                        account_id: 1,
                        name: "Knight".into(),
                        position: first_map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &first_map,
                )
                .unwrap()
        });
        let second_world = shared.clone();
        let second_map = Arc::clone(&map);
        let second = thread::spawn(move || {
            second_world
                .register_player_at_available_position(
                    Player {
                        id: 102,
                        account_id: 2,
                        name: "Druid".into(),
                        position: second_map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &second_map,
                )
                .unwrap()
        });
        let first_position = first.join().unwrap();
        let second_position = second.join().unwrap();
        assert_ne!(first_position, second_position);
        assert_eq!(shared.tick().unwrap(), 0);
        assert_eq!(shared.advance_ticks(0).unwrap(), 0);
        assert_eq!(shared.world_revision().unwrap(), 2);
        assert_eq!(shared.advance_ticks(3).unwrap(), 3);
        assert_eq!(shared.world_revision().unwrap(), 3);
        assert_eq!(shared.advance_tick().unwrap(), 4);
        assert_eq!(shared.tick().unwrap(), 4);
        shared.remove_player(101).unwrap();
        let recycled = shared
            .register_player_at_available_position(
                Player {
                    id: 103,
                    account_id: 3,
                    name: "Sorcerer".into(),
                    position: first_position,
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        assert_eq!(recycled, first_position);
        shared.remove_player(102).unwrap();
        shared.remove_player(103).unwrap();
    }

    #[test]
    fn shared_native_world_exposes_the_authoritative_revision_baseline() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        assert_eq!(shared.world_revision().unwrap(), 0);
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        assert_eq!(shared.world_revision().unwrap(), 1);
        shared.advance_tick().unwrap();
        assert_eq!(shared.world_revision().unwrap(), 2);
    }

    #[test]
    fn shared_native_registration_accepts_persisted_equipment_without_inventory_packets() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let sword = forgotten_core::ItemInstance::new(2376, 1).unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(forgotten_core::EquipmentSlot::RightHand, sword.clone());
        shared
            .register_player_at_available_position_with_vitals_and_equipment(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals::default(),
                equipment,
                &map,
            )
            .unwrap();
        assert_eq!(
            shared
                .player_equipment(101)
                .unwrap()
                .item(forgotten_core::EquipmentSlot::RightHand),
            Some(&sword)
        );
    }

    #[test]
    fn shared_native_registration_hydrates_persisted_containers_without_window_packets() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let mut container = forgotten_core::PlayerContainer::new(
            0,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            2,
        )
        .unwrap();
        let gold = ItemInstance::new(3031, 25).unwrap();
        container.items.insert(gold).unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(container.clone()).unwrap();
        shared
            .register_player_at_available_position_with_vitals_equipment_and_containers(
                Player {
                    id: 102,
                    account_id: 1,
                    name: "Druid".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals::default(),
                PlayerEquipment::default(),
                containers,
                &map,
            )
            .unwrap();
        assert_eq!(
            shared.player_containers(102).unwrap().container(0),
            Some(&container)
        );
    }

    #[test]
    fn shared_native_registration_hydrates_progression_and_tracks_change_epoch() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let mut skills = forgotten_core::PlayerSkills::default();
        skills.set(
            forgotten_core::PlayerSkill::Sword,
            forgotten_core::SkillProgress::new(65, 42).unwrap(),
        );
        let progression = PlayerProgression {
            vocation: forgotten_core::VocationId::new(4),
            skills,
        };
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_and_progression(
                Player {
                    id: 103,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals::default(),
                progression,
                PlayerEquipment::default(),
                PlayerContainers::default(),
                &map,
            )
            .unwrap();
        assert_eq!(shared.player_progression(103).unwrap(), progression);
        assert_eq!(shared.progression_epoch(), 0);
        assert!(!shared.replace_player_progression(103, progression).unwrap());
        assert_eq!(shared.progression_epoch(), 0);
        let mut changed = progression;
        changed.skills.set(
            forgotten_core::PlayerSkill::Shielding,
            forgotten_core::SkillProgress::new(61, 99).unwrap(),
        );
        assert!(shared.replace_player_progression(103, changed).unwrap());
        assert_eq!(shared.progression_epoch(), 1);
        assert_eq!(shared.player_progression(103).unwrap(), changed);
    }

    #[test]
    fn shared_native_experience_award_updates_refresh_epoch_only_on_gain() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 106,
                    account_id: 1,
                    name: "Paladin".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        let policy = ExperienceAwardPolicy::new(
            5,
            vec![forgotten_core::ExperienceAwardStage::new(1, 8, 2_000).unwrap()],
        )
        .unwrap();

        assert_eq!(shared.progression_epoch(), 0);
        let awarded = shared.award_player_experience(106, 100, &policy).unwrap();
        assert_eq!(awarded.awarded_experience, 1_000);
        assert_eq!(awarded.experience, 5_900);
        assert_eq!(shared.progression_epoch(), 1);

        let disabled = ExperienceAwardPolicy::new(0, Vec::new()).unwrap();
        let disabled_outcome = shared.award_player_experience(106, 100, &disabled).unwrap();
        assert_eq!(disabled_outcome.awarded_experience, 0);
        assert_eq!(shared.progression_epoch(), 1);
    }

    #[test]
    fn vocation_level_up_refreshes_the_native_stats_snapshot_from_authoritative_state() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let initial_vitals = PlayerVitals {
            health: 50,
            max_health: 100,
            mana: 20,
            max_mana: 50,
            capacity: 500,
            magic_level: 4,
        };
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 108,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 1,
                    experience: 0,
                    skill_points: 0,
                },
                initial_vitals,
                &map,
            )
            .unwrap();
        let outcome = shared
            .award_player_experience_with_vocation_gains(
                108,
                100,
                &ExperienceAwardPolicy::new(10, Vec::new()).unwrap(),
                VocationLevelUpGains::new(15, 5, 25),
            )
            .unwrap();
        assert_eq!(outcome.level, 5);
        assert_eq!(outcome.experience, 1_000);
        assert_eq!(outcome.gained_levels, 4);
        assert_eq!(shared.progression_epoch(), 1);
        assert_eq!(shared.vitals_epoch(), 1);

        let (player, vitals) = shared.player_and_vitals(108).unwrap();
        let mut snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(108).unwrap(),
            player_name: player.name.clone(),
            player_position: native_position(player.position),
            player_level: 1,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: forgotten_core::PlayerSkills::default(),
            ground_thing_id: 4526,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        refresh_native_player_stats_snapshot(&mut snapshot, &player, vitals);
        assert_eq!(snapshot.player_level, 5);
        assert_eq!(snapshot.player_experience, 1_000);
        assert_eq!(
            snapshot.player_vitals,
            NativeOtClientPlayerVitals {
                health: 110,
                max_health: 160,
                mana: 40,
                max_mana: 70,
                capacity: 600,
                magic_level: 4,
            }
        );
        let stats = encode_native_otclient_player_stats(
            &native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile,
            &snapshot,
        )
        .unwrap();
        assert_eq!(u16::from_le_bytes(stats.0[1..3].try_into().unwrap()), 110);
        assert_eq!(u16::from_le_bytes(stats.0[3..5].try_into().unwrap()), 160);
        assert_eq!(u16::from_le_bytes(stats.0[5..7].try_into().unwrap()), 600);
        assert_eq!(
            u32::from_le_bytes(stats.0[7..11].try_into().unwrap()),
            1_000
        );
        assert_eq!(u16::from_le_bytes(stats.0[11..13].try_into().unwrap()), 5);
        assert_eq!(u16::from_le_bytes(stats.0[14..16].try_into().unwrap()), 40);
        assert_eq!(u16::from_le_bytes(stats.0[16..18].try_into().unwrap()), 70);
        assert_eq!(stats.0[18], 4);
    }

    #[test]
    fn shared_native_registration_hydrates_conditions_without_client_effect_delivery() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let poison = PlayerCondition::new(PlayerConditionKind::Poison, 2, 7, 10).unwrap();
        let conditions = BTreeMap::from([(PlayerConditionKind::Poison, poison)]);
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                Player {
                    id: 104,
                    account_id: 1,
                    name: "Sorcerer".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals::default(),
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
                    town_id: 0,
                    respawn_state: PlayerRespawnState::default(),
                    equipment: PlayerEquipment::default(),
                    containers: PlayerContainers::default(),
                    conditions: conditions.clone(),
                },
                &map,
            )
            .unwrap();
        assert_eq!(shared.player_conditions(104).unwrap(), conditions);
    }

    #[test]
    fn shared_native_condition_tick_updates_vitals_epoch_and_expires_schedule() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let poison = PlayerCondition::new(PlayerConditionKind::Poison, 2, 7, 2).unwrap();
        let initial_vitals = PlayerVitals::default();
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                Player {
                    id: 105,
                    account_id: 1,
                    name: "Druid".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                initial_vitals,
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
                    town_id: 0,
                    respawn_state: PlayerRespawnState::default(),
                    equipment: PlayerEquipment::default(),
                    containers: PlayerContainers::default(),
                    conditions: BTreeMap::from([(PlayerConditionKind::Poison, poison)]),
                },
                &map,
            )
            .unwrap();

        assert_eq!(shared.vitals_epoch(), 0);
        let outcome = shared.apply_player_conditions(105, 2).unwrap();

        assert_eq!(outcome.applied_damage, 7);
        assert_eq!(outcome.remaining_health, initial_vitals.health - 7);
        assert_eq!(outcome.expired_conditions, 1);
        assert_eq!(shared.vitals_epoch(), 1);
        assert!(shared.player_conditions(105).unwrap().is_empty());
        assert_eq!(
            shared.player_vitals(105).unwrap().health,
            initial_vitals.health - 7
        );
    }

    #[test]
    fn native_condition_heartbeat_persists_elapsed_progress_and_expiry() {
        let path = database_path("native-condition-heartbeat-persistence");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        let player = Player {
            id: 107,
            account_id: account_id as u64,
            name: "Knight".into(),
            position: map.spawn(),
            level: 8,
            experience: 4_900,
            skill_points: 3,
        };
        database.save_player(&player).unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let poison = PlayerCondition::new(PlayerConditionKind::Poison, 3, 7, 6).unwrap();
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                player,
                PlayerVitals::default(),
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
                    town_id: 0,
                    respawn_state: PlayerRespawnState::default(),
                    equipment: PlayerEquipment::default(),
                    containers: PlayerContainers::default(),
                    conditions: BTreeMap::from([(PlayerConditionKind::Poison, poison)]),
                },
                &map,
            )
            .unwrap();

        shared.apply_player_conditions(107, 1).unwrap();
        persist_runtime_player_conditions(&mut database, &shared, 107).unwrap();
        assert_eq!(
            database
                .player_conditions(107)
                .unwrap()
                .get(&PlayerConditionKind::Poison)
                .copied()
                .unwrap(),
            PlayerCondition::from_persisted(PlayerConditionKind::Poison, 3, 7, 5, 1).unwrap()
        );

        let relogged_character = database
            .characters_for_account(account_id)
            .unwrap()
            .pop()
            .unwrap();
        let relogged_conditions = database.player_conditions(107).unwrap();
        let relogged = SharedNativeWorld::from_static_spawns(None).unwrap();
        relogged
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                Player {
                    id: relogged_character.id,
                    account_id: account_id as u64,
                    name: relogged_character.name,
                    position: relogged_character.position,
                    level: relogged_character.level,
                    experience: relogged_character.experience,
                    skill_points: relogged_character.skill_points,
                },
                PlayerVitals {
                    health: relogged_character.vitals.health,
                    max_health: relogged_character.vitals.max_health,
                    mana: relogged_character.vitals.mana,
                    max_mana: relogged_character.vitals.max_mana,
                    capacity: relogged_character.vitals.capacity,
                    magic_level: relogged_character.vitals.magic_level,
                },
                NativePlayerHydration {
                    progression: relogged_character.progression,
                    progression_attempts: relogged_character.progression_attempts,
                    town_id: relogged_character.town_id,
                    respawn_state: relogged_character.respawn_state,
                    equipment: database.player_equipment(107).unwrap(),
                    containers: database.player_containers(107).unwrap(),
                    conditions: relogged_conditions,
                },
                &map,
            )
            .unwrap();
        let resumed = relogged.apply_player_conditions(107, 2).unwrap();
        assert_eq!(resumed.applied_damage, 7);
        assert_eq!(resumed.expired_conditions, 0);
        persist_runtime_player_conditions(&mut database, &relogged, 107).unwrap();
        assert_eq!(
            database
                .player_conditions(107)
                .unwrap()
                .get(&PlayerConditionKind::Poison)
                .copied()
                .unwrap(),
            PlayerCondition::from_persisted(PlayerConditionKind::Poison, 3, 7, 3, 0).unwrap()
        );

        let expired = relogged.apply_player_conditions(107, 3).unwrap();
        assert_eq!(expired.applied_damage, 7);
        assert_eq!(expired.expired_conditions, 1);
        persist_runtime_player_conditions(&mut database, &relogged, 107).unwrap();
        assert!(database.player_conditions(107).unwrap().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn native_condition_damage_applies_and_persists_configured_death_loss() {
        let path = database_path("native-condition-death-persistence");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        let player = Player {
            id: 108,
            account_id: account_id as u64,
            name: "Druid".into(),
            position: map.spawn(),
            level: 8,
            experience: 4_900,
            skill_points: 3,
        };
        database.save_player(&player).unwrap();
        let poison = PlayerCondition::new(PlayerConditionKind::Poison, 1, 7, 1).unwrap();
        database
            .replace_player_conditions(
                player.id,
                &BTreeMap::from([(PlayerConditionKind::Poison, poison)]),
            )
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                player,
                PlayerVitals {
                    health: 7,
                    ..PlayerVitals::default()
                },
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
                    town_id: 1,
                    respawn_state: PlayerRespawnState::default(),
                    equipment: PlayerEquipment::default(),
                    containers: PlayerContainers::default(),
                    conditions: BTreeMap::from([(PlayerConditionKind::Poison, poison)]),
                },
                &map,
            )
            .unwrap();

        let (outcome, vitals, death_state) = shared
            .apply_player_conditions_with_death(108, &map, 1)
            .unwrap();
        assert_eq!(outcome.applied_damage, 7);
        assert_eq!(vitals.health, 0);
        let death_state = death_state.unwrap();
        assert!(death_state.dead);
        persist_runtime_player_conditions(&mut database, &shared, 108).unwrap();
        let multiplier = forgotten_core::ProgressionMultiplier::new(1_000).unwrap();
        let rules = PlayerProgressionRules {
            magic_level_multiplier: multiplier,
            skill_multipliers: [multiplier; 7],
        };
        let rules_by_vocation = BTreeMap::from([(VocationId::new(0), rules)]);
        assert!(apply_configured_native_death_loss(
            &mut database,
            &shared,
            108,
            DeathLossPolicy::FixedPercent(10),
            Some(&rules_by_vocation),
        )
        .unwrap());

        let reloaded = database
            .characters_for_account(account_id)
            .unwrap()
            .into_iter()
            .find(|character| character.id == 108)
            .unwrap();
        assert_eq!(reloaded.vitals.health, 0);
        assert_eq!(reloaded.experience, 4_410);
        assert!(reloaded.respawn_state.dead);
        assert!(reloaded.respawn_state.loss_applied);
        assert!(database.player_conditions(108).unwrap().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn static_target_attack_heartbeat_persists_vitals_and_configured_death_loss() {
        for (case, health, damage, policy, expected_health, expected_dead, expected_experience) in [
            (
                "nonlethal",
                15_u16,
                10_u16,
                DeathLossPolicy::DefaultFormula,
                5_u16,
                false,
                4_900_u64,
            ),
            (
                "lethal-fixed-loss",
                10_u16,
                10_u16,
                DeathLossPolicy::FixedPercent(10),
                0_u16,
                true,
                4_410_u64,
            ),
        ] {
            let path = database_path(&format!("static-target-attack-{case}"));
            let mut database = EngineDatabase::open(&path).unwrap();
            let account_id = database.create_account("operator", "hash").unwrap();
            let map = native_world_map();
            let player = Player {
                id: 101,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: map.spawn(),
                level: 8,
                experience: 4_900,
                skill_points: 3,
            };
            let player_id = player.id;
            database.save_player(&player).unwrap();
            let static_creature = forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            };
            let shared = SharedNativeWorld::from_static_spawns(Some(
                &FeTfsStaticSpawnCollection::new(vec![static_creature]).unwrap(),
            ))
            .unwrap();
            shared
                .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                    player,
                    PlayerVitals {
                        health,
                        max_health: health,
                        ..PlayerVitals::default()
                    },
                    NativePlayerHydration {
                        progression: PlayerProgression::default(),
                        progression_attempts: PlayerProgressionAttempts::default(),
                        town_id: 1,
                        respawn_state: PlayerRespawnState::default(),
                        equipment: PlayerEquipment::default(),
                        containers: PlayerContainers::default(),
                        conditions: BTreeMap::new(),
                    },
                    &map,
                )
                .unwrap();

            let outcome = advance_native_shared_world_heartbeat_with_static_target_policies(
                &shared,
                1,
                StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 1 },
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage },
                Some(&map),
            )
            .unwrap();
            assert_eq!(outcome.static_target_attacks, 1);
            assert_eq!(
                outcome.static_target_attack_player_ids,
                BTreeSet::from([101])
            );
            let multiplier = forgotten_core::ProgressionMultiplier::new(1_000).unwrap();
            let rules = PlayerProgressionRules {
                magic_level_multiplier: multiplier,
                skill_multipliers: [multiplier; 7],
            };
            let rules_by_vocation = BTreeMap::from([(VocationId::new(0), rules)]);
            persist_static_target_attack_vitals(
                &mut database,
                &shared,
                &outcome.static_target_attack_player_ids,
                policy,
                Some(&rules_by_vocation),
            )
            .unwrap();
            drop(database);

            let database = EngineDatabase::open(&path).unwrap();
            let reloaded = database
                .characters_for_account(account_id)
                .unwrap()
                .into_iter()
                .find(|character| character.id == player_id)
                .unwrap();
            assert_eq!(reloaded.vitals.health, expected_health);
            assert_eq!(reloaded.respawn_state.dead, expected_dead);
            assert_eq!(reloaded.experience, expected_experience);
            assert_eq!(
                reloaded.respawn_state.loss_applied,
                matches!(policy, DeathLossPolicy::FixedPercent(_))
            );
            drop(database);
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn static_target_death_transition_is_observed_once_by_a_native_session() {
        let map = native_world_map();
        let shared = SharedNativeWorld::from_static_spawns(Some(
            &FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }])
            .unwrap(),
        ))
        .unwrap();
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals {
                    health: 10,
                    max_health: 10,
                    ..PlayerVitals::default()
                },
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
                    town_id: 1,
                    respawn_state: PlayerRespawnState::default(),
                    equipment: PlayerEquipment::default(),
                    containers: PlayerContainers::default(),
                    conditions: BTreeMap::new(),
                },
                &map,
            )
            .unwrap();
        let mut observed_dead = false;
        assert!(!observe_native_death_transition(&shared, 101, &mut observed_dead).unwrap());

        let heartbeat = advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared,
            1,
            StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 1 },
            StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 10 },
            Some(&map),
        )
        .unwrap();
        assert_eq!(
            heartbeat.static_target_attack_player_ids,
            BTreeSet::from([101])
        );
        assert!(shared.player_respawn_state(101).unwrap().dead);
        assert!(observe_native_death_transition(&shared, 101, &mut observed_dead).unwrap());
        assert!(!observe_native_death_transition(&shared, 101, &mut observed_dead).unwrap());

        shared
            .hydrate_player_respawn_state(101, PlayerRespawnState::default())
            .unwrap();
        assert!(!observe_native_death_transition(&shared, 101, &mut observed_dead).unwrap());
        assert!(!observed_dead);
    }

    #[test]
    fn shared_native_registration_hydrates_exact_progression_attempts() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let attempts = PlayerProgressionAttempts::new([1, 2, 3, 4, 5, 6, 7], 8);
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                Player {
                    id: 105,
                    account_id: 1,
                    name: "Paladin".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals::default(),
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: attempts,
                    town_id: 42,
                    respawn_state: PlayerRespawnState::default(),
                    equipment: PlayerEquipment::default(),
                    containers: PlayerContainers::default(),
                    conditions: BTreeMap::new(),
                },
                &map,
            )
            .unwrap();
        assert_eq!(shared.player_progression_attempts(105).unwrap(), attempts);
        assert_eq!(shared.player_town(105).unwrap(), 42);
    }

    #[test]
    fn shared_native_registration_hydrates_persisted_dead_state_without_client_delivery() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let state = PlayerRespawnState {
            dead: true,
            respawn_at: Some(map.spawn()),
            death_time: Some(42),
            loss_applied: true,
        };
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                Player {
                    id: 106,
                    account_id: 1,
                    name: "DeadKnight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals {
                    health: 0,
                    ..PlayerVitals::default()
                },
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
                    town_id: 42,
                    respawn_state: state,
                    equipment: PlayerEquipment::default(),
                    containers: PlayerContainers::default(),
                    conditions: BTreeMap::new(),
                },
                &map,
            )
            .unwrap();
        assert_eq!(shared.player_respawn_state(106).unwrap(), state);
        assert_eq!(shared.player_vitals(106).unwrap().health, 0);
    }

    #[test]
    fn shared_native_regeneration_updates_vitals_epoch_only_on_recovery() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 104,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals {
                    health: 140,
                    max_health: 150,
                    mana: 45,
                    max_mana: 50,
                    capacity: 40_000,
                    magic_level: 0,
                },
                &map,
            )
            .unwrap();
        let rules = PlayerRegenerationRules {
            health: forgotten_core::RegenerationRule::new(3, 5).unwrap(),
            mana: forgotten_core::RegenerationRule::new(2, 4).unwrap(),
        };
        assert_eq!(shared.vitals_epoch(), 0);
        let unchanged = shared.apply_player_regeneration(104, rules, 1).unwrap();
        assert_eq!(unchanged.health_gained, 0);
        assert_eq!(shared.vitals_epoch(), 0);
        let recovered = shared.apply_player_regeneration(104, rules, 2).unwrap();
        assert_eq!(recovered.health_gained, 5);
        assert_eq!(recovered.mana_gained, 4);
        assert_eq!(shared.vitals_epoch(), 1);
        assert_eq!(shared.player_vitals(104).unwrap(), recovered.vitals);
    }

    #[test]
    fn shared_declared_spell_cast_uses_catalog_mana_and_cooldown_only() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 107,
                    account_id: 1,
                    name: "Sorcerer".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals {
                    mana: 50,
                    max_mana: 50,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();
        let catalog = parse_declarative_spells_xml(
            br#"<fe-spells><fe-spell id="100" manacost="20" intervalticks="2"/></fe-spells>"#,
        )
        .unwrap();
        assert_eq!(shared.vitals_epoch(), 0);
        let outcome = shared
            .apply_declarative_spell_cast(107, 100, &catalog)
            .unwrap();
        assert_eq!(outcome.mana_spent, 20);
        assert_eq!(outcome.remaining_mana, 30);
        assert_eq!(outcome.next_cast_tick, 2);
        assert_eq!(shared.vitals_epoch(), 1);
        assert_eq!(shared.player_vitals(107).unwrap().mana, 30);
        assert!(matches!(
            shared.apply_declarative_spell_cast(107, 100, &catalog),
            Err(HostError::Core(
                forgotten_core::CoreError::SpellCooldownActive { .. }
            ))
        ));
        assert!(matches!(
            shared.apply_declarative_spell_cast(107, 999, &catalog),
            Err(HostError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn shared_map_item_use_validation_is_authoritative_and_side_effect_free() {
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let adjacent = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("item-use-host", spawn);
        for position in [spawn, adjacent] {
            map.set_tile(
                position,
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: true,
                },
            )
            .unwrap();
        }
        map.set_tile_items(
            adjacent,
            vec![forgotten_core::WorldMapItem {
                server_id: 1945,
                client_thing_id: Some(1945),
                count: 1,
                action_id: Some(7),
                unique_id: None,
                text: Some("Read me".into()),
                description: None,
                teleport_destination: None,
                duration: None,
                charges: Some(3),
                children: Vec::new(),
            }],
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 108,
                    account_id: 1,
                    name: "Knight".into(),
                    position: spawn,
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        assert_eq!(shared.vitals_epoch(), 0);
        let outcome = shared
            .validate_player_item_use(
                &map,
                PlayerItemUseIntent::new(108, adjacent, 0, 1945).unwrap(),
            )
            .unwrap();
        assert_eq!(outcome.action_id, Some(7));
        assert!(outcome.has_text);
        assert_eq!(outcome.charges, Some(3));
        let two_target_outcome = shared
            .validate_player_item_use_ex(
                &map,
                PlayerItemUseExIntent::new(108, adjacent, 0, 1945, adjacent, 0, 1945).unwrap(),
            )
            .unwrap();
        assert_eq!(two_target_outcome.source.server_id, 1945);
        assert_eq!(two_target_outcome.target.server_id, 1945);
        assert_eq!(shared.vitals_epoch(), 0);
    }

    #[test]
    fn native_map_item_use_intent_requires_one_catalog_server_id() {
        let position = NativeOtClientPosition {
            x: 100,
            y: 101,
            z: 7,
        };
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                1945,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        assert_eq!(
            native_map_item_use_intent(Some(&catalog), 101, position, 102, 3),
            Some(
                PlayerItemUseIntent::new(
                    101,
                    Position {
                        x: 100,
                        y: 101,
                        z: 7,
                    },
                    3,
                    1945,
                )
                .unwrap()
            ),
        );
        assert_eq!(
            native_map_item_use_ex_intent(
                Some(&catalog),
                101,
                (position, 102, 3),
                (position, 102, 4)
            ),
            Some(
                PlayerItemUseExIntent::new(
                    101,
                    Position {
                        x: 100,
                        y: 101,
                        z: 7,
                    },
                    3,
                    1945,
                    Position {
                        x: 100,
                        y: 101,
                        z: 7,
                    },
                    4,
                    1945,
                )
                .unwrap()
            ),
        );
        assert_eq!(
            native_map_item_use_intent(Some(&catalog), 101, position, 103, 3),
            None
        );
        assert_eq!(
            native_map_item_use_creature_intent(
                Some(&catalog),
                101,
                position,
                102,
                3,
                NATIVE_OTCLIENT_PLAYER_ID_START + 99,
            ),
            Some(PlayerItemUseCreatureIntent {
                source: PlayerItemUseIntent::new(
                    101,
                    Position {
                        x: 100,
                        y: 101,
                        z: 7,
                    },
                    3,
                    1945,
                )
                .unwrap(),
                target: PlayerItemUseCreatureTarget::Player(99),
            })
        );
        assert_eq!(
            native_map_item_use_creature_intent(Some(&catalog), 101, position, 102, 3, 0x4000_0001,),
            Some(PlayerItemUseCreatureIntent {
                source: PlayerItemUseIntent::new(
                    101,
                    Position {
                        x: 100,
                        y: 101,
                        z: 7,
                    },
                    3,
                    1945,
                )
                .unwrap(),
                target: PlayerItemUseCreatureTarget::StaticCreature(0x4000_0001),
            })
        );
        catalog
            .insert(
                1946,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        assert_eq!(
            native_map_item_use_intent(Some(&catalog), 101, position, 102, 3),
            None
        );
        assert_eq!(
            native_map_item_use_ex_intent(
                Some(&catalog),
                101,
                (position, 102, 3),
                (position, 102, 4)
            ),
            None
        );
        assert_eq!(
            native_map_item_use_creature_intent(Some(&catalog), 101, position, 102, 3, 0x4000_0001,),
            None
        );
    }

    #[test]
    fn shared_native_world_tracks_and_clears_player_interaction_intent() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        for (id, name) in [(101, "Knight"), (102, "Druid")] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position: map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap();
        }
        assert_eq!(
            shared.set_player_target(101, Some(102)).unwrap(),
            PlayerInteractionIntent {
                target_player_id: Some(102),
                target_static_creature_id: None,
                follow_player_id: None,
            }
        );
        assert_eq!(
            shared.set_player_follow(101, Some(102)).unwrap(),
            PlayerInteractionIntent {
                target_player_id: Some(102),
                target_static_creature_id: None,
                follow_player_id: Some(102),
            }
        );
        shared.remove_player(102).unwrap();
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent::default()
        );
    }

    #[test]
    fn shared_native_world_replaces_authoritative_fight_mode_state() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 101,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        assert_eq!(
            shared.player_fight_mode_state(101).unwrap(),
            PlayerFightModeState::default()
        );
        let state = PlayerFightModeState {
            mode: PlayerFightMode::Defense,
            chase: true,
            secure: true,
        };
        assert!(shared.replace_player_fight_mode_state(101, state).unwrap());
        assert!(!shared.replace_player_fight_mode_state(101, state).unwrap());
        assert_eq!(shared.player_fight_mode_state(101).unwrap(), state);
    }

    #[test]
    fn native_player_interaction_ids_only_accept_the_reserved_player_range() {
        assert_eq!(
            native_player_id_to_character_id(NATIVE_OTCLIENT_PLAYER_ID_START + 101),
            Some(101)
        );
        assert_eq!(native_player_id_to_character_id(0), None);
        assert_eq!(
            native_player_id_to_character_id(NATIVE_OTCLIENT_PLAYER_ID_END),
            None
        );
    }

    #[test]
    fn native_player_interaction_application_preserves_follow_and_defers_non_players() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        for (id, name) in [(101, "Knight"), (102, "Druid")] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position: map.spawn(),
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap();
        }

        apply_native_player_interaction(
            &shared,
            101,
            NATIVE_OTCLIENT_PLAYER_ID_START + 102,
            NativePlayerInteractionKind::Target,
            false,
        )
        .unwrap();
        apply_native_player_interaction(
            &shared,
            101,
            NATIVE_OTCLIENT_PLAYER_ID_START + 102,
            NativePlayerInteractionKind::Follow,
            false,
        )
        .unwrap();
        apply_native_player_interaction(
            &shared,
            101,
            0,
            NativePlayerInteractionKind::Target,
            false,
        )
        .unwrap();
        apply_native_player_interaction(
            &shared,
            101,
            NATIVE_OTCLIENT_PLAYER_ID_END,
            NativePlayerInteractionKind::Follow,
            false,
        )
        .unwrap();
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent {
                target_player_id: None,
                target_static_creature_id: None,
                follow_player_id: Some(102),
            }
        );
    }

    #[test]
    fn native_target_selection_accepts_active_static_entities_but_follow_remains_player_only() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }])
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        apply_native_player_interaction(
            &shared,
            101,
            creature_id,
            NativePlayerInteractionKind::Target,
            false,
        )
        .unwrap();
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent {
                target_player_id: None,
                target_static_creature_id: Some(creature_id),
                follow_player_id: None,
            }
        );
        apply_native_player_interaction(
            &shared,
            101,
            creature_id,
            NativePlayerInteractionKind::Follow,
            false,
        )
        .unwrap();
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent {
                target_player_id: None,
                target_static_creature_id: Some(creature_id),
                follow_player_id: None,
            }
        );
    }

    #[test]
    fn static_creature_health_refreshes_visibility_and_native_display_frame() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 75,
                direction: 2,
            }])
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        assert_eq!(shared.visibility_epoch(), 0);
        assert!(!shared
            .set_static_creature_health_percent(creature_id, 75)
            .unwrap());
        assert_eq!(shared.visibility_epoch(), 0);
        assert!(shared
            .set_static_creature_health_percent(creature_id, 40)
            .unwrap());
        assert_eq!(shared.visibility_epoch(), 1);
        let active_static_spawns = shared.active_static_spawns().unwrap();
        assert_eq!(active_static_spawns.entities[0].health_percent, 40);
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        assert_eq!(
            native_static_creature_health_frames(&profile, &active_static_spawns)
                .unwrap()
                .into_iter()
                .map(|frame| frame.0)
                .collect::<Vec<_>>(),
            vec![vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH,
                1,
                0,
                0,
                64,
                40,
            ]]
        );
    }

    #[test]
    fn static_creature_runtime_snapshot_persists_across_fresh_shared_worlds() {
        let path = database_path("static-creature-runtime-persistence");
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns = FeTfsStaticSpawnCollection::with_respawn_intervals(
            vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 75,
                direction: 2,
            }],
            BTreeMap::from([(creature_id, 8)]),
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        shared
            .set_static_creature_health_percent(creature_id, 42)
            .unwrap();
        shared
            .lock()
            .unwrap()
            .deactivate_static_creature(creature_id)
            .unwrap();
        persist_static_creature_runtime_to_database(&shared, &path).unwrap();

        let fresh = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        assert_eq!(
            restore_static_creature_runtime_from_database(&fresh, &path).unwrap(),
            StaticCreatureRuntimeRestoreSummary {
                restored: 1,
                ignored_unknown: 0,
            }
        );
        assert_eq!(
            fresh.static_creature_runtime_snapshot().unwrap(),
            vec![StaticCreatureRuntimeSnapshot {
                id: creature_id,
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                active: false,
                health_percent: 42,
                reactivation_remaining_seconds: Some(8),
            }]
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn selected_static_melee_refreshes_visibility_and_removes_a_defeated_static_target() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::with_respawn_intervals_and_experience_rewards(
                vec![forgotten_core::FeTfsStaticEntity {
                    id: creature_id,
                    name: "Rat".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    look_type: 21,
                    head: 0,
                    body: 0,
                    legs: 0,
                    feet: 0,
                    addons: 0,
                    speed: 134,
                    health_percent: 15,
                    direction: 2,
                }],
                BTreeMap::new(),
                BTreeMap::from([(creature_id, 7_000)]),
            )
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        shared
            .set_player_static_target(101, Some(creature_id))
            .unwrap();
        let first = apply_native_selected_static_creature_melee(&shared, 101, &map)
            .unwrap()
            .unwrap();
        assert_eq!(first.applied_damage, 10);
        assert_eq!(first.remaining_health_percent, 5);
        assert!(!first.deactivated);
        assert_eq!(shared.visibility_epoch(), 2);
        assert_eq!(
            shared.active_static_spawns().unwrap().entities[0].health_percent,
            5
        );
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        assert_eq!(
            encode_native_otclient_creature_health(&profile, creature_id, 5, 100)
                .unwrap()
                .0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH,
                1,
                0,
                0,
                64,
                5,
            ]
        );
        let path = database_path("selected-static-melee-runtime");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        database
            .save_player(&Player {
                id: 101,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: map.spawn(),
                level: 8,
                experience: 0,
                skill_points: 0,
            })
            .unwrap();
        persist_static_creature_runtime_to_open_database(&shared, &mut database).unwrap();
        assert_eq!(
            database.static_creature_runtime().unwrap(),
            vec![StaticCreatureRuntimeRecord {
                creature_id,
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                active: true,
                health_percent: 5,
                reactivation_remaining_seconds: None,
            }]
        );
        assert_eq!(
            apply_native_selected_static_creature_melee(&shared, 101, &map).unwrap(),
            None
        );
        assert_eq!(shared.visibility_epoch(), 2);
        assert_eq!(
            shared.active_static_spawns().unwrap().entities[0].health_percent,
            5
        );
        advance_native_shared_world_heartbeat(&shared, 1).unwrap();

        let final_hit = apply_native_selected_static_creature_melee(&shared, 101, &map)
            .unwrap()
            .unwrap();
        assert_eq!(final_hit.applied_damage, 5);
        assert!(final_hit.deactivated);
        let award = apply_and_persist_native_static_defeat_experience(
            &mut database,
            &shared,
            101,
            creature_id,
            Some(&ExperienceAwardPolicy::new(1, Vec::new()).unwrap()),
            Some(&BTreeMap::from([(
                VocationId::new(0),
                VocationLevelUpGains::new(15, 5, 25),
            )])),
        )
        .unwrap()
        .unwrap();
        assert_eq!(award.raw_experience, 7_000);
        assert_eq!(award.awarded_experience, 7_000);
        assert!(award.gained_levels > 0);
        let persisted = database.player_by_id(101).unwrap();
        assert_eq!(persisted.experience, 7_000);
        assert!(persisted.vitals.health > PlayerVitals::default().health);
        assert!(persisted.vitals.mana > PlayerVitals::default().mana);
        assert!(persisted.vitals.capacity > PlayerVitals::default().capacity);
        assert_eq!(shared.visibility_epoch(), 3);
        assert!(shared.active_static_spawns().unwrap().entities.is_empty());
        persist_static_creature_runtime_to_open_database(&shared, &mut database).unwrap();
        assert_eq!(
            database.static_creature_runtime().unwrap()[0],
            StaticCreatureRuntimeRecord {
                creature_id,
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                active: false,
                health_percent: 0,
                reactivation_remaining_seconds: None,
            }
        );
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent::default()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn shared_static_target_attack_updates_vitals_epoch_only_after_real_damage() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let shared = SharedNativeWorld::from_static_spawns(Some(
            &FeTfsStaticSpawnCollection::new(vec![creature]).unwrap(),
        ))
        .unwrap();
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                PlayerVitals {
                    health: 5,
                    max_health: 5,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();
        assert_eq!(
            shared
                .apply_static_creature_target_damage(creature_id, 2, &map)
                .unwrap(),
            StaticCreatureTargetAttackOutcome::NoTarget
        );
        assert_eq!(shared.vitals_epoch(), 0);

        shared
            .lock()
            .unwrap()
            .select_static_creature_target(creature_id, 1)
            .unwrap();
        assert!(matches!(
            shared
                .apply_static_creature_target_damage(creature_id, 2, &map)
                .unwrap(),
            StaticCreatureTargetAttackOutcome::Applied {
                applied_damage: 2,
                remaining_health: 3,
                death_state: None,
                ..
            }
        ));
        assert_eq!(shared.vitals_epoch(), 1);

        shared
            .lock()
            .unwrap()
            .move_player(
                101,
                Position {
                    x: 99,
                    y: 100,
                    z: 7,
                },
            )
            .unwrap();
        assert_eq!(
            shared
                .apply_static_creature_target_damage(creature_id, 3, &map)
                .unwrap(),
            StaticCreatureTargetAttackOutcome::TargetNotAdjacent {
                creature_id,
                target_player_id: 101,
            }
        );
        assert_eq!(shared.vitals_epoch(), 1);
    }

    #[test]
    fn selected_player_melee_persists_authoritative_vitals_and_returns_native_target() {
        let path = database_path("selected-player-melee");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        for (id, name, position) in [
            (101_u64, "Knight", map.spawn()),
            (
                102_u64,
                "Druid",
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ),
        ] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
                    position,
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                })
                .unwrap();
        }
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: account_id as u64,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 102,
                    account_id: account_id as u64,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals {
                    health: 20,
                    max_health: 20,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();
        shared.set_player_target(101, Some(102)).unwrap();

        let (native_target_id, vitals, outcome) = apply_native_selected_player_melee(
            &mut database,
            &shared,
            101,
            &map,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: None,
                skill_rate: 1,
                death_loss_policy: DeathLossPolicy::DefaultFormula,
                declarative_weapon_catalog: None,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(native_target_id, NATIVE_OTCLIENT_PLAYER_ID_START + 102);
        assert_eq!(
            outcome.applied_damage,
            NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE
        );
        assert_eq!(vitals.health, 10);
        assert_eq!(shared.vitals_epoch(), 1);
        assert_eq!(
            database
                .characters_for_account(account_id)
                .unwrap()
                .into_iter()
                .find(|character| character.id == 102)
                .unwrap()
                .vitals
                .health,
            10
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn selected_player_melee_uses_only_an_equipped_declarative_weapon() {
        let path = database_path("selected-player-declarative-weapon");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        for (id, name, position) in [
            (111_u64, "Knight", map.spawn()),
            (
                112_u64,
                "Druid",
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ),
        ] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
                    position,
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                })
                .unwrap();
        }
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 111,
                    account_id: account_id as u64,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 112,
                    account_id: account_id as u64,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals {
                    health: 20,
                    max_health: 20,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();
        let catalog = parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2376" damage="12" intervalticks="1"/></fe-weapons>"#,
        )
        .unwrap();
        shared.set_player_target(111, Some(112)).unwrap();
        assert!(apply_native_selected_player_melee(
            &mut database,
            &shared,
            111,
            &map,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: None,
                skill_rate: 1,
                death_loss_policy: DeathLossPolicy::DefaultFormula,
                declarative_weapon_catalog: Some(&catalog),
            },
        )
        .unwrap()
        .is_none());

        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(2376, 1).unwrap(),
        );
        shared.replace_player_equipment(111, equipment).unwrap();
        let (_native_target_id, vitals, outcome) = apply_native_selected_player_melee(
            &mut database,
            &shared,
            111,
            &map,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: None,
                skill_rate: 1,
                death_loss_policy: DeathLossPolicy::DefaultFormula,
                declarative_weapon_catalog: Some(&catalog),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(outcome.requested_damage, 12);
        assert_eq!(outcome.applied_damage, 12);
        assert_eq!(vitals.health, 8);
        assert_eq!(
            database
                .characters_for_account(account_id)
                .unwrap()
                .into_iter()
                .find(|character| character.id == 112)
                .unwrap()
                .vitals
                .health,
            8
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn selected_player_melee_awards_and_persists_rate_scaled_configured_fist_tries() {
        let path = database_path("selected-player-melee-skill-try");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        for (id, name, position) in [
            (301_u64, "Knight", map.spawn()),
            (
                302_u64,
                "Druid",
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ),
        ] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
                    position,
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                })
                .unwrap();
        }
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        for (id, name, position) in [
            (301_u64, "Knight", map.spawn()),
            (
                302_u64,
                "Druid",
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ),
        ] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: account_id as u64,
                        name: name.into(),
                        position,
                        level: 8,
                        experience: 4_900,
                        skill_points: 3,
                    },
                    &map,
                )
                .unwrap();
        }
        shared.set_player_target(301, Some(302)).unwrap();
        let multiplier = forgotten_core::ProgressionMultiplier::new(1_000).unwrap();
        let rules = PlayerProgressionRules {
            magic_level_multiplier: multiplier,
            skill_multipliers: [multiplier; 7],
        };
        let rules_by_vocation = BTreeMap::from([(VocationId::new(0), rules)]);

        apply_native_selected_player_melee(
            &mut database,
            &shared,
            301,
            &map,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: Some(&rules_by_vocation),
                skill_rate: 2,
                death_loss_policy: DeathLossPolicy::DefaultFormula,
                declarative_weapon_catalog: None,
            },
        )
        .unwrap()
        .unwrap();

        let in_memory_tries = shared
            .player_progression_attempts(301)
            .unwrap()
            .skill_tries(PlayerSkill::Fist);
        assert_eq!(in_memory_tries, 2);
        assert_eq!(shared.progression_epoch(), 1);
        let persisted_tries = database
            .player_progression_attempts(301)
            .unwrap()
            .skill_tries(PlayerSkill::Fist);
        assert_eq!(persisted_tries, 2);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn selected_player_melee_applies_and_persists_fixed_configured_death_loss() {
        let path = database_path("selected-player-melee-death");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        for (id, name, position) in [
            (201_u64, "Knight", map.spawn()),
            (
                202_u64,
                "Druid",
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ),
        ] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
                    position,
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                })
                .unwrap();
        }
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 201,
                    account_id: account_id as u64,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 202,
                    account_id: account_id as u64,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals {
                    health: NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE,
                    max_health: NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();
        shared.replace_player_town(202, 1).unwrap();
        shared.set_player_target(201, Some(202)).unwrap();
        let multiplier = forgotten_core::ProgressionMultiplier::new(1_000).unwrap();
        let rules = PlayerProgressionRules {
            magic_level_multiplier: multiplier,
            skill_multipliers: [multiplier; 7],
        };
        let rules_by_vocation = BTreeMap::from([(VocationId::new(0), rules)]);

        let (_native_target_id, vitals, outcome) = apply_native_selected_player_melee(
            &mut database,
            &shared,
            201,
            &map,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: Some(&rules_by_vocation),
                skill_rate: 1,
                death_loss_policy: DeathLossPolicy::FixedPercent(10),
                declarative_weapon_catalog: None,
            },
        )
        .unwrap()
        .unwrap();
        assert!(outcome.defeated);
        assert_eq!(vitals.health, 0);
        assert_eq!(
            shared.player_respawn_state(202).unwrap(),
            PlayerRespawnState {
                dead: true,
                respawn_at: Some(map.spawn()),
                death_time: Some(0),
                loss_applied: true,
            }
        );
        assert_eq!(shared.player_and_vitals(202).unwrap().0.experience, 4_410);
        let persisted = database
            .characters_for_account(account_id)
            .unwrap()
            .into_iter()
            .find(|character| character.id == 202)
            .unwrap();
        assert_eq!(persisted.vitals.health, 0);
        assert_eq!(persisted.experience, 4_410);
        assert!(persisted.respawn_state.loss_applied);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn shared_player_visibility_tracks_join_move_and_leave() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let knight_position = shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let druid_position = shared
            .register_player_at_available_position(
                Player {
                    id: 102,
                    account_id: 2,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        assert_eq!(knight_position, map.spawn());
        assert_eq!(druid_position.x, 101);
        assert_eq!(shared.visibility_epoch(), 2);
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(knight_position),
            player_level: 8,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: forgotten_core::PlayerSkills::default(),
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let joined =
            encode_shared_native_world_viewport(&profile, &snapshot, &map, &shared, 101).unwrap();
        assert!(joined.0.windows(5).any(|window| window == b"Druid"));
        {
            let mut world = shared.lock().unwrap();
            world
                .move_player_cardinal(102, CardinalDirection::East)
                .unwrap();
        }
        shared.mark_visibility_changed();
        assert_eq!(shared.visibility_epoch(), 3);
        assert_eq!(
            shared.visible_players(101, 128, 220).unwrap()[0].position,
            native_position(Position {
                x: 102,
                y: 100,
                z: 7,
            })
        );
        let moved =
            encode_shared_native_world_viewport(&profile, &snapshot, &map, &shared, 101).unwrap();
        assert!(moved.0.windows(5).any(|window| window == b"Druid"));
        shared.remove_player(102).unwrap();
        assert_eq!(shared.visibility_epoch(), 4);
        let left =
            encode_shared_native_world_viewport(&profile, &snapshot, &map, &shared, 101).unwrap();
        assert!(!left.0.windows(5).any(|window| window == b"Druid"));
        shared.remove_player(101).unwrap();
    }

    #[test]
    fn native_render_snapshot_detaches_packet_preparation_from_world_mutation() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 102,
                    account_id: 2,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let render_snapshot = shared.native_render_snapshot(101, 0, 220).unwrap();
        let preparation = thread::spawn(move || render_snapshot.visible_players);
        shared.remove_player(102).unwrap();
        let visible_players = preparation.join().unwrap();
        assert_eq!(visible_players.len(), 1);
        assert_eq!(visible_players[0].player_id, native_player_id(102).unwrap());
        assert!(shared.visible_players(101, 0, 220).unwrap().is_empty());
    }

    #[test]
    fn shared_public_chat_broadcasts_sanitized_events_and_releases_recipients() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let _knight_position = shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 102,
                    account_id: 2,
                    name: "Druid".into(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let knight_events = shared.register_public_chat_recipient(101).unwrap();
        let druid_events = shared.register_public_chat_recipient(102).unwrap();
        assert_eq!(
            shared
                .broadcast_public_chat(101, "  hello\n world  ")
                .unwrap(),
            2
        );
        let expected = SharedPublicChatEvent {
            speaker_name: "Knight".into(),
            speaker_position: native_position(map.spawn()),
            text: "hello world".into(),
        };
        assert_eq!(knight_events.try_recv().unwrap(), expected);
        assert_eq!(druid_events.try_recv().unwrap(), expected);
        assert_eq!(shared.broadcast_public_chat(101, "   ").unwrap(), 0);
        assert_eq!(
            shared
                .broadcast_public_chat(101, &"x".repeat(NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES))
                .unwrap(),
            2
        );
        let capped = knight_events.try_recv().unwrap();
        assert_eq!(capped.speaker_name, "Knight");
        assert_eq!(capped.speaker_position, native_position(map.spawn()));
        assert_eq!(capped.text.len(), NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES);
        assert_eq!(druid_events.try_recv().unwrap(), capped);
        shared.unregister_public_chat_recipient(102);
        assert_eq!(shared.broadcast_public_chat(101, "again").unwrap(), 1);
        assert_eq!(knight_events.try_recv().unwrap().text, "again".to_string());
        assert!(matches!(
            druid_events.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        shared.unregister_public_chat_recipient(101);
        shared.remove_player(101).unwrap();
        shared.remove_player(102).unwrap();
    }

    #[test]
    fn shared_public_chat_bounds_a_slow_recipient_queue_without_unregistering_it() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let events = shared.register_public_chat_recipient(101).unwrap();
        for index in 0..NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY {
            assert_eq!(
                shared
                    .broadcast_public_chat(101, &format!("queued-{index}"))
                    .unwrap(),
                1
            );
        }
        assert_eq!(shared.broadcast_public_chat(101, "dropped").unwrap(), 0);
        assert!(events.try_recv().is_ok());
        assert_eq!(shared.broadcast_public_chat(101, "resumed").unwrap(), 1);
        shared.unregister_public_chat_recipient(101);
        shared.remove_player(101).unwrap();
    }

    fn native_empty_world_config(bind_addr: SocketAddr) -> NativeOtClientHostConfig {
        let mut config = native_otclient_config(bind_addr);
        config.empty_world = Some(NativeOtClientEmptyWorldConfig {
            ground_thing_id: 102,
            player_look_type: 128,
            outfit_first_look_type: 128,
            outfit_last_look_type: 128,
            player_speed: 220,
            server_beat: 50,
        });
        config.world_map = Some(native_world_map());
        config
    }

    #[test]
    fn native_hydrated_outfit_restores_only_configured_range_appearance() {
        let persisted = PlayerOutfit {
            look_type: 128,
            head: 1,
            body: 2,
            legs: 3,
            feet: 4,
        };
        assert_eq!(
            native_hydrated_classic_outfit(128, 128, 131, persisted),
            NativeOtClientClassicOutfit {
                look_type: 128,
                head: 1,
                body: 2,
                legs: 3,
                feet: 4,
            }
        );
        assert_eq!(
            native_hydrated_classic_outfit(128, 128, 131, PlayerOutfit::default()),
            NativeOtClientClassicOutfit {
                look_type: 128,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
            }
        );
        assert_eq!(
            native_hydrated_classic_outfit(
                128,
                128,
                131,
                PlayerOutfit {
                    look_type: 129,
                    ..persisted
                },
            ),
            NativeOtClientClassicOutfit {
                look_type: 129,
                head: 1,
                body: 2,
                legs: 3,
                feet: 4,
            }
        );
        assert_eq!(
            native_hydrated_classic_outfit(
                128,
                128,
                131,
                PlayerOutfit {
                    look_type: 132,
                    ..persisted
                },
            ),
            NativeOtClientClassicOutfit {
                look_type: 128,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
            }
        );
    }

    fn add_string(payload: &mut Vec<u8>, value: &str) {
        payload.extend_from_slice(&(value.len() as u16).to_le_bytes());
        payload.extend_from_slice(value.as_bytes());
    }

    fn native_login_request(account_id: u32, password: &str) -> Frame {
        let mut payload = vec![forgotten_protocol::NATIVE_OTCLIENT_ENTER_ACCOUNT];
        payload.extend_from_slice(&2_u16.to_le_bytes());
        payload.extend_from_slice(&740_u16.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.extend_from_slice(&account_id.to_le_bytes());
        add_string(&mut payload, password);
        add_string(&mut payload, "otcv8-test");
        payload.extend_from_slice(&1_u16.to_le_bytes());
        Frame(payload)
    }

    fn native_game_request(account_id: u32, character_name: &str, password: &str) -> Frame {
        let mut payload = vec![forgotten_protocol::NATIVE_OTCLIENT_PENDING_GAME];
        payload.extend_from_slice(&2_u16.to_le_bytes());
        payload.extend_from_slice(&740_u16.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&account_id.to_le_bytes());
        add_string(&mut payload, character_name);
        add_string(&mut payload, password);
        add_string(&mut payload, "otcv8-test");
        payload.extend_from_slice(&1_u16.to_le_bytes());
        Frame(payload)
    }

    #[test]
    fn accepts_a_bounded_probe_and_returns_the_selected_profile() {
        let database = database_path("probe");
        let host = start(test_config(), &database).unwrap();
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        write_frame(&mut stream, &probe_request()).unwrap();
        let response = read_frame(&mut stream).unwrap();
        assert_eq!(response, probe_response(FE_7_4_PROFILE));
        host.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn rejects_an_invalid_probe_with_an_error_frame() {
        let database = database_path("invalid");
        let host = start(test_config(), &database).unwrap();
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        write_frame(&mut stream, &Frame(b"BAD!\x01".to_vec())).unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap(),
            error_frame(b"invalid-probe")
        );
        host.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn rejects_an_unbounded_configuration() {
        let mut config = test_config();
        config.max_connections = 0;
        assert!(matches!(
            start(config, database_path("limit")),
            Err(HostError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn fixed_configured_death_loss_requires_vocation_progression_rules() {
        let mut config = native_otclient_config("127.0.0.1:0".parse().unwrap());
        config.death_loss_policy = DeathLossPolicy::FixedPercent(10);
        assert!(matches!(
            config.validate(),
            Err(HostError::InvalidConfiguration(message))
                if message == "fixed deathLosePercent requires validated vocation progression rules"
        ));
    }

    #[test]
    fn answers_a_raw_xml_status_request() {
        let database = database_path("status-xml");
        let status = start_status(status_config(), &database).unwrap();
        let mut stream = TcpStream::connect(status.local_addr()).unwrap();
        write_frame(
            &mut stream,
            &Frame(vec![0xff, 0x04, 0x00, b'i', b'n', b'f', b'o']),
        )
        .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.contains("<tsqp version=\"1.0\">"));
        assert!(response.contains("Forgotten Engine Test"));
        status.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn answers_a_binary_status_request() {
        let database = database_path("status-binary");
        let status = start_status(status_config(), &database).unwrap();
        let mut stream = TcpStream::connect(status.local_addr()).unwrap();
        write_frame(&mut stream, &Frame(vec![0x01, 0x88, 0x00])).unwrap();
        let response = read_frame(&mut stream).unwrap();
        assert_eq!(response.0[0], 0x20);
        assert!(response.0.contains(&0x23));
        status.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn authenticates_a_legacy_login_fixture_and_returns_an_encrypted_character_list() {
        let database_path = database_path("legacy-login");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("admin", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let key = Arc::new(LegacyRsaPrivateKey::generate().unwrap());
        let mut config = test_config();
        config.legacy_login = Some(LegacyLoginConfig {
            rsa_private_key: Arc::clone(&key),
            server_name: "Forgotten Test".into(),
            message_of_the_day: "Welcome".into(),
        });
        let host = start(config, &database_path).unwrap();
        let mut plaintext = [0; forgotten_protocol::LEGACY_RSA_BLOCK_SIZE];
        plaintext[1..5].copy_from_slice(&1_u32.to_le_bytes());
        plaintext[5..9].copy_from_slice(&2_u32.to_le_bytes());
        plaintext[9..13].copy_from_slice(&3_u32.to_le_bytes());
        plaintext[13..17].copy_from_slice(&4_u32.to_le_bytes());
        plaintext[17..19].copy_from_slice(&5_u16.to_le_bytes());
        plaintext[19..24].copy_from_slice(b"admin");
        plaintext[24..26].copy_from_slice(&28_u16.to_le_bytes());
        plaintext[26..54].copy_from_slice(b"correct horse battery staple");
        let encrypted = key.encrypt_raw_block_for_harness(&plaintext).unwrap();
        let mut payload = vec![0x01, 0xe4, 0x02];
        payload.extend_from_slice(&encrypted);
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        write_frame(&mut stream, &Frame(payload)).unwrap();
        let response = read_frame(&mut stream).unwrap();
        let response = forgotten_protocol::xtea_decrypt_packet(&response.0, [1, 2, 3, 4]).unwrap();
        assert_eq!(response[0], 0x64);
        assert!(response.windows(6).any(|window| window == b"Knight"));
        host.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn serves_a_profile_driven_native_otclient_character_list_and_game_gate() {
        let database_path = database_path("native-otclient");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();

        let login = start_native_otclient_login(
            native_otclient_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();
        let game = start_native_otclient_game(
            native_otclient_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut login_stream = TcpStream::connect(login.local_addr()).unwrap();
        write_frame(
            &mut login_stream,
            &native_login_request(
                account_id.try_into().unwrap(),
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let character_list = read_frame(&mut login_stream).unwrap();
        assert_eq!(
            character_list.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_LOGIN_CHARACTER_LIST
        );
        assert!(character_list
            .0
            .windows(6)
            .any(|window| window == b"Knight"));
        assert!(character_list
            .0
            .windows(4)
            .any(|window| window == [127, 0, 0, 1]));

        let mut game_stream = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut game_stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let game_gate = read_frame(&mut game_stream).unwrap();
        assert_eq!(
            game_gate.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_ERROR
        );
        assert!(game_gate
            .0
            .windows(
                b"Forgotten Engine native map initialization is not enabled for this selected client profile."
                    .len(),
            )
            .any(|window| {
                window
                    == b"Forgotten Engine native map initialization is not enabled for this selected client profile."
            }));

        game.shutdown().unwrap();
        login.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_lethal_condition_emits_one_classic_death_record() {
        let database_path = database_path("native-condition-death-record");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        database
            .update_player_vitals(
                1,
                PersistedPlayerVitals {
                    health: 7,
                    max_health: 7,
                    ..PersistedPlayerVitals::default()
                },
            )
            .unwrap();
        database.update_player_town(1, 1).unwrap();
        let poison = PlayerCondition::new(PlayerConditionKind::Poison, 1, 7, 1).unwrap();
        database
            .replace_player_conditions(1, &BTreeMap::from([(PlayerConditionKind::Poison, poison)]))
            .unwrap();
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut stream = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let initialization = read_frame(&mut stream).unwrap();
        assert_eq!(
            initialization.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut death_records = 0;
        for _ in 0..3 {
            let frame = read_frame(&mut stream).unwrap();
            if frame.0 == vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_DEATH] {
                death_records += 1;
            }
        }
        assert_eq!(death_records, 1);

        game.shutdown().unwrap();
        let reloaded = EngineDatabase::open(&database_path).unwrap();
        let character = reloaded
            .characters_for_account(account_id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert!(character.respawn_state.dead);
        assert_eq!(character.vitals.health, 0);
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn serves_a_native_empty_world_and_normal_cardinal_movement() {
        let database_path = database_path("native-empty-world");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        database
            .update_player_vitals(
                1,
                forgotten_persistence::PlayerVitals {
                    health: 95,
                    max_health: 150,
                    mana: 42,
                    max_mana: 50,
                    capacity: 32_000,
                    magic_level: 4,
                },
            )
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers
            .insert(
                PlayerContainer::new(
                    2,
                    ItemInstance::new(1988, 1).unwrap(),
                    "Backpack",
                    false,
                    20,
                )
                .unwrap(),
            )
            .unwrap();
        database.replace_player_containers(1, &containers).unwrap();
        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                1988,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 1988,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        native_config.item_presentation_catalog = Some(Arc::new(catalog));
        let empty_world = native_config.empty_world.as_mut().unwrap();
        empty_world.outfit_first_look_type = 128;
        empty_world.outfit_last_look_type = 131;
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 102,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 220,
                health_percent: 100,
                direction: 2,
            }])
            .unwrap(),
        ));
        Arc::get_mut(native_config.world_map.as_mut().unwrap())
            .unwrap()
            .set_tile(
                Position {
                    x: 103,
                    y: 101,
                    z: 7,
                },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: false,
                },
            )
            .unwrap();
        Arc::get_mut(native_config.world_map.as_mut().unwrap())
            .unwrap()
            .set_tile(
                Position {
                    x: 101,
                    y: 99,
                    z: 7,
                },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: false,
                },
            )
            .unwrap();
        Arc::get_mut(native_config.world_map.as_mut().unwrap())
            .unwrap()
            .set_tile_items(
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                vec![forgotten_core::WorldMapItem {
                    server_id: 1988,
                    client_thing_id: Some(1988),
                    count: 1,
                    action_id: None,
                    unique_id: None,
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            )
            .unwrap();
        Arc::get_mut(native_config.world_map.as_mut().unwrap())
            .unwrap()
            .set_tile_items(
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                vec![forgotten_core::WorldMapItem {
                    server_id: 1988,
                    client_thing_id: Some(1988),
                    count: 1,
                    action_id: None,
                    unique_id: None,
                    text: None,
                    description: None,
                    teleport_destination: Some(Position {
                        x: 110,
                        y: 110,
                        z: 7,
                    }),
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            )
            .unwrap();
        let game = start_native_otclient_game(native_config, &database_path).unwrap();

        let mut stream = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let initialization = read_frame(&mut stream).unwrap();
        assert_eq!(
            initialization.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        assert_eq!(
            initialization.0[8],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(initialization
            .0
            .windows(6)
            .any(|window| window == b"Knight"));
        assert!(initialization.0.windows(3).any(|window| window == b"Rat"));
        assert!(initialization
            .0
            .contains(&forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_STATS));
        assert!(initialization
            .0
            .contains(&forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_SKILLS));
        assert!(initialization.0.windows(4).any(|window| {
            window
                == [
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_MODES,
                    1,
                    0,
                    0,
                ]
        }));
        let expected_stats = [
            forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_STATS,
            95,
            0,
            150,
            0,
            0,
            125,
            36,
            19,
            0,
            0,
            8,
            0,
            0,
            42,
            0,
            50,
            0,
            4,
            0,
            0,
        ];
        assert!(initialization
            .0
            .windows(expected_stats.len())
            .any(|window| window == expected_stats));
        let expected_backpack = vec![
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER,
            2,
            196,
            7,
            8,
            0,
            b'B',
            b'a',
            b'c',
            b'k',
            b'p',
            b'a',
            b'c',
            b'k',
            20,
            0,
            0,
        ];
        assert_eq!(read_frame(&mut stream).unwrap().0, expected_backpack);
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH,
                1,
                0,
                0,
                64,
                100,
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_LOOK_MAP,
                100,
                0,
                100,
                0,
                7,
                196,
                7,
                0,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE,
                forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_STATUS_DEFAULT,
                30,
                0,
                b'Y',
                b'o',
                b'u',
                b' ',
                b's',
                b'e',
                b'e',
                b' ',
                b'i',
                b't',
                b'e',
                b'm',
                b' ',
                b'#',
                b'1',
                b'9',
                b'8',
                b'8',
                b' ',
                b'(',
                b'c',
                b'o',
                b'u',
                b'n',
                b't',
                b':',
                b' ',
                b'1',
                b')',
                b'.',
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_LOOK_CREATURE,
                1,
                0,
                0,
                64,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE,
                forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_STATUS_DEFAULT,
                12,
                0,
                b'Y',
                b'o',
                b'u',
                b' ',
                b's',
                b'e',
                b'e',
                b' ',
                b'R',
                b'a',
                b't',
                b'.',
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES,
                3,
                1,
                1,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_MODES,
                3,
                1,
                1,
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES,
                3,
                1,
                1,
            ]),
        )
        .unwrap();
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_QUEST_LOG, 0, 0]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_UP_ARROW_CONTAINER,
                2,
            ]),
        )
        .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        assert!(matches!(
            read_frame(&mut stream),
            Err(HostError::Io(error))
                if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock)
        ));
        stream.set_read_timeout(None).unwrap();

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_CLOSE_CONTAINER,
                2,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CLOSE_CONTAINER, 2]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_UPDATE_CONTAINER,
                2,
            ]),
        )
        .unwrap();
        assert_eq!(read_frame(&mut stream).unwrap().0, expected_backpack);

        let heartbeat = read_frame(&mut stream).unwrap();
        assert_eq!(
            heartbeat.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_PING]
        );
        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING_BACK]),
        )
        .unwrap();
        write_frame(&mut stream, &Frame(vec![0xa0, 1, 0, 1])).unwrap();
        write_frame(&mut stream, &Frame(vec![0x1d])).unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_MODES,
                1,
                0,
                1,
            ]
        );
        let ping_back = read_frame(&mut stream).unwrap();
        assert_eq!(ping_back.0, vec![0x1d]);

        let auto_walk_started = Instant::now();
        write_frame(&mut stream, &Frame(vec![0x64, 2, 1, 3])).unwrap();
        let auto_walk_east = read_frame(&mut stream).unwrap();
        assert!(auto_walk_started.elapsed() >= Duration::from_millis(500));
        assert_eq!(&auto_walk_east.0[1..7], &[100, 0, 100, 0, 7, 1]);
        assert_eq!(&auto_walk_east.0[7..12], &[101, 0, 100, 0, 7]);
        let auto_walk_edge = read_frame(&mut stream).unwrap();
        assert_eq!(
            auto_walk_edge.0[0],
            NativeOtClientCardinalDirection::East.protocol_direction() + 0x65
        );
        assert_ne!(
            auto_walk_edge.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        let replacement_started = Instant::now();
        write_frame(&mut stream, &Frame(vec![0x64, 1, 7])).unwrap();
        write_frame(&mut stream, &Frame(vec![0x64, 1, 5])).unwrap();
        let latest_path_movement = read_frame(&mut stream).unwrap();
        assert!(replacement_started.elapsed() >= Duration::from_millis(500));
        assert_eq!(&latest_path_movement.0[1..7], &[101, 0, 100, 0, 7, 1]);
        assert_eq!(&latest_path_movement.0[7..12], &[100, 0, 100, 0, 7]);
        let latest_path_edge = read_frame(&mut stream).unwrap();
        assert_eq!(latest_path_edge.0[0], 0x68);
        write_frame(&mut stream, &Frame(vec![0x67])).unwrap();
        let manual_movement = read_frame(&mut stream).unwrap();
        assert_eq!(&manual_movement.0[1..7], &[100, 0, 100, 0, 7, 1]);
        assert_eq!(&manual_movement.0[7..12], &[100, 0, 101, 0, 7]);
        let manual_edge = read_frame(&mut stream).unwrap();
        assert_eq!(
            manual_edge.0[0],
            NativeOtClientCardinalDirection::South.protocol_direction() + 0x65
        );
        assert_ne!(
            manual_edge.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_SELECT_TARGET,
                1,
                0,
                0,
                64,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH,
                1,
                0,
                0,
                64,
                90,
            ]
        );
        let static_visibility_refresh = read_frame(&mut stream).unwrap();
        assert_eq!(
            static_visibility_refresh.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH,
                1,
                0,
                0,
                64,
                90,
            ]
        );

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let movement = read_frame(&mut stream).unwrap();
        assert_eq!(
            movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        assert_eq!(&movement.0[1..7], &[100, 0, 101, 0, 7, 1]);
        assert_eq!(&movement.0[7..12], &[101, 0, 101, 0, 7]);
        let movement_edge = read_frame(&mut stream).unwrap();
        assert_eq!(
            movement_edge.0[0],
            NativeOtClientCardinalDirection::East.protocol_direction() + 0x65
        );
        assert_ne!(
            movement_edge.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            101
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .y,
            101
        );

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let second_east = read_frame(&mut stream).unwrap();
        assert_eq!(&second_east.0[1..7], &[101, 0, 101, 0, 7, 1]);
        assert_eq!(&second_east.0[7..12], &[102, 0, 101, 0, 7]);
        let second_east_edge = read_frame(&mut stream).unwrap();
        assert_eq!(second_east_edge.0[0], 0x66);

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let blocked_movement = read_frame(&mut stream).unwrap();
        assert_eq!(
            blocked_movement.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 1]
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            102
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST,
            ]),
        )
        .unwrap();
        let diagonal_movement = read_frame(&mut stream).unwrap();
        assert_eq!(
            diagonal_movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        assert_eq!(&diagonal_movement.0[1..7], &[102, 0, 101, 0, 7, 1]);
        assert_eq!(&diagonal_movement.0[7..12], &[101, 0, 100, 0, 7]);
        let north_edge = read_frame(&mut stream).unwrap();
        assert_eq!(north_edge.0[0], 0x65);
        let west_edge = read_frame(&mut stream).unwrap();
        assert_eq!(west_edge.0[0], 0x68);
        let diagonal_position = database.characters_for_account(account_id).unwrap()[0].position;
        assert_eq!(diagonal_position.x, 101);
        assert_eq!(diagonal_position.y, 100);
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST,
            ]),
        )
        .unwrap();
        let blocked_diagonal = read_frame(&mut stream).unwrap();
        assert_eq!(
            blocked_diagonal.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 3]
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            diagonal_position
        );

        write_frame(&mut stream, &Frame(vec![0x96, 1, 2, 0, b'h', b'i'])).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        assert!(matches!(
            read_frame(&mut stream),
            Err(HostError::Io(error))
                if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock)
        ));
        stream.set_read_timeout(None).unwrap();

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT,
            ]),
        )
        .unwrap();
        let outfit_window = read_frame(&mut stream).unwrap();
        assert_eq!(
            outfit_window.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT,
                128,
                0,
                0,
                0,
                0,
                128,
                131,
            ]
        );
        assert!(!outfit_window.0.contains(&0xaa));
        assert!(!outfit_window.0.contains(&0xb4));

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT,
                129,
                1,
                2,
                3,
                4,
            ]),
        )
        .unwrap();
        let applied_outfit = read_frame(&mut stream).unwrap();
        assert_eq!(
            applied_outfit.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_OUTFIT,
                1,
                0,
                0,
                16,
                129,
                1,
                2,
                3,
                4,
            ]
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].outfit,
            PlayerOutfit {
                look_type: 129,
                head: 1,
                body: 2,
                legs: 3,
                feet: 4,
            }
        );
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT,
                132,
                5,
                6,
                7,
                8,
            ]),
        )
        .unwrap();
        let rejected_outfit = read_frame(&mut stream).unwrap();
        assert_eq!(
            rejected_outfit.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_OUTFIT,
                1,
                0,
                0,
                16,
                129,
                1,
                2,
                3,
                4,
            ]
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].outfit,
            PlayerOutfit {
                look_type: 129,
                head: 1,
                body: 2,
                legs: 3,
                feet: 4,
            }
        );
        write_frame(
            &mut stream,
            &Frame(vec![0x78, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]),
        )
        .unwrap();
        write_frame(&mut stream, &Frame(vec![0xa1, 1, 0, 0, 0])).unwrap();
        write_frame(&mut stream, &Frame(vec![0x69])).unwrap();
        let cancelled = read_frame(&mut stream).unwrap();
        assert_eq!(
            cancelled.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 3]
        );
        write_frame(&mut stream, &Frame(vec![0x71])).unwrap();
        let turned = read_frame(&mut stream).unwrap();
        assert_eq!(
            turned.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 2]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                101,
                0,
                100,
                0,
                7,
                196,
                7,
                0,
                0,
            ]),
        )
        .unwrap();
        let teleport_viewport = read_frame(&mut stream).unwrap();
        assert_eq!(
            teleport_viewport.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(&teleport_viewport.0[1..6], &[110, 0, 110, 0, 7]);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            Position {
                x: 110,
                y: 110,
                z: 7,
            }
        );

        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_player_position_persists_across_an_orderly_relog() {
        let database_path = database_path("native-position-relog");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut first = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut first,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let first_initialization = read_frame(&mut first).unwrap();
        assert_eq!(&first_initialization.0[9..14], &[100, 0, 100, 0, 7]);

        write_frame(
            &mut first,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_EAST]),
        )
        .unwrap();
        let movement = read_frame(&mut first).unwrap();
        assert_eq!(
            movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        let _map_step = read_frame(&mut first).unwrap();
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            Position {
                x: 101,
                y: 100,
                z: 7,
            }
        );
        write_frame(
            &mut first,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_LEAVE_GAME]),
        )
        .unwrap();
        drop(first);

        let mut second = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut second,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let second_initialization = read_frame(&mut second).unwrap();
        assert_eq!(&second_initialization.0[9..14], &[101, 0, 100, 0, 7]);

        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_player_position_persists_across_an_abrupt_disconnect_relog() {
        let database_path = database_path("native-position-disconnect-relog");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut first = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut first,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut first).unwrap();
        write_frame(
            &mut first,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_EAST]),
        )
        .unwrap();
        let _movement = read_frame(&mut first).unwrap();
        let _map_step = read_frame(&mut first).unwrap();
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            Position {
                x: 101,
                y: 100,
                z: 7,
            }
        );
        drop(first);

        let mut relog = None;
        for _ in 0..20 {
            let mut candidate = TcpStream::connect(game.local_addr()).unwrap();
            write_frame(
                &mut candidate,
                &native_game_request(
                    account_id.try_into().unwrap(),
                    "Knight",
                    "correct horse battery staple",
                ),
            )
            .unwrap();
            let initialization = read_frame(&mut candidate).unwrap();
            if initialization.0.first()
                == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_ERROR)
            {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            relog = Some((candidate, initialization));
            break;
        }
        let (_second, initialization) =
            relog.expect("native disconnect cleanup did not release relog");
        assert_eq!(&initialization.0[9..14], &[101, 0, 100, 0, 7]);

        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn static_creature_occupancy_cancels_native_player_movement() {
        let database_path = database_path("native-static-occupancy");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }])
            .unwrap(),
        ));
        let game = start_native_otclient_game(native_config, &database_path).unwrap();

        let mut stream = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let initialization = read_frame(&mut stream).unwrap();
        assert!(initialization.0.windows(3).any(|window| window == b"Rat"));
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH,
                1,
                0,
                0,
                64,
                100,
            ]
        );
        let heartbeat = read_frame(&mut stream).unwrap();
        assert_eq!(
            heartbeat.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_PING]
        );
        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING_BACK]),
        )
        .unwrap();
        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let blocked = read_frame(&mut stream).unwrap();
        assert_eq!(
            blocked.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 2]
        );
        let character = database
            .characters_for_account(account_id)
            .unwrap()
            .remove(0);
        assert_eq!(
            character.position,
            Position {
                x: 100,
                y: 100,
                z: 7,
            }
        );

        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn shared_target_step_refreshes_native_visibility_only_after_a_real_move() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let shared = SharedNativeWorld::from_static_spawns(Some(
            &FeTfsStaticSpawnCollection::new(vec![creature]).unwrap(),
        ))
        .unwrap();
        let target_position = Position {
            x: 103,
            y: 100,
            z: 7,
        };
        assert_eq!(
            shared
                .register_player_at_available_position(
                    Player {
                        id: 101,
                        account_id: 1,
                        name: "Knight".into(),
                        position: target_position,
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap(),
            target_position
        );
        shared
            .lock()
            .unwrap()
            .select_static_creature_target(creature_id, 4)
            .unwrap();
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(target_position),
            player_level: 8,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: forgotten_core::PlayerSkills::default(),
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let epoch_before = shared.visibility_epoch();
        let (outcome, refresh) = step_shared_native_static_creature_toward_target_and_refresh(
            &profile,
            &snapshot,
            &shared,
            101,
            &map,
            creature_id,
        )
        .unwrap();
        assert_eq!(
            outcome,
            StaticCreatureTargetStepOutcome::Moved {
                target_player_id: 101,
                direction: CardinalDirection::East,
                from: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                to: Position {
                    x: 102,
                    y: 100,
                    z: 7,
                },
            }
        );
        let refresh = refresh.expect("a real target step must refresh the map");
        assert_eq!(
            refresh.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(refresh.0.windows(3).any(|window| window == b"Rat"));
        assert_eq!(shared.visibility_epoch(), epoch_before + 1);

        let (adjacent, refresh) = step_shared_native_static_creature_toward_target_and_refresh(
            &profile,
            &snapshot,
            &shared,
            101,
            &map,
            creature_id,
        )
        .unwrap();
        assert_eq!(
            adjacent,
            StaticCreatureTargetStepOutcome::AlreadyAdjacent {
                target_player_id: 101
            }
        );
        assert!(refresh.is_none());
        assert_eq!(shared.visibility_epoch(), epoch_before + 1);
    }

    #[test]
    fn server_owned_static_creature_move_refreshes_native_visibility() {
        let map = native_world_map();
        let creature = forgotten_core::FeTfsStaticEntity {
            id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: forgotten_core::PlayerSkills::default(),
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let frame = move_native_static_creature_and_refresh(
            &profile,
            &snapshot,
            &mut world,
            &map,
            NATIVE_OTCLIENT_PLAYER_ID_END + 1,
            CardinalDirection::East,
        )
        .unwrap();
        assert_eq!(
            frame.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(frame.0.windows(3).any(|window| window == b"Rat"));
        assert_eq!(
            world
                .static_creature(NATIVE_OTCLIENT_PLAYER_ID_END + 1)
                .unwrap()
                .position,
            Position {
                x: 102,
                y: 100,
                z: 7,
            }
        );
    }

    #[test]
    fn static_creature_reset_refreshes_only_when_an_entity_reactivates() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        world.deactivate_static_creature(creature_id).unwrap();
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: forgotten_core::PlayerSkills::default(),
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;

        let (summary, refresh) =
            reset_native_static_creatures_and_refresh(&profile, &snapshot, &mut world, &map)
                .unwrap();
        assert_eq!(summary.reactivated, 1);
        let refresh = refresh.expect("a reactivated entity must refresh the map");
        assert_eq!(
            refresh.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(refresh.0.windows(3).any(|window| window == b"Rat"));
        assert!(world.static_creature_lifecycle(creature_id).unwrap().active);

        let (unchanged, refresh) =
            reset_native_static_creatures_and_refresh(&profile, &snapshot, &mut world, &map)
                .unwrap();
        assert_eq!(unchanged.reactivated, 0);
        assert!(refresh.is_none());
    }

    #[test]
    fn opt_in_static_target_pursuit_moves_once_and_refreshes_only_on_a_real_step() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }])
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: Position {
                        x: 103,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let visibility_epoch = shared.visibility_epoch();
        assert_eq!(
            shared
                .pursue_static_creature_targets_once(&map, StaticTargetPursuitPolicy::Disabled)
                .unwrap(),
            StaticTargetPursuitSummary::default()
        );
        assert_eq!(shared.visibility_epoch(), visibility_epoch);
        assert_eq!(
            shared
                .pursue_static_creature_targets_once(
                    &map,
                    StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range: 4 },
                )
                .unwrap(),
            StaticTargetPursuitSummary {
                examined_static_creatures: 1,
                changed_static_targets: 1,
                moved_static_creatures: 1,
            }
        );
        assert_eq!(shared.visibility_epoch(), visibility_epoch + 1);
        assert_eq!(
            shared
                .lock()
                .unwrap()
                .static_creature(creature_id)
                .unwrap()
                .position,
            Position {
                x: 102,
                y: 100,
                z: 7,
            }
        );
        assert_eq!(
            shared
                .pursue_static_creature_targets_once(
                    &map,
                    StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range: 4 },
                )
                .unwrap(),
            StaticTargetPursuitSummary {
                examined_static_creatures: 1,
                changed_static_targets: 0,
                moved_static_creatures: 0,
            }
        );
        assert_eq!(shared.visibility_epoch(), visibility_epoch + 1);
    }

    #[test]
    fn opt_in_shared_heartbeat_acquires_static_targets_without_visibility_or_behavior() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }])
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: Position {
                        x: 103,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let visibility_epoch = shared.visibility_epoch();
        assert_eq!(
            advance_native_shared_world_heartbeat(&shared, 1).unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 1,
                reactivated_static_creatures: 0,
                changed_static_targets: 0,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
            }
        );
        assert_eq!(
            shared
                .lock()
                .unwrap()
                .static_creature_target(creature_id)
                .unwrap(),
            None
        );
        assert_eq!(
            advance_native_shared_world_heartbeat_with_target_policy(
                &shared,
                1,
                StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 4 },
            )
            .unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 2,
                reactivated_static_creatures: 0,
                changed_static_targets: 1,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
            }
        );
        assert_eq!(
            shared
                .lock()
                .unwrap()
                .static_creature_target(creature_id)
                .unwrap(),
            Some(101)
        );
        assert_eq!(shared.visibility_epoch(), visibility_epoch);
        assert_eq!(
            shared
                .acquire_static_creature_targets(
                    StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 4 },
                )
                .unwrap(),
            StaticTargetAcquisitionSummary {
                examined_static_creatures: 1,
                changed_static_targets: 0,
            }
        );
        assert!(matches!(
            shared.acquire_static_creature_targets(
                StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 0 },
            ),
            Err(HostError::Core(
                forgotten_core::CoreError::InvalidStaticCreatureTargetRange(0)
            ))
        ));
        assert_eq!(shared.visibility_epoch(), visibility_epoch);
    }

    #[test]
    fn opt_in_shared_heartbeat_applies_static_target_damage_only_when_enabled() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let shared = SharedNativeWorld::from_static_spawns(Some(
            &FeTfsStaticSpawnCollection::new(vec![creature]).unwrap(),
        ))
        .unwrap();
        shared
            .register_player_at_available_position_with_vitals(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                PlayerVitals {
                    health: 5,
                    max_health: 5,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();

        assert_eq!(
            advance_native_shared_world_heartbeat_with_static_target_policies(
                &shared,
                1,
                StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 1 },
                StaticTargetAttackPolicy::Disabled,
                Some(&map),
            )
            .unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 1,
                reactivated_static_creatures: 0,
                changed_static_targets: 1,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
            }
        );
        assert_eq!(shared.player_vitals(101).unwrap().health, 5);
        assert_eq!(shared.vitals_epoch(), 0);

        assert_eq!(
            advance_native_shared_world_heartbeat_with_static_target_policies(
                &shared,
                1,
                StaticTargetAcquisitionPolicy::Disabled,
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 2 },
                Some(&map),
            )
            .unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 2,
                reactivated_static_creatures: 0,
                changed_static_targets: 0,
                static_target_attacks: 1,
                static_target_attack_player_ids: BTreeSet::from([101]),
            }
        );
        assert_eq!(shared.player_vitals(101).unwrap().health, 3);
        assert_eq!(shared.vitals_epoch(), 1);
        assert!(matches!(
            shared.attack_static_creature_targets_once(
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 0 },
                &map,
            ),
            Err(HostError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn native_shared_heartbeat_reactivates_static_creatures_only_after_interval() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let collection = FeTfsStaticSpawnCollection::with_respawn_intervals(
            vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                position: Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                look_type: 21,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
                addons: 0,
                speed: 134,
                health_percent: 100,
                direction: 2,
            }],
            std::collections::BTreeMap::from([(creature_id, 2)]),
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&collection)).unwrap();
        shared
            .lock()
            .unwrap()
            .deactivate_static_creature(creature_id)
            .unwrap();
        let epoch_before = shared.visibility_epoch();
        assert_eq!(
            advance_native_shared_world_heartbeat(&shared, 1).unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 1,
                reactivated_static_creatures: 0,
                changed_static_targets: 0,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
            }
        );
        assert_eq!(shared.visibility_epoch(), epoch_before);
        assert_eq!(
            advance_native_shared_world_heartbeat(&shared, 1).unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 2,
                reactivated_static_creatures: 1,
                changed_static_targets: 0,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
            }
        );
        assert_eq!(shared.visibility_epoch(), epoch_before + 1);
    }

    #[test]
    fn opt_in_static_creature_policy_moves_and_refreshes_native_visibility() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            position: Position {
                x: 101,
                y: 100,
                z: 7,
            },
            look_type: 21,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
            addons: 0,
            speed: 134,
            health_percent: 100,
            direction: 2,
        };
        let mut world = WorldState::default();
        world
            .install_static_creatures(&FeTfsStaticSpawnCollection::new(vec![creature]).unwrap())
            .unwrap();
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: NATIVE_OTCLIENT_PLAYER_ID_START,
            player_name: "Knight".into(),
            player_position: NativeOtClientPosition {
                x: 100,
                y: 100,
                z: 7,
            },
            player_level: 8,
            player_experience: 0,
            player_vitals: NativeOtClientPlayerVitals::default(),
            player_skills: forgotten_core::PlayerSkills::default(),
            ground_thing_id: 102,
            player_look_type: 128,
            player_direction: NativeOtClientCardinalDirection::South.protocol_direction(),
            player_speed: 220,
            server_beat: 50,
        };
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let (batch, refresh) = apply_native_static_creature_policy_and_refresh(
            &profile,
            &snapshot,
            &mut world,
            &map,
            StaticCreatureDecisionPolicy::ClockwiseAdjacent,
        )
        .unwrap();
        assert_eq!(batch.decisions.len(), 1);
        let refresh = refresh.expect("an applied move must refresh the map");
        assert_eq!(
            refresh.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(refresh.0.windows(3).any(|window| window == b"Rat"));
        assert_eq!(
            world.static_creature(creature_id).unwrap().position,
            Position {
                x: 102,
                y: 100,
                z: 7,
            }
        );
        let (disabled, refresh) = apply_native_static_creature_policy_and_refresh(
            &profile,
            &snapshot,
            &mut world,
            &map,
            StaticCreatureDecisionPolicy::Disabled,
        )
        .unwrap();
        assert!(disabled.decisions.is_empty());
        assert!(refresh.is_none());
    }

    #[test]
    fn completes_a_challenge_bound_game_session_and_returns_a_feature_gate() {
        let database_path = database_path("game-session");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("admin", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let key = Arc::new(LegacyRsaPrivateKey::generate().unwrap());
        let host =
            start_game_session(game_session_config(Arc::clone(&key)), &database_path).unwrap();
        let mut stream = TcpStream::connect(host.local_addr()).unwrap();
        let challenge = read_frame(&mut stream).unwrap();
        assert_eq!(
            challenge.0[0],
            forgotten_protocol::LEGACY_74_GAME_CHALLENGE_OPCODE
        );
        let challenge = forgotten_protocol::Legacy74GameChallenge {
            timestamp: u32::from_le_bytes(challenge.0[1..5].try_into().unwrap()),
            random: challenge.0[5],
        };
        let bootstrap = forgotten_protocol::Legacy74GameSessionBootstrap {
            xtea_key: [1, 2, 3, 4],
            request: forgotten_protocol::Legacy74GameSessionRequest {
                client_version: 740,
                account_name: "admin".into(),
                password: "correct horse battery staple".into(),
                character_name: "Knight".into(),
                challenge,
            },
        };
        let request = forgotten_protocol::encode_legacy_74_game_session_bootstrap_for_harness(
            &key, &bootstrap,
        )
        .unwrap();
        write_frame(&mut stream, &request).unwrap();
        let response = read_frame(&mut stream).unwrap();
        let response =
            forgotten_protocol::xtea_decrypt_packet(&response.0, bootstrap.xtea_key).unwrap();
        assert_eq!(
            response[0],
            forgotten_protocol::LEGACY_74_GAME_SESSION_READY_OPCODE
        );
        let offer = read_frame(&mut stream).unwrap();
        let offer = forgotten_protocol::xtea_decrypt_packet(&offer.0, bootstrap.xtea_key).unwrap();
        assert_eq!(offer[0], forgotten_protocol::FE_OTCLIENT_EXTENDED_OPCODE);
        let acknowledgement = forgotten_protocol::encode_fe_otclient_capability_ack_for_harness();
        let acknowledgement =
            forgotten_protocol::xtea_encrypt_packet(&acknowledgement.0, bootstrap.xtea_key)
                .unwrap();
        write_frame(&mut stream, &Frame(acknowledgement)).unwrap();
        let world = read_frame(&mut stream).unwrap();
        let world = forgotten_protocol::xtea_decrypt_packet(&world.0, bootstrap.xtea_key).unwrap();
        assert_eq!(world[0], forgotten_protocol::FE_OTCLIENT_EXTENDED_OPCODE);
        assert!(world
            .windows(b"fe.example.test:443".len())
            .any(|window| window == b"fe.example.test:443"));
        assert!(world
            .windows(b"position=100,100,7".len())
            .any(|window| window == b"position=100,100,7"));
        assert!(world
            .windows(b"empty-gated".len())
            .any(|window| window == b"empty-gated"));
        let initial_viewport = read_frame(&mut stream).unwrap();
        let initial_viewport =
            forgotten_protocol::xtea_decrypt_packet(&initial_viewport.0, bootstrap.xtea_key)
                .unwrap();
        assert!(initial_viewport
            .windows(b"fe.viewport.v1;tick=0".len())
            .any(|window| window == b"fe.viewport.v1;tick=0"));
        let movement = forgotten_protocol::encode_fe_otclient_move_request_for_harness(
            forgotten_core::CardinalDirection::East,
        );
        let movement =
            forgotten_protocol::xtea_encrypt_packet(&movement.0, bootstrap.xtea_key).unwrap();
        write_frame(&mut stream, &Frame(movement)).unwrap();
        let acknowledgement = read_frame(&mut stream).unwrap();
        let acknowledgement =
            forgotten_protocol::xtea_decrypt_packet(&acknowledgement.0, bootstrap.xtea_key)
                .unwrap();
        assert!(acknowledgement
            .windows(b"fe.move.ack.v1;tick=1".len())
            .any(|window| window == b"fe.move.ack.v1;tick=1"));
        assert!(acknowledgement
            .windows(b"to=101,100,7".len())
            .any(|window| window == b"to=101,100,7"));
        let tick = read_frame(&mut stream).unwrap();
        let tick = forgotten_protocol::xtea_decrypt_packet(&tick.0, bootstrap.xtea_key).unwrap();
        assert!(tick
            .windows(b"fe.tick.v1;tick=1".len())
            .any(|window| window == b"fe.tick.v1;tick=1"));
        let viewport = read_frame(&mut stream).unwrap();
        let viewport =
            forgotten_protocol::xtea_decrypt_packet(&viewport.0, bootstrap.xtea_key).unwrap();
        assert!(viewport
            .windows(b"center=101,100,7".len())
            .any(|window| window == b"center=101,100,7"));
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0]
                .position
                .x,
            101
        );
        host.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }
}

#[cfg(test)]
mod native_timing_tests {
    use super::{native_autowalk_step_delay, NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY};
    use std::time::Duration;

    #[test]
    fn auto_walk_delay_scales_with_player_speed_and_server_beat() {
        assert_eq!(
            native_autowalk_step_delay(220, 50),
            Duration::from_millis(681)
        );
        assert!(native_autowalk_step_delay(440, 50) < native_autowalk_step_delay(220, 50));
        assert_eq!(
            native_autowalk_step_delay(1, 50),
            NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY
        );
        assert_eq!(
            native_autowalk_step_delay(1000, 750),
            Duration::from_millis(750)
        );
    }
}

#[cfg(test)]
mod native_diagnostics_tests {
    use super::{
        native_action_diagnostic_summary, native_classic_viewport_contains,
        native_diagnostic_record, NativeOtClientGameAction,
    };
    use forgotten_core::Position;
    use forgotten_protocol::NativeOtClientCardinalDirection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn diagnostic_records_are_strictly_opt_in() {
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7175);
        assert!(native_diagnostic_record(false, peer, "action=ping").is_none());
        assert_eq!(
            native_diagnostic_record(true, peer, "action=ping").as_deref(),
            Some("> Native OTCv8 trace peer=127.0.0.1:7175 action=ping")
        );
    }

    #[test]
    fn action_summaries_report_metadata_without_chat_text_or_raw_bytes() {
        assert_eq!(
            native_action_diagnostic_summary(&NativeOtClientGameAction::CardinalMove(
                NativeOtClientCardinalDirection::North
            )),
            "action=cardinal-move direction=North"
        );
        let secret_message = "correct horse battery staple".to_owned();
        let talk_summary =
            native_action_diagnostic_summary(&NativeOtClientGameAction::Talk(secret_message));
        assert_eq!(talk_summary, "action=talk text-bytes=28");
        assert!(!talk_summary.contains("correct"));
        assert!(!talk_summary.contains("68 6f 72"));
        assert_eq!(
            native_action_diagnostic_summary(&NativeOtClientGameAction::LookMap {
                position: forgotten_protocol::NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                thing_id: 102,
                stack_position: 3,
            }),
            "action=look-map position=100,101,7 thing-id=102 stack-position=3"
        );
        let battle_window_summary =
            native_action_diagnostic_summary(&NativeOtClientGameAction::UseItemOnCreature {
                source_position: forgotten_protocol::NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                source_client_thing_id: 102,
                source_stack_position: 3,
                target_creature_id: 0x4000_0001,
            });
        assert_eq!(
            battle_window_summary,
            "action=use-item-on-creature source=100,101,7 source-client-thing-id=102 source-stack-position=3 target-creature-id=1073741825"
        );
        assert!(!battle_window_summary.contains("["));
        let rotate_summary =
            native_action_diagnostic_summary(&NativeOtClientGameAction::RotateItem {
                position: forgotten_protocol::NativeOtClientPosition {
                    x: 100,
                    y: 101,
                    z: 7,
                },
                client_thing_id: 102,
                stack_position: 3,
            });
        assert_eq!(
            rotate_summary,
            "action=rotate-item position=100,101,7 client-thing-id=102 stack-position=3"
        );
        assert!(!rotate_summary.contains("["));
    }

    #[test]
    fn classic_creature_inspection_viewport_matches_the_encoded_map_window() {
        let observer = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        assert!(native_classic_viewport_contains(
            observer,
            Position { x: 92, y: 94, z: 7 }
        ));
        assert!(native_classic_viewport_contains(
            observer,
            Position {
                x: 109,
                y: 107,
                z: 7,
            }
        ));
        assert!(!native_classic_viewport_contains(
            observer,
            Position {
                x: 91,
                y: 100,
                z: 7,
            }
        ));
        assert!(!native_classic_viewport_contains(
            observer,
            Position {
                x: 100,
                y: 108,
                z: 7,
            }
        ));
        assert!(!native_classic_viewport_contains(
            observer,
            Position {
                x: 100,
                y: 100,
                z: 6,
            }
        ));
    }
}
