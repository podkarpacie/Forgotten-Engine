use forgotten_config::{
    apply_legacy_item_metadata, ensure_content_skeleton, load, load_declarative_spell_catalog,
    load_declarative_weapon_catalog, load_legacy_item_catalog, load_tfs_content_inventory,
    load_tfs_entity_catalog, load_tfs_vocation_registry, load_world_companions, load_world_map,
    materialize_tfs_static_spawns, resolve_tfs_registry_script_reference,
    resolve_tfs_spawn_references, validate_content, world_map_path, write_template,
    DeclarativeSpellCatalog, DeclarativeWeaponCatalog, EngineConfig, LegacyWorldCompanionData,
    TfsEntityCatalog, TfsRegistryCategory, TfsVocationRegistry,
};
use forgotten_core::{
    DeathLossPolicy, EquipmentSlot, ItemInstance, Player, PlayerContainer, PlayerRegenerationRules,
    PlayerRespawnState, PlayerSkill, PlayerVitals, RegenerationRule, SkillProgress, VocationId,
    WorldMap, WorldMapSource, WorldState,
};
use forgotten_host::{
    start, start_game_session, start_native_otclient_game, start_native_otclient_login,
    start_status, GameSessionHostConfig, HostConfig, LegacyLoginConfig,
    NativeOtClientEmptyWorldConfig, NativeOtClientHostConfig, StaticTargetAttackPolicy,
    StaticTargetPursuitPolicy, StatusHostConfig,
};
use forgotten_persistence::{create_backup, EngineDatabase};
use forgotten_protocol::{
    profile_by_id, CompatibilityProfile, LegacyRsaPrivateKey, OtClientEndpoint,
    COMPATIBILITY_PROFILES,
};
use forgotten_scripting::{
    DeferredScriptEvent, DeferredScriptEventKind, NoopDeferredScriptExecutor,
    SandboxedLuaCallbackDispatcher, SandboxedLuaCallbackInput, ScriptEventDispatcher,
    MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES,
};
use std::env;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(test)]
use forgotten_config::template;

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    match command {
        "init" => init(
            required_path(&arguments, 1)?,
            selected_profile(&arguments, 2)?,
        ),
        "validate" => validate(required_path(&arguments, 1)?),
        "tfs-audit" => audit_tfs_conversion(required_path(&arguments, 1)?),
        "run" => {
            let (directory, extended_diagnostics) = run_options(&arguments)?;
            run_host(directory, extended_diagnostics)
        }
        "status" => status(required_path(&arguments, 1)?),
        "generate-key" => generate_key(required_path(&arguments, 1)?),
        "backup" => backup(required_path(&arguments, 1)?),
        "command" => command_line(&arguments),
        "script" => script_command(&arguments),
        "account" => account_command(&arguments),
        "player" => player_command(&arguments),
        "compatibility" => compatibility(&arguments),
        "version" | "--version" | "-V" => version(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(format!("unknown command `{unknown}`; run `forgotten-engine help`").into()),
    }
}

#[derive(Debug)]
struct IndependentNativeStartupContent {
    companions: LegacyWorldCompanionData,
    entity_catalog: TfsEntityCatalog,
    vocation_registry: Option<TfsVocationRegistry>,
    declarative_weapon_catalog: Option<DeclarativeWeaponCatalog>,
    declarative_spell_catalog: Option<DeclarativeSpellCatalog>,
}

/// Loads only configuration-independent native content in scoped worker threads. Results are
/// joined in a fixed historical validation order, so startup remains reproducible even when I/O
/// completes in a different order. Map/item normalization and all authoritative world mutation
/// deliberately remain serialized outside this helper.
fn load_independent_native_startup_content(
    config: &EngineConfig,
    world_map: &WorldMap,
) -> Result<IndependentNativeStartupContent, Box<dyn std::error::Error>> {
    thread::scope(
        |scope| -> Result<IndependentNativeStartupContent, Box<dyn std::error::Error>> {
            let companions = scope.spawn(|| load_world_companions(config, world_map));
            let entity_catalog = scope.spawn(|| load_tfs_entity_catalog(config));
            let vocation_registry = scope.spawn(|| load_tfs_vocation_registry(config));
            let declarative_weapon_catalog =
                scope.spawn(|| load_declarative_weapon_catalog(config));
            let declarative_spell_catalog = scope.spawn(|| load_declarative_spell_catalog(config));

            let companions = companions
                .join()
                .map_err(|_| "world companion loader worker panicked")??;
            let entity_catalog = entity_catalog
                .join()
                .map_err(|_| "entity catalog loader worker panicked")??;
            let vocation_registry = vocation_registry
                .join()
                .map_err(|_| "vocation registry loader worker panicked")??;
            let declarative_weapon_catalog = declarative_weapon_catalog
                .join()
                .map_err(|_| "weapon catalog loader worker panicked")??;
            let declarative_spell_catalog = declarative_spell_catalog
                .join()
                .map_err(|_| "spell catalog loader worker panicked")??;
            Ok(IndependentNativeStartupContent {
                companions,
                entity_catalog,
                vocation_registry,
                declarative_weapon_catalog,
                declarative_spell_catalog,
            })
        },
    )
}

fn required_path(
    arguments: &[String],
    index: usize,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .map(PathBuf::from)
        .ok_or_else(|| "a Forgotten Engine world directory is required".into())
}

fn run_options(arguments: &[String]) -> Result<(PathBuf, bool), Box<dyn std::error::Error>> {
    let directory = required_path(arguments, 1)?;
    let mut extended_diagnostics = false;
    for flag in &arguments[2..] {
        match flag.as_str() {
            "--ed" | "--extended-debug" => extended_diagnostics = true,
            _ => {
                return Err(format!(
                    "unknown run option `{flag}`; use --ed for bounded extended diagnostics"
                )
                .into())
            }
        }
    }
    Ok((directory, extended_diagnostics))
}

fn selected_profile(
    arguments: &[String],
    index: usize,
) -> Result<CompatibilityProfile, Box<dyn std::error::Error>> {
    let selector = match arguments.get(index).map(String::as_str) {
        None => "fe-1.2",
        Some("--profile") => arguments
            .get(index + 1)
            .map(String::as_str)
            .ok_or("a compatibility profile is required after --profile")?,
        Some(value) => value,
    };
    profile_by_id(selector).ok_or_else(|| {
        format!("unknown compatibility profile `{selector}`; use fe-7.4, fe-8.0, or fe-1.2").into()
    })
}

fn init(
    directory: PathBuf,
    profile: CompatibilityProfile,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&directory)?;
    write_template(&directory, profile)?;
    ensure_content_skeleton(&directory)?;
    let config = load(&directory)?;
    let database = EngineDatabase::open(&config.database_path)?;
    database.record_event("info", "Forgotten Engine world initialized")?;
    println!(
        "Forgotten Engine world initialized\n> config.lua profile={} protocol={}\n> content={}\n> database={} schema={}",
        config.profile.id,
        config.profile.tibia_protocol,
        config.content_directory.display(),
        database.path().display(),
        database.schema_version()?
    );
    Ok(())
}

fn validate(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!(">> Loading config");
    let config = load(&directory)?;
    println!(">> Reconciling original content skeleton");
    ensure_content_skeleton(&directory)?;
    println!(">> Validating data content");
    let content = validate_content(&directory)?;
    let raw_world_map = load_world_map(&config)?;
    let item_catalog = load_legacy_item_catalog(&config, &raw_world_map)?;
    let world_map = match &item_catalog {
        Some(catalog) => apply_legacy_item_metadata(&raw_world_map, catalog)?,
        None => raw_world_map,
    };
    let companions = load_world_companions(&config, &world_map)?;
    let vocations = load_tfs_vocation_registry(&config)?;
    println!(">> Opening database");
    let database = EngineDatabase::open(&config.database_path)?;
    if database.schema_version()? < 1 {
        return Err("database schema is not migrated".into());
    }
    println!(
        "> Validation complete: profile={} protocol={} game-port={} status-port={} map={} tiles={} spawn={},{},{} items={} spawns={} houses={} vocations={} data={} database={}",
        config.profile.id,
        config.profile.tibia_protocol,
        config.game_protocol_port,
        config.status_protocol_port,
        config.map_name,
        world_map.tile_count(),
        world_map.spawn().x,
        world_map.spawn().y,
        world_map.spawn().z,
        item_catalog.as_ref().map_or(0, |catalog| catalog.len()),
        companions.spawns.len(),
        companions.houses.len(),
        vocations.as_ref().map_or(0, |registry| registry.len()),
        content.data_directory.display(),
        database.path().display()
    );
    Ok(())
}

fn deferred_script_event_kind(category: TfsRegistryCategory) -> DeferredScriptEventKind {
    match category {
        TfsRegistryCategory::Actions => DeferredScriptEventKind::Action,
        TfsRegistryCategory::CreatureScripts => DeferredScriptEventKind::CreatureScript,
        TfsRegistryCategory::Events => DeferredScriptEventKind::Event,
        TfsRegistryCategory::GlobalEvents => DeferredScriptEventKind::GlobalEvent,
        TfsRegistryCategory::Movements => DeferredScriptEventKind::Movement,
        TfsRegistryCategory::Spells => DeferredScriptEventKind::Spell,
        TfsRegistryCategory::TalkActions => DeferredScriptEventKind::TalkAction,
        TfsRegistryCategory::Weapons => DeferredScriptEventKind::Weapon,
        TfsRegistryCategory::Monsters => DeferredScriptEventKind::Monster,
        TfsRegistryCategory::Npcs => DeferredScriptEventKind::Npc,
    }
}

