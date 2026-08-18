# Forgotten Engine Completion Ledger

## Purpose

This ledger measures **declared replacement scope**. It is not a schedule, marketing score, or promise that a partly implemented system is compatible. Credit is earned only when FE has an authoritative runtime path, automated regression coverage, precise capability reporting, and the required profile-specific evidence.

> **A 100% ledger result is a strict production-readiness decision.** It requires complete declared FE scope, validated legacy-content migration boundaries, real-client interoperability evidence for every supported profile, and operational acceptance evidence. Until then, FE remains a controlled development and compatibility-testing system.

## Current weighted position

The current conservative estimate is **22% of the declared replacement scope**. The increase from the original foundation estimate reflects tested runtime progression, lifecycle, condition persistence, server-side condition death activation, static-world behavior, and bounded item transfer. It does not credit unsupported client delivery, gameplay parity, Lua execution, or production operations.

| System category | Weight | Current credited scope | Why it is not higher |
|---|---:|---:|---|
| Core architecture, configuration, persistence, and conversion | 18% | 13% | FE has Rust workspace foundations, SQLite migration discipline, bounded TFS-style configuration/content discovery, OTBM audit/import boundaries, and deterministic state persistence. Complete TFS configuration, database, and content-runtime behavior is not present. |
| Native profile session and protocol compatibility | 12% | 3% | The experimental profile-driven native OTCv8 740 path covers login, character selection, map delivery, movement, visibility, bounded chat, and partial state refresh. Full protocol coverage and stable client evidence are incomplete. |
| Player progression, lifecycle, and combat | 13% | 3% | FE has persisted vitals, regeneration, configured experience/skill foundations, bounded conditions, exact condition restart progress, server-side death state, loss/respawn foundations, and selected-player fixed melee. Formula combat and client lifecycle delivery are incomplete. |
| Items and world interaction | 17% | 1% | Runtime item instances, equipment, owned containers, bidirectional complete-item transfers between equipment and owned top-level containers, persistence, catalog validation, and mapped equipment deltas exist. Ground transfer, item actions, stacks, capacity, container deltas, and map interactions are not complete. |
| Creatures, NPCs, spawns, loot, and corpses | 13% | 1% | FE has catalog parsing, static spawn materialization, occupancy, deterministic bounded movement, and caller-triggered reset foundations. AI, targeting, combat, timed respawns, loot, corpses, and NPC runtime are absent. |
| Lua, social systems, economy, housing, and administration | 12% | 0% | Registry references are audited, but Lua execution and the main social, economy, housing, and moderation systems are absent. |
| Production operations and compatibility evidence | 15% | 1% | Releases, checksums, validation scripts, and documented concurrency boundaries exist. Load, soak, security, migration, cross-platform, and independent real-client evidence are incomplete. |
| **Total** | **100%** | **22%** | **Foundation and bounded-runtime stage; not production-ready.** |

## Full path to 100%

Each milestone below has an evidence target. Work may proceed in dependency order, but a milestone is not complete merely because an API or parser exists. Existing completed items are retained to show the verified base for the next work.

### Milestone A — Complete bounded player lifecycle and combat foundations: target 30%

