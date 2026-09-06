//! Persistent TCP host and bounded diagnostic session foundation for Forgotten Engine.
//!
//! This crate deliberately exposes an engine probe protocol, not a claimed Tibia wire protocol.

pub mod operator;

mod bank;
mod frames;
mod gm_commands;
mod inspection;
mod map_transfers;
mod native_diagnostics;
mod npc_shop;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedPublicChatEvent {
    speaker_name: String,
    speaker_position: NativeOtClientPosition,
    channel_id: Option<u16>,
    private: bool,
    /// Classic server-talk mode for delivery: 1 say, 2 whisper, 3 yell. Channel and private
    /// events retain their existing dedicated encoders and ignore this field.
    talk_mode: u8,
    text: String,
}
pub(crate) use npc_shop::{
    complete_native_player_quest, deliver_native_npc_shop_windows, give_items_to_player,
    handle_native_depot_open, handle_native_shop_keyword, insert_units_into_containers,
};
mod native_render;
mod session_handlers;
mod session_loop;
mod shared_native_map;
mod trade;
pub(crate) use frames::*;
pub(crate) use frames::*;
mod world_chat;
mod world_combat;
mod world_equipment;
mod world_interaction;
mod world_party;
mod world_registration;
mod world_static_creatures;

pub(crate) use bank::{
    handle_native_bank_keyword, native_carried_weight, NATIVE_BANK_NPC_RANGE_TILES,
};
pub(crate) use gm_commands::handle_native_gm_talkaction;
pub(crate) use inspection::{
    encode_shared_native_world_viewport, native_classic_weight_description,
    native_item_inspection_metadata_details, native_map_item_inspection_message,
    native_static_creature_health_frames, native_validated_map_item_text,
};
pub(crate) use native_diagnostics::{
    native_action_diagnostic_summary, native_diagnostic, native_diagnostic_record,
    NATIVE_GUILD_CHAT_CHANNEL_ID,
};
#[cfg(test)]
pub(crate) use native_render::{
    NativeRenderPreparationPool, NativeRenderPreparationRequest, NativeRenderPreparationWorker,
    NativeRenderPublication, NativeRenderPublicationError, MAX_NATIVE_RENDER_PUBLICATION_BATCH,
};
pub(crate) use session_handlers::*;
pub(crate) use session_loop::handle_native_otclient_game;
pub(crate) use trade::{
    deliver_native_trade_windows, handle_native_player_trade_request, handle_native_trade_accept,
    handle_native_trade_reject, resolve_native_trade_offer_item,
};

use forgotten_config::{
    DeclarativeNpcDialogueCatalog, DeclarativeShopCatalog, DeclarativeSpellCatalog,
    DeclarativeWeaponCatalog, LegacyItemSlotType, LegacyPublicChannelCatalog, QuestCatalog,
    WorldType,
};
use forgotten_core::{
    CardinalDirection, CombatAttackTiming, CombatDamageType, DeathLossPolicy, EmptyWorldManifest,
    EquipmentSlot, ExperienceAwardPolicy, FeTfsStaticSpawnCollection, ItemInstance,
    NativeItemPresentationCatalog, PartyDisplayRelation, PartySharedExperienceRules, Player,
    PlayerCombatDefense, PlayerCombatEvent, PlayerCombatEventOutcome, PlayerCondition,
    PlayerConditionKind, PlayerConditionOutcome, PlayerContainer,
    PlayerContainerStackToEquipmentOutcome, PlayerContainerToEquipmentOutcome,
    PlayerContainerToEquipmentSwapOutcome, PlayerContainers, PlayerEquipment,
    PlayerEquipmentSlotSwapOutcome, PlayerEquipmentStackToContainerOutcome,
    PlayerEquipmentToContainerOutcome, PlayerExperienceAwardOutcome, PlayerFightMode,
    PlayerFightModeState, PlayerInteractionIntent, PlayerItemUseCreatureIntent,
    PlayerItemUseCreatureOutcome, PlayerItemUseCreatureTarget, PlayerItemUseCreatureTargetOutcome,
    PlayerItemUseExIntent, PlayerItemUseExOutcome, PlayerItemUseIntent, PlayerItemUseOutcome,
    PlayerProgression, PlayerProgressionAttempts, PlayerProgressionRules,
    PlayerRegenerationOutcome, PlayerRegenerationRules, PlayerRespawnState, PlayerSkill,
    PlayerSkillTryOutcome, PlayerSpellCastOutcome, PlayerVitals, Position,
    StaticCreatureDamageOutcome, StaticCreatureDecisionBatch, StaticCreatureDecisionPolicy,
    StaticCreatureResetSummary, StaticCreatureRuntimeRestoreSummary, StaticCreatureRuntimeSnapshot,
    StaticCreatureTargetAttackOutcome, StaticCreatureTargetStepOutcome, VocationId,
    VocationLevelUpGains, WorldMap, WorldMapItem, WorldMapItemSourceIdentity,
    WorldMapSourceRevision, WorldState, MAX_COMBAT_EVENT_DAMAGE, MAX_ITEM_STACK_COUNT,
};
use forgotten_persistence::{
    EngineDatabase, MapItemCountOverrideRecord, MapItemRemovalJournal, PersistenceError,
    PlayerExperienceVitalsUpdate, PlayerFixedDeathLossSnapshot, PlayerOutfit,
    PlayerVitals as PersistedPlayerVitals, RuntimeMapItemChildRecord, RuntimeMapItemRecord,
    StaticCreatureRuntimeRecord,
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
    encode_legacy_74_game_session_ready, encode_login_error, encode_native_otclient_animated_text,
    encode_native_otclient_change_in_container, encode_native_otclient_channel_list,
    encode_native_otclient_character_list, encode_native_otclient_choose_outfit,
    encode_native_otclient_classic_vip_entry, encode_native_otclient_classic_vip_presence,
    encode_native_otclient_clear_target, encode_native_otclient_close_container,
    encode_native_otclient_close_trade, encode_native_otclient_counter_trade,
    encode_native_otclient_create_in_container, encode_native_otclient_creature_health,
    encode_native_otclient_creature_outfit, encode_native_otclient_creature_party_shield,
    encode_native_otclient_creature_skull, encode_native_otclient_creature_unpass,
    encode_native_otclient_delete_in_container, encode_native_otclient_delete_inventory,
    encode_native_otclient_distance_effect, encode_native_otclient_empty_quest_log,
    encode_native_otclient_failure_message, encode_native_otclient_game_announcement,
    encode_native_otclient_game_cancel_walk_facing, encode_native_otclient_game_death,
    encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players,
    encode_native_otclient_game_login_error, encode_native_otclient_game_ping,
    encode_native_otclient_game_ping_back, encode_native_otclient_login_error,
    encode_native_otclient_look_message, encode_native_otclient_magic_effect,
    encode_native_otclient_map_step_with_static_spawns_and_players,
    encode_native_otclient_map_viewport_with_static_spawns,
    encode_native_otclient_map_viewport_with_static_spawns_and_players,
    encode_native_otclient_move_creature_at, encode_native_otclient_open_container,
    encode_native_otclient_open_npc_trade, encode_native_otclient_open_public_channel,
    encode_native_otclient_own_trade, encode_native_otclient_player_goods,
    encode_native_otclient_player_modes, encode_native_otclient_player_skills,
    encode_native_otclient_player_state_bits, encode_native_otclient_player_stats,
    encode_native_otclient_private_message_from, encode_native_otclient_public_channel_say,
    encode_native_otclient_public_say, encode_native_otclient_quest_line,
    encode_native_otclient_quest_list, encode_native_otclient_read_only_text_window,
    encode_native_otclient_set_inventory, encode_native_otclient_status_message,
    encode_native_otclient_whisper, encode_native_otclient_yell, encode_status_binary,
    encode_status_metrics, encode_status_xml, generate_legacy_74_game_challenge,
    xtea_encrypt_packet, CharacterListEntry, CompatibilityProfile, EmptyWorldMovementAck, Frame,
    InitialWorldSnapshot, Legacy74GameSessionState, LegacyRsaPrivateKey,
    NativeOtClientAutoWalkDirection, NativeOtClientCardinalDirection, NativeOtClientClassicChannel,
    NativeOtClientClassicItemRecord, NativeOtClientClassicOpenContainer,
    NativeOtClientClassicOutfit, NativeOtClientClassicPartyShield,
    NativeOtClientEmptyWorldSnapshot, NativeOtClientFightMode, NativeOtClientFightModeRequest,
    NativeOtClientGameAction, NativeOtClientPlayerGood, NativeOtClientPlayerVitals,
    NativeOtClientPosition, NativeOtClientProfile, NativeOtClientShopItem, NativeOtClientTradeItem,
    NativeOtClientVisiblePlayer, OtClientEndpoint, ProtocolError, StatusPlayer, StatusRequest,
    StatusSnapshot, MAX_FRAME_SIZE, MAX_LOGIN_STRING_BYTES, NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES,
    NATIVE_OTCLIENT_MESSAGE_GM_BROADCAST, NATIVE_OTCLIENT_MESSAGE_SAY,
    NATIVE_OTCLIENT_MESSAGE_WHISPER, NATIVE_OTCLIENT_MESSAGE_YELL, NATIVE_OTCLIENT_PLAYER_ID_END,
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
const MAX_NATIVE_OTCLIENT_LOGIN_VIP_ENTRIES: usize = 128;
const MAX_NATIVE_STATIC_NPC_DIALOGUE_RANGE: u16 = 2;
const NATIVE_OTCLIENT_DEFAULT_GROUND_SPEED: u64 = 150;
const NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_NATIVE_ARMOR_MULTIPLIER_MILLI: u32 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeWorldHeartbeatOutcome {
    tick: u64,
    reactivated_static_creatures: usize,
    changed_static_targets: usize,
    static_target_attacks: usize,
    static_target_attack_player_ids: BTreeSet<u64>,
    followed_player_ids: BTreeSet<u64>,
    wandered_static_creatures: usize,
}

struct NativeHeartbeatConfig {
    pursuit_policy: StaticTargetPursuitPolicy,
    attack_policy: StaticTargetAttackPolicy,
    /// Opt-out deterministic wander movement for active static creatures (plan v49 slice 4).
    wander_policy: StaticCreatureDecisionPolicy,
    /// Wander cadence in world ticks; `0` disables wandering entirely.
    wander_every_ticks: u64,
    map_owner: Option<Arc<SharedNativeMap>>,
    database_path: PathBuf,
    death_loss_policy: DeathLossPolicy,
    progression_rules: Option<Arc<BTreeMap<VocationId, PlayerProgressionRules>>>,
    /// Configured corpse despawn delay in world-tick seconds; `0` disables decay entirely.
    corpse_despawn_seconds: u32,
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
    SelectedAdjacentFixedDamage {
        damage: u16,
    },
    /// Plan v49 slice 5 default: only creatures whose imported definition declares a bounded
    /// direct-melee range participate, and each hit cycles that declaration's min..=max values
    /// deterministically. `max_range` bounds target acquisition (aggro); adjacency is still
    /// validated per attack by the existing core primitive.
    DeclaredMeleeCycling {
        max_range: u8,
    },
}

/// Derives the acquisition range required by every enabled static-creature policy. Pursuit must
/// not depend on direct damage to discover a target; when both policies are enabled, the widest
/// bounded range permits pursuit while the existing attack primitive still validates adjacency.
fn static_target_acquisition_policy(
    pursuit_policy: StaticTargetPursuitPolicy,
    attack_policy: StaticTargetAttackPolicy,
) -> StaticTargetAcquisitionPolicy {
    let pursuit_range = match pursuit_policy {
        StaticTargetPursuitPolicy::Disabled => None,
        StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range } => Some(max_range),
    };
    let attack_range = match attack_policy {
        StaticTargetAttackPolicy::Disabled => None,
        StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { .. } => Some(1),
        StaticTargetAttackPolicy::DeclaredMeleeCycling { max_range } => Some(max_range),
    };
    match (pursuit_range, attack_range) {
        (Some(pursuit_range), Some(attack_range)) => {
            StaticTargetAcquisitionPolicy::NearestLivingPlayer {
                max_range: pursuit_range.max(attack_range),
            }
        }
        (Some(max_range), None) | (None, Some(max_range)) => {
            StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range }
        }
        (None, None) => StaticTargetAcquisitionPolicy::Disabled,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticTargetAttackSummary {
    pub examined_static_creatures: usize,
    pub cooldown_skipped_attacks: usize,
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
        StaticTargetPursuitPolicy::Disabled,
        StaticTargetAttackPolicy::Disabled,
        forgotten_core::StaticCreatureDecisionPolicy::Disabled,
        0,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn advance_native_shared_world_heartbeat_with_static_target_policies(
    shared_world: &SharedNativeWorld,
    elapsed_seconds: u16,
    target_policy: StaticTargetAcquisitionPolicy,
    pursuit_policy: StaticTargetPursuitPolicy,
    attack_policy: StaticTargetAttackPolicy,
    wander_policy: StaticCreatureDecisionPolicy,
    wander_every_ticks: u64,
    world_map: Option<&WorldMap>,
) -> Result<NativeWorldHeartbeatOutcome, HostError> {
    let tick = shared_world.advance_ticks(elapsed_seconds)?;
    let reactivated_static_creatures = shared_world.reactivate_due_static_creatures()?.reactivated;
    let mut changed_static_targets = shared_world
        .acquire_static_creature_targets(target_policy)?
        .changed_static_targets;
    let pursuit_summary = match pursuit_policy {
        StaticTargetPursuitPolicy::Disabled => StaticTargetPursuitSummary::default(),
        StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { .. } => shared_world
            .pursue_static_creature_targets_once(
                world_map.ok_or_else(|| {
                    HostError::InvalidConfiguration(
                        "static target pursuit policy requires a loaded world map".into(),
                    )
                })?,
                pursuit_policy,
            )?,
    };
    changed_static_targets += pursuit_summary.changed_static_targets;
    let static_target_attack_summary = match attack_policy {
        StaticTargetAttackPolicy::Disabled => StaticTargetAttackSummary::default(),
        StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { .. }
        | StaticTargetAttackPolicy::DeclaredMeleeCycling { .. } => shared_world
            .attack_static_creature_targets_once(
                attack_policy,
                world_map.ok_or_else(|| {
                    HostError::InvalidConfiguration(
                        "static target attack policy requires a loaded world map".into(),
                    )
                })?,
            )?,
    };
    let followed_player_ids = world_map
        .map(|map| shared_world.follow_player_targets_once(map))
        .transpose()?
        .unwrap_or_default();
    // Deterministic wander (plan v49 slice 4): every configured tick interval each active
    // creature may take one safe adjacent step via the existing occupancy-validated policy.
    // No targets, pathfinding, or randomness; movement bumps visibility so sessions refresh.
    let wandered_static_creatures = match (
        wander_policy,
        world_map,
        wander_every_ticks > 0 && tick % wander_every_ticks == 0,
    ) {
        (StaticCreatureDecisionPolicy::Disabled, _, _) => 0,
        (_, None, _) => 0,
        (_, Some(_), false) => 0,
        (policy, Some(map), true) => {
            let moved = {
                let mut world = shared_world.lock()?;
                world
                    .apply_static_creature_policy(policy, map)
                    .map_err(HostError::Core)?
            }
            .decisions
            .len();
            if moved > 0 {
                shared_world.mark_visibility_changed();
            }
            moved
        }
    };
    Ok(NativeWorldHeartbeatOutcome {
        tick,
        reactivated_static_creatures,
        changed_static_targets,
        static_target_attacks: static_target_attack_summary.applied_attacks,
        static_target_attack_player_ids: static_target_attack_summary.affected_player_ids,
        followed_player_ids,
        wandered_static_creatures,
    })
}

const NATIVE_AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

fn run_native_shared_world_heartbeat(
    shared_world: SharedNativeWorld,
    shutdown: Arc<AtomicBool>,
    config: NativeHeartbeatConfig,
) -> Result<(), HostError> {
    let mut last_tick = Instant::now();
    let mut last_auto_save = Instant::now();
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
        let target_policy =
            static_target_acquisition_policy(config.pursuit_policy, config.attack_policy);
        let world_map = config
            .map_owner
            .as_ref()
            .map(|owner| owner.render_snapshot())
            .transpose()?;
        let outcome = advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared_world,
            elapsed_seconds,
            target_policy,
            config.pursuit_policy,
            config.attack_policy,
            config.wander_policy,
            config.wander_every_ticks,
            world_map.as_deref(),
        )?;
        if !outcome.static_target_attack_player_ids.is_empty() {
            let mut database = EngineDatabase::open(&config.database_path)?;
            persist_static_target_attack_vitals(
                &mut database,
                &shared_world,
                &outcome.static_target_attack_player_ids,
                config.death_loss_policy,
                config.progression_rules.as_deref(),
            )?;
        }
        if !outcome.followed_player_ids.is_empty() {
            let database = EngineDatabase::open(&config.database_path)?;
            for player_id in outcome.followed_player_ids {
                let (player, _) = shared_world.player_and_vitals(player_id)?;
                database.update_player_position(player_id, player.position)?;
            }
        }
        // Bounded auto-save cadence: a hard kill (panel stop, power loss) loses at most one
        // interval of static runtime and player snapshot state. Most player state already
        // persists eagerly; this flush covers vitals drift and position deltas.
        if last_auto_save.elapsed() >= NATIVE_AUTOSAVE_INTERVAL {
            last_auto_save = Instant::now();
            let mut auto_save_database = EngineDatabase::open(&config.database_path)?;
            persist_static_creature_runtime_to_open_database(
                &shared_world,
                &mut auto_save_database,
            )?;
            // Party relations flush with the same bounded cadence; any party mutation
            // rebuilds the whole table from live state, so offline-stale rows are pruned
            // only after hydration had its chance at registration time.
            auto_save_database.replace_player_parties(&shared_world.party_snapshots()?)?;
            for player_id in shared_world.registered_player_ids()? {
                let (player, vitals) = shared_world.player_and_vitals(player_id)?;
                auto_save_database.update_player_position(player_id, player.position)?;
                auto_save_database.update_player_vitals(
                    player_id,
                    PersistedPlayerVitals {
                        health: vitals.health,
                        max_health: vitals.max_health,
                        mana: vitals.mana,
                        max_mana: vitals.max_mana,
                        capacity: vitals.capacity,
                        magic_level: vitals.magic_level,
                    },
                )?;
            }
        }
        if config.corpse_despawn_seconds > 0 {
            if let Some(map_owner) = config.map_owner.as_ref() {
                let now_tick = shared_world.tick()?;
                let mut database = EngineDatabase::open(&config.database_path)?;
                let removed = map_owner.remove_expired_runtime_items(&mut database, now_tick)?;
                drop(database);
                if !removed.is_empty() {
                    shared_world.mark_visibility_changed();
                }
            }
        }
    }
    Ok(())
}
const NATIVE_OTCLIENT_SHARED_CHAT_QUEUE_CAPACITY: usize = 64;
const NATIVE_OTCLIENT_SHARED_VIP_QUEUE_CAPACITY: usize = 64;
/// Audited classic whisper delivery range: same floor within one cardinal or diagonal step.
const NATIVE_CLASSIC_WHISPER_RANGE_TILES: u16 = 1;
/// Audited classic yell delivery range: same floor within 18 tiles in each axis.
const NATIVE_CLASSIC_YELL_RANGE_TILES: u16 = 18;
const NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE: u16 = 10;
/// Default corpse container server item ID for native static-monster defeats. A dedicated
/// operator mapping can replace this later; the bounded default keeps the defeat loop closed.
const NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID: u16 = 3065;
/// Bounded number of simultaneously open native corpse container windows per session.
const NATIVE_OTCLIENT_MAX_OPEN_CORPSE_WINDOWS: usize = 4;
/// Classic clients address container windows through a four-bit field, so every native corpse
/// window ID must stay inside that addressable range.
const NATIVE_OTCLIENT_CONTAINER_ADDRESSABLE_WINDOW_MAX: u8 = 0x0f;
/// Fallback display name for a corpse without operator-supplied item-name metadata.
const NATIVE_OTCLIENT_FALLBACK_CORPSE_NAME: &str = "corpse";

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
    /// Parsed TFS-style `worldType`. The native selected-player melee route uses it only to admit
    /// or reject direct player damage; skulls, zones, wars, and other PvP policy remain deferred.
    pub world_type: WorldType,
    pub advertised_game_addr: SocketAddr,
    pub max_connections: usize,
    pub session_timeout: Duration,
    /// Emits bounded session metadata only. Packet bodies and credentials are never logged.
    pub extended_diagnostics: bool,
    pub empty_world: Option<NativeOtClientEmptyWorldConfig>,
    /// Immutable startup map source. The game service creates one synchronized owner from this
    /// map and sessions/heartbeat then consume detached immutable snapshots from that owner.
    pub world_map: Option<Arc<WorldMap>>,
    /// Validated operator-supplied server-to-client item metadata. It is retained for later
    /// parser-safe inventory delivery and does not itself enable inventory packets.
    pub item_presentation_catalog: Option<Arc<NativeItemPresentationCatalog>>,
    /// Immutable validated public entries from `data/chatchannels/chatchannels.xml`. They are
    /// emitted only by the native Request Channels reply and do not create membership, messaging,
    /// scripts, moderation, persistence, guild, or party behavior.
    pub public_channel_catalog: Option<Arc<LegacyPublicChannelCatalog>>,
    /// Immutable validated `items.xml` armor values for the explicit native armor-only bridge.
    /// Shielding, weapon defense, vocation multipliers, random armor, and TFS formula parity are
    /// deliberately excluded.
    pub item_armor_by_server_id: Option<Arc<BTreeMap<u16, u16>>>,
    /// Immutable validated legacy \items.xml\ defense values applied only to the equipped
    /// left-hand (shield-hand) item inside the bounded physical mitigation bridge. Weapon-hand
    /// defense, blocking chance, and TFS formula parity remain excluded.
    pub item_shield_defense_by_server_id: Option<Arc<BTreeMap<u16, u16>>>,
    /// Optional operator-declared instant consumable effects keyed by server item ID. UseItem on
    /// owned inventory applies them directly; effects carrying a regeneration window also feed
    /// the player (slice 16).
    pub consumable_effects: Option<Arc<BTreeMap<u16, forgotten_config::ConsumableEffect>>>,
    /// Optional operator quest catalog. RequestQuestLog lists only quests the persisted player
    /// state has started, resolved through this catalog for display names.
    pub quest_catalog: Option<Arc<QuestCatalog>>,
    /// Optional operator-declared NPC shop catalog. Say keywords near a matching active NPC buy
    /// or sell bounded stacks through the durable bank balance.
    pub shop_catalog: Option<Arc<DeclarativeShopCatalog>>,
    /// Immutable validated legacy `items.xml` slot types. They are used only by the bounded
    /// native map-source-to-empty-equipment route; generic inventories, stacks, swaps, and
    /// two-handed placement remain outside this policy.
    pub item_slot_types_by_server_id: Option<Arc<BTreeMap<u16, BTreeSet<LegacyItemSlotType>>>>,
    /// Immutable validated legacy `items.xml` source weights used only to append one bounded
    /// classic weight sentence to an exact native map LookMap response. They do not enforce
    /// capacity or change item-transfer behavior.
    pub item_weight_by_server_id: Option<Arc<BTreeMap<u16, u32>>>,
    /// Immutable validated legacy `items.xml` names used only to append one bounded inspected
    /// item-name detail after exact native map LookMap validation. They do not generate articles,
    /// item descriptions, or item behavior.
    pub item_name_by_server_id: Option<Arc<BTreeMap<u16, String>>>,
    /// Immutable source OTB stackability identifiers paired with the inspection-only weight map.
    /// They do not alter FE item-stack transfer behavior.
    pub stackable_item_server_ids: Option<Arc<BTreeSet<u16>>>,
    /// Immutable legacy `speed` bonuses (BoH-style) keyed by authoritative server ID. They feed
    /// the dynamic per-player walk-speed computation for equipped boots and similar items.
    pub item_speed_bonus_by_server_id: Option<Arc<BTreeMap<u16, u16>>>,
    /// Immutable validated armor multiplier thousandths keyed by vocation. Missing vocation
    /// entries retain the deterministic `1.000` default; shielding and defense formulas remain
    /// outside this bridge.
    pub armor_multiplier_by_vocation: Option<Arc<BTreeMap<VocationId, u32>>>,
    /// Immutable display-only TFS spawn entities. No AI, combat, movement, or Lua behavior is
    /// attached at this host boundary.
    pub static_spawns: Option<Arc<FeTfsStaticSpawnCollection>>,
    /// Declared corpse sprite per lowercased creature name (plan v49 slice 6). Creatures absent
    /// from the map fall back to the bounded default corpse server id on defeat.
    /// Optional combat feedback (plan v49 slice 11): render animated damage numbers on hits.
    pub animated_damage_text_enabled: bool,
    pub corpse_server_id_by_creature_name: Option<Arc<BTreeMap<String, u16>>>,
    /// Disabled by default. When enabled, one heartbeat pass may apply bounded fixed damage from
    /// each active static creature to its already selected adjacent target. Formula, persistence,
    /// packet, loot, corpse, script, and general AI behavior remain separate and deferred.
    pub static_target_attack_policy: StaticTargetAttackPolicy,
    /// Disabled by default. When enabled, each heartbeat may use the existing deterministic
    /// nearest-living-player one-step pursuit primitive. It does not add general pathfinding,
    /// attack, loot, scripts, or NPC behavior.
    pub static_target_pursuit_policy: StaticTargetPursuitPolicy,
    /// On by default (`ClockwiseAdjacent`, plan v49 slice 4): heartbeat-driven deterministic
    /// wander where each active static creature may take one safe adjacent step per configured
    /// interval. Occupancy-validated, no targets, pathfinding, randomness, or scripts.
    pub static_creature_wander_policy: StaticCreatureDecisionPolicy,
    /// Wander cadence in world ticks (default 4); `0` disables wandering entirely.
    pub static_creature_wander_every_ticks: u64,
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
    /// Validated TFS-style global magic rate used only by the bounded scriptless declarative
    /// native spell command. Generic speech, spell words, targets, effects, runes, Lua, and
    /// complete TFS spell behavior remain separate and deferred.
    pub magic_rate: u32,
    /// Validated configured flat experience rate and optional level-stage policy. Concrete
    /// gameplay reward sources remain separate from this immutable host input.
    pub experience_award_policy: Option<Arc<ExperienceAwardPolicy>>,
    /// Explicit operator-configured shared-experience eligibility limits. Absent configuration
    /// leaves native enable requests inert; party persistence, client messages, and broad reward
    /// sources remain outside this boundary.
    pub party_shared_experience_rules: Option<PartySharedExperienceRules>,
    /// Party loot split (plan v49 slice 14): when a party member lands the killing blow, rolled
    /// loot stacks distribute round-robin across online party members' owned containers;
    /// leftovers stay in the corpse. Deterministic member order: leader first, then members by id.
    pub party_loot_split_enabled: bool,
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
    /// Optional operator-owned exact static-NPC dialogue catalog. Until the bounded proximity
    /// resolver is enabled, this validated data remains inert and cannot execute scripts.
    pub declarative_npc_dialogue_catalog: Option<Arc<DeclarativeNpcDialogueCatalog>>,
    /// Configured corpse despawn delay in authoritative world-tick seconds. `0` (the default)
    /// disables decay; a positive value expires each placed runtime corpse after the delay on a
    /// later heartbeat, removing it from the map and the durable registry together.
    pub corpse_despawn_seconds: u32,
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
    player_outfits: Arc<Mutex<BTreeMap<u64, NativeOtClientClassicOutfit>>>,
    player_directions: Arc<Mutex<BTreeMap<u64, u8>>>,
    visibility_epoch: Arc<AtomicU64>,
    vitals_epoch: Arc<AtomicU64>,
    progression_epoch: Arc<AtomicU64>,
    equipment_epoch: Arc<AtomicU64>,
    containers_epoch: Arc<AtomicU64>,
    party_epoch: Arc<AtomicU64>,
    online_players: Arc<AtomicU64>,
    chat_recipients: Arc<Mutex<BTreeMap<u64, SharedChatRecipient>>>,
    vip_presence_recipients: Arc<Mutex<BTreeMap<u64, SharedVipPresenceRecipient>>>,
    /// Operator-requested disconnects awaiting session pickup, stamped for expiry.
    pending_kicks: Arc<Mutex<BTreeMap<u64, std::time::SystemTime>>>,
    /// Players whose trade window must close at their next session drain.
    pending_trades_closed: Arc<Mutex<BTreeSet<u64>>>,
}

/// Synchronized owner for a mutable native world map. The live listener and heartbeat obtain only
/// detached immutable snapshots from this owner. Future ground-item transfers must establish the
/// documented atomic lock order with `SharedNativeWorld` before client routing is enabled.
#[derive(Debug, Clone)]
pub struct SharedNativeMap {
    pub(crate) map: Arc<Mutex<WorldMap>>,
    pub(crate) source: Arc<WorldMap>,
    pub(crate) source_item_indices: Arc<Mutex<BTreeMap<Position, Vec<u8>>>>,
    pub(crate) removed_source_items: Arc<Mutex<BTreeSet<WorldMapItemSourceIdentity>>>,
    pub(crate) source_item_count_overrides: Arc<Mutex<BTreeMap<WorldMapItemSourceIdentity, u16>>>,
    pub(crate) runtime_tile_items: Arc<Mutex<Vec<RuntimeMapItemRecord>>>,
    pub(crate) revision: Arc<AtomicU64>,
}

/// Result of one persisted, revision-bound, whole-item transfer from an authoritative imported
/// map tile into an empty player equipment slot. It carries no native packet data; listener routing
/// and map delta delivery remain separate boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMapItemToEquipmentTransferOutcome {
    pub player_id: u64,
    pub source_identity: WorldMapItemSourceIdentity,
    pub item: ItemInstance,
    pub equipment_slot: EquipmentSlot,
    pub map_revision: u64,
}

/// Result of one persisted, revision-bound whole-item transfer from a map source into one owned
/// top-level container. Native routing and client refresh delivery remain separate boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMapItemToContainerTransferOutcome {
    pub player_id: u64,
    pub source_identity: WorldMapItemSourceIdentity,
    pub item: ItemInstance,
    pub container_id: u8,
    pub map_revision: u64,
}

/// Converts one durable runtime tile-item record into its renderable map form. Runtime content
/// carries no imported client mapping, action metadata, text, or nested trees in this boundary.
fn runtime_record_to_world_map_item(record: &RuntimeMapItemRecord) -> WorldMapItem {
    WorldMapItem {
        server_id: record.server_id,
        client_thing_id: None,
        count: record.count,
        action_id: None,
        unique_id: None,
        text: None,
        description: None,
        teleport_destination: None,
        duration: None,
        charges: None,
        children: record
            .children
            .iter()
            .map(|child| WorldMapItem {
                server_id: child.server_id,
                client_thing_id: None,
                count: child.count,
                action_id: None,
                unique_id: None,
                text: None,
                description: None,
                teleport_destination: None,
                duration: None,
                charges: None,
                children: Vec::new(),
            })
            .collect(),
    }
}

/// Converts one spawned runtime corpse item into its durable registry record form.
fn runtime_world_map_item_to_record(
    position: Position,
    ordinal: u8,
    item: &WorldMapItem,
    despawn_tick: Option<u64>,
) -> Option<RuntimeMapItemRecord> {
    if item.server_id == 0
        || item.count == 0
        || item.children.len() > forgotten_persistence::MAX_RUNTIME_MAP_ITEM_CHILDREN
        || item
            .children
            .iter()
            .any(|child| child.server_id == 0 || child.count == 0 || !child.children.is_empty())
    {
        return None;
    }
    Some(RuntimeMapItemRecord {
        position,
        ordinal,
        server_id: item.server_id,
        count: item.count,
        children: item
            .children
            .iter()
            .map(|child| RuntimeMapItemChildRecord {
                server_id: child.server_id,
                count: child.count,
            })
            .collect(),
        despawn_tick,
    })
}

impl Default for SharedPublicChatEvent {
    fn default() -> Self {
        Self {
            speaker_name: String::new(),
            speaker_position: NativeOtClientPosition { x: 0, y: 0, z: 0 },
            channel_id: None,
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
            text: String::new(),
        }
    }
}

/// One queued classic VIP presence change for an exact persisted watched player ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SharedVipPresenceEvent {
    target_player_id: u32,
    online: bool,
}

#[derive(Debug, Clone)]
struct SharedChatRecipient {
    player_name: String,
    sender: mpsc::SyncSender<SharedPublicChatEvent>,
}

#[derive(Debug, Clone)]
struct SharedVipPresenceRecipient {
    watched_player_ids: BTreeSet<u32>,
    sender: mpsc::SyncSender<SharedVipPresenceEvent>,
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
            player_outfits: Arc::new(Mutex::new(BTreeMap::new())),
            player_directions: Arc::new(Mutex::new(BTreeMap::new())),
            visibility_epoch: Arc::new(AtomicU64::new(0)),
            vitals_epoch: Arc::new(AtomicU64::new(0)),
            progression_epoch: Arc::new(AtomicU64::new(0)),
            equipment_epoch: Arc::new(AtomicU64::new(0)),
            containers_epoch: Arc::new(AtomicU64::new(0)),
            party_epoch: Arc::new(AtomicU64::new(0)),
            online_players: Arc::new(AtomicU64::new(0)),
            chat_recipients: Arc::new(Mutex::new(BTreeMap::new())),
            vip_presence_recipients: Arc::new(Mutex::new(BTreeMap::new())),
            pending_kicks: Arc::new(Mutex::new(BTreeMap::new())),
            pending_trades_closed: Arc::new(Mutex::new(BTreeSet::new())),
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

    /// Returns the IDs of every registered player, sorted. Callers use this for bounded periodic
    /// snapshot flushes; it grants no mutation access.
    pub fn registered_player_ids(&self) -> Result<Vec<u64>, HostError> {
        Ok(self.lock()?.registered_player_ids())
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

    pub fn party_epoch(&self) -> u64 {
        self.party_epoch.load(Ordering::SeqCst)
    }

    /// Live registered-player count for lock-free status/metrics consumers.
    pub fn online_players(&self) -> u32 {
        u32::try_from(self.online_players.load(Ordering::SeqCst)).unwrap_or(u32::MAX)
    }

    /// Cloneable counter handle shared with the status listener thread.
    pub fn online_players_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.online_players)
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