fn audit_tfs_conversion(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!(">> Loading TFS-style configuration without executing Lua");
    let config = load(&directory)?;
    let map_path = world_map_path(&config)?;
    println!(">> Inspecting selected world data");
    let raw_world_map = load_world_map(&config)?;
    let item_catalog = load_legacy_item_catalog(&config, &raw_world_map)?;
    let world_map = match &item_catalog {
        Some(catalog) => apply_legacy_item_metadata(&raw_world_map, catalog)?,
        None => raw_world_map.clone(),
    };
    let companions = load_world_companions(&config, &world_map)?;
    let registry_inventory = load_tfs_content_inventory(&config)?;
    let entity_catalog = load_tfs_entity_catalog(&config)?;
    let vocation_registry = load_tfs_vocation_registry(&config)?;
    let spawn_resolution = resolve_tfs_spawn_references(&companions, &entity_catalog);
    let map_kind = match raw_world_map.source() {
        WorldMapSource::Otbm(_) => "OTBM",
        WorldMapSource::FeMapV1 => "FE-native",
    };

    println!(
        "TFS conversion readiness\n> config={} (FE profile={} protocol={})\n> map={} format={} tiles={} spawn={},{},{}\n> item-mappings={} spawns={} houses={} towns={} waypoints={} vocations={}\n> registries={} entries={} references={} missing-references={} unsafe-references={}\n> entities={} monsters={} npcs={} missing-definitions={} missing-scripts={} unsafe-entity-references={}\n> spawn-creatures={} resolved-spawn-creatures={} unresolved-monsters={} unresolved-npcs={}",
        directory.join("config.lua").display(),
        config.profile.id,
        config.profile.tibia_protocol,
        map_path.display(),
        map_kind,
        world_map.tile_count(),
        world_map.spawn().x,
        world_map.spawn().y,
        world_map.spawn().z,
        item_catalog.as_ref().map_or(0, |catalog| catalog.len()),
        companions.spawns.len(),
        companions.houses.len(),
        world_map.towns().count(),
        world_map.waypoints().count(),
        vocation_registry.as_ref().map_or(0, |registry| registry.len()),
        registry_inventory.present_registry_count(),
        registry_inventory.entry_count(),
        registry_inventory.reference_count(),
        registry_inventory.missing_reference_count(),
        registry_inventory.unsafe_reference_count(),
        entity_catalog.entity_count(),
        entity_catalog.monsters.len(),
        entity_catalog.npcs.len(),
        entity_catalog.missing_definitions.len(),
        entity_catalog.missing_scripts.len(),
        entity_catalog.unsafe_references.len(),
        spawn_resolution.spawn_creature_count,
        spawn_resolution.resolved_creature_count,
        spawn_resolution.unresolved_monsters.len(),
        spawn_resolution.unresolved_npcs.len(),
    );
    if matches!(raw_world_map.source(), WorldMapSource::Otbm(_)) {
        println!("> OTBM world data is importable by the current FE map pipeline.");
    } else {
        println!("> FE-native map selected; use mapFormat = \"otbm\" or auto with an .otbm file to audit legacy map data.");
    }
    for registry in registry_inventory
        .registries
        .iter()
        .filter(|registry| registry.present)
    {
        let dispatch = NoopDeferredScriptExecutor.dispatch(DeferredScriptEvent {
            kind: deferred_script_event_kind(registry.category),
            reference_count: registry.reference_count,
            missing_reference_count: registry.missing_references.len(),
            unsafe_reference_count: registry.unsafe_references.len(),
        });
        println!(
            "> registry={} entries={} references={} missing={} unsafe={} status={} dispatch={} execution={}",
            registry.category.label(),
            registry.entry_count,
            registry.reference_count,
            registry.missing_references.len(),
            registry.unsafe_references.len(),
            registry.category.runtime_status(),
            dispatch.event.kind.label(),
            dispatch.state.as_str(),
        );
        if !registry.missing_references.is_empty() {
            let paths = registry
                .missing_references
                .iter()
                .take(3)
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            println!(">   missing references (up to 3): {paths}");
        }
        if !registry.unsafe_references.is_empty() {
            println!(
                ">   unsafe relative references (up to 3): {}",
                registry
                    .unsafe_references
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if registry_inventory.present_registry_count() == 0 {
        println!("> No standard TFS XML registries were found beneath data/.");
    } else {
        println!(
            "> Registry entries were parsed for conversion inventory only. Referenced Lua scripts and creature definitions remain local and are not executed by this FE milestone."
        );
    }
    if !entity_catalog.missing_definitions.is_empty() {
        println!(
            "> missing entity definitions (up to 3): {}",
            entity_catalog
                .missing_definitions
                .iter()
                .take(3)
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !entity_catalog.missing_scripts.is_empty() {
        println!(
            "> missing NPC script references (up to 3): {}",
            entity_catalog
                .missing_scripts
                .iter()
                .take(3)
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !entity_catalog.unsafe_references.is_empty() {
        println!(
            "> unsafe entity references (up to 3): {}",
            entity_catalog
                .unsafe_references
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !spawn_resolution.unresolved_monsters.is_empty() {
        println!(
            "> unresolved spawned monsters (up to 3): {}",
            spawn_resolution
                .unresolved_monsters
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !spawn_resolution.unresolved_npcs.is_empty() {
        println!(
            "> unresolved spawned NPCs (up to 3): {}",
            spawn_resolution
                .unresolved_npcs
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if config.otclient_v8_native_enabled {
        println!("> Native OTCv8 is enabled through the explicitly configured profile.");
    } else {
        println!("> Native OTCv8 is disabled. Configure it explicitly only after choosing a matching lawful client asset set.");
    }
    Ok(())
}

fn run_host(
    directory: PathBuf,
    extended_diagnostics: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Forgotten Engine - {}", env!("CARGO_PKG_VERSION"));
    validate(directory.clone())?;
    let config = load(&directory)?;
    let raw_world_map = load_world_map(&config)?;
    let item_catalog = load_legacy_item_catalog(&config, &raw_world_map)?;
    let item_presentation_catalog = item_catalog
        .as_ref()
        .map(|catalog| catalog.native_item_presentation_catalog())
        .transpose()?;
    let item_armor_by_server_id = item_catalog
        .as_ref()
        .map(|catalog| catalog.native_xml_armor_by_server_id());
    let item_weight_by_server_id = item_catalog
        .as_ref()
        .map(|catalog| catalog.xml_weight_by_server_id());
    let stackable_item_server_ids = item_catalog
        .as_ref()
        .map(|catalog| catalog.stackable_server_ids());
    let world_map = Arc::new(match &item_catalog {
        Some(catalog) => apply_legacy_item_metadata(&raw_world_map, catalog)?,
        None => raw_world_map,
    });
    let database = EngineDatabase::open(&config.database_path)?;
    database.record_event("info", "Forgotten Engine host startup requested")?;

    println!(">> Registering services");
    if extended_diagnostics {
        println!(
            "> Extended diagnostics enabled: native session metadata is logged; credentials and packet bodies are excluded."
        );
    }
    let rsa_private_key = if config.legacy_login_enabled || config.game_session_enabled {
        if config.profile.id != "fe-7.4" {
            return Err(
                "legacy login and game-session foundations are currently available only for the fe-7.4 profile".into(),
            );
        }
        println!(">> Loading fe-7.4 foundation private key");
        Some(Arc::new(LegacyRsaPrivateKey::load_pem(
            &config.rsa_private_key_path,
        )?))
    } else {
        None
    };
    let legacy_login = match (config.legacy_login_enabled, &rsa_private_key) {
        (true, Some(rsa_private_key)) => Some(LegacyLoginConfig {
            rsa_private_key: Arc::clone(rsa_private_key),
            server_name: config.server_name.clone(),
            message_of_the_day: format!("Welcome to {}", config.server_name),
        }),
        _ => None,
    };
    let host = start(
        HostConfig {
            bind_addr: config.game_socket_addr(),
            profile: config.profile,
            max_connections: config.max_connections(),
            session_timeout: Duration::from_secs(5),
            legacy_login,
        },
        &config.database_path,
    )?;
    let status = match start_status(
        StatusHostConfig {
            bind_addr: config.status_socket_addr(),
            profile: config.profile,
            server_name: config.server_name.clone(),
            map_name: config.map_name.clone(),
            max_players: config.max_players,
            max_connections: config.max_connections(),
            session_timeout: Duration::from_secs(5),
        },
        &config.database_path,
    ) {
        Ok(status) => status,
        Err(error) => {
            host.shutdown()?;
            return Err(Box::new(error));
        }
    };
    let game_session = if config.game_session_enabled {
        let Some(rsa_private_key) = &rsa_private_key else {
            status.shutdown()?;
            host.shutdown()?;
            return Err("gameSessionEnabled requires an FE legacy private key".into());
        };
        match start_game_session(
            GameSessionHostConfig {
                bind_addr: config.game_session_socket_addr(),
                profile: config.profile,
                rsa_private_key: Arc::clone(rsa_private_key),
                advertised_endpoint: OtClientEndpoint {
                    host: config.advertised_game_session_host.clone(),
                    port: config.advertised_game_session_port,
                },
                max_connections: config.max_connections(),
                session_timeout: Duration::from_secs(5),
            },
            &config.database_path,
        ) {
            Ok(session) => Some(session),
            Err(error) => {
                status.shutdown()?;
                host.shutdown()?;
                return Err(Box::new(error));
            }
        }
    } else {
        None
    };
    let native_config = if config.otclient_v8_native_enabled {
        let advertised_ip: IpAddr = config.advertised_otclient_v8_host.parse().map_err(|_| {
            "advertisedOtClientV8Host must be an IPv4 or IPv6 address for the native client path"
        })?;
        let empty_world = if config.otclient_v8_native_empty_world_enabled {
            Some(NativeOtClientEmptyWorldConfig {
                ground_thing_id: config.otclient_v8_empty_world_ground_thing_id,
                player_look_type: config
                    .otclient_v8_player_look_type
                    .try_into()
                    .map_err(|_| "otclientV8PlayerLookType must fit the selected native profile")?,
                outfit_first_look_type: config
                    .otclient_v8_outfit_first_look_type
                    .try_into()
                    .map_err(|_| {
                        "otclientV8OutfitFirstLookType must fit the selected native profile"
                    })?,
                outfit_last_look_type: config
                    .otclient_v8_outfit_last_look_type
                    .try_into()
                    .map_err(|_| {
                        "otclientV8OutfitLastLookType must fit the selected native profile"
                    })?,
                player_speed: config.otclient_v8_player_speed,
                server_beat: config.otclient_v8_server_beat,
            })
        } else {
            None
        };
        let startup_content = load_independent_native_startup_content(&config, &world_map)?;
        let companions = startup_content.companions;
        let entity_catalog = startup_content.entity_catalog;
        let vocation_registry = startup_content.vocation_registry;
        let declarative_weapon_catalog = startup_content.declarative_weapon_catalog;
        let declarative_spell_catalog = startup_content.declarative_spell_catalog;
        let regeneration_rules = vocation_registry
            .as_ref()
            .map(|registry| {
                registry
                    .iter()
                    .map(|(id, definition)| {
                        Ok((
                            *id,
                            PlayerRegenerationRules {
                                health: RegenerationRule::new(
                                    definition.health_regeneration.interval_seconds,
                                    definition.health_regeneration.amount,
                                )?,
                                mana: RegenerationRule::new(
                                    definition.mana_regeneration.interval_seconds,
                                    definition.mana_regeneration.amount,
                                )?,
                            },
                        ))
                    })
                    .collect::<Result<std::collections::BTreeMap<_, _>, forgotten_core::CoreError>>(
                    )
            })
            .transpose()?
            .map(Arc::new);
        let progression_rules = vocation_registry
            .as_ref()
            .map(|registry| {
                registry
                    .iter()
                    .map(|(id, definition)| Ok((*id, definition.progression_rules()?)))
                    .collect::<Result<std::collections::BTreeMap<_, _>, forgotten_core::CoreError>>(
                    )
            })
            .transpose()?
            .map(Arc::new);
        let vocation_level_up_gains = vocation_registry.as_ref().map(|registry| {
            Arc::new(
                registry
                    .iter()
                    .map(|(id, definition)| (*id, definition.level_up_gains()))
                    .collect::<std::collections::BTreeMap<_, _>>(),
            )
        });
        let armor_multiplier_by_vocation = vocation_registry.as_ref().map(|registry| {
            Arc::new(
                registry
                    .iter()
                    .map(|(id, definition)| (*id, definition.armor_multiplier.milli()))
                    .collect::<std::collections::BTreeMap<_, _>>(),
            )
        });
        let experience_award_policy = Arc::new(config.experience_award_policy()?);
        let death_loss_policy = DeathLossPolicy::from_config(config.death_loss_percent)?;
        let declarative_weapon_catalog = declarative_weapon_catalog.map(Arc::new);
        if let Some(catalog) = &declarative_weapon_catalog {
            println!(
                "> Loaded {} scriptless declarative weapon definitions; equipped-item binding remains limited to the native selected-melee foundation.",
                catalog.len()
            );
        }
        let declarative_spell_catalog = declarative_spell_catalog.map(Arc::new);
        if let Some(catalog) = &declarative_spell_catalog {
            println!(
                "> Loaded {} scriptless declarative spell definitions; client invocation, effects, and Lua remain deferred.",
                catalog.len()
            );
        }
        let static_spawns = materialize_tfs_static_spawns(&companions, &entity_catalog)?;
        if !static_spawns.entities.is_empty() {
            println!(
                "> Materialized {} display-only static TFS spawn entities; AI, combat, movement, and Lua remain deferred.",
                static_spawns.entities.len()
            );
        }
        Some(NativeOtClientHostConfig {
            bind_addr: config.otclient_v8_login_socket_addr(),
            client_profile: config.otclient_v8_native_profile(),
            server_name: config.server_name.clone(),
            advertised_game_addr: SocketAddr::new(
                advertised_ip,
                config.advertised_otclient_v8_game_port,
            ),
            max_connections: config.max_connections(),
            session_timeout: Duration::from_secs(5),
            extended_diagnostics,
            empty_world,
            world_map: Some(Arc::clone(&world_map)),
            item_presentation_catalog: item_presentation_catalog.map(Arc::new),
            item_armor_by_server_id: item_armor_by_server_id.map(Arc::new),
            item_weight_by_server_id: item_weight_by_server_id.map(Arc::new),
            stackable_item_server_ids: stackable_item_server_ids.map(Arc::new),
            armor_multiplier_by_vocation,
            static_spawns: (!static_spawns.entities.is_empty()).then(|| Arc::new(static_spawns)),
            static_target_attack_policy: match config.static_creature_target_attack_damage {
                0 => StaticTargetAttackPolicy::Disabled,
                damage => StaticTargetAttackPolicy::SelectedAdjacentFixedDamage { damage },
            },
            static_target_pursuit_policy: match config.static_creature_target_pursuit_range {
                0 => StaticTargetPursuitPolicy::Disabled,
                max_range => StaticTargetPursuitPolicy::NearestLivingPlayerOneStep { max_range },
            },
            regeneration_rules,
            progression_rules,
            vocation_level_up_gains,
            skill_rate: config.skill_rate,
            experience_award_policy: Some(experience_award_policy),
            death_loss_policy,
            declarative_weapon_catalog,
            declarative_spell_catalog,
        })
    } else {
        None
    };
    let native_login = if let Some(native_config) = &native_config {
        match start_native_otclient_login(native_config.clone(), &config.database_path) {
            Ok(listener) => Some(listener),
            Err(error) => {
                if let Some(game_session) = game_session {
                    game_session.shutdown()?;
                }
                status.shutdown()?;
                host.shutdown()?;
                return Err(Box::new(error));
            }
        }
    } else {
        None
    };
    let native_game = if let Some(native_config) = native_config {
        let mut native_game_config = native_config;
        native_game_config.bind_addr = config.otclient_v8_game_socket_addr();
        match start_native_otclient_game(native_game_config, &config.database_path) {
            Ok(listener) => Some(listener),
            Err(error) => {
                if let Some(native_login) = native_login {
                    native_login.shutdown()?;
                }
                if let Some(game_session) = game_session {
                    game_session.shutdown()?;
                }
                status.shutdown()?;
                host.shutdown()?;
                return Err(Box::new(error));
            }
        }
    } else {
        None
    };
    let game_shutdown = host.shutdown_signal();
    let status_shutdown = status.shutdown_signal();
    let game_session_shutdown = game_session
        .as_ref()
        .map(|session| session.shutdown_signal());
    let native_login_shutdown = native_login
        .as_ref()
        .map(|listener| listener.shutdown_signal());
    let native_game_shutdown = native_game
        .as_ref()
        .map(|listener| listener.shutdown_signal());
    ctrlc::set_handler({
        let game_shutdown = game_shutdown.clone();
        let status_shutdown = status_shutdown.clone();
        let game_session_shutdown = game_session_shutdown.clone();
        let native_login_shutdown = native_login_shutdown.clone();
        let native_game_shutdown = native_game_shutdown.clone();
        move || {
            game_shutdown.store(true, Ordering::SeqCst);
            status_shutdown.store(true, Ordering::SeqCst);
            if let Some(game_session_shutdown) = &game_session_shutdown {
                game_session_shutdown.store(true, Ordering::SeqCst);
            }
            if let Some(native_login_shutdown) = &native_login_shutdown {
                native_login_shutdown.store(true, Ordering::SeqCst);
            }
            if let Some(native_game_shutdown) = &native_game_shutdown {
                native_game_shutdown.store(true, Ordering::SeqCst);
            }
        }
    })?;

    println!(
        "> FE game endpoint running on {} for {} / Tibia {}",
        host.local_addr(),
        config.profile.compatibility_reference,
        config.profile.tibia_protocol
    );
    println!(
        "> TFS-style status service running on {}",
        status.local_addr()
    );
    if let Some(game_session) = &game_session {
        println!(
            "> Bounded fe-7.4 game-session foundation running on {}; official-client acceptance remains unverified.",
            game_session.local_addr()
        );
    }
    if let (Some(native_login), Some(native_game)) = (&native_login, &native_game) {
        println!(
            "> Native OTClientV8 profile={} login={} game={} empty-world={}",
            config.otclient_v8_protocol_version,
            native_login.local_addr(),
            native_game.local_addr(),
            config.otclient_v8_native_empty_world_enabled,
        );
    }
    if config.legacy_login_enabled {
        println!("> Bounded 7.4 login/character-list foundation is enabled; official-client acceptance remains unverified.");
    } else {
        println!(
            "> Diagnostic probe service is enabled; legacy login remains disabled in config.lua."
        );
    }
    println!("> Server host online. Press Ctrl+C for an orderly shutdown.");

    while !game_shutdown.load(Ordering::SeqCst)
        && !status_shutdown.load(Ordering::SeqCst)
        && game_session_shutdown
            .as_ref()
            .map(|shutdown| !shutdown.load(Ordering::SeqCst))
            .unwrap_or(true)
        && native_login_shutdown
            .as_ref()
            .map(|shutdown| !shutdown.load(Ordering::SeqCst))
            .unwrap_or(true)
        && native_game_shutdown
            .as_ref()
            .map(|shutdown| !shutdown.load(Ordering::SeqCst))
            .unwrap_or(true)
    {
        thread::sleep(Duration::from_millis(100));
    }
    if let Some(game_session) = game_session {
        game_session.shutdown()?;
    }
    if let Some(native_game) = native_game {
        native_game.shutdown()?;
    }
    if let Some(native_login) = native_login {
        native_login.shutdown()?;
    }
    status.shutdown()?;
    host.shutdown()?;
    println!("> Server host stopped.");
    Ok(())
}

fn status(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load(&directory)?;
    let database = EngineDatabase::open(&config.database_path)?;
    println!(
        "serverName={} profile={} reference={} targetProtocol={} database={} schema={} events={}",
        config.server_name,
        config.profile.id,
        config.profile.compatibility_reference,
        config.profile.tibia_protocol,
        database.path().display(),
        database.schema_version()?,
        database.event_count()?
    );
    Ok(())
}

fn generate_key(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load(&directory)?;
    if config.profile.id != "fe-7.4" {
        return Err("generate-key is currently available only for the fe-7.4 profile".into());
    }
    if config.rsa_private_key_path.exists() {
        return Err(format!(
            "refusing to overwrite existing private key {}",
            config.rsa_private_key_path.display()
        )
        .into());
    }
    if let Some(parent) = config.rsa_private_key_path.parent() {
        fs::create_dir_all(parent)?;
    }
    LegacyRsaPrivateKey::generate()?.write_pem(&config.rsa_private_key_path)?;
    println!(
        "generated original FE 1024-bit legacy-login private key at {}; set legacyLoginEnabled = true only when using the bounded 7.4 login foundation",
        config.rsa_private_key_path.display()
    );
    Ok(())
}

fn backup(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load(&directory)?;
    let artifact = create_backup(&config.database_path, directory.join("backups"))?;
    println!(
        "backup={} manifest={}",
        artifact.database_copy.display(),
        artifact.manifest_path.display()
    );
    Ok(())
}

fn command_line(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let directory = required_path(arguments, 1)?;
    let action = arguments
        .get(2)
        .map(String::as_str)
        .ok_or("a command action is required")?;
    match action {
        "broadcast" => {
            let message = arguments.get(3..).unwrap_or_default().join(" ");
            if message.trim().is_empty() {
                return Err("broadcast message is required".into());
            }
            let config = load(&directory)?;
            let database = EngineDatabase::open(&config.database_path)?;
            database.record_event("command", &format!("broadcast: {message}"))?;
            println!("recorded Forgotten Engine broadcast command");
            Ok(())
        }
        unsupported => Err(format!("unsupported command action `{unsupported}`").into()),
    }
}

/// Executes one operator-requested callback only when the exact relative file path is already
/// declared by the selected TFS XML registry. This bridge is intentionally side-effect-free: the
/// sandbox receives primitive values only and exposes no TFS Lua API, world mutation, modules, or
/// filesystem access from Lua.
fn script_command(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let action = arguments
        .get(1)
        .map(String::as_str)
        .ok_or("a script action is required")?;
    match action {
        "dispatch" => {
            if arguments.len() != 9 {
                return Err("usage: script dispatch <directory> <actions|creaturescripts|events|globalevents|movements|spells|talkactions|weapons> <declared-relative-script> <callback-name> <event-kind> <subject-id> <value>".into());
            }
            let directory = required_path(arguments, 2)?;
            let category = parse_tfs_script_registry_category(arguments.get(3))?;
            let relative_path = Path::new(
                arguments
                    .get(4)
                    .ok_or("a declared relative script path is required")?,
            );
            let callback_name = arguments
                .get(5)
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("a callback name is required")?;
            let event_kind = parse_script_event_kind(arguments.get(6))?;
            let subject_id = arguments
                .get(7)
                .ok_or("a script subject ID is required")?
                .parse::<u64>()
                .map_err(|_| "script subject ID must be an unsigned 64-bit integer")?;
            let value = arguments
                .get(8)
                .ok_or("a script value is required")?
                .parse::<i64>()
                .map_err(|_| "script value must be a signed 64-bit integer")?;
            let config = load(&directory)?;
            let reference =
                resolve_tfs_registry_script_reference(&config, category, relative_path)?;
            let mut dispatcher = SandboxedLuaCallbackDispatcher::default();
            dispatcher
                .register_callback_file(
                    callback_name,
                    &reference.script_root,
                    &reference.relative_path,
                )
                .map_err(|error| format!("script callback registration rejected: {error:?}"))?;
            let outcome = dispatcher.dispatch(
                callback_name,
                &SandboxedLuaCallbackInput {
                    event_kind,
                    subject_id,
                    value,
                },
            );
            println!(
                "script-dispatch category={} registry={} script={} callback={} state={:?} instruction-checks={} value={:?}",
                reference.category.label(),
                reference.registry_path.display(),
                reference.relative_path.display(),
                callback_name,
                outcome.state,
                outcome.instruction_checks,
                outcome.value,
            );
            Ok(())
        }
        unsupported => Err(format!("unsupported script action `{unsupported}`").into()),
    }
}

fn parse_tfs_script_registry_category(
    value: Option<&String>,
) -> Result<TfsRegistryCategory, Box<dyn std::error::Error>> {
    match value.map(String::as_str) {
        Some("actions") => Ok(TfsRegistryCategory::Actions),
        Some("creaturescripts") => Ok(TfsRegistryCategory::CreatureScripts),
        Some("events") => Ok(TfsRegistryCategory::Events),
        Some("globalevents") => Ok(TfsRegistryCategory::GlobalEvents),
        Some("movements") => Ok(TfsRegistryCategory::Movements),
        Some("spells") => Ok(TfsRegistryCategory::Spells),
        Some("talkactions") => Ok(TfsRegistryCategory::TalkActions),
        Some("weapons") => Ok(TfsRegistryCategory::Weapons),
        Some("monsters" | "npcs") => Err(
            "monster and NPC registries use entity file references, not Lua script references"
                .into(),
        ),
        Some(other) => Err(format!("unsupported TFS script registry category `{other}`").into()),
        None => Err("a TFS script registry category is required".into()),
    }
}

fn parse_script_event_kind(value: Option<&String>) -> Result<String, Box<dyn std::error::Error>> {
    let value = value
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("a script event kind is required")?;
    if value.len() > MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES {
        return Err(format!(
            "script event kind exceeds the {}-byte limit",
            MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES
        )
        .into());
    }
    Ok(value.to_owned())
}

fn account_command(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let action = arguments
        .get(1)
        .map(String::as_str)
        .ok_or("an account action is required")?;
    match action {
        "create" => {
            let directory = required_path(arguments, 2)?;
            let account_name = arguments
                .get(3)
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("an account name is required")?;
            let password = arguments
                .get(4)
                .map(String::as_str)
                .filter(|value| !value.is_empty())
                .ok_or("an account password is required")?;
            if arguments.len() != 5 {
                return Err("usage: account create <directory> <account-name> <password>".into());
            }
            let config = load(&directory)?;
            let database = EngineDatabase::open(&config.database_path)?;
            let account_id =
                database.create_account_with_password(account_name.trim(), password)?;
            println!(
                "created local account name={} native-account-id={account_id}",
                account_name.trim()
            );
            Ok(())
        }
        unsupported => Err(format!("unsupported account action `{unsupported}`").into()),
    }
}

/// Runs the existing authoritative temple-respawn transition against one persisted player, then
/// commits position, vitals, and cleared lifecycle state together. It is intentionally
/// operator-controlled and has no native session, teleport packet, timer, loss, or effect path.
fn respawn_persisted_player(
    database: &mut EngineDatabase,
    player_id: u64,
    world_map: &WorldMap,
) -> Result<(forgotten_core::Position, PlayerVitals), Box<dyn std::error::Error>> {
    let character = database.player_by_id(player_id)?;
    let mut world = WorldState::default();
    world.add_player_with_vitals_and_progression(
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
    )?;
    world.replace_player_town(player_id, character.town_id)?;
    world.hydrate_player_respawn_state(player_id, character.respawn_state)?;
    let outcome = world.respawn_player(player_id)?;
    if !world_map
        .tile(outcome.position)
        .is_some_and(|tile| tile.walkable)
    {
        return Err(
            "player respawn destination is missing or not walkable in the loaded map".into(),
        );
    }
    database.update_player_position_vitals_and_respawn_state(
        player_id,
        outcome.position,
        forgotten_persistence::PlayerVitals {
            health: outcome.vitals.health,
            max_health: outcome.vitals.max_health,
            mana: outcome.vitals.mana,
            max_mana: outcome.vitals.max_mana,
            capacity: outcome.vitals.capacity,
            magic_level: outcome.vitals.magic_level,
        },
        PlayerRespawnState::default(),
    )?;
    Ok((outcome.position, outcome.vitals))
}

fn player_command(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let action = arguments
        .get(1)
        .map(String::as_str)
        .ok_or("a player action is required")?;
    match action {
        "create" => {
            let directory = required_path(arguments, 2)?;
            let account_id: u32 = arguments
                .get(3)
                .ok_or("a numeric native account ID is required")?
                .parse()
                .map_err(|_| "account ID must be an unsigned 32-bit integer")?;
            let player_name = arguments
                .get(4)
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("a character name is required")?;
            if !(arguments.len() == 5 || arguments.len() == 6) {
                return Err(
                    "usage: player create <directory> <account-id> <character-name> [vocation-id]"
                        .into(),
                );
            }
            let vocation = arguments
                .get(5)
                .map(|value| {
                    value
                        .parse::<u16>()
                        .map(VocationId::new)
                        .map_err(|_| "vocation ID must be a u16")
                })
                .transpose()?
                .unwrap_or_default();
            let config = load(&directory)?;
            let database = EngineDatabase::open(&config.database_path)?;
            let player = database.create_player_for_account_with_vocation(
                account_id,
                player_name,
                vocation,
            )?;
            println!(
                "created character name={} player-id={} account-id={} position={},{},{} level={} vocation-id={}",
                player.name,
                player.id,
                account_id,
                player.position.x,
                player.position.y,
                player.position.z,
                player.level,
                player.progression.vocation.value(),
            );
            Ok(())
        }
        "equip" => {
            if !(arguments.len() == 6 || arguments.len() == 7) {
                return Err(
                    "usage: player equip <directory> <player-id> <slot> <server-item-id> [count]"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let slot = parse_equipment_slot(arguments.get(4))?;
            let server_id = parse_u16_argument(arguments.get(5), "server item ID")?;
            let count = arguments
                .get(6)
                .map(|value| value.parse::<u16>().map_err(|_| "item count must be a u16"))
                .transpose()?
                .unwrap_or(1);
            let item = ItemInstance::new(server_id, count)?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let mut equipment = database.player_equipment(player_id)?;
            equipment.equip(slot, item);
            database.replace_player_equipment(player_id, &equipment)?;
            println!(
                "equipped player-id={player_id} slot={} server-item-id={server_id} count={count}",
                slot.code()
            );
            Ok(())
        }
        "unequip" => {
            if arguments.len() != 5 {
                return Err("usage: player unequip <directory> <player-id> <slot>".into());
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let slot = parse_equipment_slot(arguments.get(4))?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let mut equipment = database.player_equipment(player_id)?;
            if equipment.unequip(slot).is_none() {
                return Err(
                    format!("player {player_id} has no item in slot {}", slot.code()).into(),
                );
            }
            database.replace_player_equipment(player_id, &equipment)?;
            println!("unequipped player-id={player_id} slot={}", slot.code());
            Ok(())
        }
        "vocation" => {
            if arguments.len() != 5 {
                return Err("usage: player vocation <directory> <player-id> <vocation-id>".into());
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let vocation_id = parse_u16_argument(arguments.get(4), "vocation ID")?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let mut progression = database.player_progression(player_id)?;
            progression.vocation = VocationId::new(vocation_id);
            database.replace_player_progression(player_id, progression)?;
            println!("updated player vocation player-id={player_id} vocation-id={vocation_id}");
            Ok(())
        }
        "town" => {
            if arguments.len() != 5 {
                return Err("usage: player town <directory> <player-id> <town-id>".into());
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let town_id = arguments
                .get(4)
                .ok_or("a town ID is required")?
                .parse::<u32>()
                .map_err(|_| "town ID must be an unsigned 32-bit integer")?;
            let config = load(&directory)?;
            let database = EngineDatabase::open(&config.database_path)?;
            database.update_player_town(player_id, town_id)?;
            println!("updated player town player-id={player_id} town-id={town_id}");
            Ok(())
        }
        "respawn" => {
            if arguments.len() != 4 {
                return Err("usage: player respawn <directory> <player-id>".into());
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let config = load(&directory)?;
            let world_map = load_world_map(&config)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let (position, vitals) =
                respawn_persisted_player(&mut database, player_id, &world_map)?;
            println!(
                "respawned player-id={player_id} position={},{},{} health={}/{} mana={}/{}",
                position.x,
                position.y,
                position.z,
                vitals.health,
                vitals.max_health,
                vitals.mana,
                vitals.max_mana,
            );
            Ok(())
        }
        "skill" => {
            if !(arguments.len() == 6 || arguments.len() == 7) {
                return Err(
                    "usage: player skill <directory> <player-id> <fist|club|sword|axe|distance|shielding|fishing> <level> [percent]"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let skill = parse_player_skill(arguments.get(4))?;
            let level = parse_u16_argument(arguments.get(5), "skill level")?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let mut progression = database.player_progression(player_id)?;
            let percent = arguments
                .get(6)
                .map(|value| parse_u8_argument(Some(value), "skill percent"))
                .transpose()?
                .unwrap_or_else(|| progression.skills.skill(skill).percent);
            progression
                .skills
                .set(skill, SkillProgress::new(level, percent)?);
            database.replace_player_progression(player_id, progression)?;
            println!(
                "updated player skill player-id={player_id} skill={} level={level} percent={percent}",
                skill.code()
            );
            Ok(())
        }
        "skill-tries" => {
            if arguments.len() != 6 {
                return Err(
                    "usage: player skill-tries <directory> <player-id> <fist|club|sword|axe|distance|shielding|fishing> <awarded-tries>".into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let skill = parse_player_skill(arguments.get(4))?;
            let awarded_tries = arguments
                .get(5)
                .ok_or("awarded tries are required")?
                .parse::<u64>()
                .map_err(|_| "awarded tries must be an unsigned 64-bit integer")?;
            let config = load(&directory)?;
            let applied_tries = awarded_tries.saturating_mul(u64::from(config.skill_rate));
            let mut database = EngineDatabase::open(&config.database_path)?;
            let character = database.player_by_id(player_id)?;
            let registry = load_tfs_vocation_registry(&config)?
                .ok_or("player skill-tries requires data/XML/vocations.xml")?;
            let rules = registry
                .get(character.progression.vocation)
                .ok_or("player vocation has no configured progression rules")?
                .progression_rules()?;
            let mut world = WorldState::default();
            world.add_player_with_vitals_and_progression(
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
            )?;
            world.replace_player_progression_attempts(player_id, character.progression_attempts)?;
            let outcome = world.apply_player_skill_tries(player_id, skill, applied_tries, rules)?;
            database.replace_player_progression_and_attempts(
                player_id,
                world.player_progression(player_id)?,
                world.player_progression_attempts(player_id)?,
            )?;
            println!(
                "awarded player skill-tries player-id={player_id} skill={} awarded={} applied={} level={} percent={} gained-levels={} stored-tries={}",
                skill.code(),
                awarded_tries,
                applied_tries,
                outcome.progress.level,
                outcome.progress.percent,
                outcome.gained_levels,
                outcome.stored_tries,
            );
            Ok(())
        }
        "magic-mana" => {
            if arguments.len() != 5 {
                return Err(
                    "usage: player magic-mana <directory> <player-id> <awarded-mana>".into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let awarded_mana = arguments
                .get(4)
                .ok_or("awarded mana is required")?
                .parse::<u64>()
                .map_err(|_| "awarded mana must be an unsigned 64-bit integer")?;
            let config = load(&directory)?;
            let applied_mana = awarded_mana.saturating_mul(u64::from(config.magic_rate));
            let mut database = EngineDatabase::open(&config.database_path)?;
            let character = database.player_by_id(player_id)?;
            let registry = load_tfs_vocation_registry(&config)?
                .ok_or("player magic-mana requires data/XML/vocations.xml")?;
            let rules = registry
                .get(character.progression.vocation)
                .ok_or("player vocation has no configured progression rules")?
                .progression_rules()?;
            let mut world = WorldState::default();
            world.add_player_with_vitals_and_progression(
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
            )?;
            world.replace_player_progression_attempts(player_id, character.progression_attempts)?;
            let outcome = world.apply_player_magic_mana(player_id, applied_mana, rules)?;
            let vitals = world.player_vitals(player_id)?;
            database.update_player_vitals_and_progression_attempts(
                player_id,
                forgotten_persistence::PlayerVitals {
                    health: vitals.health,
                    max_health: vitals.max_health,
                    mana: vitals.mana,
                    max_mana: vitals.max_mana,
                    capacity: vitals.capacity,
                    magic_level: vitals.magic_level,
                },
                world.player_progression_attempts(player_id)?,
            )?;
            println!(
                "awarded player magic-mana player-id={player_id} awarded={} applied={} magic-level={} gained-levels={} stored-mana={}",
                awarded_mana,
                applied_mana,
                outcome.magic_level,
                outcome.gained_levels,
                outcome.stored_mana,
            );
            Ok(())
        }
        "experience" => {
            if arguments.len() != 5 {
                return Err(
                    "usage: player experience <directory> <player-id> <raw-experience>".into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let raw_experience = arguments
                .get(4)
                .ok_or("raw experience is required")?
                .parse::<u64>()
                .map_err(|_| "raw experience must be an unsigned 64-bit integer")?;
            let config = load(&directory)?;
            let policy = config.experience_award_policy()?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let character = database.player_by_id(player_id)?;
            let gains = load_tfs_vocation_registry(&config)?
                .as_ref()
                .and_then(|registry| registry.get(character.progression.vocation))
                .map(|definition| definition.level_up_gains())
                .unwrap_or_default();
            let mut world = WorldState::default();
            world.add_player_with_vitals_and_progression(
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
            )?;
            let outcome = world.award_player_experience_with_vocation_gains(
                player_id,
                raw_experience,
                &policy,
                gains,
            )?;
            database.update_player_experience_and_vitals(
                player_id,
                outcome.level,
                outcome.experience,
                forgotten_persistence::PlayerVitals {
                    health: outcome.vitals.health,
                    max_health: outcome.vitals.max_health,
                    mana: outcome.vitals.mana,
                    max_mana: outcome.vitals.max_mana,
                    capacity: outcome.vitals.capacity,
                    magic_level: outcome.vitals.magic_level,
                },
            )?;
            println!(
                "awarded player experience player-id={player_id} raw={} awarded={} experience={} level={} gained-levels={}",
                outcome.raw_experience,
                outcome.awarded_experience,
                outcome.experience,
                outcome.level,
                outcome.gained_levels,
            );
            Ok(())
        }
        "container-stow-equipped" => {
            if arguments.len() != 6 {
                return Err(
                    "usage: player container-stow-equipped <directory> <player-id> <slot> <container-id>"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let slot = parse_equipment_slot(arguments.get(4))?;
            let container_id = parse_u8_argument(arguments.get(5), "container ID")?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let character = database.player_by_id(player_id)?;
            let equipment = database.player_equipment(player_id)?;
            let containers = database.player_containers(player_id)?;
            let mut world = WorldState::default();
            world.add_player(Player {
                id: character.id,
                account_id: 0,
                name: character.name,
                position: character.position,
                level: character.level,
                experience: character.experience,
                skill_points: character.skill_points,
            })?;
            world.replace_player_equipment(player_id, equipment)?;
            world.replace_player_containers(player_id, containers)?;
            let outcome = world.move_equipment_item_to_container(player_id, slot, container_id)?;
            database.replace_player_inventory(
                player_id,
                world.player_equipment(player_id)?,
                world.player_containers(player_id)?,
            )?;
            println!(
                "moved equipped item to container player-id={player_id} slot={} container-id={} server-item-id={} count={}",
                outcome.from_slot.code(),
                outcome.container_id,
                outcome.item.server_id,
                outcome.item.count,
            );
            Ok(())
        }
        "container-equip" => {
            if arguments.len() != 7 {
                return Err(
                    "usage: player container-equip <directory> <player-id> <container-id> <item-index> <slot>"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let container_id = parse_u8_argument(arguments.get(4), "container ID")?;
            let item_index = arguments
                .get(5)
                .ok_or("container item index is required")?
                .parse::<usize>()
                .map_err(|_| "container item index must be an unsigned integer")?;
            let slot = parse_equipment_slot(arguments.get(6))?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let character = database.player_by_id(player_id)?;
            let equipment = database.player_equipment(player_id)?;
            let containers = database.player_containers(player_id)?;
            let mut world = WorldState::default();
            world.add_player(Player {
                id: character.id,
                account_id: 0,
                name: character.name,
                position: character.position,
                level: character.level,
                experience: character.experience,
                skill_points: character.skill_points,
            })?;
            world.replace_player_equipment(player_id, equipment)?;
            world.replace_player_containers(player_id, containers)?;
            let outcome = world.move_container_item_to_equipment(
                player_id,
                container_id,
                item_index,
                slot,
            )?;
            database.replace_player_inventory(
                player_id,
                world.player_equipment(player_id)?,
                world.player_containers(player_id)?,
            )?;
            println!(
                "moved container item to equipment player-id={player_id} container-id={} item-index={} slot={} server-item-id={} count={}",
                outcome.container_id,
                outcome.item_index,
                outcome.to_slot.code(),
                outcome.item.server_id,
                outcome.item.count,
            );
            Ok(())
        }
        "container-stow-stack" => {
            if arguments.len() != 7 {
                return Err(
                    "usage: player container-stow-stack <directory> <player-id> <slot> <container-id> <count>"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let slot = parse_equipment_slot(arguments.get(4))?;
            let container_id = parse_u8_argument(arguments.get(5), "container ID")?;
            let count = parse_u16_argument(arguments.get(6), "item count")?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let character = database.player_by_id(player_id)?;
            let equipment = database.player_equipment(player_id)?;
            let containers = database.player_containers(player_id)?;
            let mut world = WorldState::default();
            world.add_player(Player {
                id: character.id,
                account_id: 0,
                name: character.name,
                position: character.position,
                level: character.level,
                experience: character.experience,
                skill_points: character.skill_points,
            })?;
            world.replace_player_equipment(player_id, equipment)?;
            world.replace_player_containers(player_id, containers)?;
            let outcome =
                world.move_equipment_stack_to_container(player_id, slot, container_id, count)?;
            database.replace_player_inventory(
                player_id,
                world.player_equipment(player_id)?,
                world.player_containers(player_id)?,
            )?;
            println!(
                "moved equipped stack to container player-id={player_id} slot={} container-id={} moved-server-item-id={} moved-count={} source-remaining={:?} destination-index={} destination-count={}",
                outcome.from_slot.code(),
                outcome.container_id,
                outcome.moved_item.server_id,
                outcome.moved_item.count,
                outcome.source_remaining_count,
                outcome.destination_index,
                outcome.destination_count,
            );
            Ok(())
        }
        "container-equip-stack" => {
            if arguments.len() != 8 {
                return Err(
                    "usage: player container-equip-stack <directory> <player-id> <container-id> <item-index> <slot> <count>"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let container_id = parse_u8_argument(arguments.get(4), "container ID")?;
            let item_index = parse_usize_argument(arguments.get(5), "container item index")?;
            let slot = parse_equipment_slot(arguments.get(6))?;
            let count = parse_u16_argument(arguments.get(7), "item count")?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let character = database.player_by_id(player_id)?;
            let equipment = database.player_equipment(player_id)?;
            let containers = database.player_containers(player_id)?;
            let mut world = WorldState::default();
            world.add_player(Player {
                id: character.id,
                account_id: 0,
                name: character.name,
                position: character.position,
                level: character.level,
                experience: character.experience,
                skill_points: character.skill_points,
            })?;
            world.replace_player_equipment(player_id, equipment)?;
            world.replace_player_containers(player_id, containers)?;
            let outcome = world.move_container_stack_to_equipment(
                player_id,
                container_id,
                item_index,
                slot,
                count,
            )?;
            database.replace_player_inventory(
                player_id,
                world.player_equipment(player_id)?,
                world.player_containers(player_id)?,
            )?;
            println!(
                "moved container stack to equipment player-id={player_id} container-id={} item-index={} slot={} moved-server-item-id={} moved-count={} source-remaining={:?} destination-count={}",
                outcome.container_id,
                outcome.item_index,
                outcome.to_slot.code(),
                outcome.moved_item.server_id,
                outcome.moved_item.count,
                outcome.source_remaining_count,
                outcome.destination_count,
            );
            Ok(())
        }
        "container-create" => {
            if arguments.len() < 8 {
                return Err(
                    "usage: player container-create <directory> <player-id> <container-id> <container-server-item-id> <capacity> <name>"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let container_id = parse_u8_argument(arguments.get(4), "container ID")?;
            let server_id = parse_u16_argument(arguments.get(5), "container server item ID")?;
            let capacity = parse_u16_argument(arguments.get(6), "container capacity")?;
            let name = arguments[7..].join(" ");
            let container = PlayerContainer::new(
                container_id,
                ItemInstance::new(server_id, 1)?,
                name,
                false,
                capacity,
            )?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let mut containers = database.player_containers(player_id)?;
            if containers.container(container_id).is_some() {
                return Err(
                    format!("player {player_id} already has container {container_id}").into(),
                );
            }
            containers.insert(container)?;
            database.replace_player_containers(player_id, &containers)?;
            println!(
                "created player container player-id={player_id} container-id={container_id} server-item-id={server_id} capacity={capacity}"
            );
            Ok(())
        }
        "container-add" => {
            if !(arguments.len() == 7 || arguments.len() == 8) {
                return Err(
                    "usage: player container-add <directory> <player-id> <container-id> <server-item-id> [count]"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let container_id = parse_u8_argument(arguments.get(4), "container ID")?;
            let server_id = parse_u16_argument(arguments.get(5), "server item ID")?;
            let count = arguments
                .get(6)
                .map(|value| value.parse::<u16>().map_err(|_| "item count must be a u16"))
                .transpose()?
                .unwrap_or(1);
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let mut containers = database.player_containers(player_id)?;
            let mut container = containers
                .remove(container_id)
                .ok_or_else(|| format!("player {player_id} has no container {container_id}"))?;
            container
                .items
                .insert(ItemInstance::new(server_id, count)?)?;
            containers.insert(container)?;
            database.replace_player_containers(player_id, &containers)?;
            println!(
                "added player container item player-id={player_id} container-id={container_id} server-item-id={server_id} count={count}"
            );
            Ok(())
        }
        "container-remove" => {
            if arguments.len() != 6 {
                return Err(
                    "usage: player container-remove <directory> <player-id> <container-id> <item-index>"
                        .into(),
                );
            }
            let directory = required_path(arguments, 2)?;
            let player_id = parse_player_id(arguments.get(3))?;
            let container_id = parse_u8_argument(arguments.get(4), "container ID")?;
            let item_index = parse_usize_argument(arguments.get(5), "container item index")?;
            let config = load(&directory)?;
            let mut database = EngineDatabase::open(&config.database_path)?;
            let mut containers = database.player_containers(player_id)?;
            let mut container = containers
                .remove(container_id)
                .ok_or_else(|| format!("player {player_id} has no container {container_id}"))?;
            if container.items.remove(item_index).is_none() {
                return Err(format!(
                    "player {player_id} container {container_id} has no item at index {item_index}"
                )
                .into());
            }
            containers.insert(container)?;
            database.replace_player_containers(player_id, &containers)?;
            println!(
                "removed player container item player-id={player_id} container-id={container_id} item-index={item_index}"
            );
            Ok(())
        }
        unsupported => Err(format!("unsupported player action `{unsupported}`").into()),
    }
}

fn parse_player_id(value: Option<&String>) -> Result<u64, Box<dyn std::error::Error>> {
    value
        .ok_or("a player ID is required")?
        .parse()
        .map_err(|_| "player ID must be an unsigned 64-bit integer".into())
}

fn parse_u16_argument(
    value: Option<&String>,
    label: &str,
) -> Result<u16, Box<dyn std::error::Error>> {
    value
        .ok_or_else(|| format!("a {label} is required"))?
        .parse()
        .map_err(|_| format!("{label} must be an unsigned 16-bit integer").into())
}

fn parse_u8_argument(
    value: Option<&String>,
    label: &str,
) -> Result<u8, Box<dyn std::error::Error>> {
    value
        .ok_or_else(|| format!("a {label} is required"))?
        .parse()
        .map_err(|_| format!("{label} must be an unsigned 8-bit integer").into())
}

fn parse_usize_argument(
    value: Option<&String>,
    label: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    value
        .ok_or_else(|| format!("a {label} is required"))?
        .parse()
        .map_err(|_| format!("{label} must be a non-negative integer").into())
}

fn parse_equipment_slot(
    value: Option<&String>,
) -> Result<EquipmentSlot, Box<dyn std::error::Error>> {
    match value.map(String::as_str) {
        Some("head") => Ok(EquipmentSlot::Head),
        Some("necklace") | Some("neck") => Ok(EquipmentSlot::Neck),
        Some("backpack") => Ok(EquipmentSlot::Backpack),
        Some("armor") => Ok(EquipmentSlot::Armor),
        Some("right") | Some("right-hand") => Ok(EquipmentSlot::RightHand),
        Some("left") | Some("left-hand") => Ok(EquipmentSlot::LeftHand),
        Some("legs") => Ok(EquipmentSlot::Legs),
        Some("feet") => Ok(EquipmentSlot::Feet),
        Some("ring") => Ok(EquipmentSlot::Ring),
        Some("ammo") => Ok(EquipmentSlot::Ammo),
        _ => Err(
            "equipment slot must be head, necklace, backpack, armor, right, left, legs, feet, ring, or ammo"
                .into(),
        ),
    }
}

fn parse_player_skill(value: Option<&String>) -> Result<PlayerSkill, Box<dyn std::error::Error>> {
    match value.map(String::as_str) {
        Some("fist") => Ok(PlayerSkill::Fist),
        Some("club") => Ok(PlayerSkill::Club),
        Some("sword") => Ok(PlayerSkill::Sword),
        Some("axe") => Ok(PlayerSkill::Axe),
        Some("distance") | Some("dist") => Ok(PlayerSkill::Distance),
        Some("shielding") | Some("shield") => Ok(PlayerSkill::Shielding),
        Some("fishing") | Some("fish") => Ok(PlayerSkill::Fishing),
        _ => Err("skill must be fist, club, sword, axe, distance, shielding, or fishing".into()),
    }
}

fn capability_matrix_json() -> &'static str {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/capability-matrix.json"
    ))
}

fn compatibility(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    match arguments {
        [command] if command == "compatibility" => {}
        [command, format] if command == "compatibility" && format == "--json" => {
            println!("{}", capability_matrix_json().trim());
            return Ok(());
        }
        _ => return Err("usage: compatibility [--json]".into()),
    }
    for profile in COMPATIBILITY_PROFILES {
        println!(
            "FE {}\t{}\tTibia {}\tofficial-client={}",
            profile.fe_release,
            profile.compatibility_reference,
            profile.tibia_protocol,
            profile.complete_protocol_emulation,
        );
    }
    for entry in forgotten_scripting::compatibility_matrix() {
        println!(
            "{}\t{}\t{}",
            entry.api,
            entry.capability.as_str(),
            entry.note
        );
    }
    Ok(())
}

fn version() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Forgotten Engine build {}\nCompatibility profiles:",
        env!("CARGO_PKG_VERSION")
    );
    for profile in COMPATIBILITY_PROFILES {
        println!(
            "  FE {} — {} / Tibia {}",
            profile.fe_release, profile.compatibility_reference, profile.tibia_protocol
        );
    }
    Ok(())
}

fn help_text() -> &'static str {
    r#"Forgotten Engine

Compatibility profiles:
  fe-7.4  — Tibia 7.4 (experimental native OTCv8 empty-world fixture)
  fe-8.0  — Tibia 8.0 (protocol foundation)
  fe-1.2  — TFS 1.2 / Tibia 10.98 (protocol foundation)

Commands:
  init <directory> [--profile fe-7.4|fe-8.0|fe-1.2]
  validate <directory>
  tfs-audit <directory>
  run <directory> [--ed]
  status <directory>
  generate-key <directory>
  backup <directory>
  account create <directory> <account-name> <password>
  player create <directory> <account-id> <character-name> [vocation-id]
  player equip <directory> <player-id> <slot> <server-item-id> [count]
  player unequip <directory> <player-id> <slot>
  player vocation <directory> <player-id> <vocation-id>
  player town <directory> <player-id> <town-id>
  player respawn <directory> <player-id>
  player skill <directory> <player-id> <fist|club|sword|axe|distance|shielding|fishing> <level> [percent]
  player skill-tries <directory> <player-id> <fist|club|sword|axe|distance|shielding|fishing> <awarded-tries>
  player magic-mana <directory> <player-id> <awarded-mana>
  player experience <directory> <player-id> <raw-experience>
  player container-stow-equipped <directory> <player-id> <slot> <container-id>
  player container-equip <directory> <player-id> <container-id> <item-index> <slot>
  player container-stow-stack <directory> <player-id> <slot> <container-id> <count>
  player container-equip-stack <directory> <player-id> <container-id> <item-index> <slot> <count>
  player container-create <directory> <player-id> <container-id> <container-server-item-id> <capacity> <name>
  player container-add <directory> <player-id> <container-id> <server-item-id> [count]
  player container-remove <directory> <player-id> <container-id> <item-index>
  command <directory> broadcast <message>
  script dispatch <directory> <actions|creaturescripts|events|globalevents|movements|spells|talkactions|weapons> <declared-relative-script> <callback-name> <event-kind> <subject-id> <value>
  compatibility [--json]
  version"#
}

fn print_help() {
    println!("{}", help_text());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn selects_tibia_7_4_profile_by_direct_selector() {
        let arguments = vec!["init".to_owned(), "world".to_owned(), "fe-7.4".to_owned()];
        let profile = selected_profile(&arguments, 2).unwrap();
        assert_eq!(profile.id, "fe-7.4");
        assert_eq!(profile.tibia_protocol, "7.4");
    }

    #[test]
    fn registry_categories_map_to_typed_deferred_script_events() {
        assert_eq!(
            deferred_script_event_kind(TfsRegistryCategory::Actions),
            DeferredScriptEventKind::Action
        );
        assert_eq!(
            deferred_script_event_kind(TfsRegistryCategory::TalkActions),
            DeferredScriptEventKind::TalkAction
        );
        assert_eq!(
            deferred_script_event_kind(TfsRegistryCategory::Npcs),
            DeferredScriptEventKind::Npc
        );
    }

    #[test]
    fn rejects_unknown_profile_by_direct_selector() {
        let arguments = vec!["init".to_owned(), "world".to_owned(), "unknown".to_owned()];
        assert!(selected_profile(&arguments, 2).is_err());
    }

    #[test]
    fn run_options_enable_only_the_explicit_extended_diagnostic_flags() {
        let short = vec!["run".into(), "native-world".into(), "--ed".into()];
        assert_eq!(
            run_options(&short).unwrap(),
            (PathBuf::from("native-world"), true)
        );

        let long = vec![
            "run".into(),
            "native-world".into(),
            "--extended-debug".into(),
        ];
        assert_eq!(
            run_options(&long).unwrap(),
            (PathBuf::from("native-world"), true)
        );

        let ordinary = vec!["run".into(), "native-world".into()];
        assert_eq!(
            run_options(&ordinary).unwrap(),
            (PathBuf::from("native-world"), false)
        );

        let invalid = vec!["run".into(), "native-world".into(), "--verbose".into()];
        assert!(run_options(&invalid).is_err());
    }

    #[test]
    fn help_text_preserves_the_established_local_world_workflow() {
        let help = help_text();
        for command in [
            "init <directory>",
            "validate <directory>",
            "tfs-audit <directory>",
            "run <directory> [--ed]",
            "status <directory>",
            "generate-key <directory>",
            "backup <directory>",
            "account create <directory> <account-name> <password>",
            "player create <directory> <account-id> <character-name> [vocation-id]",
            "player equip <directory> <player-id> <slot> <server-item-id> [count]",
            "player unequip <directory> <player-id> <slot>",
            "player vocation <directory> <player-id> <vocation-id>",
            "player town <directory> <player-id> <town-id>",
            "player respawn <directory> <player-id>",
            "player skill <directory> <player-id> <fist|club|sword|axe|distance|shielding|fishing> <level> [percent]",
            "player skill-tries <directory> <player-id> <fist|club|sword|axe|distance|shielding|fishing> <awarded-tries>",
            "player magic-mana <directory> <player-id> <awarded-mana>",
            "player experience <directory> <player-id> <raw-experience>",
            "player container-stow-equipped <directory> <player-id> <slot> <container-id>",
            "player container-equip <directory> <player-id> <container-id> <item-index> <slot>",
            "player container-stow-stack <directory> <player-id> <slot> <container-id> <count>",
            "player container-equip-stack <directory> <player-id> <container-id> <item-index> <slot> <count>",
            "player container-create <directory> <player-id> <container-id> <container-server-item-id> <capacity> <name>",
            "player container-add <directory> <player-id> <container-id> <server-item-id> [count]",
            "player container-remove <directory> <player-id> <container-id> <item-index>",
            "command <directory> broadcast <message>",
            "script dispatch <directory>",
            "compatibility",
            "version",
        ] {
            assert!(
                help.contains(command),
                "missing stable CLI command: {command}"
            );
        }
    }

    #[test]
    fn explicit_tfs_registry_callback_command_dispatches_only_a_declared_file() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("forgotten-engine-registry-callback-{nonce}"));
        fs::create_dir_all(directory.join("data/actions/scripts")).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        fs::write(
            directory.join("data/actions/actions.xml"),
            r#"<actions><action itemid="100" script="scripts/safe.lua"/></actions>"#,
        )
        .unwrap();
        fs::write(
            directory.join("data/actions/scripts/safe.lua"),
            "return function(_, _, value) return value + 1 end",
        )
        .unwrap();

        let dispatched = vec![
            "script".into(),
            "dispatch".into(),
            directory.display().to_string(),
            "actions".into(),
            "scripts/safe.lua".into(),
            "safe-callback".into(),
            "operator-test".into(),
            "42".into(),
            "7".into(),
        ];
        assert!(script_command(&dispatched).is_ok());

        let mut undeclared = dispatched;
        undeclared[4] = "scripts/other.lua".into();
        assert!(script_command(&undeclared).is_err());
        assert!(parse_tfs_script_registry_category(Some(&"monsters".into())).is_err());
        assert!(parse_script_event_kind(Some(
            &"x".repeat(MAX_SANDBOXED_LUA_CALLBACK_NAME_BYTES + 1,)
        ))
        .is_err());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn equipment_slot_parsing_is_explicit_and_bounded() {
        assert_eq!(
            parse_equipment_slot(Some(&"right".to_owned())).unwrap(),
            EquipmentSlot::RightHand
        );
        assert_eq!(
            parse_equipment_slot(Some(&"left-hand".to_owned())).unwrap(),
            EquipmentSlot::LeftHand
        );
        assert_eq!(
            parse_equipment_slot(Some(&"ammo".to_owned())).unwrap(),
            EquipmentSlot::Ammo
        );
        assert!(parse_equipment_slot(Some(&"purse".to_owned())).is_err());
        assert!(parse_player_id(Some(&"7".to_owned())).is_ok());
        assert!(parse_u16_argument(Some(&"65536".to_owned()), "server item ID").is_err());
        assert_eq!(
            parse_u8_argument(Some(&"15".to_owned()), "container ID").unwrap(),
            15
        );
        assert!(parse_u8_argument(Some(&"256".to_owned()), "container ID").is_err());
        assert_eq!(
            parse_usize_argument(Some(&"2".to_owned()), "container item index").unwrap(),
            2
        );
        assert!(parse_usize_argument(Some(&"-1".to_owned()), "container item index").is_err());
        assert_eq!(
            parse_player_skill(Some(&"sword".to_owned())).unwrap(),
            PlayerSkill::Sword
        );
        assert_eq!(
            parse_player_skill(Some(&"dist".to_owned())).unwrap(),
            PlayerSkill::Distance
        );
        assert!(parse_player_skill(Some(&"alchemy".to_owned())).is_err());
    }

    #[test]
    fn compatibility_json_is_an_additive_machine_readable_profile_report() {
        let matrix = capability_matrix_json();
        assert!(matrix.contains("\"schemaVersion\": 1"));
        assert!(matrix.contains("\"id\": \"fe-7.4\""));
        assert!(matrix.contains("\"id\": \"fe-8.0\""));
        assert!(matrix.contains("\"id\": \"fe-1.2\""));
        assert!(compatibility(&["compatibility".into(), "--json".into()]).is_ok());
        assert!(compatibility(&["compatibility".into(), "--unknown".into()]).is_err());
    }

    #[test]
    fn generates_an_original_legacy_key_for_a_7_4_world() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-key-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        generate_key(directory.clone()).unwrap();
        let config = load(&directory).unwrap();
        assert!(config.rsa_private_key_path.exists());
        assert!(LegacyRsaPrivateKey::load_pem(&config.rsa_private_key_path).is_ok());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn audits_a_tfs_style_world_without_fe_only_config_assignments() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-tfs-audit-{nonce}"));
        ensure_content_skeleton(&directory).unwrap();
        fs::write(
            directory.join("config.lua"),
            r#"worldType = "pvp"
ip = "127.0.0.1"
gameProtocolPort = 7172
statusProtocolPort = 7171
maxPlayers = 0
serverName = "Private TFS"
mapName = "forgotten"
mysqlHost = "127.0.0.1"
mysqlUser = "forgottenserver"
mysqlDatabase = "forgottenserver"
experienceStages = {
  { minlevel = 1, multiplier = 7 }
}
"#,
        )
        .unwrap();

        fs::create_dir_all(directory.join("data/actions/scripts")).unwrap();
        fs::write(
            directory.join("data/actions/actions.xml"),
            r#"<actions><action itemid="100" script="scripts/rope.lua"/></actions>"#,
        )
        .unwrap();
        fs::write(
            directory.join("data/actions/scripts/rope.lua"),
            "-- private TFS action; inventory only",
        )
        .unwrap();

        audit_tfs_conversion(directory.clone()).unwrap();
        assert!(!directory.join("data/forgotten-engine.db").exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn provisions_a_native_test_account_and_character_without_sql_console_access() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-provision-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();

        account_command(&[
            "account".into(),
            "create".into(),
            directory.display().to_string(),
            "test-account".into(),
            "test-password".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Knight".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "town".into(),
            directory.display().to_string(),
            "1".into(),
            "42".into(),
        ])
        .unwrap();
        assert!(player_command(&[
            "player".into(),
            "town".into(),
            directory.display().to_string(),
            "1".into(),
            "invalid".into(),
        ])
        .is_err());

        player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Druid".into(),
            "4".into(),
        ])
        .unwrap();
        assert!(player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Invalid".into(),
            "not-a-vocation".into(),
        ])
        .is_err());

        let config = load(&directory).unwrap();
        let database = EngineDatabase::open(&config.database_path).unwrap();
        let account = database
            .authenticate_account_id(1, "test-password")
            .unwrap()
            .unwrap();
        assert_eq!(account.name, "test-account");
        let knight = account
            .characters
            .iter()
            .find(|character| character.name == "Knight")
            .unwrap();
        let druid = account
            .characters
            .iter()
            .find(|character| character.name == "Druid")
            .unwrap();
        assert_eq!(knight.town_id, 42);
        assert_eq!(knight.progression.vocation, VocationId::default());
        assert_eq!(druid.progression.vocation, VocationId::new(4));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stable_cli_experience_award_honors_legacy_config_stage_precedence() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-stage-cli-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        fs::write(
            directory.join("config.lua"),
            format!(
                "{}rateExp = 5\nexperienceStages = {{ {{ minlevel = 1, multiplier = 2 }} }}\n",
                template(profile_by_id("fe-7.4").unwrap())
            ),
        )
        .unwrap();
        account_command(&[
            "account".into(),
            "create".into(),
            directory.display().to_string(),
            "stage-account".into(),
            "stage-password".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Sorcerer".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "experience".into(),
            directory.display().to_string(),
            "1".into(),
            "10".into(),
        ])
        .unwrap();

        let config = load(&directory).unwrap();
        let database = EngineDatabase::open(&config.database_path).unwrap();
        let character = database.player_by_id(1).unwrap();
        assert_eq!(character.experience, 4_220);
        assert_eq!(character.level, 8);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stable_cli_stack_transfers_persist_the_authoritative_inventory_snapshot() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-stack-cli-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        account_command(&[
            "account".into(),
            "create".into(),
            directory.display().to_string(),
            "stack-account".into(),
            "stack-password".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Paladin".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "equip".into(),
            directory.display().to_string(),
            "1".into(),
            "right".into(),
            "2148".into(),
            "40".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "container-create".into(),
            directory.display().to_string(),
            "1".into(),
            "0".into(),
            "1988".into(),
            "5".into(),
            "Backpack".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "container-add".into(),
            directory.display().to_string(),
            "1".into(),
            "0".into(),
            "2148".into(),
            "10".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "container-stow-stack".into(),
            directory.display().to_string(),
            "1".into(),
            "right".into(),
            "0".into(),
            "15".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "container-equip-stack".into(),
            directory.display().to_string(),
            "1".into(),
            "0".into(),
            "0".into(),
            "right".into(),
            "20".into(),
        ])
        .unwrap();

        let config = load(&directory).unwrap();
        let database = EngineDatabase::open(&config.database_path).unwrap();
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .unwrap()
                .count,
            45
        );
        assert_eq!(
            database
                .player_containers(1)
                .unwrap()
                .container(0)
                .unwrap()
                .items
                .item(0)
                .unwrap()
                .count,
            5
        );
        assert!(player_command(&[
            "player".into(),
            "container-equip-stack".into(),
            directory.display().to_string(),
            "1".into(),
            "0".into(),
            "0".into(),
            "right".into(),
            "0".into(),
        ])
        .is_err());
        assert_eq!(
            database
                .player_equipment(1)
                .unwrap()
                .item(EquipmentSlot::RightHand)
                .unwrap()
                .count,
            45
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stable_cli_magic_mana_awards_persist_configured_vocation_progress() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-magic-mana-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        let config_path = directory.join("config.lua");
        fs::write(
            &config_path,
            format!(
                "{}\nrateMagic = 2\n",
                fs::read_to_string(&config_path).unwrap()
            ),
        )
        .unwrap();
        let vocations = directory.join("data/XML/vocations.xml");
        fs::create_dir_all(vocations.parent().unwrap()).unwrap();
        fs::write(
            &vocations,
            r#"<vocations>
  <vocation id="1" name="Sorcerer" manamultiplier="1.000" gainhpticks="1" gainhpamount="0" gainmanaticks="1" gainmanaamount="0" gainsoulticks="1">
    <skill id="0" multiplier="1.000"/>
    <skill id="1" multiplier="1.000"/>
    <skill id="2" multiplier="1.000"/>
    <skill id="3" multiplier="1.000"/>
    <skill id="4" multiplier="1.000"/>
    <skill id="5" multiplier="1.000"/>
    <skill id="6" multiplier="1.000"/>
  </vocation>
</vocations>"#,
        )
        .unwrap();
        account_command(&[
            "account".into(),
            "create".into(),
            directory.display().to_string(),
            "magic-account".into(),
            "magic-password".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Sorcerer".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "vocation".into(),
            directory.display().to_string(),
            "1".into(),
            "1".into(),
        ])
        .unwrap();
        for awarded_mana in ["400", "400"] {
            player_command(&[
                "player".into(),
                "magic-mana".into(),
                directory.display().to_string(),
                "1".into(),
                awarded_mana.into(),
            ])
            .unwrap();
        }

        let config = load(&directory).unwrap();
        let database = EngineDatabase::open(&config.database_path).unwrap();
        let character = database.player_by_id(1).unwrap();
        assert_eq!(character.vitals.magic_level, 1);
        assert_eq!(character.progression_attempts.magic_mana(), 0);
        assert!(player_command(&[
            "player".into(),
            "magic-mana".into(),
            directory.display().to_string(),
            "1".into(),
            "invalid".into(),
        ])
        .is_err());
        assert_eq!(database.player_by_id(1).unwrap().vitals.magic_level, 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stable_cli_skill_tries_awards_persist_configured_vocation_progress() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("forgotten-engine-skill-tries-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        let config_path = directory.join("config.lua");
        fs::write(
            &config_path,
            format!(
                "{}\nrateSkill = 2\n",
                fs::read_to_string(&config_path).unwrap()
            ),
        )
        .unwrap();
        let vocations = directory.join("data/XML/vocations.xml");
        fs::create_dir_all(vocations.parent().unwrap()).unwrap();
        fs::write(
            &vocations,
            r#"<vocations>
  <vocation id="1" name="Knight" manamultiplier="1.000" gainhpticks="1" gainhpamount="0" gainmanaticks="1" gainmanaamount="0" gainsoulticks="1">
    <skill id="0" multiplier="1.000"/>
    <skill id="1" multiplier="1.000"/>
    <skill id="2" multiplier="1.000"/>
    <skill id="3" multiplier="1.000"/>
    <skill id="4" multiplier="1.000"/>
    <skill id="5" multiplier="1.000"/>
    <skill id="6" multiplier="1.000"/>
  </vocation>
</vocations>"#,
        )
        .unwrap();
        account_command(&[
            "account".into(),
            "create".into(),
            directory.display().to_string(),
            "skill-account".into(),
            "skill-password".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "create".into(),
            directory.display().to_string(),
            "1".into(),
            "Knight".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "vocation".into(),
            directory.display().to_string(),
            "1".into(),
            "1".into(),
        ])
        .unwrap();
        player_command(&[
            "player".into(),
            "skill-tries".into(),
            directory.display().to_string(),
            "1".into(),
            "sword".into(),
            "25".into(),
        ])
        .unwrap();

        let config = load(&directory).unwrap();
        let database = EngineDatabase::open(&config.database_path).unwrap();
        let character = database.player_by_id(1).unwrap();
        assert_eq!(
            character.progression.skills.skill(PlayerSkill::Sword),
            SkillProgress::new(11, 0).unwrap()
        );
        assert_eq!(
            character
                .progression_attempts
                .skill_tries(PlayerSkill::Sword),
            0
        );
        assert!(player_command(&[
            "player".into(),
            "skill-tries".into(),
            directory.display().to_string(),
            "1".into(),
            "sword".into(),
            "invalid".into(),
        ])
        .is_err());
        assert_eq!(
            database
                .player_by_id(1)
                .unwrap()
                .progression
                .skills
                .skill(PlayerSkill::Sword),
            SkillProgress::new(11, 0).unwrap()
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn parallel_native_startup_content_load_is_reproducible_and_ordered() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("forgotten-engine-parallel-load-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        write_template(&directory, profile_by_id("fe-7.4").unwrap()).unwrap();
        fs::create_dir_all(directory.join("data/world")).unwrap();
        fs::write(
            directory.join("data/world/forgotten.femap"),
            "format=fe-map-v1\nspawn=100,100,7\nfill=99,99,101,101,7,0,true\n",
        )
        .unwrap();
        let config = load(&directory).unwrap();
        let world_map = load_world_map(&config).unwrap();
        let first = load_independent_native_startup_content(&config, &world_map).unwrap();
        let second = load_independent_native_startup_content(&config, &world_map).unwrap();
        assert_eq!(first.companions, second.companions);
        assert_eq!(first.entity_catalog, second.entity_catalog);
        assert_eq!(first.vocation_registry, second.vocation_registry);
        assert_eq!(
            first.declarative_weapon_catalog,
            second.declarative_weapon_catalog
        );
        assert_eq!(
            first.declarative_spell_catalog,
            second.declarative_spell_catalog
        );

        fs::create_dir_all(directory.join("data/XML")).unwrap();
        fs::create_dir_all(directory.join("data/weapons")).unwrap();
        fs::write(
            directory.join("data/XML/vocations.xml"),
            "<vocations><vocation",
        )
        .unwrap();
        fs::write(
            directory.join("data/weapons/forgotten-engine-weapons.xml"),
            "<weapons><weapon",
        )
        .unwrap();
        let error = load_independent_native_startup_content(&config, &world_map)
            .unwrap_err()
            .to_string();
        assert!(error.contains("vocations.xml"), "unexpected error: {error}");
        let _ = fs::remove_dir_all(directory);
    }
}
