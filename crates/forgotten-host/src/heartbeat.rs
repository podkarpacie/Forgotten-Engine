//! Shared-world heartbeat runtime and static-target policy: connected-but-bounded
//! static-creature activity, acquisition/pursuit/attack policies, and the tick loop
//! that advances the shared world and reactivates due creatures.

use super::*;

pub(crate) const MAX_EMPTY_WORLD_MOVES_PER_SESSION: usize = 64;
pub(crate) const NATIVE_OTCLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const NATIVE_OTCLIENT_HEARTBEAT_POLL_INTERVAL: Duration = Duration::from_millis(25);
pub(crate) const MAX_NATIVE_OTCLIENT_LOGIN_VIP_ENTRIES: usize = 128;
pub(crate) const MAX_NATIVE_STATIC_NPC_DIALOGUE_RANGE: u16 = 2;
pub(crate) const NATIVE_OTCLIENT_DEFAULT_GROUND_SPEED: u64 = 150;
pub(crate) const NATIVE_OTCLIENT_AUTOWALK_MAX_DELAY: Duration = Duration::from_secs(2);
pub(crate) const DEFAULT_NATIVE_ARMOR_MULTIPLIER_MILLI: u32 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeWorldHeartbeatOutcome {
    pub(crate) tick: u64,
    pub(crate) reactivated_static_creatures: usize,
    pub(crate) changed_static_targets: usize,
    pub(crate) static_target_attacks: usize,
    pub(crate) static_target_attack_player_ids: BTreeSet<u64>,
    pub(crate) followed_player_ids: BTreeSet<u64>,
    pub(crate) wandered_static_creatures: usize,
}

pub(crate) struct NativeHeartbeatConfig {
    pub(crate) pursuit_policy: StaticTargetPursuitPolicy,
    pub(crate) attack_policy: StaticTargetAttackPolicy,
    /// Opt-out deterministic wander movement for active static creatures (plan v49 slice 4).
    pub(crate) wander_policy: StaticCreatureDecisionPolicy,
    /// Wander cadence in world ticks; `0` disables wandering entirely.
    pub(crate) wander_every_ticks: u64,
    pub(crate) map_owner: Option<Arc<SharedNativeMap>>,
    pub(crate) database_path: PathBuf,
    pub(crate) death_loss_policy: DeathLossPolicy,
    pub(crate) progression_rules: Option<Arc<BTreeMap<VocationId, PlayerProgressionRules>>>,
    /// Configured corpse despawn delay in world-tick seconds; `0` disables decay entirely.
    pub(crate) corpse_despawn_seconds: u32,
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
pub(crate) fn static_target_acquisition_policy(
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
pub(crate) fn advance_native_shared_world_heartbeat(
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
pub(crate) fn advance_native_shared_world_heartbeat_with_target_policy(
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
pub(crate) fn advance_native_shared_world_heartbeat_with_static_target_policies(
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

pub(crate) const NATIVE_AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

pub(crate) fn run_native_shared_world_heartbeat(
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