    pub fn replace_player_combat_defense(
        &self,
        player_id: u64,
        defense: PlayerCombatDefense,
    ) -> Result<bool, HostError> {
        self.lock()?
            .replace_player_combat_defense(player_id, defense)
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

    /// Active haste modifier percent for one player (0 when none) — plan v49 slice 12.
    pub fn player_speed_bonus_percent(&self, player_id: u64) -> u16 {
        self.lock()
            .map_or(0, |world| world.player_speed_bonus_percent(player_id))
    }

    /// Deterministic loot-split recipient order for one killer (plan v49 slice 14): the party
    /// leader first, then members in ascending id order; the killer is included wherever they
    /// sit in that order. Empty output means the killer has no party and nothing is distributed.
    /// Unknown players yield empty output too — the corpse path must never fail on a vanished
    /// killer, so this helper fails open instead of propagating the unknown-player error.
    pub fn party_loot_split_targets(&self, player_id: u64) -> Result<Vec<u64>, HostError> {
        let world = self.lock()?;
        let Ok(Some(leader)) = world
            .player_party_leader(player_id)
            .map_err(HostError::Core)
        else {
            return Ok(Vec::new());
        };
        let mut targets = vec![leader];
        if let Ok(members) = world.player_party_members(leader) {
            targets.extend(members);
        }
        Ok(targets)
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

    /// Captures all world-owned data needed for native map rendering under one short lock. The
    /// returned snapshot contains owned values so protocol encoding and socket writes can proceed
    /// concurrently without retaining the authoritative-world mutex.
    fn native_render_snapshot(
        &self,
        observer_id: u64,
        look_type: u8,
        speed: u16,
    ) -> Result<NativeWorldRenderSnapshot, HostError> {
        let (static_spawns, player_snapshots, invisible_ids) = {
            let world = self.lock()?;
            (
                world.active_static_spawn_collection(),
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
        let visible_players = player_snapshots
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
            .collect::<Result<Vec<_>, HostError>>()?;
        Ok(NativeWorldRenderSnapshot {
            static_spawns,
            visible_players,
        })
    }

    /// Resolves one connected player ID by exact case-insensitive name.
    pub fn online_player_id_by_name(&self, name: &str) -> Result<Option<u64>, HostError> {
        let lowered = name.trim().to_lowercase();
        if lowered.is_empty() {
            return Ok(None);
        }
        let snapshots = self.lock()?.player_render_snapshots();
        Ok(snapshots
            .into_iter()
            .find(|player| player.name.to_lowercase() == lowered)
            .map(|player| player.id))
    }

    /// True when the player is currently registered in the shared world.
    pub fn has_player(&self, player_id: u64) -> Result<bool, HostError> {
        Ok(self
            .lock()?
            .player_render_snapshots()
            .iter()
            .any(|player| player.id == player_id))
    }

    /// Reads one connected player's authoritative position.
    pub fn player_position(&self, player_id: u64) -> Result<forgotten_core::Position, HostError> {
        self.lock()?
            .player_render_snapshots()
            .into_iter()
            .find(|player| player.id == player_id)
            .map(|player| player.position)
            .ok_or(forgotten_core::CoreError::UnknownPlayer(player_id))
            .map_err(HostError::Core)
    }

    /// Reads one connected player's position and facing direction byte for summon anchoring.
    pub fn player_position_and_facing(
        &self,
        player_id: u64,
    ) -> Result<Option<(forgotten_core::Position, u8)>, HostError> {
        let position = self.player_position(player_id)?;
        let direction = self
            .player_directions
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .get(&player_id)
            .copied()
            .unwrap_or(2);
        Ok(Some((position, direction)))
    }

    /// Any connected player's anchor for console spawns that omit an explicit character.
    pub fn any_online_player_anchor(
        &self,
    ) -> Result<Option<(forgotten_core::Position, u8)>, HostError> {
        let first_id = self.lock()?.registered_player_ids().into_iter().next();
        match first_id {
            Some(player_id) => self.player_position_and_facing(player_id),
            None => Ok(None),
        }
    }

    /// Applies an operator gamemaster tier to a connected player's live state. Persistence is
    /// the caller's responsibility; this only keeps sessions consistent.
    pub fn update_player_gm_level(&self, _player_id: u64, _level: u8) -> Result<(), HostError> {
        // GM tier currently lives only in SQLite plus command authorization at request time;
        // no per-session cached copy exists yet. Kept as an explicit boundary so future
        // client-visible GM indicators have one integration point.
        Ok(())
    }

    /// Operator teleport for a connected player with full map validation and persistence
    /// handled by the bridge caller.
    pub fn teleport_player_for_operator(
        &self,
        player_id: u64,
        destination: forgotten_core::Position,
    ) -> Result<(), HostError> {
        {
            let mut world = self.lock()?;
            world
                .teleport_player(player_id, destination)
                .map_err(HostError::Core)?;
        }
        self.mark_visibility_changed();
        Ok(())
    }

    /// Summons an entity clone one tile in front of the named player's facing, validating
    /// walkability and occupancy through the core transition.
    pub fn spawn_dynamic_entity_in_front_of_player(
        &self,
        player_id: u64,
        entity_name: &str,
    ) -> Result<u32, HostError> {
        let Some((anchor, direction_byte)) = self.player_position_and_facing(player_id)? else {
            return Err(HostError::Core(forgotten_core::CoreError::UnknownPlayer(
                player_id,
            )));
        };
        let delta = match direction_byte {
            0 => (0_i32, -1_i32), // north
            1 => (1, 0),          // east
            2 => (0, 1),          // south
            3 => (-1, 0),         // west
            _ => (0, 1),
        };
        let target = forgotten_core::Position {
            x: (anchor.x as i32 + delta.0).max(0) as u16,
            y: (anchor.y as i32 + delta.1).max(0) as u16,
            z: anchor.z,
        };
        let spawned = {
            let mut world = self.lock()?;
            world
                .spawn_dynamic_entity(entity_name, target, direction_byte)
                .map_err(HostError::Core)?
        };
        self.mark_visibility_changed();
        Ok(spawned)
    }

    /// Summons an entity one tile in front of an explicit anchor position.
    pub fn spawn_dynamic_entity_in_front(
        &self,
        entity_name: &str,
        anchor: forgotten_core::Position,
        direction_byte: u8,
    ) -> Result<u32, HostError> {
        let delta = match direction_byte {
            0 => (0_i32, -1_i32), // north
            1 => (1, 0),          // east
            2 => (0, 1),          // south
            3 => (-1, 0),         // west
            _ => (0, 1),
        };
        let target = forgotten_core::Position {
            x: (anchor.x as i32 + delta.0).max(0) as u16,
            y: (anchor.y as i32 + delta.1).max(0) as u16,
            z: anchor.z,
        };
        let spawned = {
            let mut world = self.lock()?;
            world
                .spawn_dynamic_entity(entity_name, target, direction_byte)
                .map_err(HostError::Core)?
        };
        self.mark_visibility_changed();
        Ok(spawned)
    }

    /// Delivers guild chat to every online member of the sender's guild through the classic
    /// channel-talk record (mode 7, guild channel id 0x00F1). The sender always receives their
    /// own message. Membership resolves from SQLite; delivery is session-local.
    pub fn broadcast_guild_chat(
        &self,
        sender_id: u64,
        message: &str,
        member_ids: &[u64],
    ) -> Result<usize, HostError> {
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
        // Guild channel id: FE reserves 0x00F1 for the guild chat channel on classic profiles.
        const NATIVE_GUILD_CHANNEL_ID: u16 = 0x00F1;
        let event = SharedPublicChatEvent {
            speaker_name: sender.name,
            speaker_position: native_position(sender.position),
            channel_id: Some(NATIVE_GUILD_CHANNEL_ID),
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
            text: truncate_native_chat_text(&body),
        };
        let mut recipients = self
            .chat_recipients
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        let mut delivered = 0;
        for member_id in member_ids {
            let Some(recipient) = recipients.get(member_id) else {
                continue;
            };
            match recipient.sender.try_send(event.clone()) {
                Ok(()) => delivered += 1,
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    recipients.remove(member_id);
                }
                Err(mpsc::TrySendError::Full(_)) => {}
            }
        }
        Ok(delivered)
    }

    /// Requests an orderly disconnect of one connected player at the next session drain.
    /// Returns false when the player is not online.
    pub fn request_kick(&self, player_id: u64) -> Result<bool, HostError> {
        let online = self.has_player(player_id)?;
        if !online {
            return Ok(false);
        }
        self.pending_kicks
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?
            .insert(player_id, std::time::SystemTime::now());
        Ok(true)
    }

    /// Cancels any live trade touching one player and flags the other participant's session
    /// so its drain loop closes their trade window too. Returns the other player id.
    pub fn cancel_player_trade_and_signal(&self, player_id: u64) -> Result<Option<u64>, HostError> {
        let other = {
            let mut world = self.lock()?;
            world.cancel_player_trade(player_id)
        };
        if let Some(other_id) = other {
            self.pending_trades_closed
                .lock()
                .map_err(|_| HostError::SharedWorldUnavailable)?
                .insert(other_id);
        }
        Ok(other)
    }

    /// Consumes a pending kick for one player: returns true exactly once per requested kick.
    /// The native session loop calls this every iteration so operator kicks disconnect the
    /// target within roughly one heartbeat.
    pub fn take_pending_kick(&self, player_id: u64) -> Result<bool, HostError> {
        let mut kicks = self
            .pending_kicks
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        Ok(kicks.remove(&player_id).is_some())
    }

    /// Consumes the trade-closed signal for one player (their window must close).
    pub fn take_pending_trade_close(&self, player_id: u64) -> Result<bool, HostError> {
        let mut closed = self
            .pending_trades_closed
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        Ok(closed.remove(&player_id))
    }

    /// Restores a connected player's health and mana to their maximums. Used by operator
    /// `/heal`; returns false when the player is offline.
    pub fn restore_player_vitals(&self, player_id: u64) -> Result<bool, HostError> {
        let Ok((_, vitals)) = self.player_and_vitals(player_id) else {
            return Ok(false);
        };
        let restored = PlayerVitals {
            health: vitals.max_health,
            mana: vitals.max_mana,
            ..vitals
        };
        {
            let mut world = self.lock()?;
            world
                .update_player_vitals(player_id, restored)
                .map_err(HostError::Core)?;
        }
        self.vitals_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    /// Bumps the containers epoch so every session re-emits open-container deltas. Operator
    /// item delivery must call this or clients never learn a container changed.
    pub fn mark_containers_changed(&self) {
        self.containers_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Flags both participants' sessions to close their trade windows after a successful swap
    /// or any cancellation path that bypasses per-player signaling.
    pub fn signal_trade_closed(&self, initiator: u64, counterparty: u64) -> Result<(), HostError> {
        let mut closed = self
            .pending_trades_closed
            .lock()
            .map_err(|_| HostError::SharedWorldUnavailable)?;
        closed.insert(initiator);
        closed.insert(counterparty);
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
    vip_presence_announced: bool,
}

impl Drop for SharedNativePlayerRegistration {
    fn drop(&mut self) {
        self.world.unregister_public_chat_recipient(self.player_id);
        if self.vip_presence_announced {
            let _ = self.world.publish_vip_presence(self.player_id, false);
        }
        self.world.unregister_vip_presence_recipient(self.player_id);
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
        if self.corpse_despawn_seconds > 86_400 {
            return Err(HostError::InvalidConfiguration(
                "corpseDespawnSeconds must stay within one day".into(),
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
    online_players: Arc<AtomicU64>,
    /// Bound loopback port of the optional live operator bridge, if started.
    operator_bridge_port: Option<u16>,
    thread: Option<JoinHandle<Result<(), HostError>>>,
}

impl HostHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Loopback port of the live operator bridge when this listener runs with one enabled.
    pub fn operator_bridge_port(&self) -> Option<u16> {
        self.operator_bridge_port
    }

    /// Live registered-player counter shared with the running world; feed this to
    /// `start_status` so fe-metrics reports the real players-online figure.
    pub fn online_players_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.online_players)
    }

    pub fn shutdown_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }

    pub fn shutdown(mut self) -> Result<(), HostError> {
        self.shutdown.store(true, Ordering::SeqCst);
        // Double shutdown is treated as an idempotent no-op rather than a panic.
        let Some(handle) = self.thread.take() else {
            return Ok(());
        };
        match handle.join() {
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
    let online_players = Arc::new(AtomicU64::new(0));
    let database_path = database_path.as_ref().to_path_buf();
    let thread_shutdown = Arc::clone(&shutdown);
    let thread_online_players = Arc::clone(&online_players);
    let thread = thread::spawn(move || {
        serve(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
            thread_online_players,
        )
    });

    Ok(HostHandle {
        local_addr,
        shutdown,
        online_players: Arc::new(AtomicU64::new(0)),
        operator_bridge_port: None,
        thread: Some(thread),
    })
}

pub fn start_status(
    config: StatusHostConfig,
    database_path: impl AsRef<Path>,
    online_players: Arc<AtomicU64>,
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
            online_players,
            Instant::now(),
        )
    });
    Ok(HostHandle {
        local_addr,
        shutdown,
        online_players: Arc::new(AtomicU64::new(0)),
        operator_bridge_port: None, // placeholder-fix
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
        online_players: Arc::new(AtomicU64::new(0)),
        operator_bridge_port: None,
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
        online_players: Arc::new(AtomicU64::new(0)),
        operator_bridge_port: None,
        thread: Some(thread),
    })
}

pub fn start_native_otclient_game(
    config: NativeOtClientHostConfig,
    database_path: impl AsRef<Path>,
) -> Result<HostHandle, HostError> {
    start_native_otclient_game_with_bridge(config, database_path, false)
}

/// Starts the native game listener and optionally the loopback operator bridge. The bridge is
/// enabled for normal server runs so operators and Forgotten Cloud can act on the live world;
/// focused protocol tests disable it to keep port usage minimal.
pub fn start_native_otclient_game_with_bridge(
    config: NativeOtClientHostConfig,
    database_path: impl AsRef<Path>,
    enable_operator_bridge: bool,
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
    let shared_map = config
        .world_map
        .as_deref()
        .map(|world_map| {
            let database = EngineDatabase::open(&database_path)?;
            let journal = database.map_item_removal_journal()?;
            let count_overrides = database.map_item_count_overrides()?;
            let runtime_items = database.runtime_map_items()?;
            SharedNativeMap::recover_complete_map_item_state(
                world_map.clone(),
                journal.as_ref(),
                count_overrides.as_ref(),
                runtime_items.as_ref(),
            )
        })
        .transpose()?
        .map(Arc::new);
    let thread_shutdown = Arc::clone(&shutdown);
    let thread_online_players = shared_world.online_players_counter();
    let bridge_port = if enable_operator_bridge {
        let (port, _bridge_shutdown) =
            operator::start_operator_bridge(operator::OperatorBridgeConfig {
                shared_world: shared_world.clone(),
                database_path: database_path.clone(),
            })?;
        Some(port)
    } else {
        None
    };
    let thread = thread::spawn(move || {
        serve_native_otclient_game(
            listener,
            config,
            database_path,
            thread_shutdown,
            active_connections,
            shared_world,
            shared_map,
        )
    });
    Ok(HostHandle {
        local_addr,
        shutdown,
        online_players: thread_online_players,
        operator_bridge_port: bridge_port,
        thread: Some(thread),
    })
}

fn serve(
    listener: TcpListener,
    config: HostConfig,
    database_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicUsize>,
    _online_players: Arc<AtomicU64>,
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
    online_players: Arc<AtomicU64>,
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
                let session_online = Arc::clone(&online_players);
                thread::spawn(move || {
                    let result = handle_status_session(
                        &mut stream,
                        peer,
                        &session_config,
                        &session_database_path,
                        &session_online,
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

/// Best-effort reattach of a relogging player to their persisted party. The stored relation
/// is only consumed when the stored leader is online; otherwise the row is left in place so
/// a later leader login can still reform the party through the same path.
fn try_hydrate_persisted_party(
    shared_world: &SharedNativeWorld,
    database: &EngineDatabase,
    player_id: u64,
) -> Result<(), HostError> {
    let Some(leader_id) = database.party_leader_of(player_id)? else {
        return Ok(());
    };
    if leader_id == player_id {
        return Ok(());
    }
    let leader_online = shared_world.player_and_vitals(leader_id).is_ok();
    if !leader_online {
        return Ok(());
    }
    let leader_leads_live_party = matches!(
        shared_world.lock()?.player_party_leader(leader_id),
        Ok(Some(_))
    );
    if leader_leads_live_party {
        shared_world.add_existing_party_member(leader_id, player_id)?;
    } else {
        shared_world.restore_party_snapshot(leader_id, &[player_id])?;
    }
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
    shared_map: Option<Arc<SharedNativeMap>>,
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
    let heartbeat_pursuit_policy = config.static_target_pursuit_policy;
    let heartbeat_attack_policy = config.static_target_attack_policy;
    let heartbeat_wander_policy = config.static_creature_wander_policy;
    let heartbeat_wander_every_ticks = config.static_creature_wander_every_ticks;
    let heartbeat_map_owner = shared_map.clone();
    let heartbeat_database_path = database_path.clone();
    let heartbeat_death_loss_policy = config.death_loss_policy;
    let heartbeat_progression_rules = config.progression_rules.clone();
    let heartbeat_corpse_despawn_seconds = config.corpse_despawn_seconds;
    let auth_rate_limiter = Arc::new(NativeAuthRateLimiter::default());
    let heartbeat = thread::spawn(move || {
        run_native_shared_world_heartbeat(
            heartbeat_world,
            heartbeat_shutdown,
            NativeHeartbeatConfig {
                pursuit_policy: heartbeat_pursuit_policy,
                attack_policy: heartbeat_attack_policy,
                wander_policy: heartbeat_wander_policy,
                wander_every_ticks: heartbeat_wander_every_ticks,
                map_owner: heartbeat_map_owner,
                database_path: heartbeat_database_path,
                death_loss_policy: heartbeat_death_loss_policy,
                progression_rules: heartbeat_progression_rules,
                corpse_despawn_seconds: heartbeat_corpse_despawn_seconds,
            },
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
                let session_map = shared_map.clone();
                let session_rate_limiter = Arc::clone(&auth_rate_limiter);
                thread::spawn(move || {
                    let result = (|| {
                        let session_config = native_session_config_with_map_snapshot(
                            session_config,
                            session_map.as_deref(),
                        )?;
                        handle_native_otclient_game(
                            &mut stream,
                            peer,
                            &session_config,
                            &session_database_path,
                            &session_world,
                            session_map.as_deref(),
                            &session_rate_limiter,
                        )
                    })();
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

/// Builds one session-local native configuration with a detached map snapshot from the live map
/// owner. The immutable configuration map is an initialization input only; each accepted session
/// receives the synchronized owner's current map state without retaining its lock.
fn native_session_config_with_map_snapshot(
    mut config: NativeOtClientHostConfig,
    map_owner: Option<&SharedNativeMap>,
) -> Result<NativeOtClientHostConfig, HostError> {
    config.world_map = map_owner
        .map(SharedNativeMap::render_snapshot)
        .transpose()?;
    Ok(config)
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
            direct_melee_cooldown_remaining_ticks: record.direct_melee_cooldown_remaining_ticks,
            direct_melee_damage_sequence: record.direct_melee_damage_sequence,
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
            direct_melee_cooldown_remaining_ticks: snapshot.direct_melee_cooldown_remaining_ticks,
            direct_melee_damage_sequence: snapshot.direct_melee_damage_sequence,
        })
        .collect::<Vec<_>>();
    database
        .replace_static_creature_runtime(&records)
        .map_err(HostError::Persistence)
}

/// Applies the existing authoritative temple-respawn transition before a new native game session
/// registers the persisted character. Stock OTCv8 740's death dialog logs out and starts another
/// character login; there is no separate revival request to route on the prior dead session.
/// This boundary does not add a timer, death-loss record, teleport effect, or in-session revive.
fn respawn_persisted_native_player_for_relog(
    database: &mut EngineDatabase,
    player_id: u64,
    world_map: &WorldMap,
) -> Result<(), HostError> {
    let character = database
        .player_by_id(player_id)
        .map_err(HostError::Persistence)?;
    let mut world = WorldState::default();
    world
        .add_player_with_vitals_and_progression(
            Player {
                id: character.id,
                account_id: 0,
                name: character.name,
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
            character.progression,
        )
        .map_err(HostError::Core)?;
    world
        .replace_player_town(player_id, character.town_id)
        .map_err(HostError::Core)?;
    world
        .hydrate_player_respawn_state(player_id, character.respawn_state)
        .map_err(HostError::Core)?;
    let outcome = world.respawn_player(player_id).map_err(HostError::Core)?;
    if !world_map
        .tile(outcome.position)
        .is_some_and(|tile| tile.walkable)
    {
        return Err(HostError::InvalidConfiguration(
            "native temple respawn destination is missing or not walkable in the selected world map"
                .into(),
        ));
    }
    database
        .update_player_position_vitals_and_respawn_state(
            player_id,
            outcome.position,
            PersistedPlayerVitals {
                health: outcome.vitals.health,
                max_health: outcome.vitals.max_health,
                mana: outcome.vitals.mana,
                max_mana: outcome.vitals.max_mana,
                capacity: outcome.vitals.capacity,
                magic_level: outcome.vitals.magic_level,
            },
            PlayerRespawnState::default(),
        )
        .map_err(HostError::Persistence)?;
    Ok(())
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
    // Plan v49 slice 17: banned accounts get a clean login error instead of a character list.
    if let Some(reason) = database
        .active_account_ban(u64::try_from(account.id).unwrap_or(0))
        .map_err(HostError::Persistence)?
    {
        write_frame(
            stream,
            &encode_native_otclient_login_error(&format!("Your account is banned. {reason}")),
        )?;
        record_event(
            database_path,
            "info",
            &format!(
                "native client login rejected banned-account={} peer={peer}",
                account.id
            ),
        );
        return Ok(());
    }
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

/// Bounded fixed-window brute-force guard for native game authentication. Failures are counted
/// per peer IP; once a peer exceeds the window budget every further attempt is rejected before
/// any database work happens until the window elapses.
const NATIVE_AUTH_MAX_FAILURES_PER_WINDOW: u32 = 8;
const NATIVE_AUTH_WINDOW: Duration = Duration::from_secs(60);
/// Bounded fixed-window chat flood control: at most this many routed talk records per window;
/// anything beyond is suppressed before shared delivery.
const CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW: usize = 10;
const CHAT_FLOOD_WINDOW: Duration = Duration::from_secs(2);

#[derive(Default)]
struct NativeAuthRateLimiter {
    failures: Mutex<BTreeMap<IpAddr, (u32, Instant)>>,
}

impl NativeAuthRateLimiter {
    fn is_blocked(&self, peer: IpAddr) -> bool {
        // A poisoned limiter must not lock every peer out; availability wins here because the
        // database password check still runs for unblocked peers.
        let Ok(failures) = self.failures.lock() else {
            return false;
        };
        match failures.get(&peer) {
            Some((count, window_start)) => {
                *count >= NATIVE_AUTH_MAX_FAILURES_PER_WINDOW
                    && window_start.elapsed() < NATIVE_AUTH_WINDOW
            }
            None => false,
        }
    }

    fn register_failure(&self, peer: IpAddr) {
        // A poisoned mutex must not crash the accept loop; skipping one failure record
        // degrades rate-limiting accuracy but keeps the listener alive.
        let Ok(mut failures) = self.failures.lock() else {
            return;
        };
        let now = Instant::now();
        let entry = failures.entry(peer).or_insert((0, now));
        if entry.1.elapsed() >= NATIVE_AUTH_WINDOW {
            *entry = (0, now);
        }
        entry.0 = entry.0.saturating_add(1);
    }
}

fn drain_shared_public_chat(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    events: &mpsc::Receiver<SharedPublicChatEvent>,
    open_public_channel_ids: &BTreeSet<u16>,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    loop {
        match events.try_recv() {
            Ok(event) => {
                let Some((record, mode, event_kind)) = (if event.private {
                    Some((
                        encode_native_otclient_private_message_from(
                            profile,
                            &event.speaker_name,
                            &event.text,
                        )
                        .map_err(HostError::Protocol)?,
                        4,
                        "private-chat",
                    ))
                } else {
                    match event.channel_id {
                        Some(channel_id) if !open_public_channel_ids.contains(&channel_id) => None,
                        Some(channel_id) => Some((
                            encode_native_otclient_public_channel_say(
                                profile,
                                &event.speaker_name,
                                channel_id,
                                &event.text,
                            )
                            .map_err(HostError::Protocol)?,
                            7,
                            "public-channel-chat",
                        )),
                        None => match event.talk_mode {
                            NATIVE_OTCLIENT_MESSAGE_WHISPER => Some((
                                encode_native_otclient_whisper(
                                    profile,
                                    &event.speaker_name,
                                    event.speaker_position,
                                    &event.text,
                                )
                                .map_err(HostError::Protocol)?,
                                2,
                                "whisper-chat",
                            )),
                            NATIVE_OTCLIENT_MESSAGE_YELL => Some((
                                encode_native_otclient_yell(
                                    profile,
                                    &event.speaker_name,
                                    event.speaker_position,
                                    &event.text,
                                )
                                .map_err(HostError::Protocol)?,
                                3,
                                "yell-chat",
                            )),
                            NATIVE_OTCLIENT_MESSAGE_GM_BROADCAST => Some((
                                // Console broadcasts ride the 0xB4/0x13 game-announcement
                                // class: white center-screen text mirrored into the Server
                                // Log tab. The mode-9 GM talk record renders console-red only.
                                encode_native_otclient_game_announcement(
                                    profile,
                                    format!("{}: {}", event.speaker_name, event.text).as_str(),
                                )
                                .map_err(HostError::Protocol)?,
                                9,
                                "console-broadcast",
                            )),
                            _ => Some((
                                encode_native_otclient_public_say(
                                    profile,
                                    &event.speaker_name,
                                    event.speaker_position,
                                    &event.text,
                                )
                                .map_err(HostError::Protocol)?,
                                1,
                                "public-chat",
                            )),
                        },
                    }
                }) else {
                    continue;
                };
                write_frame(stream, &record)?;
                native_diagnostic(
                    extended_diagnostics,
                    peer,
                    &format!(
                        "outbound={} opcode=0xaa mode={} text-bytes={}",
                        event_kind,
                        mode,
                        event.text.len(),
                    ),
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// Drains only queued presence changes for the current active session's persisted VIP targets.
/// Queue disconnects are harmless because the enclosing native session owns both endpoints.
fn drain_shared_vip_presence(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    events: &mpsc::Receiver<SharedVipPresenceEvent>,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    loop {
        match events.try_recv() {
            Ok(event) => {
                let record = encode_native_otclient_classic_vip_presence(
                    profile,
                    event.target_player_id,
                    event.online,
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &record)?;
                native_diagnostic(
                    extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=vip-presence opcode=0x{:02x} online={}",
                        record.0[0], event.online
                    ),
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// Sends one current, bounded party-shield snapshot only after an authoritative party epoch
/// change. The shared-world lock is released before every socket write, and no visibility or UI
/// policy beyond the current all-active native render set is claimed here.
fn refresh_native_party_shields(
    stream: &mut TcpStream,
    shared_world: &SharedNativeWorld,
    profile: &NativeOtClientProfile,
    observer_id: u64,
    observed_party_epoch: &mut u64,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    let party_epoch = shared_world.party_epoch();
    if party_epoch == *observed_party_epoch {
        return Ok(());
    }
    let frames = shared_world.party_display_frames(profile, observer_id)?;
    for frame in &frames {
        write_frame(stream, frame)?;
    }
    *observed_party_epoch = party_epoch;
    native_diagnostic(
        extended_diagnostics,
        peer,
        &format!(
            "outbound=party-shield-refresh records={} epoch={party_epoch}",
            frames.len()
        ),
    );
    Ok(())
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
    // Operator freeze (plan v49 slice 18): frozen characters face the requested direction but
    // never step, identical to the classic blocked response.
    if database.player_frozen(character_id)? {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    }
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
    *facing = direction;
    shared_world.update_player_facing(character_id, *facing)?;
    if let Some(teleport_destination) =
        native_stepped_on_map_teleport_destination(world_map, destination)
    {
        if activate_native_map_teleport_item(
            stream,
            profile,
            snapshot,
            database,
            shared_world,
            character_id,
            world_map,
            player_position,
            *facing,
            teleport_destination,
        )? {
            return Ok(false);
        }
    }
    shared_world.mark_visibility_changed();
    database.update_player_position_and_facing(
        character_id,
        destination,
        facing.protocol_direction(),
    )?;
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

fn native_stepped_on_map_teleport_destination(
    world_map: &WorldMap,
    position: Position,
) -> Option<Position> {
    let mut destinations = world_map
        .tile_items(position)?
        .iter()
        .filter_map(|item| item.teleport_destination);
    let destination = destinations.next()?;
    destinations.next().is_none().then_some(destination)
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
    const NATIVE_OTCLIENT_MAX_TELEPORT_HOPS: usize = 8;
    let final_destination = {
        let mut world = shared_world.lock()?;
        let mut current_destination = destination;
        let mut visited_destinations = BTreeSet::new();
        let mut final_destination = None;
        for _ in 0..NATIVE_OTCLIENT_MAX_TELEPORT_HOPS {
            if !visited_destinations.insert(current_destination)
                || !world_map.is_walkable(current_destination)
                || world.is_static_creature_occupied(current_destination)
                || world.is_player_occupied(current_destination)
            {
                break;
            }
            let (_, reached_destination) = world
                .teleport_player(character_id, current_destination)
                .map_err(HostError::Core)?;
            final_destination = Some(reached_destination);
            let Some(next_destination) =
                native_stepped_on_map_teleport_destination(world_map, reached_destination)
            else {
                break;
            };
            current_destination = next_destination;
        }
        final_destination
    };
    let Some(destination) = final_destination else {
        return Ok(false);
    };

    shared_world.mark_visibility_changed();
    database.update_player_position_and_facing(
        character_id,
        destination,
        facing.protocol_direction(),
    )?;
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
    // Operator freeze (plan v49 slice 18): frozen characters cancel the diagonal attempt.
    if database.player_frozen(character_id)? {
        write_frame(
            stream,
            &encode_native_otclient_game_cancel_walk_facing(profile, facing.protocol_direction())
                .map_err(HostError::Protocol)?,
        )?;
        return Ok(false);
    }
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
    *facing = steps[1];
    shared_world.update_player_facing(character_id, *facing)?;
    if let Some(teleport_destination) =
        native_stepped_on_map_teleport_destination(world_map, destination)
    {
        if activate_native_map_teleport_item(
            stream,
            profile,
            snapshot,
            database,
            shared_world,
            character_id,
            world_map,
            player_position,
            *facing,
            teleport_destination,
        )? {
            return Ok(false);
        }
    }
    shared_world.mark_visibility_changed();
    database.update_player_position_and_facing(
        character_id,
        destination,
        facing.protocol_direction(),
    )?;
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

/// Classic-style effective walk speed: configured base speed plus the `speed` bonus of
/// equipped boots (BoH-style items). Bonuses add directly to the base; there is no stacking
/// across multiple slots because only the feet slot contributes.
pub fn native_effective_player_speed(
    base_speed: u16,
    equipment: &PlayerEquipment,
    speed_bonus_by_server_id: Option<&BTreeMap<u16, u16>>,
) -> u16 {
    let mut speed = base_speed;
    if let Some(bonuses) = speed_bonus_by_server_id {
        for slot in [EquipmentSlot::Feet] {
            if let Some(item) = equipment.item(slot) {
                if let Some(bonus) = bonuses.get(&item.server_id) {
                    speed = speed.saturating_add(*bonus);
                }
            }
        }
    }
    // Classic clients treat 0 or absurd speeds badly; keep a sane ceiling.
    speed.clamp(1, 2_000)
}

/// Applies the active haste condition (plan v49 slice 12): additive percent on the effective
/// speed, re-clamped so no bonus can push past the same sane ceiling.
pub fn native_hasted_speed(effective_speed: u16, speed_bonus_percent: u16) -> u16 {
    if speed_bonus_percent == 0 {
        return effective_speed;
    }
    let hasted = (u32::from(effective_speed) * (100 + u32::from(speed_bonus_percent))) / 100;
    u16::try_from(hasted).unwrap_or(2_000).clamp(1, 2_000)
}

/// Classic haste duration for spell-granted speed conditions (utani hur family).
const NATIVE_HASTE_DURATION_SECONDS: u16 = 25;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativePlayerInteractionOutcome {
    Applied,
    Rejected,
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
    if native_player_id(observer_id).is_ok_and(|native_id| native_id == native_creature_id) {
        // Self-look: the client addresses the observer's own creature id at its tile
        // stack position; classic servers answer with the fixed self sentence.
        return Ok(Some("You see yourself.".into()));
    }
    let message = if let Some(player_id) = native_player_id_to_character_id(native_creature_id) {
        let Some(target) = world.player(player_id) else {
            return Ok(None);
        };
        let level = target.level;
        native_classic_viewport_contains(observer.position, target.position).then(|| {
            format!(
                "You see {target_name}. (Level {level})",
                target_name = target.name
            )
        })
    } else if let Some(lifecycle) = world.static_creature_lifecycle(native_creature_id) {
        if !lifecycle.active
            || !native_classic_viewport_contains(observer.position, lifecycle.position)
        {
            return Ok(None);
        }
        world.static_creature(native_creature_id).map(|creature| {
            if creature.name_description.is_empty() {
                format!("You see {}.", creature.name)
            } else {
                // TFS nameDescription carries its own article ("a rat").
                format!("You see {}.", creature.name_description)
            }
        })
    } else {
        // Unresolved creature ids (stale client cache, unmapped ranges) still get an answer:
        // classic servers never leave a look unanswered.
        Some(format!("You see a creature (id {native_creature_id})."))
    };
    Ok(message.filter(|message| message.len() <= NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES))
}

/// Bounded non-numeric look reply for bare ground and unmapped decorations. Resolves the
/// topmost tile item's imported item name when the operator catalog provides one, degrades to
/// a generic item sentence without one, and answers plain ground tiles with "You see ground."
/// Raw numeric ids are never echoed back to clients (live-test regression A2).
fn native_ground_look_message(
    world_map: &WorldMap,
    position: Position,
    name_by_server_id: Option<&BTreeMap<u16, String>>,
) -> String {
    let resolved_name = world_map
        .tile_items(position)
        .and_then(|items| items.last())
        .and_then(|item| name_by_server_id.and_then(|names| names.get(&item.server_id).cloned()));
    match resolved_name {
        Some(name) => format!("You see {name}."),
        None => {
            if world_map
                .tile_items(position)
                .is_some_and(|items| !items.is_empty())
            {
                "You see an item.".into()
            } else {
                "You see ground.".into()
            }
        }
    }
}

fn apply_native_player_interaction(
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
fn cancel_native_player_attack_and_follow(
    shared_world: &SharedNativeWorld,
    player_id: u64,
) -> Result<(), HostError> {
    shared_world.set_player_target(player_id, None)?;
    shared_world.set_player_follow(player_id, None)?;
    Ok(())
}

/// Parses only FE's explicit numeric declarative-spell command. This intentionally does not
/// interpret arbitrary player speech, TFS spell words, parameters, rune use, or target text.
fn native_declarative_spell_command_id(message: &str) -> Option<u16> {
    const PREFIX: &str = "!fe cast ";
    let spell_id = message.strip_prefix(PREFIX)?.parse::<u16>().ok()?;
    (spell_id != 0).then_some(spell_id)
}

/// Resolves one exact, normalized public-speech keyword against the closest active validated NPC
/// in the bounded same-floor dialogue range. The result is stateless: it does not create focus,
/// parameters, quest progress, shops, travel, or a Lua callback.
fn resolve_native_static_npc_dialogue(
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
fn apply_and_persist_native_declarative_spell_cast(
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

struct NativeSelectedPlayerMeleePolicy<'a> {
    progression_rules: Option<&'a BTreeMap<VocationId, PlayerProgressionRules>>,
    skill_rate: u32,
    death_loss_policy: DeathLossPolicy,
    armor_by_server_id: Option<&'a BTreeMap<u16, u16>>,
    shield_defense_by_server_id: Option<&'a BTreeMap<u16, u16>>,
    armor_multiplier_by_vocation: Option<&'a BTreeMap<VocationId, u32>>,
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
fn apply_native_selected_player_melee_for_world_type(
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

/// Places one deterministic loot corpse on the defeated static creature's tile. The corpse is a
/// runtime-only map item (no source identity, no journal entry) whose children are the rolled
/// loot. The caller owns defeat validation, persistence of the map state, and client delivery.
/// Maps authoritative condition kinds onto the legacy client PlayerState bit flags (plan v49
/// slice 13): poison 0x0001, burning 0x0002, energy 0x0004.
fn native_condition_state_bits(
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
fn native_guild_channel_context(
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
fn native_declared_corpse_server_id(
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

fn spawn_native_static_defeat_corpse(
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
            | forgotten_core::CoreError::StaticNpcNotAttackable(_)
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

#[derive(Debug)]
pub enum HostError {
    Core(forgotten_core::CoreError),
    Io(std::io::Error),
    Protocol(ProtocolError),
    Persistence(forgotten_persistence::PersistenceError),
    InvalidConfiguration(String),
    InvalidProbe(&'static str),
    SharedWorldUnavailable,
    RenderPreparationUnavailable,
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
            Self::RenderPreparationUnavailable => b"render-preparation-unavailable",
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
    use forgotten_config::{
        parse_declarative_npc_dialogue_xml, parse_declarative_spells_xml,
        parse_declarative_weapons_xml, parse_tfs_public_channels_xml,
    };
    use forgotten_config::{parse_declarative_shops_xml, parse_quests_xml};
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
            world_type: WorldType::Pvp,
            advertised_game_addr: "127.0.0.1:7265".parse().unwrap(),
            max_connections: 2,
            session_timeout: Duration::from_millis(250),
            extended_diagnostics: false,
            empty_world: None,
            world_map: None,
            item_presentation_catalog: None,
            public_channel_catalog: None,
            item_armor_by_server_id: None,
            item_shield_defense_by_server_id: None,
            consumable_effects: None,
            quest_catalog: None,
            shop_catalog: None,
            item_slot_types_by_server_id: None,
            item_weight_by_server_id: None,
            item_name_by_server_id: None,
            stackable_item_server_ids: None,
            item_speed_bonus_by_server_id: None,
            armor_multiplier_by_vocation: None,
            static_spawns: None,
            corpse_server_id_by_creature_name: None,
            animated_damage_text_enabled: false,
            static_target_attack_policy: StaticTargetAttackPolicy::Disabled,
            static_target_pursuit_policy: StaticTargetPursuitPolicy::Disabled,
            static_creature_wander_policy:
                forgotten_core::StaticCreatureDecisionPolicy::ClockwiseAdjacent,
            static_creature_wander_every_ticks: 4,
            regeneration_rules: None,
            progression_rules: None,
            vocation_level_up_gains: None,
            skill_rate: 1,
            magic_rate: 1,
            experience_award_policy: None,
            party_shared_experience_rules: None,
            party_loot_split_enabled: false,
            death_loss_policy: DeathLossPolicy::DefaultFormula,
            declarative_weapon_catalog: None,
            declarative_spell_catalog: None,
            declarative_npc_dialogue_catalog: None,
            corpse_despawn_seconds: 0,
        }
    }

    #[test]
    fn native_static_npc_dialogue_requires_one_active_nearby_validated_npc() {
        let map = native_world_map();
        let monster_id = 0x4000_0001;
        let npc_id = 0x4000_0002;
        let collection = FeTfsStaticSpawnCollection::with_combat_metadata_and_npc_ids(
            vec![
                forgotten_core::FeTfsStaticEntity {
                    id: monster_id,
                    name: "Guide".into(),
                    name_description: String::new(),
                    position: Position {
                        x: 102,
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
                },
                forgotten_core::FeTfsStaticEntity {
                    id: npc_id,
                    name: "Guide".into(),
                    name_description: String::new(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    look_type: 128,
                    head: 0,
                    body: 0,
                    legs: 0,
                    feet: 0,
                    addons: 0,
                    speed: 134,
                    health_percent: 100,
                    direction: 2,
                },
            ],
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::from([npc_id]),
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&collection)).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 109,
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
        let catalog = parse_declarative_npc_dialogue_xml(
            br#"<fe-npc-dialogues><npc name="Guide"><response keyword="hi" text="Welcome, traveler."/></npc></fe-npc-dialogues>"#,
        )
        .unwrap();
        assert_eq!(
            resolve_native_static_npc_dialogue(&shared, 109, &catalog, "  hi\t").unwrap(),
            Some((
                npc_id,
                "Guide".into(),
                NativeOtClientPosition {
                    x: 101,
                    y: 100,
                    z: 7,
                },
                "Welcome, traveler.".into(),
            ))
        );
        assert!(
            resolve_native_static_npc_dialogue(&shared, 109, &catalog, "trade")
                .unwrap()
                .is_none()
        );
        shared
            .lock()
            .unwrap()
            .deactivate_static_creature(npc_id)
            .unwrap();
        assert!(
            resolve_native_static_npc_dialogue(&shared, 109, &catalog, "hi")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn static_target_acquisition_includes_enabled_pursuit_without_direct_attack() {
        assert_eq!(
            static_target_acquisition_policy(
                StaticTargetPursuitPolicy::Disabled,
                StaticTargetAttackPolicy::Disabled,
            ),
            StaticTargetAcquisitionPolicy::Disabled
        );
        assert_eq!(
            static_target_acquisition_policy(
                StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range: 6 },
                StaticTargetAttackPolicy::Disabled,
            ),
            StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 6 }
        );
        assert_eq!(
            static_target_acquisition_policy(
                StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range: 4 },
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 2 },
            ),
            StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 4 }
        );
        assert_eq!(
            static_target_acquisition_policy(
                StaticTargetPursuitPolicy::Disabled,
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 2 },
            ),
            StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 1 }
        );
    }

    #[test]
    fn native_legacy_slot_types_allow_only_exact_representable_equipment_slots() {
        let slot_types = BTreeMap::from([
            (4526, BTreeSet::from([LegacyItemSlotType::LeftHand])),
            (4527, BTreeSet::from([LegacyItemSlotType::Hand])),
            (4528, BTreeSet::from([LegacyItemSlotType::TwoHanded])),
        ]);
        assert!(native_legacy_slot_types_allow_equipment_slot(
            Some(&slot_types),
            4526,
            EquipmentSlot::LeftHand,
        ));
        assert!(!native_legacy_slot_types_allow_equipment_slot(
            Some(&slot_types),
            4526,
            EquipmentSlot::Head,
        ));
        assert!(native_legacy_slot_types_allow_equipment_slot(
            Some(&slot_types),
            4527,
            EquipmentSlot::RightHand,
        ));
        assert!(native_legacy_slot_types_allow_equipment_slot(
            Some(&slot_types),
            4527,
            EquipmentSlot::LeftHand,
        ));
        assert!(!native_legacy_slot_types_allow_equipment_slot(
            Some(&slot_types),
            4528,
            EquipmentSlot::RightHand,
        ));
        assert!(native_legacy_slot_types_allow_equipment_slot(
            Some(&slot_types),
            4999,
            EquipmentSlot::Head,
        ));
        assert!(native_legacy_slot_types_allow_equipment_slot(
            None,
            9999,
            EquipmentSlot::RightHand,
        ));
    }

    #[test]
    fn native_channel_list_entries_use_only_validated_public_catalog_entries() {
        let catalog = parse_tfs_public_channels_xml(
            br#"<channels><channel id="7" name="Trade" public="true"/><channel id="2" name="Staff" public="false"/><channel id="1" name="World Chat" public="true"/></channels>"#,
        )
        .unwrap();
        assert_eq!(
            native_classic_channel_list_entries(Some(&catalog)),
            vec![
                NativeOtClientClassicChannel {
                    id: 1,
                    name: "World Chat".into(),
                },
                NativeOtClientClassicChannel {
                    id: 7,
                    name: "Trade".into(),
                },
            ]
        );
        assert!(native_classic_channel_list_entries(None).is_empty());
        assert_eq!(
            native_configured_public_channel(Some(&catalog), 7),
            Some(NativeOtClientClassicChannel {
                id: 7,
                name: "Trade".into(),
            })
        );
        assert!(native_configured_public_channel(Some(&catalog), 2).is_none());
        assert!(native_configured_public_channel(None, 7).is_none());
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
    fn shared_native_map_replaces_tile_items_and_detaches_render_snapshots() {
        let map = SharedNativeMap::new((*native_world_map()).clone());
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        assert_eq!(map.revision(), 0);
        let initial = map.render_snapshot().unwrap();
        assert_eq!(initial.tile_items(position), None);

        let first_item = WorldMapItem {
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
        };
        assert_eq!(
            map.replace_tile_items(position, vec![first_item]).unwrap(),
            1
        );
        let first_snapshot = map.render_snapshot().unwrap();
        assert_eq!(
            first_snapshot.tile_items(position).unwrap()[0].server_id,
            1988
        );

        assert_eq!(map.replace_tile_items(position, Vec::new()).unwrap(), 2);
        assert_eq!(map.revision(), 2);
        assert_eq!(
            first_snapshot.tile_items(position).unwrap()[0].server_id,
            1988
        );
        assert!(map
            .render_snapshot()
            .unwrap()
            .tile_items(position)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn shared_native_map_snapshot_survives_concurrent_tile_replacement() {
        let map = SharedNativeMap::new((*native_world_map()).clone());
        let position = Position {
            x: 102,
            y: 100,
            z: 7,
        };
        let snapshot_before_mutation = map.render_snapshot().unwrap();
        let mutating_owner = map.clone();
        std::thread::spawn(move || {
            mutating_owner
                .replace_tile_items(
                    position,
                    vec![WorldMapItem {
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
                .unwrap()
        })
        .join()
        .unwrap();

        assert_eq!(snapshot_before_mutation.tile_items(position), None);
        assert_eq!(map.revision(), 1);
        assert_eq!(
            map.render_snapshot().unwrap().tile_items(position).unwrap()[0].server_id,
            1988
        );
    }

    #[test]
    fn native_session_map_snapshot_uses_current_shared_map_owner_state() {
        let source_map = native_world_map();
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let owner = SharedNativeMap::new((*source_map).clone());
        owner
            .replace_tile_items(
                position,
                vec![WorldMapItem {
                    server_id: 2148,
                    client_thing_id: Some(3031),
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
        let mut config = native_otclient_config("127.0.0.1:0".parse().unwrap());
        config.world_map = Some(source_map.clone());

        let session_config = native_session_config_with_map_snapshot(config, Some(&owner)).unwrap();
        assert!(source_map.tile_items(position).is_none());
        assert_eq!(
            session_config
                .world_map
                .as_ref()
                .unwrap()
                .tile_items(position)
                .unwrap()[0]
                .server_id,
            2148
        );
        assert_eq!(owner.revision(), 1);
    }

    #[test]
    fn shared_native_map_recovers_revision_matched_source_item_removals() {
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                position,
                vec![
                    WorldMapItem {
                        server_id: 2148,
                        client_thing_id: Some(3031),
                        count: 1,
                        action_id: None,
                        unique_id: None,
                        text: None,
                        description: None,
                        teleport_destination: None,
                        duration: None,
                        charges: None,
                        children: Vec::new(),
                    },
                    WorldMapItem {
                        server_id: 2376,
                        client_thing_id: Some(2376),
                        count: 1,
                        action_id: None,
                        unique_id: None,
                        text: None,
                        description: None,
                        teleport_destination: None,
                        duration: None,
                        charges: None,
                        children: Vec::new(),
                    },
                ],
            )
            .unwrap();
        let removed = source_map.source_item_identity(position, 1).unwrap();
        let journal = MapItemRemovalJournal {
            map_revision: source_map.source_revision(),
            removed_items: vec![removed],
        };

        let recovered =
            SharedNativeMap::recover_from_removal_journal(source_map.clone(), Some(&journal))
                .unwrap();
        let snapshot = recovered.render_snapshot().unwrap();
        assert_eq!(snapshot.tile_items(position).unwrap().len(), 1);
        assert_eq!(snapshot.tile_items(position).unwrap()[0].server_id, 2148);
        assert_eq!(recovered.removal_journal().unwrap(), journal);
        assert_eq!(recovered.revision(), 0);
    }

    #[test]
    fn map_item_count_override_recovery_reduces_runtime_stack_and_rejects_non_reduction() {
        let mut source_map = (*native_world_map()).clone();
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        source_map
            .set_tile_items(
                position,
                vec![WorldMapItem {
                    server_id: 2148,
                    client_thing_id: Some(3031),
                    count: 7,
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
        let identity = source_map.source_item_identity(position, 0).unwrap();
        let overrides = (
            source_map.source_revision(),
            vec![MapItemCountOverrideRecord {
                source_identity: identity,
                remaining_count: 3,
            }],
        );

        let recovered = SharedNativeMap::recover_from_map_item_state(
            source_map.clone(),
            None,
            Some(&overrides),
        )
        .unwrap();
        assert_eq!(source_map.tile_items(position).unwrap()[0].count, 7);
        assert_eq!(
            recovered
                .render_snapshot()
                .unwrap()
                .tile_items(position)
                .unwrap()[0]
                .count,
            3
        );
        assert_eq!(recovered.count_overrides().unwrap(), overrides.1);

        let invalid = (
            source_map.source_revision(),
            vec![MapItemCountOverrideRecord {
                source_identity: identity,
                remaining_count: 7,
            }],
        );
        assert!(matches!(
            SharedNativeMap::recover_from_map_item_state(source_map, None, Some(&invalid)),
            Err(HostError::Core(forgotten_core::CoreError::InvalidMap(_)))
        ));
    }

    #[test]
    fn runtime_corpse_registry_places_persists_and_recovers_across_owners() {
        let database_path = database_path("runtime-corpse-registry");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let source_map = (*native_world_map()).clone();
        let position = Position {
            x: 103,
            y: 102,
            z: 7,
        };
        let owner =
            SharedNativeMap::recover_complete_map_item_state(source_map.clone(), None, None, None)
                .unwrap();
        assert_eq!(owner.runtime_tile_items().unwrap(), Vec::new());

        let corpse = WorldMapItem {
            server_id: 3065,
            client_thing_id: None,
            count: 1,
            action_id: None,
            unique_id: None,
            text: None,
            description: None,
            teleport_destination: None,
            duration: None,
            charges: None,
            children: vec![
                WorldMapItem {
                    server_id: 2148,
                    client_thing_id: None,
                    count: 12,
                    action_id: None,
                    unique_id: None,
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                },
                WorldMapItem {
                    server_id: 2681,
                    client_thing_id: None,
                    count: 2,
                    action_id: None,
                    unique_id: None,
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                },
            ],
        };
        let placed_revision = owner
            .add_runtime_tile_item(&mut database, position, corpse.clone(), None)
            .unwrap()
            .expect("a bounded corpse is placed");
        assert_eq!(placed_revision, 1);
        let snapshot = owner.render_snapshot().unwrap();
        let placed_items = snapshot.tile_items(position).unwrap();
        assert_eq!(placed_items.len(), 1);
        assert_eq!(placed_items[0].server_id, 3065);
        assert_eq!(
            placed_items[0]
                .children
                .iter()
                .map(|child| (child.server_id, child.count))
                .collect::<Vec<_>>(),
            vec![(2148, 12), (2681, 2)]
        );

        // A second corpse on the same tile appends with the next ordinal and advances revision.
        let second = WorldMapItem {
            server_id: 3058,
            client_thing_id: None,
            count: 1,
            action_id: None,
            unique_id: None,
            text: None,
            description: None,
            teleport_destination: None,
            duration: None,
            charges: None,
            children: Vec::new(),
        };
        assert_eq!(
            owner
                .add_runtime_tile_item(&mut database, position, second, None)
                .unwrap()
                .expect("a second corpse is placed"),
            2
        );
        assert_eq!(
            owner
                .render_snapshot()
                .unwrap()
                .tile_items(position)
                .unwrap()
                .len(),
            2
        );
        let registry = owner.runtime_tile_items().unwrap();
        assert_eq!(registry.len(), 2);
        assert_eq!(registry[0].ordinal, 0);
        assert_eq!(registry[1].ordinal, 1);

        // A fresh owner recovers both corpses with their loot children from durable state.
        let recovered_state = database.runtime_map_items().unwrap().expect("registry");
        let recovered = SharedNativeMap::recover_complete_map_item_state(
            source_map.clone(),
            None,
            None,
            Some(&recovered_state),
        )
        .unwrap();
        let recovered_snapshot = recovered.render_snapshot().unwrap();
        let recovered_items = recovered_snapshot.tile_items(position).unwrap();
        assert_eq!(recovered_items.len(), 2);
        assert_eq!(recovered_items[0].server_id, 3065);
        assert_eq!(
            recovered_items[0]
                .children
                .iter()
                .map(|child| (child.server_id, child.count))
                .collect::<Vec<_>>(),
            vec![(2148, 12), (2681, 2)]
        );
        assert_eq!(recovered.runtime_tile_items().unwrap(), registry);
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn runtime_corpse_registry_fails_closed_on_incompatible_durable_state() {
        let source_map = (*native_world_map()).clone();
        let position = Position {
            x: 104,
            y: 102,
            z: 7,
        };
        let incompatible_revision =
            WorldMapSourceRevision(source_map.source_revision().0 ^ 0xdead_beef);
        let stale = (
            incompatible_revision,
            vec![RuntimeMapItemRecord {
                position,
                ordinal: 0,
                server_id: 3065,
                count: 1,
                children: Vec::new(),
                despawn_tick: Some(5),
            }],
        );
        assert!(matches!(
            SharedNativeMap::recover_complete_map_item_state(
                source_map.clone(),
                None,
                None,
                Some(&stale)
            ),
            Err(HostError::Core(forgotten_core::CoreError::InvalidMap(message)))
                if message.contains("revision")
        ));

        let missing_tile = (
            source_map.source_revision(),
            vec![RuntimeMapItemRecord {
                position: Position {
                    x: 200,
                    y: 200,
                    z: 7,
                },
                ordinal: 0,
                server_id: 3065,
                count: 1,
                children: Vec::new(),
                despawn_tick: Some(5),
            }],
        );
        assert!(matches!(
            SharedNativeMap::recover_complete_map_item_state(
                source_map.clone(),
                None,
                None,
                Some(&missing_tile)
            ),
            Err(HostError::Core(forgotten_core::CoreError::InvalidMap(message)))
                if message.contains("missing source tile")
        ));

        let gapped_ordinals = (
            source_map.source_revision(),
            vec![
                RuntimeMapItemRecord {
                    position,
                    ordinal: 0,
                    server_id: 3065,
                    count: 1,
                    children: Vec::new(),
                    despawn_tick: None,
                },
                RuntimeMapItemRecord {
                    position,
                    ordinal: 2,
                    server_id: 3065,
                    count: 1,
                    children: Vec::new(),
                    despawn_tick: None,
                },
            ],
        );
        assert!(matches!(
            SharedNativeMap::recover_complete_map_item_state(
                source_map,
                None,
                None,
                Some(&gapped_ordinals)
            ),
            Err(HostError::Core(forgotten_core::CoreError::InvalidMap(message)))
                if message.contains("contiguous")
        ));
    }

    #[test]
    fn static_defeat_corpse_spawn_persists_the_durable_runtime_registry() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns = FeTfsStaticSpawnCollection::with_loot_tables(
            vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::new(),
            BTreeMap::from([(
                creature_id,
                vec![forgotten_core::StaticCreatureLootEntry {
                    item_id: 2148,
                    chance: 100_000,
                    min_count: 3,
                    max_count: 3,
                }],
            )]),
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        let source_map = (*native_world_map()).clone();
        let map_owner =
            SharedNativeMap::recover_complete_map_item_state(source_map, None, None, None).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: Position {
                        x: 100,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &native_world_map(),
            )
            .unwrap();
        shared
            .set_player_static_target(101, Some(creature_id))
            .unwrap();

        let database_path = database_path("static-defeat-corpse-registry");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        assert_eq!(map_owner.runtime_tile_items().unwrap(), Vec::new());
        assert_eq!(database.runtime_map_items().unwrap(), None);

        let first = apply_native_selected_static_creature_melee(&shared, 101, &native_world_map())
            .unwrap()
            .unwrap();
        assert!(!first.deactivated);
        advance_native_shared_world_heartbeat(&shared, 1).unwrap();
        let final_hit =
            apply_native_selected_static_creature_melee(&shared, 101, &native_world_map())
                .unwrap()
                .unwrap();
        assert!(final_hit.deactivated);

        let corpse_position = spawn_native_static_defeat_corpse(
            &shared,
            &map_owner,
            &mut database,
            creature_id,
            1,
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID,
            0,
            &[],
        )
        .unwrap()
        .expect("a deterministic always-chance loot roll spawns a corpse");
        assert_eq!(
            corpse_position,
            Position {
                x: 101,
                y: 100,
                z: 7,
            }
        );
        let snapshot = map_owner.render_snapshot().unwrap();
        let tile_items = snapshot.tile_items(corpse_position).unwrap();
        assert_eq!(tile_items.len(), 1);
        assert_eq!(
            tile_items[0].server_id,
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID
        );
        assert_eq!(
            tile_items[0]
                .children
                .iter()
                .map(|child| (child.server_id, child.count))
                .collect::<Vec<_>>(),
            vec![(2148, 3)]
        );

        let (revision, records) = database.runtime_map_items().unwrap().expect("registry");
        assert_eq!(revision, map_owner.source_revision());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].position, corpse_position);
        assert_eq!(
            records[0].server_id,
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID
        );
        assert_eq!(
            records[0]
                .children
                .iter()
                .map(|child| (child.server_id, child.count))
                .collect::<Vec<_>>(),
            vec![(2148, 3)]
        );

        // An equal defeat seed reproduces the exact same durable corpse content deterministically.
        let repeat_roll = shared
            .roll_defeated_static_creature_loot(creature_id, 1)
            .unwrap();
        assert_eq!(
            repeat_roll.items,
            vec![(2148_u16, 3_u16)],
            "equal seeds must produce equal loot"
        );
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn party_loot_split_distributes_stacks_round_robin_and_leftovers_stay_in_corpse() {
        // Three deterministic always-chance loot stacks; two party members receive alternating
        // stacks, and a corpse still spawns (always leaves the declared corpse) with no
        // leftover loot because every stack found a member container.
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns = FeTfsStaticSpawnCollection::with_loot_tables(
            vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Splitter".into(),
                name_description: String::new(),
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
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::new(),
            BTreeMap::from([(
                creature_id,
                vec![
                    forgotten_core::StaticCreatureLootEntry {
                        item_id: 2148,
                        chance: 100_000,
                        min_count: 1,
                        max_count: 1,
                    },
                    forgotten_core::StaticCreatureLootEntry {
                        item_id: 2675,
                        chance: 100_000,
                        min_count: 2,
                        max_count: 2,
                    },
                    forgotten_core::StaticCreatureLootEntry {
                        item_id: 2676,
                        chance: 100_000,
                        min_count: 3,
                        max_count: 3,
                    },
                ],
            )]),
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        let source_map = (*native_world_map()).clone();
        let map_owner =
            SharedNativeMap::recover_complete_map_item_state(source_map, None, None, None).unwrap();
        let database_path = database_path("party-loot-split-round-robin");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        // Two persisted characters in one party; split deliveries persist through the same
        // replace_player_containers path as /give, so the rows must exist.
        let leader_account = database.create_account("leader", "password").unwrap();
        let leader_character = database
            .create_player_for_account(leader_account as u32, "Leader")
            .unwrap();
        let member_account = database.create_account("member", "password").unwrap();
        let member_character = database
            .create_player_for_account(member_account as u32, "Member")
            .unwrap();
        let (leader_id, member_id) = (leader_character.id, member_character.id);
        for (player_id, name, account_id) in [
            (leader_id, "Leader", leader_account),
            (member_id, "Member", member_account),
        ] {
            shared
                .register_player_at_available_position(
                    Player {
                        id: player_id,
                        account_id: account_id as u64,
                        name: name.into(),
                        position: Position {
                            x: 100,
                            y: 100,
                            z: 7,
                        },
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &native_world_map(),
                )
                .unwrap();
        }
        shared.invite_to_party(leader_id, member_id).unwrap();
        shared
            .accept_party_invitation(member_id, leader_id)
            .unwrap();
        // Each member needs an owned top-level container with space, like the login
        // provisioning path gives every character.
        for player_id in [leader_id, member_id] {
            let backpack = forgotten_core::PlayerContainer::new(
                0_u8,
                ItemInstance::new(2854_u16, 1_u16).unwrap(),
                "Backpack",
                false,
                20_u16,
            )
            .unwrap();
            shared
                .replace_player_containers(player_id, {
                    let mut containers = forgotten_core::PlayerContainers::default();
                    containers.insert(backpack).unwrap();
                    containers
                })
                .unwrap();
        }
        // Deterministic recipient order: leader first, then ascending members.
        assert_eq!(
            shared.party_loot_split_targets(member_id).unwrap(),
            vec![leader_id, member_id]
        );
        assert_eq!(
            shared.party_loot_split_targets(999).unwrap(),
            Vec::<u64>::new(),
            "a player without a party never triggers distribution"
        );

        let roll = shared
            .roll_defeated_static_creature_loot(creature_id, 1)
            .unwrap();
        assert_eq!(roll.items.len(), 3, "all three entries must always roll");

        let corpse_position = spawn_native_static_defeat_corpse(
            &shared,
            &map_owner,
            &mut database,
            creature_id,
            1,
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID,
            0,
            &[leader_id, member_id],
        )
        .unwrap()
        .expect("split defeat still spawns the declared corpse");

        // Round-robin: stack 1 -> leader(101), stack 2 -> member(102), stack 3 -> leader(101).
        let container_items = |containers: &forgotten_core::PlayerContainers| -> Vec<(u16, u16)> {
            let mut items = Vec::new();
            for (_, container) in containers.iter() {
                for item in container.items.iter() {
                    items.push((item.server_id, item.count));
                }
            }
            items
        };
        let leader_items = container_items(&shared.player_containers(leader_id).unwrap());
        let member_items = container_items(&shared.player_containers(member_id).unwrap());
        assert_eq!(
            leader_items,
            vec![(2148_u16, 1_u16), (2676, 3)],
            "leader receives stacks 1 and 3"
        );
        assert_eq!(
            member_items,
            vec![(2675_u16, 2_u16)],
            "member receives stack 2"
        );

        // The corpse still exists (always leaves the declared corpse) with no leftover loot.
        let snapshot = map_owner.render_snapshot().unwrap();
        let tile_items = snapshot.tile_items(corpse_position).unwrap();
        assert_eq!(tile_items.len(), 1);
        assert!(tile_items[0].children.is_empty());
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn condition_kinds_map_to_legacy_player_state_bits() {
        use forgotten_core::{PlayerCondition, PlayerConditionKind};
        let mut conditions = BTreeMap::new();
        conditions.insert(
            PlayerConditionKind::Poison,
            PlayerCondition::new(PlayerConditionKind::Poison, 4, 2, 16).unwrap(),
        );
        assert_eq!(native_condition_state_bits(&conditions), 0x0001);
        conditions.insert(
            PlayerConditionKind::Energy,
            PlayerCondition::new(PlayerConditionKind::Energy, 4, 2, 16).unwrap(),
        );
        assert_eq!(native_condition_state_bits(&conditions), 0x0005);
        conditions.insert(
            PlayerConditionKind::Burning,
            PlayerCondition::new(PlayerConditionKind::Burning, 4, 2, 16).unwrap(),
        );
        assert_eq!(native_condition_state_bits(&conditions), 0x0007);
    }

    #[test]
    fn dynamic_summon_corpses_take_into_owned_inventory_end_to_end() {
        // Plan v49 slice 7 acceptance: a /spawn summon (dynamic id range) that dies leaves its
        // declared loot inside a runtime corpse, and that corpse takes into owned equipment
        // through the exact registry path socket clients use.
        let template_id = 0x5000_0001;
        let mut template = forgotten_core::FeTfsStaticEntity {
            id: template_id,
            name: "Lootrat".into(),
            name_description: String::new(),
            position: Position {
                x: 105,
                y: 103,
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
        let _ = &mut template;
        let static_spawns = FeTfsStaticSpawnCollection::with_loot_tables(
            vec![template],
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeSet::new(),
            std::collections::BTreeMap::from([(
                template_id,
                vec![forgotten_core::StaticCreatureLootEntry {
                    item_id: 2148,
                    chance: 100_000,
                    min_count: 3,
                    max_count: 3,
                }],
            )]),
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        let map_owner = SharedNativeMap::recover_complete_map_item_state(
            (*native_world_map()).clone(),
            None,
            None,
            None,
        )
        .unwrap();
        let database_path = database_path("dynamic-summon-corpse-take");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let player_id = 101_u64;
        database
            .save_player(&Player {
                id: player_id,
                account_id: u64::from(u32::try_from(account_id).unwrap()),
                name: "Knight".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 0,
                skill_points: 0,
            })
            .unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: player_id,
                    account_id: u64::from(u32::try_from(account_id).unwrap()),
                    name: "Knight".into(),
                    position: Position {
                        x: 100,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &native_world_map(),
            )
            .unwrap();
        // Park the import template far away and deactivate it: only the summoned clone fights.
        shared
            .lock()
            .unwrap()
            .deactivate_static_creature(template_id)
            .unwrap();

        let dynamic_id = shared
            .spawn_dynamic_entity_in_front_of_player(101, "Lootrat")
            .unwrap();
        assert!(dynamic_id >= 0x7000_0000);
        shared
            .set_player_static_target(101, Some(dynamic_id))
            .unwrap();

        let mut deactivated = false;
        for _ in 0..40 {
            if let Some(outcome) =
                apply_native_selected_static_creature_melee(&shared, 101, &native_world_map())
                    .unwrap()
            {
                persist_static_creature_runtime_to_open_database(&shared, &mut database).unwrap();
                if outcome.deactivated {
                    deactivated = true;
                    break;
                }
                advance_native_shared_world_heartbeat(&shared, 2).unwrap();
            }
        }
        assert!(deactivated, "the summoned creature must reach defeat");

        let corpse_position = spawn_native_static_defeat_corpse(
            &shared,
            &map_owner,
            &mut database,
            dynamic_id,
            7,
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID,
            0,
            &[],
        )
        .unwrap()
        .expect("defeated summons leave their declared corpse");
        let runtime_corpse = map_owner
            .runtime_tile_item(corpse_position, 0)
            .unwrap()
            .expect("corpse registered on the tile");
        assert_eq!(
            runtime_corpse.server_id,
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID
        );
        assert_eq!(
            runtime_corpse
                .children
                .iter()
                .map(|child| child.server_id)
                .collect::<Vec<_>>(),
            vec![2148]
        );

        // Take the rolled gold from the open corpse window into the empty right hand.
        map_owner
            .move_runtime_item_to_inventory(
                &shared,
                &mut database,
                101,
                corpse_position,
                0,
                Some(0),
                3,
                forgotten_core::PlayerGroundDropSource::EquipmentSlot(EquipmentSlot::RightHand),
                None,
            )
            .unwrap()
            .expect("dynamic corpse take succeeds");
        assert_eq!(
            database
                .player_equipment(101)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .map(|item| (item.server_id, item.count)),
            Some((2148, 3))
        );
        drop(database);
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn fired_rune_charges_decrement_owned_container_stacks_and_persist() {
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                3198,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 11698,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        let database_path = database_path("rune-charge-consume");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 101,
                account_id: u64::from(u32::try_from(account_id).unwrap()),
                name: "Knight".into(),
                position: map.spawn(),
                level: 8,
                experience: 0,
                skill_points: 0,
            })
            .unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: u64::from(u32::try_from(account_id).unwrap()),
                    name: "Knight".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &map,
            )
            .unwrap();
        let mut backpack =
            PlayerContainer::new(2, ItemInstance::new(1988, 1).unwrap(), "Backpack", false, 4)
                .unwrap();
        backpack
            .items
            .insert(ItemInstance::new(3198, 3).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        shared.replace_player_containers(101, containers).unwrap();
        let mut sent_container_windows: BTreeMap<u8, NativeRenderedContainerWindow> =
            native_rendered_container_windows(
                &profile,
                Some(&catalog),
                &shared.player_containers(101).unwrap(),
                &BTreeSet::new(),
            );

        // Classic flagged container address: window 2, item index 0.
        let source = NativeOtClientPosition {
            x: u16::MAX,
            y: 0x40 | 2,
            z: 0,
        };
        assert!(consume_declared_rune_charge(
            &shared,
            &mut database,
            101,
            source,
            0,
            &profile,
            Some(&catalog),
            &mut sent_container_windows
        ));
        assert_eq!(
            database
                .player_containers(101)
                .unwrap()
                .container(2)
                .unwrap()
                .items
                .item(0)
                .map(|item| (item.server_id, item.count)),
            Some((3198, 2))
        );
        // The baseline tracked the consumed stack's current rendering (count is carried by
        // subtype only for catalog-declared stackables, absent here).
        assert_eq!(
            sent_container_windows
                .get(&2)
                .and_then(|window| window.as_ref())
                .and_then(|(_, slots)| slots.first())
                .map(|record| (record.client_thing_id, record.subtype)),
            Some((11698, None))
        );

        // Map-tile and equipment-style sources are not owned container stacks.
        let map_source = NativeOtClientPosition {
            x: 102,
            y: 100,
            z: 7,
        };
        assert!(!consume_declared_rune_charge(
            &shared,
            &mut database,
            101,
            map_source,
            0,
            &profile,
            Some(&catalog),
            &mut sent_container_windows
        ));
        drop(database);
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn gm_item_freeze_and_quest_completion_flow() {
        let path = database_path("gm-item-freeze-quest");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id: i64 = database.create_account("owner", "password").unwrap();
        let character = database
            .create_player_for_account(account_id as u32, "Knight")
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();

        shared
            .register_player_at_available_position(
                Player {
                    id: u64::from(character.id),
                    account_id: account_id as u64,
                    name: "Knight".into(),
                    position: Position {
                        x: 100,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &native_world_map(),
            )
            .unwrap();

        // /item self-delivers into the first container.
        let starter_containers = {
            let mut containers = PlayerContainers::default();
            containers
                .insert(
                    PlayerContainer::new(
                        0,
                        ItemInstance::new(1988, 1).unwrap(),
                        "Backpack",
                        false,
                        20,
                    )
                    .unwrap(),
                )
                .unwrap();
            containers
        };
        database
            .replace_player_containers(u64::from(character.id), &starter_containers)
            .unwrap();
        shared
            .replace_player_containers(u64::from(character.id), starter_containers)
            .unwrap();

        let reply = handle_native_gm_talkaction(
            &shared,
            &mut database,
            u64::from(character.id),
            "/item 2148 100",
            2,
            None,
        )
        .unwrap()
        .unwrap();
        assert!(reply.contains("Delivered"));
        assert!(database
            .player_containers(u64::from(character.id))
            .unwrap()
            .container(0)
            .unwrap()
            .items
            .iter()
            .any(|item| item.server_id == 2148 && item.count == 100));

        // Freeze persists and survives relog reads.
        handle_native_gm_talkaction(
            &shared,
            &mut database,
            u64::from(character.id),
            "/unfreeze Knight",
            1,
            None,
        )
        .unwrap();
        let frozen_reply = handle_native_gm_talkaction(
            &shared,
            &mut database,
            u64::from(character.id),
            "/freeze Knight",
            1,
            None,
        )
        .unwrap()
        .unwrap();
        assert!(frozen_reply.contains("Froze Knight"));
        assert!(database.player_frozen(u64::from(character.id)).unwrap());

        // Quest completion flips the flag and grants declared rewards into the backpack.
        let mut quest_catalog = QuestCatalog::default();
        let mut quest_bytes = Vec::new();
        quest_bytes.extend_from_slice(
            br#"<fe-quests><fe-quest id="7" name="Rat Hunt"><fe-reward itemid="2148" count="50"/></fe-quest></fe-quests>"#,
        );
        quest_catalog = parse_quests_xml(&quest_bytes).unwrap();
        database
            .replace_player_quests(u64::from(character.id), &[(7_u16, false)])
            .unwrap();
        let granted = complete_native_player_quest(
            &shared,
            &mut database,
            u64::from(character.id),
            7,
            Some(&quest_catalog),
        )
        .unwrap()
        .expect("first completion grants");
        assert_eq!(granted, vec![(2148, 50)]);
        assert!(database
            .player_quests(u64::from(character.id))
            .unwrap()
            .iter()
            .any(|(id, completed)| *id == 7 && *completed));
        // Second completion is a no-op.
        assert!(complete_native_player_quest(
            &shared,
            &mut database,
            u64::from(character.id),
            7,
            Some(&quest_catalog)
        )
        .unwrap()
        .is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn guild_members_get_channel_entry_and_motd_context() {
        let path = database_path("guild-channel-motd");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id: i64 = database.create_account("leader", "password").unwrap();
        let character = database
            .create_player_for_account(account_id as u32, "Leader")
            .unwrap();
        let guild = database
            .create_guild(character.id, "Iron Vanguard", "Raid at sunset")
            .unwrap();

        // Non-members get nothing.
        assert!(native_guild_channel_context(&database, 999).is_none());

        let (channel, motd) =
            native_guild_channel_context(&database, character.id).expect("member context");
        assert_eq!(channel.id, NATIVE_GUILD_CHAT_CHANNEL_ID);
        assert_eq!(channel.name, "Iron Vanguard");
        assert_eq!(motd, "Raid at sunset");
        assert_eq!(
            database.guild_name_and_motd(guild.id).unwrap(),
            Some(("Iron Vanguard".to_owned(), "Raid at sunset".to_owned()))
        );
        // Unknown guild ids stay None.
        assert!(database.guild_name_and_motd(0).unwrap().is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn native_declared_corpse_resolution_prefers_the_imported_declaration() {
        let declared = BTreeMap::from([("rat".to_owned(), 3073_u16)]);
        assert_eq!(
            native_declared_corpse_server_id(Some(&declared), Some("Rat".into())),
            3073
        );
        // Name matching is case-insensitive; unknown and missing names use the default.
        assert_eq!(
            native_declared_corpse_server_id(Some(&declared), Some("rat".into())),
            3073
        );
        assert_eq!(
            native_declared_corpse_server_id(Some(&declared), Some("Dragon".into())),
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID
        );
        assert_eq!(
            native_declared_corpse_server_id(None, Some("Rat".into())),
            NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID
        );
    }

    #[test]
    fn defeat_spawns_the_declared_corpse_even_when_loot_is_empty() {
        let creature_id = 0x4000_0009;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
        let map_owner = SharedNativeMap::recover_complete_map_item_state(
            (*native_world_map()).clone(),
            None,
            None,
            None,
        )
        .unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: 101,
                    account_id: 1,
                    name: "Knight".into(),
                    position: Position {
                        x: 100,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 0,
                    skill_points: 0,
                },
                &native_world_map(),
            )
            .unwrap();
        shared
            .set_player_static_target(101, Some(creature_id))
            .unwrap();

        let database_path = database_path("declared-corpse-no-loot");
        let mut database = EngineDatabase::open(&database_path).unwrap();

        // Empty loot rolls previously swallowed the corpse entirely (plan v49 slice 6 gap).
        let roll = shared
            .roll_defeated_static_creature_loot(creature_id, 1)
            .unwrap();
        let _ = roll;

        let mut deactivated = false;
        for _ in 0..40 {
            if let Some(outcome) =
                apply_native_selected_static_creature_melee(&shared, 101, &native_world_map())
                    .unwrap()
            {
                if outcome.deactivated {
                    deactivated = true;
                    break;
                }
                advance_native_shared_world_heartbeat(&shared, 1).unwrap();
            }
        }
        assert!(deactivated, "creature must reach its death transition");

        let corpse_position = spawn_native_static_defeat_corpse(
            &shared,
            &map_owner,
            &mut database,
            creature_id,
            1,
            3073,
            0,
            &[],
        )
        .unwrap()
        .expect("defeated creatures always leave their declared corpse");
        let snapshot = map_owner.render_snapshot().unwrap();
        let tile_items = snapshot.tile_items(corpse_position).unwrap();
        assert_eq!(tile_items.len(), 1);
        assert_eq!(tile_items[0].server_id, 3073);
        assert!(tile_items[0].children.is_empty());
        drop(database);
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn runtime_tile_item_lookup_distinguishes_source_items_from_runtime_additions() {
        let mut source_map = (*native_world_map()).clone();
        let position = Position {
            x: 105,
            y: 103,
            z: 7,
        };
        source_map
            .set_tile_items(
                position,
                vec![WorldMapItem {
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
        let corpse_position = Position {
            x: 106,
            y: 103,
            z: 7,
        };
        let registry = (
            source_map.source_revision(),
            vec![RuntimeMapItemRecord {
                position: corpse_position,
                ordinal: 0,
                server_id: 3065,
                count: 1,
                children: Vec::new(),
                despawn_tick: None,
            }],
        );
        let owner = SharedNativeMap::recover_complete_map_item_state(
            source_map,
            None,
            None,
            Some(&registry),
        )
        .unwrap();

        // A corpse alone on its tile occupies index 0 because no surviving source item precedes
        // it; imported items never resolve through the runtime boundary.
        assert!(owner
            .runtime_tile_item(corpse_position, 0)
            .unwrap()
            .is_some_and(|item| item.server_id == 3065));
        assert_eq!(owner.runtime_tile_item(corpse_position, 1).unwrap(), None);
        // Imported-only tiles never resolve through the runtime boundary.
        assert_eq!(owner.runtime_tile_item(position, 0).unwrap(), None);
        // Missing tiles and out-of-range indexes stay None rather than erroring.
        assert_eq!(
            owner
                .runtime_tile_item(
                    Position {
                        x: 200,
                        y: 200,
                        z: 7
                    },
                    0
                )
                .unwrap(),
            None
        );
    }

    #[test]
    fn native_throw_item_takes_loot_from_an_open_corpse_window() {
        let database_path = database_path("native-corpse-take");
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

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        let corpse_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        {
            Arc::get_mut(native_config.world_map.as_mut().unwrap())
                .unwrap()
                .set_tile_items(
                    corpse_position,
                    vec![WorldMapItem {
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
        }
        let source_revision = native_config.world_map.as_ref().unwrap().source_revision();
        database
            .replace_runtime_map_items(
                source_revision,
                &[RuntimeMapItemRecord {
                    position: corpse_position,
                    ordinal: 0,
                    server_id: NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID,
                    count: 1,
                    children: vec![
                        RuntimeMapItemChildRecord {
                            server_id: 2148,
                            count: 12,
                        },
                        RuntimeMapItemChildRecord {
                            server_id: 2681,
                            count: 2,
                        },
                    ],
                    despawn_tick: None,
                }],
            )
            .unwrap();
        drop(database);

        let mut catalog = NativeItemPresentationCatalog::default();
        for server_id in [3065, 2148, 2681, 1988] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id: server_id,
                        requires_classic_740_subtype: false,
                    },
                )
                .unwrap();
        }
        native_config.item_presentation_catalog = Some(Arc::new(catalog));
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
        read_data_frame(&mut stream);

        // Open the corpse window (stack index 1 above the imported item).
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                101,
                0,
                100,
                0,
                7,
                0xf9,
                0x0b,
                1,
                0,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_data_frame(&mut stream).0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER
        );

        // Take all twelve gold coins from corpse child 0 into the empty right hand.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                0xff,
                0xff,
                0x4f,
                0x00,
                0x00,
                0x64,
                0x08,
                0x00,
                0xff,
                0xff,
                5,
                0x00,
                0x00,
                12,
            ]),
        )
        .unwrap();
        // The refreshed corpse window lists only the remaining food child.
        let refreshed_corpse = read_data_frame(&mut stream);
        assert_eq!(
            refreshed_corpse.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER
        );
        // [opcode, id, corpse-record(2), len(2)="corpse"(6), capacity, parent, count=1, item]
        assert_eq!(&refreshed_corpse.0[6..12], b"corpse");
        assert_eq!(refreshed_corpse.0[14], 1);
        assert_eq!(&refreshed_corpse.0[15..17], &[0x79, 0x0a]);
        // The right hand receives the exact moved stack.
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                EquipmentSlot::RightHand.code(),
                0x64,
                0x08
            ]
        );
        // A full-map refresh follows the registry change.
        assert_eq!(
            read_data_frame(&mut stream).0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );

        // Durable state: the corpse keeps one child and the hand holds the coins.
        drop(stream);
        game.shutdown().unwrap();
        let database = EngineDatabase::open(&database_path).unwrap();
        let (_, records) = database.runtime_map_items().unwrap().expect("registry");
        assert_eq!(records[0].children.len(), 1);
        assert_eq!(records[0].children[0].server_id, 2681);
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .map(|item| (item.server_id, item.count)),
            Some((2148, 12))
        );
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_item_picks_up_a_dropped_ground_stack_into_equipment() {
        let database_path = database_path("native-ground-pickup");
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
        let stack_position = Position {
            x: 101,
            y: 101,
            z: 7,
        };
        let source_revision = native_config.world_map.as_ref().unwrap().source_revision();
        {
            let mut database = EngineDatabase::open(&database_path).unwrap();
            database
                .replace_runtime_map_items(
                    source_revision,
                    &[RuntimeMapItemRecord {
                        position: stack_position,
                        ordinal: 0,
                        server_id: 2148,
                        count: 10,
                        children: Vec::new(),
                        despawn_tick: None,
                    }],
                )
                .unwrap();
        }

        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                2148,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 2148,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        native_config.item_presentation_catalog = Some(Arc::new(catalog));
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
        read_data_frame(&mut stream);

        // Pick the whole dropped stack up from its solo-tile tail index into the right hand.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                101,
                0,
                101,
                0,
                7,
                0x64,
                0x08,
                0,
                0xff,
                0xff,
                5,
                0,
                0,
                10,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                EquipmentSlot::RightHand.code(),
                0x64,
                0x08
            ]
        );
        let refreshed = read_data_frame(&mut stream);
        assert_eq!(
            refreshed.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(!refreshed.0.windows(2).any(|window| window == [0x64, 0x08]));

        drop(stream);
        game.shutdown().unwrap();
        let database = EngineDatabase::open(&database_path).unwrap();
        assert_eq!(database.runtime_map_items().unwrap(), None);
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .map(|item| (item.server_id, item.count)),
            Some((2148, 10))
        );
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn move_player_stack_to_ground_unit_persists_and_publishes() {
        let database_path = database_path("ground-drop-unit");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account("ground-drop-operator", "hash")
            .unwrap();
        database
            .save_player(&Player {
                id: 9,
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
        let source_map = (*native_world_map()).clone();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let owner =
            SharedNativeMap::recover_complete_map_item_state(source_map.clone(), None, None, None)
                .unwrap();
        let map_snapshot = owner.render_snapshot().unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            forgotten_core::ItemInstance::new(2148, 10).unwrap(),
        );
        shared
            .register_player_at_available_position_with_vitals_equipment_containers_progression_and_conditions(
                Player {
                    id: 9,
                    account_id: 1,
                    name: "Knight".into(),
                    position: Position {
                        x: 100,
                        y: 100,
                        z: 7,
                    },
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                PlayerVitals::default(),
                NativePlayerHydration {
                    progression: PlayerProgression::default(),
                    progression_attempts: PlayerProgressionAttempts::default(),
                    town_id: 1,
                    respawn_state: PlayerRespawnState::default(),
                    equipment,
                    containers: PlayerContainers::default(),
                    conditions: BTreeMap::new(),
                },
                &map_snapshot,
            )
            .unwrap();
        let outcome = owner
            .move_player_stack_to_ground(
                &shared,
                &mut database,
                9,
                forgotten_core::PlayerGroundDropSource::EquipmentSlot(EquipmentSlot::RightHand),
                Position {
                    x: 101,
                    y: 101,
                    z: 7,
                },
                4,
                None,
            )
            .unwrap()
            .expect("adjacent walkable drop succeeds");
        assert_eq!(outcome.moved_item.count, 4);
        assert_eq!(outcome.source_remaining_count, Some(6));
        assert_eq!(
            shared
                .player_equipment(9)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .map(|i| i.count),
            Some(6)
        );
        let (_, records) = database.runtime_map_items().unwrap().expect("registry");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].count, 4);
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_item_drops_an_equipped_stack_onto_the_ground_and_persists_it() {
        let database_path = database_path("native-ground-drop");
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
        // Ten gold coins in the right hand; four will be dropped onto an adjacent tile.
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            forgotten_core::ItemInstance::new(2148, 10).unwrap(),
        );
        database.replace_player_equipment(1, &equipment).unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        // One imported item on the destination tile means the dropped stack lands at a
        // client-visible tail index of that tile's ordered list.
        {
            Arc::get_mut(native_config.world_map.as_mut().unwrap())
                .unwrap()
                .set_tile_items(
                    Position {
                        x: 101,
                        y: 101,
                        z: 7,
                    },
                    vec![WorldMapItem {
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
        }
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                2148,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 2148,
                    requires_classic_740_subtype: true,
                },
            )
            .unwrap();
        native_config.item_presentation_catalog = Some(Arc::new(catalog));
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
        let initialization = read_data_frame(&mut stream);
        assert_eq!(
            initialization.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        // Bootstrap delivers the equipped stack as its own inventory record.
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                EquipmentSlot::RightHand.code(),
                0x64,
                0x08,
                10
            ]
        );

        // Drop four of the ten right-hand coins onto the adjacent south-east tile.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                0xff,
                0xff,
                5,
                0,
                0,
                0x64,
                0x08,
                0,
                101,
                0,
                101,
                0,
                7,
                4,
            ]),
        )
        .unwrap();
        // The remaining six coins emit an exact set delta, then the map refresh arrives.
        let delete_delta = read_data_frame(&mut stream);
        assert_eq!(
            delete_delta.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                EquipmentSlot::RightHand.code(),
                0x64,
                0x08,
                6
            ]
        );
        let refreshed = read_data_frame(&mut stream);
        assert_eq!(
            refreshed.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert!(refreshed.0.windows(2).any(|window| window == [0x64, 0x08]));

        // The dropped stack is durable registry state and the slot keeps its remainder.
        drop(stream);
        game.shutdown().unwrap();
        let database = EngineDatabase::open(&database_path).unwrap();
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .map(|item| item.count),
            Some(6)
        );
        let (_, records) = database.runtime_map_items().unwrap().expect("registry");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].position.x, 101);
        assert_eq!(records[0].position.y, 101);
        assert_eq!(records[0].server_id, 2148);
        assert_eq!(records[0].count, 4);
        assert_eq!(records[0].despawn_tick, None);
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn runtime_corpse_despawn_removes_only_due_items_and_persists_survivors() {
        let database_path = database_path("runtime-corpse-despawn");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let source_map = (*native_world_map()).clone();
        let due_position = Position {
            x: 107,
            y: 104,
            z: 7,
        };
        let immortal_position = Position {
            x: 108,
            y: 104,
            z: 7,
        };
        let registry = (
            source_map.source_revision(),
            vec![
                RuntimeMapItemRecord {
                    position: due_position,
                    ordinal: 0,
                    server_id: 3065,
                    count: 1,
                    children: vec![RuntimeMapItemChildRecord {
                        server_id: 2148,
                        count: 4,
                    }],
                    despawn_tick: Some(5),
                },
                RuntimeMapItemRecord {
                    position: immortal_position,
                    ordinal: 0,
                    server_id: 3065,
                    count: 1,
                    children: Vec::new(),
                    despawn_tick: None,
                },
            ],
        );
        let owner = SharedNativeMap::recover_complete_map_item_state(
            source_map.clone(),
            None,
            None,
            Some(&registry),
        )
        .unwrap();

        // Before the due tick nothing is removed and no durable change happens.
        assert_eq!(
            owner
                .remove_expired_runtime_items(&mut database, 4)
                .unwrap(),
            Vec::<Position>::new()
        );
        assert!(owner
            .render_snapshot()
            .unwrap()
            .tile_items(due_position)
            .is_some());
        assert_eq!(owner.runtime_tile_items().unwrap().len(), 2);

        // At the due tick exactly the doomed corpse disappears from memory and disk.
        let removed = owner
            .remove_expired_runtime_items(&mut database, 5)
            .unwrap();
        assert_eq!(removed, vec![due_position]);
        let snapshot = owner.render_snapshot().unwrap();
        assert!(snapshot.tile_items(due_position).unwrap().is_empty());
        assert!(snapshot.tile_items(immortal_position).unwrap().len() == 1);
        let (_, survivors) = database.runtime_map_items().unwrap().expect("registry");
        assert_eq!(survivors.len(), 1);
        assert_eq!(survivors[0].position, immortal_position);
        assert_eq!(survivors[0].despawn_tick, None);

        // An immortal-only registry never expires.
        assert_eq!(
            owner
                .remove_expired_runtime_items(&mut database, u64::MAX)
                .unwrap(),
            Vec::<Position>::new()
        );
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_use_item_opens_a_runtime_corpse_container_window() {
        let database_path = database_path("native-corpse-window");
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

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        // Persist one corpse beside an imported item on an adjacent tile before startup.
        let corpse_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        {
            Arc::get_mut(native_config.world_map.as_mut().unwrap())
                .unwrap()
                .set_tile_items(
                    corpse_position,
                    vec![WorldMapItem {
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
        }
        let source_revision = native_config.world_map.as_ref().unwrap().source_revision();
        database
            .replace_runtime_map_items(
                source_revision,
                &[RuntimeMapItemRecord {
                    position: corpse_position,
                    ordinal: 0,
                    server_id: NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID,
                    count: 1,
                    children: vec![
                        RuntimeMapItemChildRecord {
                            server_id: 2148,
                            count: 12,
                        },
                        RuntimeMapItemChildRecord {
                            server_id: 2681,
                            count: 2,
                        },
                    ],
                    despawn_tick: None,
                }],
            )
            .unwrap();
        drop(database);

        let mut catalog = NativeItemPresentationCatalog::default();
        for server_id in [3065, 2148, 2681, 1988] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id: server_id,
                        requires_classic_740_subtype: false,
                    },
                )
                .unwrap();
        }
        native_config.item_presentation_catalog = Some(Arc::new(catalog));
        native_config.item_name_by_server_id = Some(Arc::new(BTreeMap::from([(
            (3065_u16),
            "dead rat".to_string(),
        )])));

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

        // Use the corpse at stack index 1 (above the imported source item).
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                101,
                0,
                100,
                0,
                7,
                0xf9,
                0x0b,
                1,
                0,
            ]),
        )
        .unwrap();
        let expected_window = vec![
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER,
            0x0f,
            0xf9,
            0x0b,
            8,
            0,
            b'd',
            b'e',
            b'a',
            b'd',
            b' ',
            b'r',
            b'a',
            b't',
            2,
            0,
            2,
            0x64,
            0x08,
            0x79,
            0x0a,
        ];
        assert_eq!(read_data_frame(&mut stream).0, expected_window);

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_CLOSE_CONTAINER,
                0x0f,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CLOSE_CONTAINER,
                0x0f
            ]
        );
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_game_startup_rematerializes_persisted_corpses_into_the_initial_viewport() {
        let database_path = database_path("native-corpse-restart");
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

        let mut source_map = (*native_world_map()).clone();
        let corpse_position = Position {
            x: 102,
            y: 101,
            z: 7,
        };
        // One imported item keeps the runtime corpse at a rendered tile index.
        source_map
            .set_tile_items(
                corpse_position,
                vec![WorldMapItem {
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
        database
            .replace_runtime_map_items(
                source_map.source_revision(),
                &[RuntimeMapItemRecord {
                    position: corpse_position,
                    ordinal: 0,
                    server_id: NATIVE_OTCLIENT_DEFAULT_CORPSE_SERVER_ID,
                    count: 1,
                    children: vec![RuntimeMapItemChildRecord {
                        server_id: 2148,
                        count: 7,
                    }],
                    despawn_tick: Some(999),
                }],
            )
            .unwrap();
        drop(database);

        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.world_map = Some(Arc::new(source_map));
        let game = start_native_otclient_game(config, &database_path).unwrap();
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
        // The recovered corpse is encoded as its server item id (3065 = 0x0BF9 little-endian)
        // on its tile next to the imported item.
        assert!(initialization
            .0
            .windows(2)
            .any(|window| window == [0xf9, 0x0b]));
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_game_startup_rejects_a_map_item_journal_with_a_different_source_revision() {
        let database_path = database_path("native-map-journal-revision-mismatch");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let source_map = native_world_map();
        let incompatible_revision = WorldMapSourceRevision(source_map.source_revision().0 ^ 1);
        database
            .replace_map_item_removal_journal(&MapItemRemovalJournal {
                map_revision: incompatible_revision,
                removed_items: vec![WorldMapItemSourceIdentity {
                    map_revision: incompatible_revision,
                    position: Position {
                        x: 100,
                        y: 100,
                        z: 7,
                    },
                    item_index: 0,
                }],
            })
            .unwrap();
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.world_map = Some(source_map);

        assert!(matches!(
            start_native_otclient_game(config, &database_path),
            Err(HostError::Core(forgotten_core::CoreError::InvalidMap(_)))
        ));
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn source_map_complete_stack_merges_into_matching_equipment_stack() {
        let database_path = database_path("source-map-item-pickup");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let player = Player {
            id: 701,
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
        };
        database.save_player(&player).unwrap();
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                position,
                vec![WorldMapItem {
                    server_id: 2148,
                    client_thing_id: Some(3031),
                    count: 7,
                    action_id: Some(12),
                    unique_id: Some(34),
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            )
            .unwrap();
        let shared_map = SharedNativeMap::new(source_map.clone());
        let shared_world = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared_world
            .register_player_at_available_position(player, &source_map)
            .unwrap();
        let mut equipment = PlayerEquipment::default();
        let mut existing_stack = ItemInstance::new(2148, 5).unwrap();
        existing_stack.action_id = Some(12);
        existing_stack.unique_id = Some(34);
        equipment.equip(EquipmentSlot::RightHand, existing_stack);
        database.replace_player_equipment(701, &equipment).unwrap();
        shared_world
            .replace_player_equipment(701, equipment)
            .unwrap();

        let outcome = shared_map
            .move_source_item_to_empty_equipment(
                &shared_world,
                &mut database,
                701,
                position,
                0,
                EquipmentSlot::RightHand,
            )
            .unwrap();

        assert_eq!(outcome.player_id, 701);
        assert_eq!(
            outcome.source_identity.map_revision,
            source_map.source_revision()
        );
        assert_eq!(outcome.source_identity.position, position);
        assert_eq!(outcome.source_identity.item_index, 0);
        assert_eq!(outcome.item.server_id, 2148);
        assert_eq!(outcome.item.count, 7);
        assert_eq!(outcome.item.action_id, Some(12));
        assert_eq!(outcome.item.unique_id, Some(34));
        assert_eq!(outcome.equipment_slot, EquipmentSlot::RightHand);
        assert_eq!(outcome.map_revision, 1);
        assert_eq!(shared_map.revision(), 1);
        assert_eq!(shared_world.equipment_epoch(), 2);
        assert!(shared_map
            .render_snapshot()
            .unwrap()
            .tile_items(position)
            .unwrap()
            .is_empty());
        let equipment = shared_world.player_equipment(701).unwrap();
        let mut merged_stack = ItemInstance::new(2148, 12).unwrap();
        merged_stack.action_id = Some(12);
        merged_stack.unique_id = Some(34);
        assert_eq!(
            equipment.item(EquipmentSlot::RightHand),
            Some(&merged_stack)
        );
        assert_eq!(database.player_equipment(701).unwrap(), equipment);
        assert_eq!(
            database.map_item_removal_journal().unwrap(),
            Some(MapItemRemovalJournal {
                map_revision: source_map.source_revision(),
                removed_items: vec![outcome.source_identity],
            })
        );
        assert_eq!(
            shared_map.removal_journal().unwrap(),
            database.map_item_removal_journal().unwrap().unwrap()
        );

        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn source_map_partial_stack_transfer_persists_inventory_and_count_override() {
        let database_path = database_path("source-map-item-container-transfer");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let player = Player {
            id: 703,
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
        };
        database.save_player(&player).unwrap();
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                position,
                vec![WorldMapItem {
                    server_id: 2148,
                    client_thing_id: Some(3031),
                    count: 7,
                    action_id: Some(12),
                    unique_id: Some(34),
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            )
            .unwrap();
        let shared_map = SharedNativeMap::new(source_map.clone());
        let shared_world = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared_world
            .register_player_at_available_position(player, &source_map)
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers
            .insert(
                forgotten_core::PlayerContainer::new(
                    2,
                    ItemInstance::new(1988, 1).unwrap(),
                    "Bag",
                    false,
                    20,
                )
                .unwrap(),
            )
            .unwrap();
        database
            .replace_player_containers(703, &containers)
            .unwrap();
        shared_world
            .replace_player_containers(703, containers)
            .unwrap();

        let outcome = shared_map
            .move_source_item_stack_to_top_level_container(
                &shared_world,
                &mut database,
                703,
                position,
                0,
                3,
                2,
            )
            .unwrap();

        let containers = shared_world.player_containers(703).unwrap();
        assert_eq!(outcome.player_id, 703);
        assert_eq!(outcome.container_id, 2);
        assert_eq!(outcome.item.server_id, 2148);
        assert_eq!(outcome.item.count, 3);
        assert_eq!(outcome.item.action_id, Some(12));
        assert_eq!(outcome.item.unique_id, Some(34));
        assert_eq!(
            containers.container(2).unwrap().items.item(0),
            Some(&outcome.item)
        );
        assert_eq!(database.player_containers(703).unwrap(), containers);
        assert_eq!(
            shared_map
                .render_snapshot()
                .unwrap()
                .tile_items(position)
                .unwrap()[0]
                .count,
            4
        );
        assert_eq!(shared_world.containers_epoch(), 2);
        assert_eq!(database.map_item_removal_journal().unwrap(), None);
        let expected_overrides = Some((
            source_map.source_revision(),
            vec![MapItemCountOverrideRecord {
                source_identity: outcome.source_identity,
                remaining_count: 4,
            }],
        ));
        assert_eq!(
            database.map_item_count_overrides().unwrap(),
            expected_overrides
        );
        let recovered = SharedNativeMap::recover_from_map_item_state(
            source_map,
            None,
            expected_overrides.as_ref(),
        )
        .unwrap();
        assert_eq!(
            recovered
                .render_snapshot()
                .unwrap()
                .tile_items(position)
                .unwrap()[0]
                .count,
            4
        );
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn source_map_partial_stack_transfer_to_equipment_persists_count_override_and_full_removal() {
        let database_path = database_path("source-map-item-equipment-transfer");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let player = Player {
            id: 705,
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
        };
        database.save_player(&player).unwrap();
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                position,
                vec![WorldMapItem {
                    server_id: 2148,
                    client_thing_id: Some(3031),
                    count: 7,
                    action_id: Some(12),
                    unique_id: Some(34),
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            )
            .unwrap();
        let shared_map = SharedNativeMap::new(source_map.clone());
        let shared_world = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared_world
            .register_player_at_available_position(player, &source_map)
            .unwrap();

        let partial = shared_map
            .move_source_item_stack_to_equipment(
                &shared_world,
                &mut database,
                705,
                position,
                0,
                3,
                EquipmentSlot::RightHand,
            )
            .unwrap();

        assert_eq!(partial.player_id, 705);
        assert_eq!(partial.item.count, 3);
        assert_eq!(partial.equipment_slot, EquipmentSlot::RightHand);
        assert_eq!(
            shared_world
                .player_equipment(705)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .unwrap()
                .count,
            3
        );
        assert_eq!(
            shared_map
                .render_snapshot()
                .unwrap()
                .tile_items(position)
                .unwrap()[0]
                .count,
            4
        );
        assert_eq!(database.map_item_removal_journal().unwrap(), None);
        let expected_overrides = Some((
            source_map.source_revision(),
            vec![MapItemCountOverrideRecord {
                source_identity: partial.source_identity,
                remaining_count: 4,
            }],
        ));
        assert_eq!(
            database.map_item_count_overrides().unwrap(),
            expected_overrides
        );
        assert_eq!(
            shared_map.count_overrides().unwrap(),
            expected_overrides.as_ref().unwrap().1
        );
        let recovered = SharedNativeMap::recover_from_map_item_state(
            source_map.clone(),
            None,
            expected_overrides.as_ref(),
        )
        .unwrap();
        assert_eq!(
            recovered
                .render_snapshot()
                .unwrap()
                .tile_items(position)
                .unwrap()[0]
                .count,
            4
        );

        let complete = shared_map
            .move_source_item_stack_to_equipment(
                &shared_world,
                &mut database,
                705,
                position,
                0,
                4,
                EquipmentSlot::RightHand,
            )
            .unwrap();

        assert_eq!(complete.source_identity, partial.source_identity);
        assert_eq!(complete.item.count, 4);
        assert!(shared_map
            .render_snapshot()
            .unwrap()
            .tile_items(position)
            .unwrap()
            .is_empty());
        assert_eq!(
            shared_world
                .player_equipment(705)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .unwrap()
                .count,
            7
        );
        assert_eq!(database.map_item_count_overrides().unwrap(), None);
        assert_eq!(shared_map.count_overrides().unwrap(), Vec::new());
        assert_eq!(
            database.map_item_removal_journal().unwrap(),
            Some(MapItemRemovalJournal {
                map_revision: source_map.source_revision(),
                removed_items: vec![partial.source_identity],
            })
        );
        assert_eq!(shared_world.equipment_epoch(), 2);
        assert_eq!(shared_map.revision(), 2);

        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn source_map_complete_stack_merges_into_matching_top_level_container_stack() {
        let database_path = database_path("source-map-item-container-stack-merge");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let player = Player {
            id: 704,
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
        };
        database.save_player(&player).unwrap();
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                position,
                vec![WorldMapItem {
                    server_id: 2148,
                    client_thing_id: Some(3031),
                    count: 7,
                    action_id: Some(12),
                    unique_id: Some(34),
                    text: None,
                    description: None,
                    teleport_destination: None,
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            )
            .unwrap();
        let shared_map = SharedNativeMap::new(source_map.clone());
        let shared_world = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared_world
            .register_player_at_available_position(player, &source_map)
            .unwrap();
        let mut bag = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Bag",
            false,
            20,
        )
        .unwrap();
        let mut existing_stack = ItemInstance::new(2148, 5).unwrap();
        existing_stack.action_id = Some(12);
        existing_stack.unique_id = Some(34);
        bag.items.insert(existing_stack).unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(bag).unwrap();
        database
            .replace_player_containers(704, &containers)
            .unwrap();
        shared_world
            .replace_player_containers(704, containers)
            .unwrap();

        let outcome = shared_map
            .move_source_item_to_top_level_container(
                &shared_world,
                &mut database,
                704,
                position,
                0,
                2,
            )
            .unwrap();

        let mut merged_stack = ItemInstance::new(2148, 12).unwrap();
        merged_stack.action_id = Some(12);
        merged_stack.unique_id = Some(34);
        let containers = shared_world.player_containers(704).unwrap();
        assert_eq!(containers.container(2).unwrap().items.len(), 1);
        assert_eq!(
            containers.container(2).unwrap().items.item(0),
            Some(&merged_stack)
        );
        assert_eq!(database.player_containers(704).unwrap(), containers);
        assert!(shared_map
            .render_snapshot()
            .unwrap()
            .tile_items(position)
            .unwrap()
            .is_empty());
        assert_eq!(
            database.map_item_removal_journal().unwrap(),
            Some(MapItemRemovalJournal {
                map_revision: source_map.source_revision(),
                removed_items: vec![outcome.source_identity],
            })
        );
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn source_map_item_pickup_rejects_incompatible_occupied_slot_without_mutation() {
        let database_path = database_path("source-map-item-pickup-occupied-slot");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let player = Player {
            id: 702,
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
        };
        database.save_player(&player).unwrap();
        let position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                position,
                vec![WorldMapItem {
                    server_id: 2148,
                    client_thing_id: Some(3031),
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
        let shared_map = SharedNativeMap::new(source_map.clone());
        let shared_world = SharedNativeWorld::from_static_spawns(None).unwrap();
        let mut occupied_equipment = PlayerEquipment::default();
        occupied_equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(2376, 1).unwrap(),
        );
        shared_world
            .register_player_at_available_position_with_vitals_and_equipment(
                player,
                PlayerVitals::default(),
                occupied_equipment.clone(),
                &source_map,
            )
            .unwrap();
        database
            .replace_player_equipment(702, &occupied_equipment)
            .unwrap();
        let equipment_epoch = shared_world.equipment_epoch();

        assert!(matches!(
            shared_map.move_source_item_to_empty_equipment(
                &shared_world,
                &mut database,
                702,
                position,
                0,
                EquipmentSlot::RightHand,
            ),
            Err(HostError::Core(
                forgotten_core::CoreError::IncompatibleItemStacks
            ))
        ));

        assert_eq!(shared_map.revision(), 0);
        assert_eq!(shared_world.equipment_epoch(), equipment_epoch);
        assert_eq!(
            shared_map
                .render_snapshot()
                .unwrap()
                .tile_items(position)
                .unwrap()[0]
                .server_id,
            2148
        );
        assert_eq!(
            shared_world.player_equipment(702).unwrap(),
            occupied_equipment
        );
        assert_eq!(database.player_equipment(702).unwrap(), occupied_equipment);
        assert_eq!(database.map_item_removal_journal().unwrap(), None);

        let _ = fs::remove_file(database_path);
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
    fn native_equipment_armor_defense_sums_only_the_six_explicit_armor_slots() {
        let mut equipment = PlayerEquipment::default();
        for (slot, server_id) in [
            (EquipmentSlot::Head, 100),
            (EquipmentSlot::Neck, 101),
            (EquipmentSlot::Armor, 102),
            (EquipmentSlot::Legs, 103),
            (EquipmentSlot::Feet, 104),
            (EquipmentSlot::Ring, 105),
            (EquipmentSlot::LeftHand, 106),
            (EquipmentSlot::RightHand, 107),
        ] {
            equipment.equip(slot, ItemInstance::new(server_id, 1).unwrap());
        }
        let armor_by_server_id = BTreeMap::from([
            (100, 1),
            (101, 2),
            (102, 3),
            (103, 4),
            (104, 5),
            (105, 6),
            (106, 500),
            (107, 500),
        ]);

        assert_eq!(
            native_equipment_armor_defense(Some(&armor_by_server_id), None, &equipment, 1_000),
            PlayerCombatDefense::new(21).unwrap()
        );
        assert_eq!(
            native_equipment_armor_defense(Some(&armor_by_server_id), None, &equipment, 1_200),
            PlayerCombatDefense::new(25).unwrap()
        );
        assert_eq!(
            native_equipment_armor_defense(None, None, &equipment, 1_000),
            PlayerCombatDefense::default()
        );
    }

    #[test]
    fn native_classic_equipment_look_requires_one_exact_mapped_fixed_slot_item() {
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                2463,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 100,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap());
        let position = NativeOtClientPosition {
            x: u16::MAX,
            y: EquipmentSlot::Armor.code().into(),
            z: 0,
        };

        assert_eq!(
            native_classic_equipment_look_item(Some(&catalog), &equipment, position, 100, 0),
            Some((EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap()))
        );
        assert_eq!(
            native_equipment_item_inspection_message(
                EquipmentSlot::Armor,
                &ItemInstance::new(2463, 1).unwrap(),
                None,
                None,
                None,
            ),
            "Equipment slot 4: item 2463 (count 1)."
        );
        assert_eq!(
            native_equipment_item_inspection_message(
                EquipmentSlot::Armor,
                &ItemInstance::new(2463, 1).unwrap(),
                Some(&BTreeMap::from([(2463, "Plate Armor".to_string())])),
                Some(&BTreeMap::from([(2463, 8_000)])),
                None,
            ),
            "Equipment slot 4: item 2463 (count 1). Name: Plate Armor. It weighs 80.00 oz."
        );
        assert!(
            native_classic_equipment_look_item(Some(&catalog), &equipment, position, 99, 0)
                .is_none()
        );
        assert!(
            native_classic_equipment_look_item(Some(&catalog), &equipment, position, 100, 1)
                .is_none()
        );
        assert!(native_classic_equipment_look_item(
            Some(&catalog),
            &equipment,
            NativeOtClientPosition {
                y: position.y | 0x40,
                ..position
            },
            100,
            0,
        )
        .is_none());
    }

    #[test]
    fn native_classic_container_look_requires_one_exact_open_top_level_item() {
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                4526,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let mut containers = PlayerContainers::default();
        let mut container = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Bag",
            false,
            20,
        )
        .unwrap();
        container
            .items
            .insert(ItemInstance::new(4526, 3).unwrap())
            .unwrap();
        containers.insert(container).unwrap();
        let position = NativeOtClientPosition {
            x: u16::MAX,
            y: 0x40 | 2,
            z: 0,
        };

        assert_eq!(
            native_classic_container_look_item(
                Some(&catalog),
                &containers,
                &BTreeSet::new(),
                position,
                102,
                0,
            ),
            Some((2, ItemInstance::new(4526, 3).unwrap()))
        );
        assert_eq!(
            native_container_item_inspection_message(
                2,
                &ItemInstance::new(4526, 3).unwrap(),
                None,
                None,
                None,
            ),
            "Container 2: item 4526 (count 3)."
        );
        assert_eq!(
            native_container_item_inspection_message(
                2,
                &ItemInstance::new(4526, 3).unwrap(),
                Some(&BTreeMap::from([(4526, "Arrow".to_string())])),
                Some(&BTreeMap::from([(4526, 120)])),
                Some(&BTreeSet::from([4526])),
            ),
            "Container 2: item 4526 (count 3). Name: Arrow. It weighs 3.60 oz."
        );
        assert!(native_classic_container_look_item(
            Some(&catalog),
            &containers,
            &BTreeSet::from([2]),
            position,
            102,
            0,
        )
        .is_none());
        assert!(native_classic_container_look_item(
            Some(&catalog),
            &containers,
            &BTreeSet::new(),
            position,
            101,
            0,
        )
        .is_none());
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
    fn native_container_deltas_emit_minimal_records_with_exact_fallbacks() {
        let config = native_otclient_config("127.0.0.1:0".parse().unwrap());
        let profile = &config.client_profile;
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id, subtype) in
            [(1988, 1988, false), (2376, 2376, false), (2544, 100, true)]
        {
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
        let backpack_with = |items: &[ItemInstance], capacity: u16| {
            let mut container = forgotten_core::PlayerContainer::new(
                2,
                ItemInstance::new(1988, 1).unwrap(),
                "Backpack",
                false,
                capacity,
            )
            .unwrap();
            for item in items {
                container.items.insert(item.clone()).unwrap();
            }
            let mut containers = PlayerContainers::default();
            containers.insert(container).unwrap();
            containers
        };
        let empty_ids = BTreeSet::new();
        let rendered = |containers: &PlayerContainers| {
            native_rendered_container_windows(profile, Some(&catalog), containers, &empty_ids)
        };

        // Pure append: one CreateInContainer for the added stack.
        let sent = rendered(&backpack_with(&[ItemInstance::new(2376, 1).unwrap()], 20));
        let grown = backpack_with(
            &[
                ItemInstance::new(2376, 1).unwrap(),
                ItemInstance::new(2544, 30).unwrap(),
            ],
            20,
        );
        let frames = native_container_delta_frames(
            profile,
            Some(&catalog),
            &grown,
            &empty_ids,
            &mut sent.clone(),
        )
        .unwrap();
        assert_eq!(
            frames,
            vec![Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATE_IN_CONTAINER,
                2,
                0x64,
                0x00,
                30,
            ])]
        );

        // Contiguous middle removal: descending deletes only.
        let sent = rendered(&backpack_with(
            &[
                ItemInstance::new(2376, 1).unwrap(),
                ItemInstance::new(2544, 5).unwrap(),
                ItemInstance::new(1988, 2).unwrap(),
            ],
            20,
        ));
        let removed = backpack_with(
            &[
                ItemInstance::new(2376, 1).unwrap(),
                ItemInstance::new(1988, 2).unwrap(),
            ],
            20,
        );
        let frames = native_container_delta_frames(
            profile,
            Some(&catalog),
            &removed,
            &empty_ids,
            &mut sent.clone(),
        )
        .unwrap();
        assert_eq!(
            frames,
            vec![Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_DELETE_IN_CONTAINER,
                2,
                1,
            ])]
        );

        // Same-slot stack rewrite: one ChangeInContainer at the touched index.
        let sent = rendered(&backpack_with(&[ItemInstance::new(2544, 10).unwrap()], 20));
        let merged = backpack_with(&[ItemInstance::new(2544, 47).unwrap()], 20);
        let frames = native_container_delta_frames(
            profile,
            Some(&catalog),
            &merged,
            &empty_ids,
            &mut sent.clone(),
        )
        .unwrap();
        assert_eq!(
            frames,
            vec![Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CHANGE_IN_CONTAINER,
                2,
                0,
                0x64,
                0x00,
                47,
            ])]
        );

        // Capacity change is not expressible as deltas: exact full resend.
        let sent = rendered(&backpack_with(&[ItemInstance::new(2376, 1).unwrap()], 20));
        let resized = backpack_with(&[ItemInstance::new(2376, 1).unwrap()], 19);
        let frames = native_container_delta_frames(
            profile,
            Some(&catalog),
            &resized,
            &empty_ids,
            &mut sent.clone(),
        )
        .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER
        );

        // Window unknown to the baseline (freshly opened): full OpenContainer resend.
        let fresh_sent: BTreeMap<u8, NativeRenderedContainerWindow> = BTreeMap::new();
        let frames = native_container_delta_frames(
            profile,
            Some(&catalog),
            &backpack_with(&[], 20),
            &empty_ids,
            &mut fresh_sent.clone(),
        )
        .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER
        );

        // No visible change: zero records.
        let unchanged = backpack_with(&[ItemInstance::new(2376, 1).unwrap()], 20);
        let sent = rendered(&unchanged);
        let frames = native_container_delta_frames(
            profile,
            Some(&catalog),
            &unchanged,
            &empty_ids,
            &mut sent.clone(),
        )
        .unwrap();
        assert!(frames.is_empty());
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
            PlayerCondition::from_persisted(PlayerConditionKind::Poison, 3, 7, 0, 5, 1).unwrap()
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
            PlayerCondition::from_persisted(PlayerConditionKind::Poison, 3, 7, 0, 3, 0).unwrap()
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
                name_description: String::new(),
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
                StaticTargetPursuitPolicy::Disabled,
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage },
                forgotten_core::StaticCreatureDecisionPolicy::Disabled,
                0,
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
                name_description: String::new(),
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
            StaticTargetPursuitPolicy::Disabled,
            StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 10 },
            forgotten_core::StaticCreatureDecisionPolicy::Disabled,
            0,
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
    fn native_declarative_spell_cast_persists_rate_scaled_magic_progression() {
        let path = database_path("native-declarative-spell-magic-progression");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        let player = Player {
            id: 108,
            account_id: account_id as u64,
            name: "Sorcerer".into(),
            position: map.spawn(),
            level: 8,
            experience: 4_900,
            skill_points: 3,
        };
        database.save_player(&player).unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        shared
            .register_player_at_available_position_with_vitals(
                player,
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
        let multiplier = forgotten_core::ProgressionMultiplier::new(1_000).unwrap();
        let rules_by_vocation = BTreeMap::from([(
            VocationId::new(0),
            PlayerProgressionRules {
                magic_level_multiplier: multiplier,
                skill_multipliers: [multiplier; 7],
            },
        )]);

        assert_eq!(
            native_declarative_spell_command_id("!fe cast 100"),
            Some(100)
        );
        assert_eq!(native_declarative_spell_command_id("!fe cast 0"), None);
        assert_eq!(
            native_declarative_spell_command_id("!fe cast 100 extra"),
            None
        );
        assert_eq!(native_declarative_spell_command_id("exura"), None);
        let (cast, magic) = apply_and_persist_native_declarative_spell_cast(
            &mut database,
            &shared,
            108,
            100,
            &catalog,
            &rules_by_vocation,
            2,
        )
        .unwrap();

        assert_eq!(cast.mana_spent, 20);
        assert_eq!(cast.remaining_mana, 30);
        assert_eq!(magic.magic_level, 0);
        assert_eq!(magic.stored_mana, 40);
        assert_eq!(shared.player_vitals(108).unwrap().mana, 30);
        assert_eq!(
            shared
                .player_progression_attempts(108)
                .unwrap()
                .magic_mana(),
            40
        );
        assert_eq!(shared.vitals_epoch(), 1);
        let persisted = database
            .characters_for_account(account_id)
            .unwrap()
            .into_iter()
            .find(|character| character.id == 108)
            .unwrap();
        assert_eq!(persisted.vitals.mana, 30);
        assert_eq!(persisted.vitals.magic_level, 0);
        assert_eq!(persisted.progression_attempts.magic_mana(), 40);
        assert!(matches!(
            apply_and_persist_native_declarative_spell_cast(
                &mut database,
                &shared,
                108,
                100,
                &catalog,
                &rules_by_vocation,
                2,
            ),
            Err(HostError::Core(
                forgotten_core::CoreError::SpellCooldownActive { .. }
            ))
        ));
        assert_eq!(shared.player_vitals(108).unwrap().mana, 30);
        assert_eq!(
            shared
                .player_progression_attempts(108)
                .unwrap()
                .magic_mana(),
            40
        );
        let _ = fs::remove_file(path);
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
    fn native_gm_talkaction_reply_is_followed_by_a_full_viewport_resend() {
        let database_path = database_path("native-gm-spawn-viewport");
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
        database.update_player_gm_level(1, 2).unwrap();
        drop(database);

        let native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
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

        // GM SAY "/spawn Rat": the reply must be followed immediately by a full-map
        // viewport resend. The previous behavior swallowed the visibility bump here,
        // leaving summons invisible until relog (live-test regression A1).
        let message = b"/spawn Rat";
        let mut talk = vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_TALK, 1];
        talk.extend_from_slice(&(message.len() as u16).to_le_bytes());
        talk.extend_from_slice(message);
        write_frame(&mut stream, &Frame(talk)).unwrap();

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut saw_reply = false;
        let viewport_after_reply = loop {
            let frame = read_frame(&mut stream).unwrap();
            if saw_reply {
                break frame;
            }
            if frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE) {
                saw_reply = true;
            }
        };
        assert_eq!(
            viewport_after_reply.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
    }

    #[test]
    fn native_map_inspection_appends_only_exact_imported_stack_weight() {
        let position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("weight-inspection", position);
        map.set_tile(
            position,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile_items(
            position,
            vec![forgotten_core::WorldMapItem {
                server_id: 1945,
                client_thing_id: Some(1945),
                count: 3,
                action_id: None,
                unique_id: None,
                text: None,
                description: Some("A bounded imported description.".into()),
                teleport_destination: None,
                duration: None,
                charges: None,
                children: Vec::new(),
            }],
        )
        .unwrap();
        let outcome = PlayerItemUseOutcome {
            player_id: 7,
            position,
            stack_index: 0,
            server_id: 1945,
            count: 3,
            action_id: None,
            unique_id: None,
            has_text: false,
            charges: None,
            teleport_destination: None,
        };

        assert_eq!(
            native_map_item_inspection_message(
                &map,
                &outcome,
                Some(&BTreeMap::from([(1945, "Dragon Ham".to_string())])),
                Some(&BTreeMap::from([(1945, 1_800)])),
                Some(&BTreeSet::from([1945])),
            ),
            "You see item #1945 (count: 3). Name: Dragon Ham. It weighs 54.00 oz. A bounded imported description."
        );
        assert_eq!(
            native_map_item_inspection_message(
                &map,
                &outcome,
                None,
                Some(&BTreeMap::from([(1945, 1_800)])),
                Some(&BTreeSet::from([1945])),
            ),
            "You see item #1945 (count: 3). It weighs 54.00 oz. A bounded imported description."
        );
        assert_eq!(
            native_map_item_inspection_message(&map, &outcome, None, None, None),
            "You see item #1945 (count: 3). A bounded imported description."
        );
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
        cancel_native_player_attack_and_follow(&shared, 101).unwrap();
        assert_eq!(
            shared.player_interaction_intent(101).unwrap(),
            PlayerInteractionIntent::default()
        );
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
        shared
            .hydrate_player_respawn_state(
                102,
                PlayerRespawnState {
                    dead: true,
                    respawn_at: Some(map.spawn()),
                    death_time: Some(1),
                    loss_applied: true,
                },
            )
            .unwrap();
        assert_eq!(
            apply_native_player_interaction(
                &shared,
                101,
                NATIVE_OTCLIENT_PLAYER_ID_START + 102,
                NativePlayerInteractionKind::Target,
                false,
            )
            .unwrap(),
            NativePlayerInteractionOutcome::Rejected
        );
        assert_eq!(
            apply_native_player_interaction(
                &shared,
                101,
                NATIVE_OTCLIENT_PLAYER_ID_START + 102,
                NativePlayerInteractionKind::Follow,
                false,
            )
            .unwrap(),
            NativePlayerInteractionOutcome::Rejected
        );
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
                name_description: String::new(),
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
    fn native_creature_look_prefers_name_description_and_answers_self_looks() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: "a rat".into(),
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

        assert_eq!(
            native_creature_inspection_message(&shared, 101, NATIVE_OTCLIENT_PLAYER_ID_START + 101)
                .unwrap()
                .as_deref(),
            Some("You see yourself.")
        );
        assert_eq!(
            native_creature_inspection_message(&shared, 101, creature_id)
                .unwrap()
                .as_deref(),
            Some("You see a rat.")
        );
    }

    #[test]
    fn native_ground_look_replies_use_imported_names_and_never_numeric_ids() {
        let item_position = Position {
            x: 105,
            y: 103,
            z: 7,
        };
        let mut map = WorldMap::new(
            "ground-look",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        map.set_tile(
            item_position,
            WorldMapTile {
                ground_thing_id: 102,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile_items(
            item_position,
            vec![WorldMapItem {
                server_id: 2666,
                client_thing_id: None,
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
        let names = BTreeMap::from([(2666u16, "Dragon Ham".to_owned())]);

        assert_eq!(
            native_ground_look_message(&map, item_position, Some(&names)),
            "You see Dragon Ham."
        );
        assert_eq!(
            native_ground_look_message(&map, item_position, None),
            "You see an item."
        );

        let bare_ground = Position {
            x: 107,
            y: 107,
            z: 7,
        };
        assert_eq!(
            native_ground_look_message(&map, bare_ground, Some(&names)),
            "You see ground."
        );
    }

    #[test]
    fn static_creature_health_refreshes_visibility_and_native_display_frame() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
    fn static_target_deactivation_emits_only_classic_clear_target_control() {
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        assert!(native_static_target_deactivation_frames(&profile, false)
            .unwrap()
            .is_empty());
        assert_eq!(
            native_static_target_deactivation_frames(&profile, true)
                .unwrap()
                .into_iter()
                .map(|frame| frame.0)
                .collect::<Vec<_>>(),
            vec![vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CLEAR_TARGET]]
        );
    }

    #[test]
    fn selected_player_death_emits_only_classic_clear_target_control() {
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        assert!(native_selected_player_death_target_frames(&profile, false)
            .unwrap()
            .is_empty());
        assert_eq!(
            native_selected_player_death_target_frames(&profile, true)
                .unwrap()
                .into_iter()
                .map(|frame| frame.0)
                .collect::<Vec<_>>(),
            vec![vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CLEAR_TARGET]]
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
                name_description: String::new(),
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
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
            }]
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn static_creature_damage_sequence_persists_across_fresh_shared_worlds() {
        let path = database_path("static-creature-damage-sequence-persistence");
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 2;
        let static_spawns = FeTfsStaticSpawnCollection::with_combat_metadata(
            vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::from([(creature_id, 2_000)]),
            BTreeMap::from([(
                creature_id,
                forgotten_core::StaticCreatureDirectMeleeDamageRange {
                    min_damage: 2,
                    max_damage: 4,
                },
            )]),
        )
        .unwrap();
        let map = native_world_map();
        let player = Player {
            id: 101,
            account_id: 1,
            name: "Knight".into(),
            position: map.spawn(),
            level: 8,
            experience: 0,
            skill_points: 0,
        };
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        shared
            .register_player_at_available_position(player.clone(), &map)
            .unwrap();
        shared
            .acquire_static_creature_targets(StaticTargetAcquisitionPolicy::NearestLivingPlayer {
                max_range: 1,
            })
            .unwrap();
        assert!(matches!(
            shared.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                requested_damage: 2,
                ..
            })
        ));
        persist_static_creature_runtime_to_database(&shared, &path).unwrap();
        assert_eq!(
            EngineDatabase::open(&path)
                .unwrap()
                .static_creature_runtime()
                .unwrap()[0]
                .direct_melee_damage_sequence,
            1
        );
        assert_eq!(
            EngineDatabase::open(&path)
                .unwrap()
                .static_creature_runtime()
                .unwrap()[0]
                .direct_melee_cooldown_remaining_ticks,
            Some(2)
        );

        let fresh = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        fresh
            .register_player_at_available_position(player, &map)
            .unwrap();
        assert_eq!(
            restore_static_creature_runtime_from_database(&fresh, &path).unwrap(),
            StaticCreatureRuntimeRestoreSummary {
                restored: 1,
                ignored_unknown: 0,
            }
        );
        fresh
            .acquire_static_creature_targets(StaticTargetAcquisitionPolicy::NearestLivingPlayer {
                max_range: 1,
            })
            .unwrap();
        assert!(matches!(
            fresh.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::CooldownNotDue {
                creature_id: _,
                due_tick: 2,
            })
        ));
        advance_native_shared_world_heartbeat(&fresh, 2).unwrap();
        assert!(matches!(
            fresh.apply_static_creature_target_damage(creature_id, 1, &map),
            Ok(StaticCreatureTargetAttackOutcome::Applied {
                requested_damage: 3,
                ..
            })
        ));
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
                    name_description: String::new(),
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
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
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
            None,
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
                direct_melee_cooldown_remaining_ticks: None,
                direct_melee_damage_sequence: 0,
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
            name_description: String::new(),
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
    fn shared_static_target_attack_summary_counts_direct_melee_cooldown_as_skipped() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 2;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
            &FeTfsStaticSpawnCollection::with_runtime_metadata(
                vec![creature],
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::from([(creature_id, 2_000)]),
            )
            .unwrap(),
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
                    health: 10,
                    max_health: 10,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();
        shared
            .lock()
            .unwrap()
            .select_static_creature_target(creature_id, 1)
            .unwrap();

        let first = shared
            .attack_static_creature_targets_once(
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 1 },
                &map,
            )
            .unwrap();
        assert_eq!(first.applied_attacks, 1);
        assert_eq!(first.cooldown_skipped_attacks, 0);

        let second = shared
            .attack_static_creature_targets_once(
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 1 },
                &map,
            )
            .unwrap();
        assert_eq!(second.applied_attacks, 0);
        assert_eq!(second.total_applied_damage, 0);
        assert_eq!(second.cooldown_skipped_attacks, 1);
    }

    #[test]
    fn shared_static_target_attack_uses_imported_direct_melee_damage_over_fixed_fallback() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 3;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
            &FeTfsStaticSpawnCollection::with_combat_metadata(
                vec![creature],
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::from([(
                    creature_id,
                    forgotten_core::StaticCreatureDirectMeleeDamageRange {
                        min_damage: 3,
                        max_damage: 4,
                    },
                )]),
            )
            .unwrap(),
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
                    health: 20,
                    max_health: 20,
                    ..PlayerVitals::default()
                },
                &map,
            )
            .unwrap();
        shared
            .lock()
            .unwrap()
            .select_static_creature_target(creature_id, 1)
            .unwrap();

        let first = shared
            .attack_static_creature_targets_once(
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 1 },
                &map,
            )
            .unwrap();
        let second = shared
            .attack_static_creature_targets_once(
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 1 },
                &map,
            )
            .unwrap();
        assert_eq!(first.total_applied_damage, 3);
        assert_eq!(second.total_applied_damage, 4);
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
        let mut target_equipment = PlayerEquipment::default();
        target_equipment.equip(EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap());
        shared
            .replace_player_equipment(102, target_equipment)
            .unwrap();
        let armor_by_server_id = BTreeMap::from([(2463, 3)]);
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
                armor_by_server_id: Some(&armor_by_server_id),
                shield_defense_by_server_id: None,
                armor_multiplier_by_vocation: None,
                declarative_weapon_catalog: None,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(native_target_id, NATIVE_OTCLIENT_PLAYER_ID_START + 102);
        assert_eq!(
            outcome.applied_damage,
            NATIVE_OTCLIENT_SELECTED_PLAYER_MELEE_DAMAGE - 3
        );
        assert_eq!(vitals.health, 13);
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
            13
        );
        assert!(apply_native_selected_player_melee(
            &mut database,
            &shared,
            101,
            &map,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: None,
                skill_rate: 1,
                death_loss_policy: DeathLossPolicy::DefaultFormula,
                armor_by_server_id: Some(&armor_by_server_id),
                shield_defense_by_server_id: None,
                armor_multiplier_by_vocation: None,
                declarative_weapon_catalog: None,
            },
        )
        .unwrap()
        .is_none());
        assert_eq!(shared.vitals_epoch(), 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn native_game_auth_blocks_peers_after_the_bounded_failure_window() {
        let database_path = database_path("native-auth-rate-limit");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        drop(database);
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let wrong_password_frame = native_game_request(
            account_id.try_into().unwrap(),
            "Knight",
            "definitely-not-the-password",
        );
        for _ in 0..NATIVE_AUTH_MAX_FAILURES_PER_WINDOW {
            let mut stream = TcpStream::connect(game.local_addr()).unwrap();
            write_frame(&mut stream, &wrong_password_frame).unwrap();
            assert_ne!(
                read_frame(&mut stream).unwrap().0[0],
                forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE,
                "a wrong password must never authenticate"
            );
        }
        // The next failure budget overflow switches the peer to pre-auth rejection.
        let limiter_probe = {
            let mut stream = TcpStream::connect(game.local_addr()).unwrap();
            write_frame(&mut stream, &wrong_password_frame).unwrap();
            read_frame(&mut stream).unwrap()
        };
        assert_eq!(
            limiter_probe.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_ERROR
        );

        // A correct password from the blocked peer is still refused before authentication.
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
        let blocked = read_frame(&mut stream).unwrap();
        assert_eq!(
            blocked.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_ERROR
        );
        assert!(String::from_utf8_lossy(&blocked.0).contains("Too many failed attempts"));
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_talk_flood_is_suppressed_beyond_the_bounded_window_budget() {
        let database_path = database_path("native-chat-flood");
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
        drop(database);
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
        read_data_frame(&mut stream);

        for _ in 0..(CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW + 5) {
            write_frame(&mut stream, &Frame(vec![0x96, 1, 2, 0, b'h', b'i'])).unwrap();
        }
        let mut delivered = 0usize;
        stream
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        loop {
            match read_frame(&mut stream) {
                Ok(frame) if frame.0 == [forgotten_protocol::NATIVE_OTCLIENT_GAME_PING] => {
                    continue;
                }
                Ok(frame)
                    if frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_TALK) =>
                {
                    delivered += 1;
                    assert!(delivered <= CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW);
                }
                Ok(_) => break,
                Err(HostError::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("native session ended during flood probe: {error}"),
            }
        }
        assert_eq!(
            delivered, CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW,
            "exactly the bounded window budget is delivered"
        );
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_bank_keywords_deposit_and_withdraw_gold_near_an_npc() {
        let database_path = database_path("native-bank-keywords");
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
        // A backpack holding 30 gold coins stands next to the player; an NPC banker is adjacent.
        let mut containers = PlayerContainers::default();
        let mut backpack = PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        backpack
            .items
            .merge_or_insert_stack(ItemInstance::new(2148, 30).unwrap())
            .unwrap();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(1, &containers).unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.static_creature_wander_policy =
            forgotten_core::StaticCreatureDecisionPolicy::Disabled;
        native_config.static_creature_wander_every_ticks = 0;
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::with_combat_metadata_and_npc_ids(
                vec![forgotten_core::FeTfsStaticEntity {
                    id: NATIVE_OTCLIENT_PLAYER_ID_END + 2,
                    name: "Banker".into(),
                    name_description: String::new(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    look_type: 128,
                    head: 0,
                    body: 0,
                    legs: 0,
                    feet: 0,
                    addons: 0,
                    speed: 134,
                    health_percent: 100,
                    direction: 2,
                }],
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeSet::from([NATIVE_OTCLIENT_PLAYER_ID_END + 2]),
            )
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
        read_data_frame(&mut stream);
        // Bootstrap follows with one static-creature health record for the nearby banker.
        let bootstrap_health = read_data_frame(&mut stream);
        assert_eq!(
            bootstrap_health.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH
        );

        // Deposit all carried coins near the banker. Classic Talk framing: mode byte then text.
        write_frame(
            &mut stream,
            &Frame(vec![
                0x96, 1, 11, 0, b'd', b'e', b'p', b'o', b's', b'i', b't', b' ', b'a', b'l', b'l',
            ]),
        )
        .unwrap();
        let reply = read_data_frame(&mut stream);
        assert!(String::from_utf8_lossy(&reply.0).contains("You deposited 30 gold."));

        drop(stream);
        game.shutdown().unwrap();
        let database = EngineDatabase::open(&database_path).unwrap();
        assert_eq!(database.player_bank_balance(1).unwrap(), 30);
        let drained = database.player_containers(1).unwrap();
        let backpack_items: Vec<(u16, u16)> = drained
            .container(2)
            .unwrap()
            .items
            .iter()
            .map(|item| (item.server_id, item.count))
            .collect();
        assert!(
            !backpack_items.iter().any(|(id, _)| *id == 2148),
            "carried gold must leave inventory on deposit"
        );
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_use_item_consumes_a_backpack_potion_and_heals_the_drinker() {
        let database_path = database_path("native-consumable");
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
                    health: 50,
                    max_health: 150,
                    mana: 10,
                    max_mana: 50,
                    capacity: 32_000,
                    magic_level: 0,
                },
            )
            .unwrap();
        let mut containers = PlayerContainers::default();
        let mut backpack = PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        backpack
            .items
            .merge_or_insert_stack(ItemInstance::new(2666, 2).unwrap())
            .unwrap();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(1, &containers).unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.consumable_effects = Some(Arc::new(BTreeMap::from([(
            2666_u16,
            forgotten_config::ConsumableEffect {
                health: 25,
                mana: 0,
                regeneration_seconds: 0,
            },
        )])));
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
        read_data_frame(&mut stream);

        // Drink from the backpack potion stack: own-container addressing with child index 0.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                0xff,
                0xff,
                0x42,
                0x00,
                0x00,
                0x6a,
                0x0a,
                0x00,
                0x00,
            ]),
        )
        .unwrap();
        let healed = read_data_frame(&mut stream);
        assert_eq!(
            healed.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH
        );
        // 75 of 150 maximum health renders as a 50-percent display value.
        assert_eq!(*healed.0.last().unwrap(), 50);

        drop(stream);
        game.shutdown().unwrap();
        let database = EngineDatabase::open(&database_path).unwrap();
        let character = database
            .characters_for_account(account_id)
            .unwrap()
            .into_iter()
            .next()
            .expect("character persists");
        assert_eq!(character.vitals.health, 75);
        let drained = database.player_containers(1).unwrap();
        let potion_count = drained
            .container(2)
            .unwrap()
            .items
            .item(0)
            .map(|item| item.count);
        assert_eq!(potion_count, Some(1));
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_shop_keywords_buy_and_sell_through_the_bank_balance() {
        let database_path = database_path("native-shop-keywords");
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
        database.set_player_bank_balance(1, 500).unwrap();
        let mut containers = PlayerContainers::default();
        let mut backpack = PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        backpack
            .items
            .merge_or_insert_stack(ItemInstance::new(3294, 2).unwrap())
            .unwrap();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(1, &containers).unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.static_creature_wander_policy =
            forgotten_core::StaticCreatureDecisionPolicy::Disabled;
        native_config.static_creature_wander_every_ticks = 0;
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::with_combat_metadata_and_npc_ids(
                vec![forgotten_core::FeTfsStaticEntity {
                    id: NATIVE_OTCLIENT_PLAYER_ID_END + 3,
                    name: "Trader".into(),
                    name_description: String::new(),
                    position: Position {
                        x: 101,
                        y: 100,
                        z: 7,
                    },
                    look_type: 128,
                    head: 0,
                    body: 0,
                    legs: 0,
                    feet: 0,
                    addons: 0,
                    speed: 134,
                    health_percent: 100,
                    direction: 2,
                }],
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeSet::from([NATIVE_OTCLIENT_PLAYER_ID_END + 3]),
            )
            .unwrap(),
        ));
        let shop_catalog = parse_declarative_shops_xml(
            br#"<fe-shops><fe-shop npc="Trader"><fe-item id="3294" sell="200"/><fe-item id="2666" buy="75"/></fe-shop></fe-shops>"#,
        )
        .unwrap();
        native_config.shop_catalog = Some(Arc::new(shop_catalog));
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
        read_data_frame(&mut stream);
        // Consume the banker-style bootstrap health record for the nearby NPC.
        read_data_frame(&mut stream);

        // Sell both swords at 150 gold each.
        // Sell both swords: sell <item-id> <count>, at 200 gold each from the catalog.
        write_frame(
            &mut stream,
            &Frame(vec![
                0x96, 1, 11, 0, b's', b'e', b'l', b'l', b' ', b'3', b'2', b'9', b'4', b' ', b'2',
            ]),
        )
        .unwrap();
        let reply = read_data_frame(&mut stream);
        assert!(String::from_utf8_lossy(&reply.0).contains("You sold"));

        drop(stream);
        game.shutdown().unwrap();
        let database = EngineDatabase::open(&database_path).unwrap();
        assert_eq!(database.player_bank_balance(1).unwrap(), 500 + 400);
        let drained = database.player_containers(1).unwrap();
        assert!(drained
            .container(2)
            .unwrap()
            .items
            .iter()
            .all(|item| item.server_id != 3294));
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_quest_log_lists_started_quests_from_the_operator_catalog() {
        let database_path = database_path("native-quest-log");
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
        // The player has started two quests; one unknown catalog id stays filtered out.
        database
            .replace_player_quests(1, &[(100, false), (101, true), (999, false)])
            .unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.quest_catalog = Some(Arc::new(
            parse_quests_xml(
                br#"<fe-quests>
                        <fe-quest id="100" name="The Rat Hunt"/>
                        <fe-quest id="101" name="Sewer Secrets"/>
                    </fe-quests>"#,
            )
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
        read_data_frame(&mut stream);

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG,
            ]),
        )
        .unwrap();
        let quest_frame = read_data_frame(&mut stream);
        assert_eq!(
            quest_frame.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_QUEST_LOG
        );
        // Two entries: [opcode][count u16][id u16][name string]...
        assert_eq!(quest_frame.0[1], 2);
        assert_eq!(quest_frame.0[3], 100);
        assert!(String::from_utf8_lossy(&quest_frame.0).contains("The Rat Hunt"));
        assert!(String::from_utf8_lossy(&quest_frame.0).contains("Sewer Secrets"));
        assert!(!String::from_utf8_lossy(&quest_frame.0).contains("999"));

        // Session remains usable after the quest exchange.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_data_frame(&mut stream).0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_QUEST_LOG
        );
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_use_item_opens_an_equipped_backpack_container_window() {
        let database_path = database_path("native-backpack-window");
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
        // Equipped backpack plus one owned container holding a mapped item.
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::Backpack, ItemInstance::new(1988, 1).unwrap());
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut containers = PlayerContainers::default();
        let mut backpack = PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        backpack
            .items
            .merge_or_insert_stack(ItemInstance::new(2148, 5).unwrap())
            .unwrap();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(1, &containers).unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        let mut catalog = NativeItemPresentationCatalog::default();
        for server_id in [1988, 2148] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id: server_id,
                        requires_classic_740_subtype: false,
                    },
                )
                .unwrap();
        }
        native_config.item_presentation_catalog = Some(Arc::new(catalog));
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
        read_data_frame(&mut stream);
        // Bootstrap equipment records for the equipped backpack.
        assert_eq!(
            read_data_frame(&mut stream).0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY
        );

        // Use the equipped backpack (slot code 3).
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                0xff,
                0xff,
                3,
                0x00,
                0x00,
                0xc4,
                0x07,
                0x00,
                0x00,
            ]),
        )
        .unwrap();
        let window = read_data_frame(&mut stream);
        assert_eq!(
            window.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER
        );
        // Container id 2 opens; after the bounded "Backpack" name the item count follows.
        assert_eq!(window.0[1], 2);
        assert_eq!(&window.0[6..14], b"Backpack");
        assert_eq!(window.0[16], 1);

        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_use_item_inside_container_window_opens_nested_content_window() {
        let database_path = database_path("native-nested-content-window");
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
        // Equipped backpack holding one nested bag with content.
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::Backpack, ItemInstance::new(1988, 1).unwrap());
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut containers = PlayerContainers::default();
        let mut backpack = PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        let mut inner_bag = ItemInstance::new(1988, 1).unwrap();
        inner_bag
            .insert_content(ItemInstance::new(2148, 5).unwrap())
            .unwrap();
        backpack.items.merge_or_insert_stack(inner_bag).unwrap();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(1, &containers).unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        let mut catalog = NativeItemPresentationCatalog::default();
        for server_id in [1988, 2148] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id: server_id,
                        requires_classic_740_subtype: false,
                    },
                )
                .unwrap();
        }
        native_config.item_presentation_catalog = Some(Arc::new(catalog));
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
        read_data_frame(&mut stream);
        assert_eq!(
            read_data_frame(&mut stream).0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY
        );

        // Open the owned backpack window (container id 2).
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                0xff,
                0xff,
                3,
                0x00,
                0x00,
                0xc4,
                0x07,
                0x00,
                0x00,
            ]),
        )
        .unwrap();
        let parent_window = read_data_frame(&mut stream);
        assert_eq!(
            parent_window.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER
        );
        assert_eq!(parent_window.0[1], 2);

        // Use the nested bag at index 0 inside window 2: a has_parent child window opens.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                0xff,
                0xff,
                0x42,
                0x00,
                0x00,
                0xc4,
                0x07,
                0x00,
                0x00,
            ]),
        )
        .unwrap();
        let child_window = read_data_frame(&mut stream);
        assert_eq!(
            child_window.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CONTAINER
        );
        // Ephemeral window ids avoid owned container ids, so 0 or 1 is expected.
        assert!(child_window.0[1] <= 1);
        assert_eq!(&child_window.0[6..19], b"1988 contents");
        assert_eq!(child_window.0[19], ItemInstance::MAX_CONTENT_SLOTS as u8);
        // has_parent flag set so classic clients render the up-arrow back to the parent.
        assert_eq!(child_window.0[20], 1);
        assert_eq!(child_window.0[21], 1);

        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_quest_line_returns_declared_missions_for_started_quests() {
        let database_path = database_path("native-quest-line");
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
        database.replace_player_quests(1, &[(100, false)]).unwrap();

        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.quest_catalog = Some(Arc::new(
            parse_quests_xml(
                br#"<fe-quests>
                        <fe-quest id="100" name="The Rat Hunt">
                            <fe-mission name="Kill Rats" description="Slay ten rats in the sewers."/>
                            <fe-mission name="Report Back" description="Return to Captain Harsky."/>
                        </fe-quest>
                    </fe-quests>"#,
            )
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
        read_data_frame(&mut stream);

        // Request the quest line for the started quest.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LINE,
                100,
                0,
            ]),
        )
        .unwrap();
        let line = read_data_frame(&mut stream);
        assert_eq!(
            line.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_QUEST_LINE
        );
        assert_eq!(&line.0[1..3], &[100, 0]);
        assert_eq!(line.0[3], 2);
        assert!(String::from_utf8_lossy(&line.0).contains("Kill Rats"));
        assert!(String::from_utf8_lossy(&line.0).contains("Captain Harsky"));

        // An unknown quest still receives a valid empty line frame.
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LINE,
                200,
                0,
            ]),
        )
        .unwrap();
        let empty_line = read_data_frame(&mut stream);
        assert_eq!(
            empty_line.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_QUEST_LINE,
                200,
                0,
                0
            ]
        );

        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn shield_hand_legacy_defense_extends_the_bounded_physical_reduction() {
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap());
        equipment.equip(EquipmentSlot::LeftHand, ItemInstance::new(2511, 1).unwrap());
        let armor_by_server_id = BTreeMap::from([(2463_u16, 3_u16)]);
        let defense_by_server_id = BTreeMap::from([(2511_u16, 5_u16)]);

        // Without shield metadata the reduction stays the armor-only value.
        assert_eq!(
            native_equipment_armor_defense(Some(&armor_by_server_id), None, &equipment, 1_000)
                .physical_flat_reduction,
            3
        );
        // The left-hand item's legacy defense adds to the same capped sum.
        assert_eq!(
            native_equipment_armor_defense(
                Some(&armor_by_server_id),
                Some(&defense_by_server_id),
                &equipment,
                1_000
            )
            .physical_flat_reduction,
            8
        );
        // Defense in the right hand never counts: only the shield hand is interpreted.
        let mut right_hand_defense = PlayerEquipment::default();
        right_hand_defense.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(2511, 1).unwrap(),
        );
        assert_eq!(
            native_equipment_armor_defense(
                None,
                Some(&defense_by_server_id),
                &right_hand_defense,
                1_000
            )
            .physical_flat_reduction,
            0
        );
    }

    #[test]
    fn no_pvp_world_type_rejects_selected_player_melee_without_side_effects() {
        let path = database_path("selected-player-no-pvp");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        for (id, name, position) in [
            (151_u64, "Knight", map.spawn()),
            (
                152_u64,
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
            (151_u64, "Knight", map.spawn()),
            (
                152_u64,
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
        shared.set_player_target(151, Some(152)).unwrap();
        let before_vitals = shared.player_vitals(152).unwrap();
        let before_attempts = shared.player_progression_attempts(151).unwrap();

        assert!(apply_native_selected_player_melee_for_world_type(
            &mut database,
            &shared,
            151,
            &map,
            WorldType::NoPvp,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: None,
                skill_rate: 1,
                death_loss_policy: DeathLossPolicy::DefaultFormula,
                armor_by_server_id: None,
                shield_defense_by_server_id: None,
                armor_multiplier_by_vocation: None,
                declarative_weapon_catalog: None,
            },
        )
        .unwrap()
        .is_none());
        assert_eq!(shared.player_vitals(152).unwrap(), before_vitals);
        assert_eq!(
            shared.player_progression_attempts(151).unwrap(),
            before_attempts
        );
        assert_eq!(shared.vitals_epoch(), 0);
        assert_eq!(shared.progression_epoch(), 0);
        assert_eq!(
            shared
                .player_interaction_intent(151)
                .unwrap()
                .target_player_id,
            Some(152)
        );
        assert_eq!(
            database
                .characters_for_account(account_id)
                .unwrap()
                .into_iter()
                .find(|character| character.id == 152)
                .unwrap()
                .vitals,
            forgotten_persistence::PlayerVitals::default()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn selected_player_melee_uses_only_an_equipped_declarative_weapon_and_awards_matching_skill() {
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
        .unwrap()
        .with_adjacent_melee_skills(Some(&BTreeMap::from([(2376, PlayerSkill::Sword)])));
        let multiplier = forgotten_core::ProgressionMultiplier::new(1_000).unwrap();
        let rules_by_vocation = BTreeMap::from([(
            VocationId::new(0),
            PlayerProgressionRules {
                magic_level_multiplier: multiplier,
                skill_multipliers: [multiplier; 7],
            },
        )]);
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
                armor_by_server_id: None,
                shield_defense_by_server_id: None,
                armor_multiplier_by_vocation: None,
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
        let mut target_equipment = PlayerEquipment::default();
        target_equipment.equip(EquipmentSlot::Armor, ItemInstance::new(2463, 1).unwrap());
        shared
            .replace_player_equipment(112, target_equipment)
            .unwrap();
        let armor_by_server_id = BTreeMap::from([(2463, 5)]);
        let armor_multiplier_by_vocation = BTreeMap::from([(VocationId::new(0), 1_200)]);
        let (_native_target_id, vitals, outcome) = apply_native_selected_player_melee(
            &mut database,
            &shared,
            111,
            &map,
            NativeSelectedPlayerMeleePolicy {
                progression_rules: Some(&rules_by_vocation),
                skill_rate: 2,
                death_loss_policy: DeathLossPolicy::DefaultFormula,
                armor_by_server_id: Some(&armor_by_server_id),
                shield_defense_by_server_id: None,
                armor_multiplier_by_vocation: Some(&armor_multiplier_by_vocation),
                declarative_weapon_catalog: Some(&catalog),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(outcome.requested_damage, 12);
        assert_eq!(outcome.applied_damage, 6);
        assert_eq!(vitals.health, 14);
        assert_eq!(
            shared
                .player_progression_attempts(111)
                .unwrap()
                .skill_tries(PlayerSkill::Sword),
            2
        );
        assert_eq!(
            shared
                .player_progression_attempts(111)
                .unwrap()
                .skill_tries(PlayerSkill::Fist),
            0
        );
        assert_eq!(
            database
                .player_progression_attempts(111)
                .unwrap()
                .skill_tries(PlayerSkill::Sword),
            2
        );
        assert_eq!(
            database
                .characters_for_account(account_id)
                .unwrap()
                .into_iter()
                .find(|character| character.id == 112)
                .unwrap()
                .vitals
                .health,
            14
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn selected_player_melee_awards_and_persists_each_declared_adjacent_weapon_skill_try() {
        let path = database_path("selected-player-melee-adjacent-weapon-skill-tries");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id = database.create_account("operator", "hash").unwrap();
        let map = native_world_map();
        let scenarios = [
            (401_u64, 402_u64, 2376_u16, PlayerSkill::Sword, 100_u16),
            (403_u64, 404_u64, 2383_u16, PlayerSkill::Axe, 103_u16),
            (405_u64, 406_u64, 2398_u16, PlayerSkill::Club, 106_u16),
        ];
        for (attacker_id, target_id, _weapon_id, _skill, x) in scenarios {
            for (id, name, position) in [
                (
                    attacker_id,
                    format!("Attacker-{attacker_id}"),
                    Position { x, y: 100, z: 7 },
                ),
                (
                    target_id,
                    format!("Target-{target_id}"),
                    Position {
                        x: x.saturating_add(1),
                        y: 100,
                        z: 7,
                    },
                ),
            ] {
                database
                    .save_player(&Player {
                        id,
                        account_id: account_id as u64,
                        name,
                        position,
                        level: 8,
                        experience: 4_900,
                        skill_points: 3,
                    })
                    .unwrap();
            }
        }
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let vocation = VocationId::new(4);
        let progression = PlayerProgression {
            vocation,
            skills: forgotten_core::PlayerSkills::default(),
        };
        for (attacker_id, target_id, _weapon_id, _skill, x) in scenarios {
            shared
                .register_player_at_available_position_with_vitals_equipment_containers_and_progression(
                    Player {
                        id: attacker_id,
                        account_id: account_id as u64,
                        name: format!("Attacker-{attacker_id}"),
                        position: Position { x, y: 100, z: 7 },
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
            shared
                .register_player_at_available_position(
                    Player {
                        id: target_id,
                        account_id: account_id as u64,
                        name: format!("Target-{target_id}"),
                        position: Position {
                            x: x.saturating_add(1),
                            y: 100,
                            z: 7,
                        },
                        level: 8,
                        experience: 4_900,
                        skill_points: 3,
                    },
                    &map,
                )
                .unwrap();
        }
        let catalog = parse_declarative_weapons_xml(
            br#"<fe-weapons><weapon itemid="2376" damage="1" intervalticks="1"/><weapon itemid="2383" damage="1" intervalticks="1"/><weapon itemid="2398" damage="1" intervalticks="1"/></fe-weapons>"#,
        )
        .unwrap()
        .with_adjacent_melee_skills(Some(&BTreeMap::from([
            (2376, PlayerSkill::Sword),
            (2383, PlayerSkill::Axe),
            (2398, PlayerSkill::Club),
        ])));
        let multiplier = forgotten_core::ProgressionMultiplier::new(1_000).unwrap();
        let rules_by_vocation = BTreeMap::from([(
            vocation,
            PlayerProgressionRules {
                magic_level_multiplier: multiplier,
                skill_multipliers: [multiplier; 7],
            },
        )]);
        for (attacker_id, target_id, weapon_id, skill, _x) in scenarios {
            let mut equipment = PlayerEquipment::default();
            equipment.equip(
                EquipmentSlot::RightHand,
                ItemInstance::new(weapon_id, 1).unwrap(),
            );
            shared
                .replace_player_equipment(attacker_id, equipment)
                .unwrap();
            shared
                .set_player_target(attacker_id, Some(target_id))
                .unwrap();
            assert!(apply_native_selected_player_melee(
                &mut database,
                &shared,
                attacker_id,
                &map,
                NativeSelectedPlayerMeleePolicy {
                    progression_rules: Some(&rules_by_vocation),
                    skill_rate: 3,
                    death_loss_policy: DeathLossPolicy::DefaultFormula,
                    armor_by_server_id: None,
                    shield_defense_by_server_id: None,
                    armor_multiplier_by_vocation: None,
                    declarative_weapon_catalog: Some(&catalog),
                },
            )
            .unwrap()
            .is_some());
            assert_eq!(
                shared
                    .player_progression_attempts(attacker_id)
                    .unwrap()
                    .skill_tries(skill),
                3
            );
            assert_eq!(
                database
                    .player_progression_attempts(attacker_id)
                    .unwrap()
                    .skill_tries(skill),
                3
            );
        }
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
                armor_by_server_id: None,
                shield_defense_by_server_id: None,
                armor_multiplier_by_vocation: None,
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
        shared.set_player_follow(201, Some(202)).unwrap();
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
                armor_by_server_id: None,
                shield_defense_by_server_id: None,
                armor_multiplier_by_vocation: None,
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
        assert_eq!(
            shared.player_interaction_intent(201).unwrap(),
            PlayerInteractionIntent::default()
        );
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
    fn shared_player_visibility_uses_authoritative_updated_look_type() {
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
        assert_eq!(shared.visibility_epoch(), 2);
        assert_eq!(
            shared.visible_players(101, 128, 220).unwrap()[0].outfit,
            NativeOtClientClassicOutfit {
                look_type: 128,
                head: 0,
                body: 0,
                legs: 0,
                feet: 0,
            }
        );

        shared
            .update_player_outfit(
                102,
                NativeOtClientClassicOutfit {
                    look_type: 129,
                    head: 1,
                    body: 2,
                    legs: 3,
                    feet: 4,
                },
            )
            .unwrap();

        assert_eq!(shared.visibility_epoch(), 3);
        let visible = shared.visible_players(101, 128, 220).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].player_id, native_player_id(102).unwrap());
        assert_eq!(
            visible[0].position,
            native_position(Position {
                x: 101,
                y: 100,
                z: 7
            })
        );
        assert_eq!(
            visible[0].outfit,
            NativeOtClientClassicOutfit {
                look_type: 129,
                head: 1,
                body: 2,
                legs: 3,
                feet: 4,
            }
        );
        assert_eq!(
            visible[0].direction,
            NativeOtClientCardinalDirection::South.protocol_direction()
        );

        shared
            .update_player_facing(102, NativeOtClientCardinalDirection::East)
            .unwrap();

        assert_eq!(shared.visibility_epoch(), 4);
        let visible = shared.visible_players(101, 128, 220).unwrap();
        assert_eq!(
            visible[0].direction,
            NativeOtClientCardinalDirection::East.protocol_direction()
        );

        let (damage, vitals) = shared.apply_player_melee_damage(101, 102, 75).unwrap();
        assert_eq!(damage.applied_damage, 75);
        assert_eq!(vitals.health, 75);
        assert_eq!(shared.vitals_epoch(), 1);
        assert_eq!(
            shared.visible_players(101, 128, 220).unwrap()[0].health_percent,
            50
        );
        shared.remove_player(102).unwrap();
        shared.remove_player(101).unwrap();
    }

    #[test]
    fn native_outfit_change_refreshes_visible_peer_with_authoritative_look_type() {
        let database_path = database_path("native-peer-outfit-refresh");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        for (id, name, position) in [
            (
                1,
                "Knight",
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
            ),
            (
                2,
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
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.empty_world.as_mut().unwrap().outfit_last_look_type = 129;
        let game = start_native_otclient_game(config, &database_path).unwrap();

        let mut knight = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut knight,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut knight).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        let mut druid = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut druid,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Druid",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let initial_druid_view = read_frame(&mut druid).unwrap();
        let knight_name = [6, 0, b'K', b'n', b'i', b'g', b'h', b't'];
        let initial_knight_name_index = initial_druid_view
            .0
            .windows(knight_name.len())
            .position(|window| window == knight_name)
            .unwrap();
        assert_eq!(
            &initial_druid_view.0[initial_knight_name_index + knight_name.len() + 2
                ..initial_knight_name_index + knight_name.len() + 7],
            &[128, 0, 0, 0, 0]
        );

        write_frame(
            &mut knight,
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
        assert_eq!(
            read_frame(&mut knight).unwrap().0,
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

        druid
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let refreshed_druid_view = (0..3)
            .map(|_| read_frame(&mut druid).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP)
            })
            .expect("peer session did not receive a viewport refresh after outfit change");
        let refreshed_knight_name_index = refreshed_druid_view
            .0
            .windows(knight_name.len())
            .position(|window| window == knight_name)
            .unwrap();
        assert_eq!(
            &refreshed_druid_view.0[refreshed_knight_name_index + knight_name.len() + 2
                ..refreshed_knight_name_index + knight_name.len() + 7],
            &[129, 1, 2, 3, 4]
        );

        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_TURN_EAST]),
        )
        .unwrap();
        let turn_ack = (0..3)
            .map(|_| read_frame(&mut knight).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK)
            })
            .expect("turn acknowledgement was not delivered");
        assert_eq!(
            turn_ack.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 1]
        );
        let peer_turn_refresh = (0..3)
            .map(|_| read_frame(&mut druid).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP)
            })
            .expect("peer session did not receive a viewport refresh after turn");
        let turned_knight_name_index = peer_turn_refresh
            .0
            .windows(knight_name.len())
            .position(|window| window == knight_name)
            .unwrap();
        assert_eq!(
            peer_turn_refresh.0[turned_knight_name_index + knight_name.len() + 1],
            NativeOtClientCardinalDirection::East.protocol_direction()
        );

        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_WEST]),
        )
        .unwrap();
        let peer_move_refresh = (0..3)
            .map(|_| read_frame(&mut druid).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP)
            })
            .expect("peer session did not receive a viewport refresh after cardinal movement");
        let moved_knight_name_index = peer_move_refresh
            .0
            .windows(knight_name.len())
            .position(|window| window == knight_name)
            .unwrap();
        assert_eq!(
            peer_move_refresh.0[moved_knight_name_index + knight_name.len() + 1],
            NativeOtClientCardinalDirection::West.protocol_direction()
        );

        drop(knight);
        drop(druid);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_vitals_refresh_rerenders_peer_health_percent() {
        let database_path = database_path("native-peer-health");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        for (id, name, position) in [
            (
                1,
                "Knight",
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
            ),
            (
                2,
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
        let shared_world = SharedNativeWorld::from_static_spawns(None).unwrap();
        let game = start_native_otclient_game_with_shared_world(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
            shared_world.clone(),
        )
        .unwrap();

        let mut knight = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut knight,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut knight).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        let mut druid = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut druid,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Druid",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let initial_druid_view = read_frame(&mut druid).unwrap();
        let knight_name = [6, 0, b'K', b'n', b'i', b'g', b'h', b't'];
        let initial_knight_name_index = initial_druid_view
            .0
            .windows(knight_name.len())
            .position(|window| window == knight_name)
            .unwrap();
        assert_eq!(
            initial_druid_view.0[initial_knight_name_index + knight_name.len()],
            100
        );

        let (damage, vitals) = shared_world.apply_player_melee_damage(2, 1, 75).unwrap();
        assert_eq!(damage.applied_damage, 75);
        assert_eq!(vitals.health, 75);

        druid
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let refresh_deadline = Instant::now() + Duration::from_secs(5);
        let peer_health_refresh = loop {
            match read_frame(&mut druid) {
                Ok(frame)
                    if frame.0.first()
                        == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP) =>
                {
                    break frame;
                }
                Ok(_) => {}
                Err(HostError::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) && Instant::now() < refresh_deadline => {}
                Err(error) => panic!("peer session failed before vital refresh: {error}"),
            }
            assert!(
                Instant::now() < refresh_deadline,
                "peer session did not receive a viewport refresh after vital change"
            );
        };
        let refreshed_knight_name_index = peer_health_refresh
            .0
            .windows(knight_name.len())
            .position(|window| window == knight_name)
            .unwrap();
        assert_eq!(
            peer_health_refresh.0[refreshed_knight_name_index + knight_name.len()],
            50
        );

        drop(knight);
        drop(druid);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_moves_one_mapped_item_between_empty_equipment_slots() {
        let database_path = database_path("native-slot-transfer");
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
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 1).unwrap(),
        );
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                4526,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                EquipmentSlot::LeftHand.code(),
                0,
                0,
                1,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::LeftHand)
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let persisted = database.player_equipment(1).unwrap();
        assert!(persisted.item(EquipmentSlot::RightHand).is_none());
        assert_eq!(
            persisted.item(EquipmentSlot::LeftHand),
            Some(&ItemInstance::new(4526, 1).unwrap())
        );
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let updates = (0..4)
            .map(|_| read_frame(&mut client).unwrap().0)
            .collect::<Vec<_>>();
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_DELETE_INVENTORY,
                    EquipmentSlot::RightHand.code(),
                ]
        }));
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                    EquipmentSlot::LeftHand.code(),
                    102,
                    0,
                ]
        }));
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_swaps_complete_mapped_items_between_occupied_equipment_slots() {
        let database_path = database_path("native-occupied-slot-swap");
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
        let sword = ItemInstance::new(4526, 1).unwrap();
        let shield = ItemInstance::new(4527, 1).unwrap();
        let mut equipment = PlayerEquipment::default();
        equipment.equip(EquipmentSlot::RightHand, sword.clone());
        equipment.equip(EquipmentSlot::LeftHand, shield.clone());
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                4526,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        catalog
            .insert(
                4527,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 103,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                EquipmentSlot::LeftHand.code(),
                0,
                0,
                1,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            let persisted = database.player_equipment(1).unwrap();
            if persisted.item(EquipmentSlot::RightHand) == Some(&shield)
                && persisted.item(EquipmentSlot::LeftHand) == Some(&sword)
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let persisted = database.player_equipment(1).unwrap();
        assert_eq!(persisted.item(EquipmentSlot::RightHand), Some(&shield));
        assert_eq!(persisted.item(EquipmentSlot::LeftHand), Some(&sword));
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_moves_one_mapped_source_map_item_to_empty_equipment_slot() {
        let database_path = database_path("native-map-source-to-equipment");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let source_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: source_position,
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                source_position,
                vec![WorldMapItem {
                    server_id: 4526,
                    client_thing_id: Some(102),
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
        let source_identity = source_map.source_item_identity(source_position, 0).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                4526,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.world_map = Some(Arc::new(source_map));
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                100,
                0,
                100,
                0,
                7,
                102,
                0,
                0,
                255,
                255,
                EquipmentSlot::LeftHand.code(),
                0,
                0,
                1,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::LeftHand)
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::LeftHand),
            Some(&ItemInstance::new(4526, 1).unwrap())
        );
        assert_eq!(
            database.map_item_removal_journal().unwrap(),
            Some(MapItemRemovalJournal {
                map_revision: source_identity.map_revision,
                removed_items: vec![source_identity],
            })
        );
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_eq!(
            read_frame(&mut client).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                EquipmentSlot::LeftHand.code(),
                102,
                0,
            ]
        );
        let refreshed_viewport = read_frame(&mut client).unwrap().0;
        assert_eq!(
            refreshed_viewport.first().copied(),
            Some(forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP)
        );
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_moves_one_mapped_source_map_item_to_top_level_container() {
        let database_path = database_path("native-map-source-to-container");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let source_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: source_position,
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers
            .insert(
                forgotten_core::PlayerContainer::new(
                    2,
                    ItemInstance::new(1988, 1).unwrap(),
                    "Bag",
                    false,
                    20,
                )
                .unwrap(),
            )
            .unwrap();
        database.replace_player_containers(1, &containers).unwrap();
        let mut source_map = (*native_world_map()).clone();
        source_map
            .set_tile_items(
                source_position,
                vec![WorldMapItem {
                    server_id: 4526,
                    client_thing_id: Some(102),
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
        let source_identity = source_map.source_item_identity(source_position, 0).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id) in [(4526, 102), (1988, 1988)] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype: false,
                    },
                )
                .unwrap();
        }
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.world_map = Some(Arc::new(source_map));
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        let _container = read_frame(&mut client).unwrap();
        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                100,
                0,
                100,
                0,
                7,
                102,
                0,
                0,
                255,
                255,
                0x40 | 2,
                0,
                0,
                1,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_containers(1)
                .unwrap()
                .container(2)
                .unwrap()
                .items
                .item(0)
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            database
                .player_containers(1)
                .unwrap()
                .container(2)
                .unwrap()
                .items
                .item(0),
            Some(&ItemInstance::new(4526, 1).unwrap())
        );
        assert_eq!(
            database.map_item_removal_journal().unwrap(),
            Some(MapItemRemovalJournal {
                map_revision: source_identity.map_revision,
                removed_items: vec![source_identity],
            })
        );
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_eq!(
            read_frame(&mut client).unwrap().0.first().copied(),
            Some(0x6e)
        );
        assert_eq!(
            read_frame(&mut client).unwrap().0.first().copied(),
            Some(forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP)
        );
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_look_map_inspects_one_exact_mapped_equipment_slot() {
        let database_path = database_path("native-equipment-look");
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
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 1).unwrap(),
        );
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        catalog
            .insert(
                4526,
                forgotten_core::NativeItemPresentation {
                    client_thing_id: 102,
                    requires_classic_740_subtype: false,
                },
            )
            .unwrap();
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_LOOK_MAP,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                102,
                0,
                0,
            ]),
        )
        .unwrap();
        let response = read_frame(&mut client).unwrap();
        let expected_message = "Equipment slot 5: item 4526 (count 1).";
        assert_eq!(
            response.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE
        );
        assert_eq!(
            response.0[1],
            forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_LOOK
        );
        assert_eq!(
            u16::from_le_bytes([response.0[2], response.0[3]]) as usize,
            expected_message.len()
        );
        assert_eq!(&response.0[4..], expected_message.as_bytes());

        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_look_map_inspects_one_exact_open_top_level_container_item() {
        let database_path = database_path("native-container-look");
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
        let mut containers = PlayerContainers::default();
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
        containers.insert(container).unwrap();
        database.replace_player_containers(1, &containers).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id) in [(1988, 1988), (4526, 102)] {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype: false,
                    },
                )
                .unwrap();
        }
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _container = read_frame(&mut client).unwrap();
        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_LOOK_MAP,
                255,
                255,
                0x40 | 2,
                0,
                0,
                102,
                0,
                0,
            ]),
        )
        .unwrap();
        let response = read_frame(&mut client).unwrap();
        let expected_message = "Container 2: item 4526 (count 3).";
        assert_eq!(
            response.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE
        );
        assert_eq!(
            response.0[1],
            forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_LOOK
        );
        assert_eq!(
            u16::from_le_bytes([response.0[2], response.0[3]]) as usize,
            expected_message.len()
        );
        assert_eq!(&response.0[4..], expected_message.as_bytes());

        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_moves_one_mapped_equipment_item_to_top_level_container() {
        let database_path = database_path("native-equipment-container-transfer");
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
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 1).unwrap(),
        );
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut containers = PlayerContainers::default();
        containers
            .insert(
                forgotten_core::PlayerContainer::new(
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
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id, requires_classic_740_subtype) in
            [(1988, 1988, false), (4526, 102, true)]
        {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype,
                    },
                )
                .unwrap();
        }
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        let initial_container = read_frame(&mut client).unwrap();
        assert_eq!(initial_container.0.first(), Some(&0x6e));

        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                0x40 | 2,
                0,
                0,
                1,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .is_none()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(database
            .player_equipment(1)
            .unwrap()
            .item(EquipmentSlot::RightHand)
            .is_none());
        let persisted_containers = database.player_containers(1).unwrap();
        assert_eq!(
            persisted_containers.container(2).unwrap().items.item(0),
            Some(&ItemInstance::new(4526, 1).unwrap())
        );

        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let updates = (0..2)
            .map(|_| read_frame(&mut client).unwrap().0)
            .collect::<Vec<_>>();
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_DELETE_INVENTORY,
                    EquipmentSlot::RightHand.code(),
                ]
        }));
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATE_IN_CONTAINER,
                    2,
                    102,
                    0,
                    1,
                ]
        }));
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_merges_partial_mapped_equipment_stack_into_matching_container_item() {
        let database_path = database_path("native-equipment-container-stack-merge");
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
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 25).unwrap(),
        );
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut backpack = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        backpack
            .items
            .insert(ItemInstance::new(4526, 40).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(1, &containers).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id, requires_classic_740_subtype) in
            [(1988, 1988, false), (4526, 102, true)]
        {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype,
                    },
                )
                .unwrap();
        }
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        let initial_container = read_frame(&mut client).unwrap();
        assert_eq!(initial_container.0.first(), Some(&0x6e));

        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                0x40 | 2,
                0,
                0,
                10,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .map(|item| item.count)
                == Some(15)
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&ItemInstance::new(4526, 15).unwrap())
        );
        assert_eq!(
            database
                .player_containers(1)
                .unwrap()
                .container(2)
                .unwrap()
                .items
                .item(0),
            Some(&ItemInstance::new(4526, 50).unwrap())
        );
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let updates = (0..2)
            .map(|_| read_frame(&mut client).unwrap().0)
            .collect::<Vec<_>>();
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                    EquipmentSlot::RightHand.code(),
                    102,
                    0,
                    15,
                ]
        }));
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_CHANGE_IN_CONTAINER,
                    2,
                    0,
                    102,
                    0,
                    50,
                ]
        }));

        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                0x40 | 2,
                0,
                0,
                15,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .is_none()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(database
            .player_equipment(1)
            .unwrap()
            .item(EquipmentSlot::RightHand)
            .is_none());
        assert_eq!(
            database
                .player_containers(1)
                .unwrap()
                .container(2)
                .unwrap()
                .items
                .item(0),
            Some(&ItemInstance::new(4526, 65).unwrap())
        );
        let full_merge_updates = (0..12)
            .map(|_| read_frame(&mut client).unwrap().0)
            .collect::<Vec<_>>();
        assert!(full_merge_updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_DELETE_INVENTORY,
                    EquipmentSlot::RightHand.code(),
                ]
        }));
        assert!(
            full_merge_updates.iter().any(|frame| {
                frame
                    == &vec![
                        forgotten_protocol::NATIVE_OTCLIENT_GAME_CHANGE_IN_CONTAINER,
                        2,
                        0,
                        102,
                        0,
                        65,
                    ]
            }),
            "missing merged-stack container change: {full_merge_updates:?}"
        );
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_moves_one_mapped_top_level_container_item_to_empty_equipment_slot() {
        let database_path = database_path("native-container-equipment-transfer");
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
            .insert(ItemInstance::new(4526, 1).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(container).unwrap();
        database.replace_player_containers(1, &containers).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id, requires_classic_740_subtype) in
            [(1988, 1988, false), (4526, 102, true)]
        {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype,
                    },
                )
                .unwrap();
        }
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let initial_container = read_frame(&mut client).unwrap();
        assert_eq!(
            initial_container.0,
            vec![
                0x6e, 2, 196, 7, 8, 0, b'B', b'a', b'c', b'k', b'p', b'a', b'c', b'k', 20, 0, 1,
                102, 0, 1,
            ]
        );

        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                0x40 | 2,
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                1,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&ItemInstance::new(4526, 1).unwrap())
        );
        assert!(database
            .player_containers(1)
            .unwrap()
            .container(2)
            .unwrap()
            .items
            .is_empty());

        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let updates = (0..2)
            .map(|_| read_frame(&mut client).unwrap().0)
            .collect::<Vec<_>>();
        assert!(
            updates.iter().any(|frame| {
                frame
                    == &vec![
                        forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                        EquipmentSlot::RightHand.code(),
                        102,
                        0,
                        1,
                    ]
            }),
            "missing mapped equipment update: {updates:?}"
        );
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_DELETE_IN_CONTAINER,
                    2,
                    0,
                ]
        }));
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_throw_merges_partial_mapped_container_stack_into_matching_equipment_stack() {
        let database_path = database_path("native-container-equipment-stack-merge");
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
        let mut equipment = PlayerEquipment::default();
        equipment.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(4526, 20).unwrap(),
        );
        database.replace_player_equipment(1, &equipment).unwrap();
        let mut backpack = forgotten_core::PlayerContainer::new(
            2,
            ItemInstance::new(1988, 1).unwrap(),
            "Backpack",
            false,
            20,
        )
        .unwrap();
        backpack
            .items
            .insert(ItemInstance::new(4526, 40).unwrap())
            .unwrap();
        let mut containers = PlayerContainers::default();
        containers.insert(backpack).unwrap();
        database.replace_player_containers(1, &containers).unwrap();
        let mut catalog = NativeItemPresentationCatalog::default();
        for (server_id, client_thing_id, requires_classic_740_subtype) in
            [(1988, 1988, false), (4526, 102, true)]
        {
            catalog
                .insert(
                    server_id,
                    forgotten_core::NativeItemPresentation {
                        client_thing_id,
                        requires_classic_740_subtype,
                    },
                )
                .unwrap();
        }
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.item_presentation_catalog = Some(Arc::new(catalog));
        let game = start_native_otclient_game(config, &database_path).unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let _initialization = read_frame(&mut client).unwrap();
        let _equipment = read_frame(&mut client).unwrap();
        let initial_container = read_frame(&mut client).unwrap();
        assert_eq!(initial_container.0.first(), Some(&0x6e));

        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                0x40 | 2,
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                10,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .map(|item| item.count)
                == Some(30)
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&ItemInstance::new(4526, 30).unwrap())
        );
        assert_eq!(
            database
                .player_containers(1)
                .unwrap()
                .container(2)
                .unwrap()
                .items
                .item(0),
            Some(&ItemInstance::new(4526, 30).unwrap())
        );
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let updates = (0..2)
            .map(|_| read_frame(&mut client).unwrap().0)
            .collect::<Vec<_>>();
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                    EquipmentSlot::RightHand.code(),
                    102,
                    0,
                    30,
                ]
        }));
        assert!(updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_CHANGE_IN_CONTAINER,
                    2,
                    0,
                    102,
                    0,
                    30,
                ]
        }));

        write_frame(
            &mut client,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_THROW_ITEM,
                255,
                255,
                0x40 | 2,
                0,
                0,
                102,
                0,
                0,
                255,
                255,
                EquipmentSlot::RightHand.code(),
                0,
                0,
                30,
            ]),
        )
        .unwrap();
        for _ in 0..20 {
            if database
                .player_containers(1)
                .unwrap()
                .container(2)
                .unwrap()
                .items
                .is_empty()
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand),
            Some(&ItemInstance::new(4526, 60).unwrap())
        );
        assert!(database
            .player_containers(1)
            .unwrap()
            .container(2)
            .unwrap()
            .items
            .is_empty());
        let full_merge_updates = (0..12)
            .map(|_| read_frame(&mut client).unwrap().0)
            .collect::<Vec<_>>();
        assert!(full_merge_updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_SET_INVENTORY,
                    EquipmentSlot::RightHand.code(),
                    102,
                    0,
                    60,
                ]
        }));
        assert!(full_merge_updates.iter().any(|frame| {
            frame
                == &vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_DELETE_IN_CONTAINER,
                    2,
                    0,
                ]
        }));
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_relog_respawns_a_persisted_dead_character_at_temple_before_full_map_bootstrap() {
        let database_path = database_path("native-relog-temple-respawn");
        let mut database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        let death_position = Position {
            x: 101,
            y: 100,
            z: 7,
        };
        let temple_position = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Knight".into(),
                position: death_position,
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        database
            .update_player_position_vitals_and_respawn_state(
                1,
                death_position,
                PersistedPlayerVitals {
                    health: 0,
                    max_health: 150,
                    mana: 0,
                    max_mana: 40,
                    capacity: 4_000,
                    magic_level: 0,
                },
                PlayerRespawnState {
                    dead: true,
                    respawn_at: Some(temple_position),
                    death_time: Some(1),
                    loss_applied: true,
                },
            )
            .unwrap();
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();
        let mut client = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut client,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        let initialization = read_frame(&mut client).unwrap();
        assert!(initialization.0.windows(6).any(|window| {
            window
                == [
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP,
                    100,
                    0,
                    100,
                    0,
                    7,
                ]
        }));
        let persisted = database.player_by_id(1).unwrap();
        assert_eq!(persisted.position, temple_position);
        assert_eq!(
            persisted.vitals,
            PersistedPlayerVitals {
                health: 150,
                max_health: 150,
                mana: 40,
                max_mana: 40,
                capacity: 4_000,
                magic_level: 0,
            }
        );
        assert_eq!(persisted.respawn_state, PlayerRespawnState::default());
        drop(client);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_empty_quest_log_and_channel_list_responses_keep_session_usable() {
        let database_path = database_path("native-empty-quest-log");
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
        assert_eq!(
            read_frame(&mut stream).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

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
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT,
                128,
                0,
                0,
                0,
                0,
                128,
                128,
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CHANNELS, 0]
        );

        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_private_message_delivery_is_authenticated_and_exactly_session_local() {
        let database_path = database_path("native-private-message");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        for (id, name, position) in [
            (
                1,
                "Knight",
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
            ),
            (
                2,
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
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut knight = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut knight,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut knight).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        let mut druid = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut druid,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Druid",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut druid).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        write_frame(
            &mut knight,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_TALK,
                5,
                5,
                0,
                b'D',
                b'r',
                b'u',
                b'i',
                b'd',
                2,
                0,
                b'h',
                b'i',
            ]),
        )
        .unwrap();
        druid
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let private_message = (0..4)
            .map(|_| read_frame(&mut druid).unwrap())
            .find(|frame| frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_TALK))
            .expect("recipient session did not receive private message");
        assert_eq!(
            private_message.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TALK,
                6,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                4,
                2,
                0,
                b'h',
                b'i',
            ]
        );

        drop(knight);
        drop(druid);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_classic_vip_add_edit_and_remove_are_authenticated_and_persisted() {
        let database_path = database_path("native-classic-vip");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        for (id, name) in [(1, "Knight"), (2, "Druid")] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
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
        }
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
        assert_eq!(
            read_frame(&mut stream).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_ADD_VIP,
                5,
                0,
                b'D',
                b'r',
                b'u',
                b'i',
                b'd',
            ]),
        )
        .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let vip_add = (0..4)
            .map(|_| read_frame(&mut stream).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_ADD)
            })
            .expect("native VIP add response was not delivered");
        assert_eq!(
            vip_add.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_ADD,
                2,
                0,
                0,
                0,
                5,
                0,
                b'D',
                b'r',
                b'u',
                b'i',
                b'd',
                0,
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_EDIT_VIP,
                2,
                0,
                0,
                0,
                4,
                0,
                b'n',
                b'o',
                b't',
                b'e',
                3,
                0,
                0,
                0,
                1,
            ]),
        )
        .unwrap();
        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_PING_BACK]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REMOVE_VIP,
                2,
                0,
                0,
                0,
            ]),
        )
        .unwrap();
        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_PING_BACK]
        );

        drop(stream);
        game.shutdown().unwrap();
        let database = EngineDatabase::open(&database_path).unwrap();
        assert!(database
            .account_vip_entries(account_id.try_into().unwrap())
            .unwrap()
            .is_empty());
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_classic_party_requests_route_to_the_shared_session_local_core() {
        fn party_target_frame(opcode: u8, character_id: u32) -> Frame {
            let mut bytes = vec![opcode];
            bytes.extend_from_slice(
                &(forgotten_protocol::NATIVE_OTCLIENT_PLAYER_ID_START + character_id).to_le_bytes(),
            );
            Frame(bytes)
        }

        fn wait_for_ping_back(stream: &mut TcpStream) -> Vec<Frame> {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut preceding_frames = Vec::new();
            loop {
                match read_frame(stream) {
                    Ok(frame)
                        if frame.0 == vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_PING_BACK] =>
                    {
                        return preceding_frames;
                    }
                    Ok(frame) => preceding_frames.push(frame),
                    Err(HostError::Io(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) && Instant::now() < deadline => {}
                    Err(error) => {
                        panic!("native session ended before party action was observed: {error}")
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "native session did not remain usable after party action"
                );
            }
        }

        let database_path = database_path("native-classic-party-routing");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        for (id, name, position) in [
            (
                1,
                "Knight",
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
            ),
            (
                2,
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
        let shared_world = SharedNativeWorld::from_static_spawns(None).unwrap();
        let shared_experience_rules = PartySharedExperienceRules {
            maximum_range: 30,
            maximum_floor_delta: 1,
            activity_window_ticks: 60,
        };
        let mut native_config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        native_config.party_shared_experience_rules = Some(shared_experience_rules);
        let game = start_native_otclient_game_with_shared_world(
            native_config,
            &database_path,
            shared_world.clone(),
        )
        .unwrap();

        let mut knight = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut knight,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut knight).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        let mut druid = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut druid,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Druid",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut druid).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        write_frame(
            &mut druid,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        let _ = wait_for_ping_back(&mut druid);

        write_frame(
            &mut knight,
            &party_target_frame(
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY,
                2,
            ),
        )
        .unwrap();
        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        let knight_invite_frames = wait_for_ping_back(&mut knight);
        assert!(knight_invite_frames.iter().any(|frame| {
            frame.0
                == vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_PARTY,
                    1,
                    0,
                    0,
                    16,
                    4,
                ]
        }));
        assert!(knight_invite_frames.iter().any(|frame| {
            frame.0
                == vec![
                    forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_PARTY,
                    2,
                    0,
                    0,
                    16,
                    2,
                ]
        }));
        write_frame(
            &mut knight,
            &party_target_frame(
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REVOKE_PARTY_INVITATION,
                2,
            ),
        )
        .unwrap();
        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut knight);
        write_frame(
            &mut druid,
            &party_target_frame(forgotten_protocol::NATIVE_OTCLIENT_CLIENT_JOIN_PARTY, 1),
        )
        .unwrap();
        write_frame(
            &mut druid,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut druid);
        assert_eq!(
            shared_world.lock().unwrap().player_party_leader(1).unwrap(),
            None
        );
        assert_eq!(
            shared_world.lock().unwrap().player_party_leader(2).unwrap(),
            None
        );

        write_frame(
            &mut knight,
            &party_target_frame(
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY,
                2,
            ),
        )
        .unwrap();
        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut knight);
        write_frame(
            &mut druid,
            &party_target_frame(forgotten_protocol::NATIVE_OTCLIENT_CLIENT_JOIN_PARTY, 1),
        )
        .unwrap();
        write_frame(
            &mut druid,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut druid);
        assert_eq!(
            shared_world.lock().unwrap().player_party_leader(1).unwrap(),
            Some(1)
        );
        assert_eq!(
            shared_world
                .lock()
                .unwrap()
                .player_party_members(1)
                .unwrap(),
            vec![2]
        );

        write_frame(
            &mut knight,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE,
                1,
                0,
            ]),
        )
        .unwrap();
        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut knight);
        assert!(
            shared_world
                .lock()
                .unwrap()
                .party_shared_experience_state(1, shared_experience_rules)
                .unwrap()
                .requested
        );

        write_frame(
            &mut knight,
            &party_target_frame(
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PASS_PARTY_LEADERSHIP,
                2,
            ),
        )
        .unwrap();
        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut knight);
        assert_eq!(
            shared_world.lock().unwrap().player_party_leader(1).unwrap(),
            Some(2)
        );
        assert_eq!(
            shared_world
                .lock()
                .unwrap()
                .player_party_members(2)
                .unwrap(),
            vec![1]
        );

        write_frame(
            &mut druid,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_LEAVE_PARTY]),
        )
        .unwrap();
        write_frame(
            &mut druid,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut druid);
        assert_eq!(
            shared_world.lock().unwrap().player_party_leader(1).unwrap(),
            Some(1)
        );
        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_LEAVE_PARTY]),
        )
        .unwrap();
        write_frame(
            &mut knight,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_PING]),
        )
        .unwrap();
        wait_for_ping_back(&mut knight);
        assert_eq!(
            shared_world.lock().unwrap().player_party_leader(1).unwrap(),
            None
        );
        assert_eq!(
            shared_world.lock().unwrap().player_party_leader(2).unwrap(),
            None
        );

        drop(knight);
        drop(druid);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_classic_login_delivers_persisted_vip_entries_after_initialization() {
        let database_path = database_path("native-classic-login-vip");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        for (id, name) in [(1, "Knight"), (2, "Druid")] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
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
        }
        database
            .add_account_vip_entry(
                account_id.try_into().unwrap(),
                "Druid",
                "persisted metadata",
                4,
                true,
            )
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
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let vip_entry = (0..4)
            .map(|_| read_frame(&mut stream).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_ADD)
            })
            .expect("persisted VIP entry was not delivered after native initialization");
        assert_eq!(
            vip_entry.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_ADD,
                2,
                0,
                0,
                0,
                5,
                0,
                b'D',
                b'r',
                b'u',
                b'i',
                b'd',
                0,
            ]
        );
        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_classic_vip_watcher_receives_target_login_and_logout_presence_frames() {
        let database_path = database_path("native-classic-vip-presence");
        let database = EngineDatabase::open(&database_path).unwrap();
        let account_id = database
            .create_account_with_password("operator", "correct horse battery staple")
            .unwrap();
        for (id, name) in [(1, "Knight"), (2, "Druid")] {
            database
                .save_player(&Player {
                    id,
                    account_id: account_id as u64,
                    name: name.into(),
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
        }
        database
            .add_account_vip_entry(account_id.try_into().unwrap(), "Druid", "", 0, false)
            .unwrap();
        let game = start_native_otclient_game(
            native_empty_world_config("127.0.0.1:0".parse().unwrap()),
            &database_path,
        )
        .unwrap();

        let mut knight = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut knight,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut knight).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        knight
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let initial_vip = (0..4)
            .map(|_| read_frame(&mut knight).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_ADD)
            })
            .expect("watcher did not receive the persisted VIP entry");
        assert_eq!(
            initial_vip.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_ADD,
                2,
                0,
                0,
                0,
                5,
                0,
                b'D',
                b'r',
                b'u',
                b'i',
                b'd',
                0,
            ]
        );

        let mut druid = TcpStream::connect(game.local_addr()).unwrap();
        write_frame(
            &mut druid,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Druid",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut druid).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        let online = (0..4)
            .map(|_| read_frame(&mut knight).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_STATE)
            })
            .expect("watcher did not receive the target online VIP frame");
        assert_eq!(
            online.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_STATE,
                2,
                0,
                0,
                0
            ]
        );

        drop(druid);
        let offline = (0..4)
            .map(|_| read_frame(&mut knight).unwrap())
            .find(|frame| {
                frame.0.first() == Some(&forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_LOGOUT)
            })
            .expect("watcher did not receive the target offline VIP frame");
        assert_eq!(
            offline.0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_VIP_LOGOUT,
                2,
                0,
                0,
                0
            ]
        );

        drop(knight);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_configured_public_channel_lifecycle_is_authenticated_and_session_local() {
        let database_path = database_path("native-configured-public-channel");
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
        let mut config = native_empty_world_config("127.0.0.1:0".parse().unwrap());
        config.public_channel_catalog = Some(Arc::new(
            parse_tfs_public_channels_xml(
                br#"<channels><channel id="7" name="Trade" public="true"/></channels>"#,
            )
            .unwrap(),
        ));
        let game = start_native_otclient_game(config, &database_path).unwrap();
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
        assert_eq!(
            read_frame(&mut stream).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CHANNELS,
                1,
                7,
                0,
                5,
                0,
                b'T',
                b'r',
                b'a',
                b'd',
                b'e',
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_JOIN_CHANNEL,
                7,
                0,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_OPEN_CHANNEL,
                7,
                0,
                5,
                0,
                b'T',
                b'r',
                b'a',
                b'd',
                b'e',
                0,
                0,
                0,
                0,
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_TALK,
                7,
                7,
                0,
                2,
                0,
                b'h',
                b'i',
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TALK,
                6,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                7,
                7,
                0,
                2,
                0,
                b'h',
                b'i',
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_LEAVE_CHANNEL,
                7,
                0,
            ]),
        )
        .unwrap();
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_TALK,
                7,
                7,
                0,
                7,
                0,
                b'i',
                b'g',
                b'n',
                b'o',
                b'r',
                b'e',
                b'd',
            ]),
        )
        .unwrap();
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT,
                128,
                0,
                0,
                0,
                0,
                128,
                128,
            ]
        );
        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_rejected_target_selection_emits_classic_clear_target_record() {
        let database_path = database_path("native-rejected-target-selection");
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
        let mut stream = TcpStream::connect(game.local_addr()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write_frame(
            &mut stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_SELECT_TARGET,
                1,
                0,
                0,
                0,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CLEAR_TARGET]
        );
        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn native_explicit_target_clear_emits_classic_clear_target_record() {
        let database_path = database_path("native-explicit-target-clear");
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
        let mut stream = TcpStream::connect(game.local_addr()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write_frame(
            &mut stream,
            &native_game_request(
                account_id.try_into().unwrap(),
                "Knight",
                "correct horse battery staple",
            ),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_LOGIN_STATE
        );
        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_SELECT_TARGET,
                0,
                0,
                0,
                0,
            ]),
        )
        .unwrap();
        assert_eq!(
            read_frame(&mut stream).unwrap().0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CLEAR_TARGET]
        );
        drop(stream);
        game.shutdown().unwrap();
        let _ = fs::remove_file(database_path);
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
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(map.spawn()),
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
        let render_snapshot = shared.native_render_snapshot(101, 128, 220).unwrap();
        let expected = encode_native_otclient_map_viewport_with_static_spawns_and_players(
            &profile,
            &snapshot,
            &map,
            Some(&render_snapshot.static_spawns),
            Some(&render_snapshot.visible_players),
        )
        .unwrap();
        let (worker, worker_thread) = NativeRenderPreparationWorker::start(Duration::from_secs(1));
        let prepared = worker
            .prepare(profile, snapshot, Arc::clone(&map), render_snapshot)
            .unwrap();
        shared.remove_player(102).unwrap();
        drop(worker);
        worker_thread.join().unwrap();
        assert_eq!(prepared, expected);
        assert!(prepared.0.windows(5).any(|window| window == b"Druid"));
        assert!(shared.visible_players(101, 0, 220).unwrap().is_empty());
    }

    #[test]
    fn native_render_preparation_pool_orders_detached_publications_and_rejects_invalid_batches() {
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
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(map.spawn()),
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
        let render_snapshot = shared.native_render_snapshot(101, 128, 220).unwrap();
        let publication = |sequence, x| {
            let mut snapshot = snapshot.clone();
            snapshot.player_position.x = x;
            NativeRenderPublication {
                sequence,
                profile: profile.clone(),
                snapshot,
                world_map: Arc::clone(&map),
                render_snapshot: render_snapshot.clone(),
            }
        };
        let ordered_publications = [
            publication(10, 100),
            publication(20, 101),
            publication(30, 102),
        ];
        let expected = ordered_publications
            .iter()
            .map(|publication| {
                encode_native_otclient_map_viewport_with_static_spawns_and_players(
                    &publication.profile,
                    &publication.snapshot,
                    publication.world_map.as_ref(),
                    Some(&publication.render_snapshot.static_spawns),
                    Some(&publication.render_snapshot.visible_players),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let (pool, worker_threads) =
            NativeRenderPreparationPool::start(3, Duration::from_secs(1)).unwrap();
        let prepared = pool
            .prepare_batch(vec![
                publication(30, 102),
                publication(10, 100),
                publication(20, 101),
            ])
            .unwrap();
        shared.remove_player(102).unwrap();
        assert_eq!(prepared, expected);
        assert!(prepared
            .iter()
            .all(|frame| frame.0.windows(5).any(|window| window == b"Druid")));

        assert!(matches!(
            NativeRenderPreparationPool::start(0, Duration::from_secs(1)),
            Err(NativeRenderPublicationError::InvalidWorkerCount(0))
        ));
        assert_eq!(
            pool.prepare_batch(vec![publication(1, 100), publication(1, 101)]),
            Err(NativeRenderPublicationError::DuplicateSequence(1))
        );
        let over_limit = (0..=MAX_NATIVE_RENDER_PUBLICATION_BATCH as u64)
            .map(|sequence| publication(sequence, 100))
            .collect::<Vec<_>>();
        assert_eq!(
            pool.prepare_batch(over_limit),
            Err(NativeRenderPublicationError::PublicationLimitExceeded {
                limit: MAX_NATIVE_RENDER_PUBLICATION_BATCH,
            })
        );

        drop(pool);
        for worker_thread in worker_threads {
            worker_thread.join().unwrap();
        }
        assert!(shared.visible_players(101, 0, 220).unwrap().is_empty());
    }

    #[test]
    #[ignore = "run explicitly in release mode to collect local native-render benchmark samples"]
    fn benchmark_native_render_preparation_direct_and_worker() {
        use std::time::Instant;

        const SAMPLE_COUNT: usize = 9;
        const ITERATIONS_PER_SAMPLE: usize = 1_000;

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
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(map.spawn()),
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
        let render_snapshot = shared.native_render_snapshot(101, 128, 220).unwrap();
        let expected = encode_native_otclient_map_viewport_with_static_spawns_and_players(
            &profile,
            &snapshot,
            &map,
            Some(&render_snapshot.static_spawns),
            Some(&render_snapshot.visible_players),
        )
        .unwrap();

        let mut direct_samples = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let started = Instant::now();
            for _ in 0..ITERATIONS_PER_SAMPLE {
                let prepared = encode_native_otclient_map_viewport_with_static_spawns_and_players(
                    &profile,
                    &snapshot,
                    &map,
                    Some(&render_snapshot.static_spawns),
                    Some(&render_snapshot.visible_players),
                )
                .unwrap();
                assert_eq!(prepared, expected);
            }
            direct_samples.push(started.elapsed());
        }

        let (worker, worker_thread) = NativeRenderPreparationWorker::start(Duration::from_secs(1));
        let mut worker_samples = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let started = Instant::now();
            for _ in 0..ITERATIONS_PER_SAMPLE {
                let prepared = worker
                    .prepare(
                        profile.clone(),
                        snapshot.clone(),
                        Arc::clone(&map),
                        render_snapshot.clone(),
                    )
                    .unwrap();
                assert_eq!(prepared, expected);
            }
            worker_samples.push(started.elapsed());
        }
        drop(worker);
        worker_thread.join().unwrap();

        let micros = |samples: &[Duration]| {
            samples
                .iter()
                .map(|sample| sample.as_micros())
                .collect::<Vec<_>>()
        };
        println!(
            "native-render-benchmark scenario=two-visible-players samples={SAMPLE_COUNT} iterations_per_sample={ITERATIONS_PER_SAMPLE} direct_total_us={:?} worker_total_us={:?}",
            micros(&direct_samples),
            micros(&worker_samples),
        );
    }

    #[test]
    #[ignore = "run explicitly in release mode to collect local concurrent native-render benchmark samples"]
    fn benchmark_native_render_preparation_concurrent_direct_and_workers() {
        use std::sync::Barrier;
        use std::time::Instant;

        const SAMPLE_COUNT: usize = 7;
        const SESSION_COUNT: usize = 3;
        const ITERATIONS_PER_SESSION: usize = 500;

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
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(map.spawn()),
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
        let render_snapshot = shared.native_render_snapshot(101, 128, 220).unwrap();
        let expected = encode_native_otclient_map_viewport_with_static_spawns_and_players(
            &profile,
            &snapshot,
            &map,
            Some(&render_snapshot.static_spawns),
            Some(&render_snapshot.visible_players),
        )
        .unwrap();

        let mut direct_samples = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let barrier = Arc::new(Barrier::new(SESSION_COUNT + 1));
            let mut sessions = Vec::with_capacity(SESSION_COUNT);
            for _ in 0..SESSION_COUNT {
                let barrier = Arc::clone(&barrier);
                let profile = profile.clone();
                let snapshot = snapshot.clone();
                let map = Arc::clone(&map);
                let render_snapshot = render_snapshot.clone();
                let expected = expected.clone();
                sessions.push(thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..ITERATIONS_PER_SESSION {
                        let prepared =
                            encode_native_otclient_map_viewport_with_static_spawns_and_players(
                                &profile,
                                &snapshot,
                                &map,
                                Some(&render_snapshot.static_spawns),
                                Some(&render_snapshot.visible_players),
                            )
                            .unwrap();
                        assert_eq!(prepared, expected);
                    }
                }));
            }
            let started = Instant::now();
            barrier.wait();
            for session in sessions {
                session.join().unwrap();
            }
            direct_samples.push(started.elapsed());
        }

        let (first_worker, first_worker_thread) =
            NativeRenderPreparationWorker::start(Duration::from_secs(1));
        let (second_worker, second_worker_thread) =
            NativeRenderPreparationWorker::start(Duration::from_secs(1));
        let (third_worker, third_worker_thread) =
            NativeRenderPreparationWorker::start(Duration::from_secs(1));
        let workers = [
            first_worker.clone(),
            second_worker.clone(),
            third_worker.clone(),
        ];
        let mut worker_samples = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let barrier = Arc::new(Barrier::new(SESSION_COUNT + 1));
            let mut sessions = Vec::with_capacity(SESSION_COUNT);
            for worker in workers.clone() {
                let barrier = Arc::clone(&barrier);
                let profile = profile.clone();
                let snapshot = snapshot.clone();
                let map = Arc::clone(&map);
                let render_snapshot = render_snapshot.clone();
                let expected = expected.clone();
                sessions.push(thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..ITERATIONS_PER_SESSION {
                        let prepared = worker
                            .prepare(
                                profile.clone(),
                                snapshot.clone(),
                                Arc::clone(&map),
                                render_snapshot.clone(),
                            )
                            .unwrap();
                        assert_eq!(prepared, expected);
                    }
                }));
            }
            let started = Instant::now();
            barrier.wait();
            for session in sessions {
                session.join().unwrap();
            }
            worker_samples.push(started.elapsed());
        }
        drop(workers);
        drop(first_worker);
        drop(second_worker);
        drop(third_worker);
        first_worker_thread.join().unwrap();
        second_worker_thread.join().unwrap();
        third_worker_thread.join().unwrap();

        let micros = |samples: &[Duration]| {
            samples
                .iter()
                .map(|sample| sample.as_micros())
                .collect::<Vec<_>>()
        };
        println!(
            "native-render-concurrent-benchmark scenario=three-immutable-session-streams samples={SAMPLE_COUNT} sessions={SESSION_COUNT} iterations_per_session={ITERATIONS_PER_SESSION} direct_total_us={:?} worker_total_us={:?}",
            micros(&direct_samples),
            micros(&worker_samples),
        );
    }

    #[test]
    #[ignore = "run explicitly in release mode to collect deterministic publication-pool benchmark samples"]
    fn benchmark_native_render_preparation_ordered_publication_pool() {
        use std::time::Instant;

        const SAMPLE_COUNT: usize = 9;
        const BATCHES_PER_SAMPLE: usize = 500;
        const PUBLICATIONS_PER_BATCH: usize = 3;

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
        let profile = native_otclient_config("127.0.0.1:0".parse().unwrap()).client_profile;
        let snapshot = NativeOtClientEmptyWorldSnapshot {
            player_id: native_player_id(101).unwrap(),
            player_name: "Knight".into(),
            player_position: native_position(map.spawn()),
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
        let render_snapshot = shared.native_render_snapshot(101, 128, 220).unwrap();
        let publication = |sequence, x| {
            let mut snapshot = snapshot.clone();
            snapshot.player_position.x = x;
            NativeRenderPublication {
                sequence,
                profile: profile.clone(),
                snapshot,
                world_map: Arc::clone(&map),
                render_snapshot: render_snapshot.clone(),
            }
        };
        let publications = [
            publication(10, 100),
            publication(20, 101),
            publication(30, 102),
        ];
        let expected = publications
            .iter()
            .map(|publication| {
                encode_native_otclient_map_viewport_with_static_spawns_and_players(
                    &publication.profile,
                    &publication.snapshot,
                    publication.world_map.as_ref(),
                    Some(&publication.render_snapshot.static_spawns),
                    Some(&publication.render_snapshot.visible_players),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        let mut direct_samples = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let started = Instant::now();
            for _ in 0..BATCHES_PER_SAMPLE {
                let direct = publications
                    .iter()
                    .map(|publication| {
                        encode_native_otclient_map_viewport_with_static_spawns_and_players(
                            &publication.profile,
                            &publication.snapshot,
                            publication.world_map.as_ref(),
                            Some(&publication.render_snapshot.static_spawns),
                            Some(&publication.render_snapshot.visible_players),
                        )
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(direct, expected);
            }
            direct_samples.push(started.elapsed());
        }

        let (pool, worker_threads) =
            NativeRenderPreparationPool::start(PUBLICATIONS_PER_BATCH, Duration::from_secs(1))
                .unwrap();
        let mut pool_samples = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let started = Instant::now();
            for _ in 0..BATCHES_PER_SAMPLE {
                assert_eq!(pool.prepare_batch(publications.to_vec()).unwrap(), expected);
            }
            pool_samples.push(started.elapsed());
        }
        drop(pool);
        for worker_thread in worker_threads {
            worker_thread.join().unwrap();
        }

        let micros = |samples: &[Duration]| {
            samples
                .iter()
                .map(|sample| sample.as_micros())
                .collect::<Vec<_>>()
        };
        println!(
            "native-render-publication-pool-benchmark scenario=ordered-three-publication-batches samples={SAMPLE_COUNT} batches_per_sample={BATCHES_PER_SAMPLE} publications_per_batch={PUBLICATIONS_PER_BATCH} direct_total_us={:?} pool_total_us={:?}",
            micros(&direct_samples),
            micros(&pool_samples),
        );
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
        let knight_events = shared
            .register_public_chat_recipient(101, "Knight")
            .unwrap();
        let druid_events = shared.register_public_chat_recipient(102, "Druid").unwrap();
        assert_eq!(
            shared
                .broadcast_public_chat(101, "  hello\n world  ")
                .unwrap(),
            2
        );
        let expected = SharedPublicChatEvent {
            speaker_name: "Knight".into(),
            speaker_position: native_position(map.spawn()),
            channel_id: None,
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
            text: "hello world".into(),
        };
        assert_eq!(knight_events.try_recv().unwrap(), expected);
        assert_eq!(druid_events.try_recv().unwrap(), expected);
        assert_eq!(
            shared
                .broadcast_configured_public_channel_chat(101, 7, "  trade\n offer  ")
                .unwrap(),
            2
        );
        let channel_event = SharedPublicChatEvent {
            speaker_name: "Knight".into(),
            speaker_position: native_position(map.spawn()),
            channel_id: Some(7),
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
            text: "trade offer".into(),
        };
        assert_eq!(knight_events.try_recv().unwrap(), channel_event);
        assert_eq!(druid_events.try_recv().unwrap(), channel_event);
        assert_eq!(
            shared
                .send_private_chat(101, "Druid", "  private\n hello  ")
                .unwrap(),
            1
        );
        assert!(matches!(
            knight_events.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(
            druid_events.try_recv().unwrap(),
            SharedPublicChatEvent {
                speaker_name: "Knight".into(),
                speaker_position: native_position(map.spawn()),
                channel_id: None,
                private: true,
                talk_mode: NATIVE_OTCLIENT_MESSAGE_SAY,
                text: "private hello".into(),
            }
        );
        assert_eq!(shared.send_private_chat(101, "druid", "miss").unwrap(), 0);
        assert!(matches!(
            druid_events.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
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
    fn shared_whisper_reaches_only_nearby_same_floor_listeners() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        // Speaker at spawn (100,100,7); listener adjacent; far listener same floor; cross-floor
        // listener adjacent in x/y but on a different floor.
        for (id, name, position) in [
            (
                101u64,
                "Knight",
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
            ),
            (
                102,
                "Druid",
                Position {
                    x: 101,
                    y: 100,
                    z: 7,
                },
            ),
            (
                103,
                "Far",
                Position {
                    x: 130,
                    y: 130,
                    z: 7,
                },
            ),
            (
                104,
                "Below",
                Position {
                    x: 101,
                    y: 100,
                    z: 6,
                },
            ),
        ] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position,
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap();
        }
        let knight_events = shared
            .register_public_chat_recipient(101, "Knight")
            .unwrap();
        let druid_events = shared.register_public_chat_recipient(102, "Druid").unwrap();
        let far_events = shared.register_public_chat_recipient(103, "Far").unwrap();
        let below_events = shared.register_public_chat_recipient(104, "Below").unwrap();
        assert_eq!(shared.broadcast_whisper_chat(101, "psst").unwrap(), 2);
        let expected = SharedPublicChatEvent {
            speaker_name: "Knight".into(),
            speaker_position: native_position(Position {
                x: 100,
                y: 100,
                z: 7,
            }),
            channel_id: None,
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_WHISPER,
            text: "psst".into(),
        };
        assert_eq!(knight_events.try_recv().unwrap(), expected);
        assert_eq!(druid_events.try_recv().unwrap(), expected);
        assert!(matches!(
            far_events.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(matches!(
            below_events.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        for id in [101u64, 102, 103, 104] {
            shared.unregister_public_chat_recipient(id);
            shared.remove_player(id).unwrap();
        }
    }

    #[test]
    fn shared_yell_reaches_wide_range_but_not_cross_floor() {
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let map = native_world_map();
        for (id, name, position) in [
            (
                201u64,
                "Speaker",
                Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
            ),
            (
                202,
                "Near",
                Position {
                    x: 110,
                    y: 110,
                    z: 7,
                },
            ),
            (
                203,
                "TooFar",
                Position {
                    x: 140,
                    y: 140,
                    z: 7,
                },
            ),
            (
                204,
                "Below",
                Position {
                    x: 110,
                    y: 110,
                    z: 6,
                },
            ),
        ] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position,
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap();
        }
        let speaker_events = shared
            .register_public_chat_recipient(201, "Speaker")
            .unwrap();
        let near_events = shared.register_public_chat_recipient(202, "Near").unwrap();
        let too_far_events = shared
            .register_public_chat_recipient(203, "TooFar")
            .unwrap();
        let below_events = shared.register_public_chat_recipient(204, "Below").unwrap();
        assert_eq!(shared.broadcast_yell_chat(201, "hail").unwrap(), 2);
        let expected = SharedPublicChatEvent {
            speaker_name: "Speaker".into(),
            speaker_position: native_position(Position {
                x: 100,
                y: 100,
                z: 7,
            }),
            channel_id: None,
            private: false,
            talk_mode: NATIVE_OTCLIENT_MESSAGE_YELL,
            text: "hail".into(),
        };
        assert_eq!(speaker_events.try_recv().unwrap(), expected);
        assert_eq!(near_events.try_recv().unwrap(), expected);
        assert!(matches!(
            too_far_events.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(matches!(
            below_events.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        for id in [201u64, 202, 203, 204] {
            shared.unregister_public_chat_recipient(id);
            shared.remove_player(id).unwrap();
        }
    }

    #[test]
    fn shared_vip_presence_delivers_only_to_matching_active_watchers() {
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
        let knight_events = shared
            .register_vip_presence_recipient(101, BTreeSet::from([102]))
            .unwrap();
        let druid_events = shared
            .register_vip_presence_recipient(102, BTreeSet::from([101, 102]))
            .unwrap();

        assert_eq!(shared.publish_vip_presence(102, true).unwrap(), 1);
        assert_eq!(
            knight_events.try_recv().unwrap(),
            SharedVipPresenceEvent {
                target_player_id: 102,
                online: true,
            }
        );
        assert!(matches!(
            druid_events.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        assert_eq!(shared.publish_vip_presence(101, false).unwrap(), 1);
        assert_eq!(
            druid_events.try_recv().unwrap(),
            SharedVipPresenceEvent {
                target_player_id: 101,
                online: false,
            }
        );

        shared.unregister_vip_presence_recipient(101);
        assert_eq!(shared.publish_vip_presence(102, false).unwrap(), 0);
        assert!(matches!(
            knight_events.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        shared.unregister_vip_presence_recipient(102);
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
        let events = shared
            .register_public_chat_recipient(101, "Knight")
            .unwrap();
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

    fn start_native_otclient_game_with_shared_world(
        config: NativeOtClientHostConfig,
        database_path: impl AsRef<Path>,
        shared_world: SharedNativeWorld,
    ) -> Result<HostHandle, HostError> {
        config.validate()?;
        let listener = TcpListener::bind(config.bind_addr)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let active_connections = Arc::new(AtomicUsize::new(0));
        let database_path = database_path.as_ref().to_path_buf();
        let shared_map = config
            .world_map
            .as_deref()
            .map(|world_map| Arc::new(SharedNativeMap::new(world_map.clone())));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread = thread::spawn(move || {
            serve_native_otclient_game(
                listener,
                config,
                database_path,
                thread_shutdown,
                active_connections,
                shared_world,
                shared_map,
            )
        });
        Ok(HostHandle {
            local_addr,
            shutdown,
            online_players: Arc::new(AtomicU64::new(0)),
            operator_bridge_port: None,
            thread: Some(thread),
        })
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

    /// Reads the next session frame while discarding unsolicited server heartbeat pings so
    /// timing-sensitive socket regressions stay synchronized whenever a one-second idle window
    /// elapses between exchanges. Never use this where the ping record itself is expected.
    fn read_data_frame(stream: &mut TcpStream) -> Frame {
        loop {
            let frame = read_frame(stream).unwrap();
            if frame.0 == [forgotten_protocol::NATIVE_OTCLIENT_GAME_PING] {
                continue;
            }
            return frame;
        }
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
        let status = start_status(status_config(), &database, Arc::new(AtomicU64::new(0))).unwrap();
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
    fn answers_an_fe_metrics_status_request_with_authoritative_counters() {
        let database = database_path("status-metrics");
        {
            let database = EngineDatabase::open(&database).unwrap();
            database
                .create_account_with_password("operator", "correct horse battery staple")
                .unwrap();
            database
                .save_player(&Player {
                    id: 1,
                    account_id: 1,
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
        }
        let status = start_status(status_config(), &database, Arc::new(AtomicU64::new(0))).unwrap();
        let mut stream = TcpStream::connect(status.local_addr()).unwrap();
        write_frame(
            &mut stream,
            &Frame({
                let mut payload = vec![0xff, 0x0a, 0x00];
                payload.extend_from_slice(b"fe-metrics");
                payload
            }),
        )
        .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        eprintln!("metrics response: {response}");
        assert!(response.contains("\"registered_accounts\":1"));
        assert!(response.contains("\"registered_characters\":1"));
        assert!(response.contains("\"players_online_cap\":100"));
        assert!(response.contains("\"players_online\":"));
        assert!(response.contains("\"schema_version\":"));
        status.shutdown().unwrap();
        let _ = fs::remove_file(database);
    }

    #[test]
    fn answers_a_binary_status_request() {
        let database = database_path("status-binary");
        let status = start_status(status_config(), &database, Arc::new(AtomicU64::new(0))).unwrap();
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
        // This regression pins an exact sequential frame dialogue; the slice-4 default-on
        // wander would inject unsolicited viewport refreshes whenever the installed creature
        // steps, so freeze it here.
        native_config.static_creature_wander_policy =
            forgotten_core::StaticCreatureDecisionPolicy::Disabled;
        native_config.static_creature_wander_every_ticks = 0;
        let empty_world = native_config.empty_world.as_mut().unwrap();
        empty_world.outfit_first_look_type = 128;
        empty_world.outfit_last_look_type = 131;
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                name_description: String::new(),
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
                    text: Some("Read me".into()),
                    description: Some("An old inscription.".into()),
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
                    x: 102,
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
        Arc::get_mut(native_config.world_map.as_mut().unwrap())
            .unwrap()
            .set_tile_items(
                Position {
                    x: 110,
                    y: 111,
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
                        x: 108,
                        y: 108,
                        z: 7,
                    }),
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
                    x: 109,
                    y: 109,
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
                        x: 106,
                        y: 106,
                        z: 7,
                    }),
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
                    x: 106,
                    y: 105,
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
                        x: 105,
                        y: 105,
                        z: 7,
                    }),
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
                    x: 105,
                    y: 105,
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
                        x: 104,
                        y: 104,
                        z: 7,
                    }),
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
                    x: 104,
                    y: 103,
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
                        x: 103,
                        y: 103,
                        z: 7,
                    }),
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
                    x: 103,
                    y: 103,
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
                        x: 104,
                        y: 103,
                        z: 7,
                    }),
                    duration: None,
                    charges: None,
                    children: Vec::new(),
                }],
            )
            .unwrap();
        native_config.item_weight_by_server_id = Some(Arc::new(BTreeMap::from([(1988, 1_800)])));
        native_config.item_name_by_server_id =
            Some(Arc::new(BTreeMap::from([(1988, "Ham".to_string())])));
        native_config.stackable_item_server_ids = Some(Arc::new(BTreeSet::new()));
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
        assert_eq!(read_data_frame(&mut stream).0, expected_backpack);
        assert_eq!(
            read_data_frame(&mut stream).0,
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
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                100,
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
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_EDIT_TEXT,
                0,
                0,
                0,
                0,
                196,
                7,
                7,
                0,
                7,
                0,
                b'R',
                b'e',
                b'a',
                b'd',
                b' ',
                b'm',
                b'e',
                0,
                0,
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
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE,
                forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_LOOK,
                81,
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
                b' ',
                b'N',
                b'a',
                b'm',
                b'e',
                b':',
                b' ',
                b'H',
                b'a',
                b'm',
                b'.',
                b' ',
                b'I',
                b't',
                b' ',
                b'w',
                b'e',
                b'i',
                b'g',
                b'h',
                b's',
                b' ',
                b'1',
                b'8',
                b'.',
                b'0',
                b'0',
                b' ',
                b'o',
                b'z',
                b'.',
                b' ',
                b'A',
                b'n',
                b' ',
                b'o',
                b'l',
                b'd',
                b' ',
                b'i',
                b'n',
                b's',
                b'c',
                b'r',
                b'i',
                b'p',
                b't',
                b'i',
                b'o',
                b'n',
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
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TEXT_MESSAGE,
                forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_LOOK,
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
            read_data_frame(&mut stream).0,
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
            read_data_frame(&mut stream).0,
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
            read_data_frame(&mut stream).0,
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
        assert_eq!(read_data_frame(&mut stream).0, expected_backpack);

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
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_PLAYER_MODES,
                1,
                0,
                1,
            ]
        );
        let ping_back = read_data_frame(&mut stream);
        assert_eq!(ping_back.0, vec![0x1d]);

        let auto_walk_started = Instant::now();
        write_frame(&mut stream, &Frame(vec![0x64, 2, 1, 3])).unwrap();
        let auto_walk_east = read_data_frame(&mut stream);
        assert!(auto_walk_started.elapsed() >= Duration::from_millis(500));
        assert_eq!(&auto_walk_east.0[1..7], &[100, 0, 100, 0, 7, 1]);
        assert_eq!(&auto_walk_east.0[7..12], &[101, 0, 100, 0, 7]);
        let auto_walk_edge = read_data_frame(&mut stream);
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
        let latest_path_movement = read_data_frame(&mut stream);
        assert!(replacement_started.elapsed() >= Duration::from_millis(500));
        assert_eq!(&latest_path_movement.0[1..7], &[101, 0, 100, 0, 7, 1]);
        assert_eq!(&latest_path_movement.0[7..12], &[100, 0, 100, 0, 7]);
        let latest_path_edge = read_data_frame(&mut stream);
        assert_eq!(latest_path_edge.0[0], 0x68);
        write_frame(&mut stream, &Frame(vec![0x67])).unwrap();
        let manual_movement = read_data_frame(&mut stream);
        assert_eq!(&manual_movement.0[1..7], &[100, 0, 100, 0, 7, 1]);
        assert_eq!(&manual_movement.0[7..12], &[100, 0, 101, 0, 7]);
        let manual_edge = read_data_frame(&mut stream);
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
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_CREATURE_HEALTH,
                1,
                0,
                0,
                64,
                90,
            ]
        );
        let static_visibility_refresh = read_data_frame(&mut stream);
        assert_eq!(
            static_visibility_refresh.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(
            read_data_frame(&mut stream).0,
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
        let movement = read_data_frame(&mut stream);
        assert_eq!(
            movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        assert_eq!(&movement.0[1..7], &[100, 0, 101, 0, 7, 1]);
        assert_eq!(&movement.0[7..12], &[101, 0, 101, 0, 7]);
        let movement_edge = read_data_frame(&mut stream);
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
        let second_east = read_data_frame(&mut stream);
        assert_eq!(&second_east.0[1..7], &[101, 0, 101, 0, 7, 1]);
        assert_eq!(&second_east.0[7..12], &[102, 0, 101, 0, 7]);
        let second_east_edge = read_data_frame(&mut stream);
        assert_eq!(second_east_edge.0[0], 0x66);

        write_frame(&mut stream, &Frame(vec![0x66])).unwrap();
        let blocked_movement = read_data_frame(&mut stream);
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
        let diagonal_movement = read_data_frame(&mut stream);
        assert_eq!(
            diagonal_movement.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_MOVE_CREATURE
        );
        assert_eq!(&diagonal_movement.0[1..7], &[102, 0, 101, 0, 7, 1]);
        assert_eq!(&diagonal_movement.0[7..12], &[101, 0, 100, 0, 7]);
        let north_edge = read_data_frame(&mut stream);
        assert_eq!(north_edge.0[0], 0x65);
        let west_edge = read_data_frame(&mut stream);
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
        let blocked_diagonal = read_data_frame(&mut stream);
        assert_eq!(
            blocked_diagonal.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 3]
        );
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            diagonal_position
        );

        write_frame(&mut stream, &Frame(vec![0x96, 1, 2, 0, b'h', b'i'])).unwrap();
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![
                forgotten_protocol::NATIVE_OTCLIENT_GAME_TALK,
                6,
                0,
                b'K',
                b'n',
                b'i',
                b'g',
                b'h',
                b't',
                forgotten_protocol::NATIVE_OTCLIENT_MESSAGE_SAY,
                101,
                0,
                100,
                0,
                7,
                2,
                0,
                b'h',
                b'i',
            ]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT,
            ]),
        )
        .unwrap();
        let outfit_window = read_data_frame(&mut stream);
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
        let applied_outfit = read_data_frame(&mut stream);
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
        let rejected_outfit = read_data_frame(&mut stream);
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
        assert_eq!(
            read_data_frame(&mut stream).0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CLEAR_TARGET]
        );
        write_frame(&mut stream, &Frame(vec![0x69])).unwrap();
        let cancelled = read_data_frame(&mut stream);
        assert_eq!(
            cancelled.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 3]
        );
        write_frame(&mut stream, &Frame(vec![0x71])).unwrap();
        let turned = read_data_frame(&mut stream);
        assert_eq!(
            turned.0,
            vec![forgotten_protocol::NATIVE_OTCLIENT_GAME_CANCEL_WALK, 2]
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_USE_ITEM,
                102,
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
        let teleport_viewport = read_data_frame(&mut stream);
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

        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_SOUTH]),
        )
        .unwrap();
        let stepped_teleport_viewport = read_data_frame(&mut stream);
        assert_eq!(
            stepped_teleport_viewport.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(&stepped_teleport_viewport.0[1..6], &[108, 0, 108, 0, 7]);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            Position {
                x: 108,
                y: 108,
                z: 7,
            }
        );

        write_frame(
            &mut stream,
            &Frame(vec![
                forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_EAST,
            ]),
        )
        .unwrap();
        let diagonal_teleport_viewport = read_data_frame(&mut stream);
        assert_eq!(
            diagonal_teleport_viewport.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(&diagonal_teleport_viewport.0[1..6], &[106, 0, 106, 0, 7]);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            Position {
                x: 106,
                y: 106,
                z: 7,
            }
        );

        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_NORTH]),
        )
        .unwrap();
        let chain_teleport_viewport = read_data_frame(&mut stream);
        assert_eq!(
            chain_teleport_viewport.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(&chain_teleport_viewport.0[1..6], &[104, 0, 104, 0, 7]);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            Position {
                x: 104,
                y: 104,
                z: 7,
            }
        );

        write_frame(
            &mut stream,
            &Frame(vec![forgotten_protocol::NATIVE_OTCLIENT_CLIENT_WALK_NORTH]),
        )
        .unwrap();
        let cycle_teleport_viewport = read_data_frame(&mut stream);
        assert_eq!(
            cycle_teleport_viewport.0[0],
            forgotten_protocol::NATIVE_OTCLIENT_GAME_FULL_MAP
        );
        assert_eq!(&cycle_teleport_viewport.0[1..6], &[104, 0, 103, 0, 7]);
        assert_eq!(
            database.characters_for_account(account_id).unwrap()[0].position,
            Position {
                x: 104,
                y: 103,
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

        // Session teardown races the reconnect: the old session thread may still hold the
        // character registered when the new login arrives. Retry like the abrupt-disconnect
        // variant until the shared world releases the previous session.
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
        let (_second, second_initialization) =
            relog.expect("native disconnect cleanup did not release relog");
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
        native_config.static_creature_wander_policy =
            forgotten_core::StaticCreatureDecisionPolicy::Disabled;
        native_config.static_creature_wander_every_ticks = 0;
        native_config.static_spawns = Some(Arc::new(
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: NATIVE_OTCLIENT_PLAYER_ID_END + 1,
                name: "Rat".into(),
                name_description: String::new(),
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
            read_data_frame(&mut stream).0,
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
        let blocked = read_data_frame(&mut stream);
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
            name_description: String::new(),
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
            name_description: String::new(),
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
            name_description: String::new(),
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
                name_description: String::new(),
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
    fn heartbeat_declared_melee_attacks_skip_undeclared_creatures() {
        let declared_id = 0x4000_0001;
        let undeclared_id = 0x4000_0002;
        let entity = |id: u32, x: u16| forgotten_core::FeTfsStaticEntity {
            id,
            name: "Rat".into(),
            name_description: String::new(),
            position: Position { x, y: 100, z: 7 },
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
        let mut map = WorldMap::new(
            "declared-melee",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        for x in 99..=103 {
            for y in 99..=101 {
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
        // Declared creature adjacent east; undeclared creature adjacent west. Both see the
        // player at the spawn tile; only the declared one may attack under slice-5 policy.
        let static_spawns = FeTfsStaticSpawnCollection::with_combat_metadata(
            vec![entity(declared_id, 101), entity(undeclared_id, 99)],
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::from([(declared_id, 2_000_u32)]),
            std::collections::BTreeMap::from([(
                declared_id,
                forgotten_core::StaticCreatureDirectMeleeDamageRange {
                    min_damage: 2,
                    max_damage: 4,
                },
            )]),
        )
        .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(Some(&static_spawns)).unwrap();
        let map = Arc::new(map);
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
            .lock()
            .unwrap()
            .update_player_vitals(
                101,
                forgotten_core::PlayerVitals {
                    health: 50,
                    max_health: 50,
                    ..forgotten_core::PlayerVitals::default()
                },
            )
            .unwrap();
        shared
            .lock()
            .unwrap()
            .select_static_creature_target(declared_id, 1)
            .unwrap();

        let outcome = advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared,
            1,
            StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 1 },
            StaticTargetPursuitPolicy::Disabled,
            StaticTargetAttackPolicy::DeclaredMeleeCycling { max_range: 1 },
            forgotten_core::StaticCreatureDecisionPolicy::Disabled,
            0,
            Some(&map),
        )
        .unwrap();
        assert_eq!(outcome.static_target_attacks, 1);
        assert_eq!(
            outcome.static_target_attack_player_ids,
            BTreeSet::from([101])
        );
        assert_eq!(
            shared.lock().unwrap().player_vitals(101).unwrap().health,
            48
        );

        // The next due attack (after the configured 2-tick melee cooldown from the imported
        // 2000ms interval) cycles to the second declared value (3).
        advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared,
            2,
            StaticTargetAcquisitionPolicy::NearestLivingPlayer { max_range: 1 },
            StaticTargetPursuitPolicy::Disabled,
            StaticTargetAttackPolicy::DeclaredMeleeCycling { max_range: 1 },
            forgotten_core::StaticCreatureDecisionPolicy::Disabled,
            0,
            Some(&map),
        )
        .unwrap();
        assert_eq!(
            shared.lock().unwrap().player_vitals(101).unwrap().health,
            45
        );
    }

    #[test]
    fn heartbeat_wander_moves_active_creatures_on_configured_ticks_and_bumps_visibility() {
        let creature_id = 0x4000_0001;
        let mut map = WorldMap::new(
            "wander",
            Position {
                x: 100,
                y: 100,
                z: 7,
            },
        );
        for x in 99..=103 {
            map.set_tile(
                Position { x, y: 100, z: 7 },
                WorldMapTile {
                    ground_thing_id: 102,
                    walkable: true,
                },
            )
            .unwrap();
        }
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
        let visibility_before = shared.visibility_epoch();
        let position_before = shared
            .lock()
            .unwrap()
            .static_creature(creature_id)
            .unwrap()
            .position;

        // Non-due tick (tick 1 with every 4): no movement, no visibility bump.
        let outcome = advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared,
            1,
            StaticTargetAcquisitionPolicy::Disabled,
            StaticTargetPursuitPolicy::Disabled,
            StaticTargetAttackPolicy::Disabled,
            forgotten_core::StaticCreatureDecisionPolicy::ClockwiseAdjacent,
            4,
            Some(&map),
        )
        .unwrap();
        assert_eq!(outcome.wandered_static_creatures, 0);
        assert_eq!(shared.visibility_epoch(), visibility_before);
        assert_eq!(
            shared
                .lock()
                .unwrap()
                .static_creature(creature_id)
                .unwrap()
                .position,
            position_before
        );

        // Due tick (cumulative tick 4): exactly one occupancy-validated wander step and a
        // visibility bump so open sessions refresh their viewports.
        let outcome = advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared,
            3,
            StaticTargetAcquisitionPolicy::Disabled,
            StaticTargetPursuitPolicy::Disabled,
            StaticTargetAttackPolicy::Disabled,
            forgotten_core::StaticCreatureDecisionPolicy::ClockwiseAdjacent,
            4,
            Some(&map),
        )
        .unwrap();
        assert_eq!(outcome.wandered_static_creatures, 1);
        assert_eq!(shared.visibility_epoch(), visibility_before + 1);
        let position_after = shared
            .lock()
            .unwrap()
            .static_creature(creature_id)
            .unwrap()
            .position;
        assert_ne!(position_after, position_before);

        // Disabled policy keeps the world frozen even on due ticks.
        let visibility_now = shared.visibility_epoch();
        let outcome = advance_native_shared_world_heartbeat_with_static_target_policies(
            &shared,
            4,
            StaticTargetAcquisitionPolicy::Disabled,
            StaticTargetPursuitPolicy::Disabled,
            StaticTargetAttackPolicy::Disabled,
            forgotten_core::StaticCreatureDecisionPolicy::Disabled,
            4,
            Some(&map),
        )
        .unwrap();
        assert_eq!(outcome.wandered_static_creatures, 0);
        assert_eq!(shared.visibility_epoch(), visibility_now);
    }

    #[test]
    fn opt_in_shared_heartbeat_acquires_static_targets_without_visibility_or_behavior() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let static_spawns =
            FeTfsStaticSpawnCollection::new(vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
                followed_player_ids: BTreeSet::new(),
                wandered_static_creatures: 0,
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
                followed_player_ids: BTreeSet::new(),
                wandered_static_creatures: 0,
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

        assert_eq!(
            advance_native_shared_world_heartbeat_with_static_target_policies(
                &shared,
                1,
                StaticTargetAcquisitionPolicy::Disabled,
                StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range: 4 },
                StaticTargetAttackPolicy::Disabled,
                forgotten_core::StaticCreatureDecisionPolicy::Disabled,
                0,
                Some(map.as_ref()),
            )
            .unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 3,
                reactivated_static_creatures: 0,
                changed_static_targets: 0,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
                followed_player_ids: BTreeSet::new(),
                wandered_static_creatures: 0,
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
    }

    #[test]
    fn opt_in_shared_heartbeat_applies_static_target_damage_only_when_enabled() {
        let map = native_world_map();
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let creature = forgotten_core::FeTfsStaticEntity {
            id: creature_id,
            name: "Rat".into(),
            name_description: String::new(),
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
                StaticTargetPursuitPolicy::Disabled,
                StaticTargetAttackPolicy::Disabled,
                forgotten_core::StaticCreatureDecisionPolicy::Disabled,
                0,
                Some(&map),
            )
            .unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 1,
                reactivated_static_creatures: 0,
                changed_static_targets: 1,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
                followed_player_ids: BTreeSet::new(),
                wandered_static_creatures: 0,
            }
        );
        assert_eq!(shared.player_vitals(101).unwrap().health, 5);
        assert_eq!(shared.vitals_epoch(), 0);

        assert_eq!(
            advance_native_shared_world_heartbeat_with_static_target_policies(
                &shared,
                1,
                StaticTargetAcquisitionPolicy::Disabled,
                StaticTargetPursuitPolicy::Disabled,
                StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage: 2 },
                forgotten_core::StaticCreatureDecisionPolicy::Disabled,
                0,
                Some(&map),
            )
            .unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 2,
                reactivated_static_creatures: 0,
                changed_static_targets: 0,
                static_target_attacks: 1,
                static_target_attack_player_ids: BTreeSet::from([101]),
                followed_player_ids: BTreeSet::new(),
                wandered_static_creatures: 0,
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
    fn native_shared_heartbeat_steps_a_current_player_follow_intent_once() {
        let map = native_world_map();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        for (id, name, position) in [
            (101_u64, "Knight", map.spawn()),
            (
                102_u64,
                "Druid",
                Position {
                    x: 103,
                    y: 100,
                    z: 7,
                },
            ),
        ] {
            shared
                .register_player_at_available_position(
                    Player {
                        id,
                        account_id: id,
                        name: name.into(),
                        position,
                        level: 8,
                        experience: 0,
                        skill_points: 0,
                    },
                    &map,
                )
                .unwrap();
        }
        shared.set_player_follow(101, Some(102)).unwrap();
        let visibility_epoch = shared.visibility_epoch();

        assert_eq!(
            advance_native_shared_world_heartbeat_with_static_target_policies(
                &shared,
                1,
                StaticTargetAcquisitionPolicy::Disabled,
                StaticTargetPursuitPolicy::Disabled,
                StaticTargetAttackPolicy::Disabled,
                forgotten_core::StaticCreatureDecisionPolicy::Disabled,
                0,
                Some(&map),
            )
            .unwrap(),
            NativeWorldHeartbeatOutcome {
                tick: 1,
                reactivated_static_creatures: 0,
                changed_static_targets: 0,
                static_target_attacks: 0,
                static_target_attack_player_ids: BTreeSet::new(),
                followed_player_ids: BTreeSet::from([101]),
                wandered_static_creatures: 0,
            }
        );
        assert_eq!(shared.visibility_epoch(), visibility_epoch + 1);
        assert_eq!(
            shared.player_and_vitals(101).unwrap().0.position,
            Position {
                x: 101,
                y: 100,
                z: 7,
            }
        );
    }

    #[test]
    fn native_shared_heartbeat_reactivates_static_creatures_only_after_interval() {
        let creature_id = NATIVE_OTCLIENT_PLAYER_ID_END + 1;
        let collection = FeTfsStaticSpawnCollection::with_respawn_intervals(
            vec![forgotten_core::FeTfsStaticEntity {
                id: creature_id,
                name: "Rat".into(),
                name_description: String::new(),
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
                followed_player_ids: BTreeSet::new(),
                wandered_static_creatures: 0,
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
                followed_player_ids: BTreeSet::new(),
                wandered_static_creatures: 0,
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
            name_description: String::new(),
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
        let talk_summary = native_action_diagnostic_summary(&NativeOtClientGameAction::Talk(
            forgotten_protocol::NativeOtClientTalkRequest {
                mode: 1,
                channel_id: None,
                recipient: None,
                message: secret_message,
            },
        ));
        assert_eq!(
            talk_summary,
            "action=talk mode=1 channel-id=0 text-bytes=28"
        );
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
/// Gamemaster talkaction and dynamic-spawn coverage: command recognition, GM promotion
/// persistence, spawn authorization against imported templates, and the operator bridge.
#[cfg(test)]
mod gm_talkaction_tests {
    use super::*;
    use forgotten_core::WorldMapTile;
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn database_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forgotten-engine-gm-{name}-{nonce}.db"))
    }

    #[test]
    fn non_slash_messages_are_not_gm_commands() {
        let path = database_path("non-slash");
        let mut database = EngineDatabase::open(&path).unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let reply =
            handle_native_gm_talkaction(&shared, &mut database, 1, "hello everyone", 2, None)
                .unwrap();
        assert!(reply.is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn unknown_gm_verbs_get_an_available_command_list() {
        let path = database_path("unknown-verb");
        let mut database = EngineDatabase::open(&path).unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let reply =
            handle_native_gm_talkaction(&shared, &mut database, 1, "/frobnicate", 1, None).unwrap();
        assert!(reply.unwrap().contains("Unknown GM command"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn gm_promotion_persists_and_reports_scope_errors() {
        let path = database_path("gm-promote");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id: i64 = database.create_account("gm-owner", "password").unwrap();
        let character = database
            .create_player_for_account(account_id as u32, "Target")
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();

        let missing_scope =
            handle_native_gm_talkaction(&shared, &mut database, 1, "/gm Target", 1, None)
                .unwrap()
                .unwrap();
        assert!(missing_scope.contains("Usage"));

        let offline_scope =
            handle_native_gm_talkaction(&shared, &mut database, 1, "/gm online Target", 1, None)
                .unwrap()
                .unwrap();
        assert!(offline_scope.contains("not online"));

        let promoted =
            handle_native_gm_talkaction(&shared, &mut database, 1, "/gm offline Target 2", 1, None)
                .unwrap()
                .unwrap();
        assert!(promoted.contains("level 2"));
        assert_eq!(
            database.player_gm_level(u64::from(character.id)).unwrap(),
            2
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn gm_ban_unban_and_mute_round_trip_through_talkactions() {
        let path = database_path("gm-ban-mute");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id: i64 = database.create_account("troublemaker", "password").unwrap();
        database
            .create_player_for_account(account_id as u32, "Target")
            .unwrap();
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();

        // Permanent ban with a reason, then the login gate sees it.
        let reply = handle_native_gm_talkaction(
            &shared,
            &mut database,
            1,
            "/ban Target Botting in depot",
            1,
            None,
        )
        .unwrap()
        .unwrap();
        assert!(reply.contains("Permanently banned"));
        let ban_reason = database
            .active_account_ban(account_id as u64)
            .unwrap()
            .expect("ban recorded");
        assert!(ban_reason.contains("Botting"));

        // Unban lifts it.
        let lifted =
            handle_native_gm_talkaction(&shared, &mut database, 1, "/unban Target", 1, None)
                .unwrap()
                .unwrap();
        assert!(lifted.contains("1 ban(s)"));
        assert!(database
            .active_account_ban(account_id as u64)
            .unwrap()
            .is_none());

        // Mute records remaining seconds; unmute clears and reports the not-muted case.
        let muted = handle_native_gm_talkaction(&shared, &mut database, 1, "/mute Target", 1, None)
            .unwrap()
            .unwrap();
        assert!(muted.contains("Muted Target"));
        assert!(database
            .account_mute_remaining_seconds(account_id as u64)
            .unwrap()
            .is_some());
        let unmuted =
            handle_native_gm_talkaction(&shared, &mut database, 1, "/unmute Target", 1, None)
                .unwrap()
                .unwrap();
        assert!(unmuted.contains("Unmuted"));
        let again =
            handle_native_gm_talkaction(&shared, &mut database, 1, "/unmute Target", 1, None)
                .unwrap()
                .unwrap();
        assert!(again.contains("was not muted"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn spawn_rejects_unknown_entities_and_summons_known_ones_in_front() {
        let path = database_path("spawn");
        let mut database = EngineDatabase::open(&path).unwrap();
        let account_id: i64 = database.create_account("spawner", "password").unwrap();
        let character = database
            .create_player_for_account(account_id as u32, "Summoner")
            .unwrap();
        let player_id = u64::from(character.id);
        let monster_id = 0x4000_0001_u32;
        let collection = FeTfsStaticSpawnCollection::with_combat_metadata_and_npc_ids(
            vec![forgotten_core::FeTfsStaticEntity {
                id: monster_id,
                name: "Rat".into(),
                name_description: String::new(),
                position: Position { x: 90, y: 90, z: 7 },
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
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::new(),
        )
        .unwrap();
        let spawn = Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = WorldMap::new("gm-spawn-test", spawn);
        for x in 80..=120u16 {
            for y in 80..=120u16 {
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
            name: "Gm Temple".into(),
            temple_position: spawn,
        })
        .unwrap();
        map.validate().unwrap();
        let map = std::sync::Arc::new(map);
        let shared = SharedNativeWorld::from_static_spawns(Some(&collection)).unwrap();
        shared
            .register_player_at_available_position(
                Player {
                    id: player_id,
                    account_id: account_id as u64,
                    name: "Summoner".into(),
                    position: map.spawn(),
                    level: 8,
                    experience: 4_900,
                    skill_points: 3,
                },
                &map,
            )
            .unwrap();
        shared
            .update_player_facing(player_id, NativeOtClientCardinalDirection::South)
            .unwrap();

        let unknown = handle_native_gm_talkaction(
            &shared,
            &mut database,
            player_id,
            "/spawn Dragon",
            1,
            None,
        )
        .unwrap()
        .unwrap();
        assert!(unknown.contains("Unknown entity"));

        let summoned =
            handle_native_gm_talkaction(&shared, &mut database, player_id, "/spawn Rat", 1, None)
                .unwrap()
                .unwrap();
        assert!(summoned.contains("Summoned"));
        let records = {
            let world = shared.lock().unwrap();
            world.dynamic_spawn_records()
        };
        assert_eq!(records.len(), 1);
        assert!(records[0].0 >= 0x7000_0000);
        // The summon lands one tile south of the spawn facing.
        assert_eq!(records[0].2.y, map.spawn().y + 1);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn operator_bridge_answers_status_over_tcp() {
        let path = database_path("bridge-tcp");
        let shared = SharedNativeWorld::from_static_spawns(None).unwrap();
        let (port, shutdown) = operator::start_operator_bridge(operator::OperatorBridgeConfig {
            shared_world: shared,
            database_path: path.clone(),
        })
        .unwrap();
        let mut stream =
            std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writeln!(stream, r#"{{"op":"status"}}"#).unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains("\"ok\":true"));
        assert!(line.contains("players-online=0"));
        shutdown.store(true, Ordering::SeqCst);
        let _ = fs::remove_file(&path);
    }
}

/// Dynamic per-player speed coverage: boots-slot speed bonuses stack onto the configured base,
/// non-boot items never contribute, and unknown item IDs leave the base unchanged.
#[cfg(test)]
mod effective_speed_tests {
    use super::*;

    #[test]
    fn hasted_speed_applies_bounded_percent_and_keeps_ceiling() {
        // Zero bonus is the identity.
        assert_eq!(native_hasted_speed(220, 0), 220);
        // Additive percent: 40% haste on 220 -> 308.
        assert_eq!(native_hasted_speed(220, 40), 308);
        // The ceiling holds even at the maximum bonus.
        assert_eq!(native_hasted_speed(2_000, 100), 2_000);
        // Small bases round down deterministically (integer arithmetic).
        assert_eq!(native_hasted_speed(3, 40), 4);
    }

    #[test]
    fn boots_speed_bonus_stacks_onto_base_and_unknown_items_do_not() {
        let base = 220_u16;
        let mut equipment = PlayerEquipment::default();
        let empty_map: Option<&BTreeMap<u16, u16>> = None;
        assert_eq!(
            native_effective_player_speed(base, &equipment, empty_map),
            base
        );

        // Boots of haste style bonus on the feet slot.
        equipment.equip(EquipmentSlot::Feet, ItemInstance::new(2195, 1).unwrap());
        let bonuses = BTreeMap::from([(2195_u16, 40_u16)]);
        assert_eq!(
            native_effective_player_speed(base, &equipment, Some(&bonuses)),
            260
        );

        // A sword in the right hand with a speed attribute must not contribute.
        let mut wrong_slot = PlayerEquipment::default();
        wrong_slot.equip(
            EquipmentSlot::RightHand,
            ItemInstance::new(2195, 1).unwrap(),
        );
        assert_eq!(
            native_effective_player_speed(base, &wrong_slot, Some(&bonuses)),
            base
        );

        // Unknown server ids contribute nothing.
        let other_bonuses = BTreeMap::from([(9999_u16, 100_u16)]);
        assert_eq!(
            native_effective_player_speed(base, &equipment, Some(&other_bonuses)),
            base
        );
    }
}