- [x] Apply `rateExp` and bounded `stages.xml` ranges to authoritative experience awards.
- [x] Apply matching bounded `vocations.xml` `gainhp`, `gainmana`, and `gaincap` values to the stable CLI's atomic authoritative experience-level-up persistence transaction; native HUD delivery and full advancement parity remain deferred.
- [x] Retain bounded nested TFS-style vocation formula defense and armor multipliers as fixed-point metadata without changing live mitigation behavior or claiming formula compatibility.
- [x] Retain bounded TFS-style vocation attack-speed and base-speed metadata with verified defaults, without changing live movement or combat timing behavior.
- [x] Award configured vocation-based fist tries from supported fixed melee events.
- [x] Persist authoritative condition state, including exact elapsed interval remainder, across restart.
- [x] Activate persisted server-side death state for lethal bounded condition damage at a validated assigned town temple.
- [x] Add an operator-controlled temple respawn that delegates to the authoritative dead-state transition and atomically persists validated position, vitals, and cleared lifecycle state; automatic client delivery remains deferred.
- [x] Apply explicit configured `deathLosePercent` values from `1` through `100` after supported native player-melee, condition, and static-creature heartbeat deaths, atomically persisting level, experience, vitals, visible skills, exact attempts, and marked lifecycle state; default formula, promotion, blessing, and client loss delivery remain deferred.
- [x] Define typed physical damage classification, bounded adjacent-melee event contracts, attack timing, and deterministic player cooldown state.
- [x] Decode and retain bounded native fight-mode, chase-mode, and secure-mode state with verified defaults, without attaching unverified combat, pursuit, persistence, packet, or UI behavior.
- [x] Append the verified three-byte native 740 player-modes record to initialization from authoritative fight-mode state; state-change delivery, later PvP-mode variants, and formula effects remain deferred.
- [x] Define bounded profile-neutral flat physical mitigation without silently inventing TFS formulas.
- [ ] Map verified profile-specific armor, shielding, equipment, weapon, and resistance formulas into the typed mitigation contract.
- [x] Parse bounded FE-owned scriptless weapon declarations that can construct typed physical adjacent-melee events.
- [x] Bind validated declarative weapons only to authoritative right-hand equipment on the profile-approved native selected-melee path, without executing scripts.
- [ ] Add verified profile-specific action paths and legacy-compatible semantics without claiming unverified TFS behavior.
- [x] Define typed bounded spell-cast resource and cooldown accounting with no packet, target, formula, effect, or script behavior.
- [x] Parse bounded FE-owned scriptless spell declarations that construct typed mana-and-cooldown events.
- [x] Establish a direct side-effect-free Lua expression sandbox with no standard libraries, typed primitive-only output, and bounded source/memory/instruction limits; imported script execution, callbacks, and APIs remain deferred.
- [x] Load the optional declarative spell catalog as immutable native-host input without enabling any client cast path.
- [x] Resolve a declared spell through synchronized host state for bounded mana and cooldown accounting only.
- [ ] Add profile-gated spell definitions, client request validation, target handling, formulas, effects, and safe script boundaries only after packet evidence.
- [ ] Deliver verified client-visible condition health, death, temple-respawn, and approved loss state for every supported profile.
- [ ] Add PvP legality, skulls, frags, death/corpse ownership rules, and explicit policy configuration.

### Milestone B — Complete authoritative items and world interaction: target 43%

- [x] Establish validated runtime items, equipment, owned containers, and bounded complete-item transfers between equipment and owned top-level containers.
- [x] Synchronize catalog-mapped authoritative equipment replacements to the selected native 740 session through deterministic parser-verified set/delete inventory deltas; item movement and real-client validation remain deferred.
- [x] Synchronize changed complete catalog-mapped top-level containers to native 740 sessions by re-emitting the existing parser-verified open-container record; close and item-delta behavior remain deferred.
- [x] Decode native 740 CloseContainer and return the parser-verified close record while filtering that top-level client view from later session refreshes without mutating shared or persisted container state; nesting, up-arrow, update, and item-delta controls remain deferred.
- [x] Expose existing complete-item equipment/container transactions through the shared world and advance both native refresh epochs only after core success; persistence and client routing remain deferred.
- [x] Retain bounded legacy `items.xml` armor, defense, extra-defense, attack-speed, and verified weapon-type metadata for known OTB items without changing live combat behavior or claiming formula compatibility.
- [ ] Synchronize inventory, equipment, and containers in each supported native session profile.
- [ ] Implement authoritative item transfer, stack splitting/merging, capacity, and ownership rules across equipment, ground, containers, depot, and inbox surfaces.
- [x] Define side-effect-free authoritative validation for same-tile or adjacent exact top-level map-item use intents.
- [x] Expose the side-effect-free map-item validation contract through synchronized host state.
- [x] Decode bounded native 740 UseItem input and route it into synchronized no-mutation map validation only after a unique validated client-to-server item mapping; missing or ambiguous mappings remain rejected and action behavior stays deferred.
- [ ] Implement verified item use, use-with, move, look, doors, levers, switches, teleporters, and simple map actions.
- [ ] Add depot, mail, and ground/container persistence with consistency and crash-recovery tests.
- [ ] Validate legacy OTB/OTBM content mappings against active item/action runtime behavior without importing private data.

### Milestone C — Complete creature, NPC, spawn, loot, and corpse runtime: target 58%

- [x] Establish bounded static spawn materialization, occupancy, safe deterministic movement, and reset foundations.
- [x] Add bounded deterministic static target selection that records the nearest living same-floor player under an explicit capped range, with stable tie-breaking and lifecycle cleanup; pursuit, combat, and client delivery remain deferred.
- [x] Add a caller-triggered single deterministic cardinal target step that delegates map and occupancy validation to the authoritative movement boundary; pursuit loops, routing, AI, combat automation, and client delivery remain deferred.
- [x] Route a real caller-triggered target step through the shared native-world visibility epoch and established full-map refresh; autonomous scheduling and real-client evidence remain deferred.
- [x] Add an explicit opt-in shared-heartbeat nearest-player target acquisition policy with capped range, deterministic ordering, and inert default heartbeat behavior; movement, combat, packets, and client target delivery remain deferred.
- [x] Add an explicit one-step static pursuit policy that selects a bounded nearest target then reuses authoritative map/occupancy movement once per active static creature; default scheduling, routing, combat, and client delivery remain deferred.
- [ ] Add deterministic monster/NPC state, spawn timing, movement, target selection, pathfinding boundaries, and combat participation.
- [ ] Add death, corpse, loot, decay, and respawn runtime behavior with owner and persistence rules.
- [ ] Add bounded NPC dialogue, shops, travel, and typed event boundaries.
- [ ] Validate imported private monster, NPC, and spawn content against the active runtime and report unsupported fields precisely.

### Milestone D — Complete scripting, social, economy, housing, and administration: target 73%

- [ ] Add sandboxed Lua event execution with permission limits, typed TFS/FE API contracts, safe script loading, compatibility adapters, and explicit event-family semantics. The completed expression sandbox is intentionally not an imported-script or callback runner.
- [ ] Add typed creature, player, item, movement, combat, and global event dispatch with compatibility adapters where evidence supports them.
- [ ] Add parties, channels, private messages, moderation controls, and persistence.
- [ ] Add guild baseline, banking/trading baseline, houses, beds, rent, access lists, depots, and operator administration APIs.
- [ ] Add audited migration adapters for supported TFS configuration and data surfaces, with explicit exclusions for unsafe or unimplemented scripting behavior.

### Milestone E — Complete profile-driven protocol and client compatibility: target 87%

- [ ] Finish profile-driven native protocol coverage for the declared FE 7.4 surface and resolve every release-blocking real-client parser or state mismatch with captured-safe evidence. The local 740 stats-refresh path now rehydrates a vocation-level-up from authoritative level, experience, and vitals, but still needs a native gameplay source and real-client confirmation.
- [ ] Implement and validate FE 8.0 profile runtime and protocol behavior without hardcoding release-specific assumptions.
- [ ] Implement and validate FE 1.2 / Tibia 10.98 profile runtime and protocol behavior without hardcoding release-specific assumptions.
- [ ] Add repeatable lawful unmodified OTCv8 tests for login, movement, state, chat, combat, inventory, interactions, lifecycle, and world simulation in every supported profile.
- [ ] Maintain versioned capability matrices and protocol fixtures that distinguish verified behavior from deferred behavior.

### Milestone F — Complete operations, performance, migration, security, and release evidence: target 100%

- [ ] Establish deterministic multithreaded execution boundaries with ordering, contention, race, and snapshot-consistency regression tests.
- [ ] Establish load, soak, memory, fault-recovery, backup/restore, migration, and security acceptance thresholds; satisfy them on supported Linux and Windows releases.
- [ ] Validate restores, schema upgrades, and lawful private TFS-style migration directories using safe aggregate reports only.
- [ ] Build, checksum, smoke-test, and retain reproducible Linux and Windows release artifacts for each supported profile.
- [ ] Run independent real-client interoperability and operator acceptance validation for all declared profile capabilities.
- [ ] Resolve every production release blocker, document supported operational limits, and complete a final security and reliability review before claiming production readiness.

## Evidence rule

A ledger item can be marked complete only when all applicable conditions are met.

| Evidence type | Required proof |
|---|---|
| Runtime behavior | The authoritative FE implementation performs the declared behavior, including defined failure handling. |
| Automated regression | Unit, integration, migration, and concurrency tests cover normal, boundary, and failure cases. |
| Compatibility boundary | The capability matrix states the profile, content adapter, supported behavior, and remaining deferrals. |
| Client evidence | Every client-facing change has reproducible lawful real-client evidence for the affected profile. |
| Operational evidence | Performance, reliability, security, backup, restore, migration, and release checks satisfy the declared acceptance criteria. |

## Current next slice

The next implementation slice is **native 740 level-up HUD delivery**, followed by **inventory synchronization**. The new core/persistence transaction must first be rendered through a profile-verified player-stats refresh without reintroducing the reported client parser error. Inventory work will then expose already authoritative equipment safely before broader transfer rules. Neither slice claims full formula, spell, PvP, or TFS parity.
