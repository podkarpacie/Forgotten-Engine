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
| Items and world interaction | 17% | 1% | Runtime item instances, equipment, owned containers, a bounded equipment-to-container transfer, persistence, and catalog validation exist. Native inventory synchronization, ground transfer, item actions, and map interactions are not complete. |
| Creatures, NPCs, spawns, loot, and corpses | 13% | 1% | FE has catalog parsing, static spawn materialization, occupancy, deterministic bounded movement, and caller-triggered reset foundations. AI, targeting, combat, timed respawns, loot, corpses, and NPC runtime are absent. |
| Lua, social systems, economy, housing, and administration | 12% | 0% | Registry references are audited, but Lua execution and the main social, economy, housing, and moderation systems are absent. |
| Production operations and compatibility evidence | 15% | 1% | Releases, checksums, validation scripts, and documented concurrency boundaries exist. Load, soak, security, migration, cross-platform, and independent real-client evidence are incomplete. |
| **Total** | **100%** | **22%** | **Foundation and bounded-runtime stage; not production-ready.** |

## Full path to 100%

Each milestone below has an evidence target. Work may proceed in dependency order, but a milestone is not complete merely because an API or parser exists. Existing completed items are retained to show the verified base for the next work.

### Milestone A — Complete bounded player lifecycle and combat foundations: target 30%

- [x] Apply `rateExp` and bounded `stages.xml` ranges to authoritative experience awards.
- [x] Award configured vocation-based fist tries from supported fixed melee events.
- [x] Persist authoritative condition state, including exact elapsed interval remainder, across restart.
- [x] Activate persisted server-side death state for lethal bounded condition damage at a validated assigned town temple.
- [ ] Define typed damage classes, defensive values, attack timing, cooldown state, and bounded combat-event contracts.
- [ ] Add data-driven weapon use and vocation formulas without executing scripts or claiming unverified TFS behavior.
- [ ] Add profile-gated spell definitions, cooldowns, mana use, target validation, and effects only after packet evidence.
- [ ] Deliver verified client-visible condition health, death, temple-respawn, and approved loss state for every supported profile.
- [ ] Add PvP legality, skulls, frags, death/corpse ownership rules, and explicit policy configuration.

### Milestone B — Complete authoritative items and world interaction: target 43%

- [x] Establish validated runtime items, equipment, owned containers, and bounded equipment-to-container transfer.
- [ ] Synchronize inventory, equipment, and containers in each supported native session profile.
- [ ] Implement authoritative item transfer, stack splitting/merging, capacity, and ownership rules across equipment, ground, containers, depot, and inbox surfaces.
- [ ] Implement verified item use, use-with, move, look, doors, levers, switches, teleporters, and simple map actions.
- [ ] Add depot, mail, and ground/container persistence with consistency and crash-recovery tests.
- [ ] Validate legacy OTB/OTBM content mappings against active item/action runtime behavior without importing private data.

### Milestone C — Complete creature, NPC, spawn, loot, and corpse runtime: target 58%

- [x] Establish bounded static spawn materialization, occupancy, safe deterministic movement, and reset foundations.
- [ ] Add deterministic monster/NPC state, spawn timing, movement, target selection, pathfinding boundaries, and combat participation.
- [ ] Add death, corpse, loot, decay, and respawn runtime behavior with owner and persistence rules.
- [ ] Add bounded NPC dialogue, shops, travel, and typed event boundaries.
- [ ] Validate imported private monster, NPC, and spawn content against the active runtime and report unsupported fields precisely.

### Milestone D — Complete scripting, social, economy, housing, and administration: target 73%

- [ ] Add sandboxed Lua execution with permission limits, typed API contracts, time/resource limits, deterministic failure behavior, and safe script loading.
- [ ] Add typed creature, player, item, movement, combat, and global event dispatch with compatibility adapters where evidence supports them.
- [ ] Add parties, channels, private messages, moderation controls, and persistence.
- [ ] Add guild baseline, banking/trading baseline, houses, beds, rent, access lists, depots, and operator administration APIs.
- [ ] Add audited migration adapters for supported TFS configuration and data surfaces, with explicit exclusions for unsafe or unimplemented scripting behavior.

### Milestone E — Complete profile-driven protocol and client compatibility: target 87%

- [ ] Finish profile-driven native protocol coverage for the declared FE 7.4 surface and resolve every release-blocking real-client parser or state mismatch with captured-safe evidence.
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

The next implementation slice is **typed bounded combat events and attack timing**. It will extend the current fixed selected-player melee foundation through explicit data types and deterministic server-side timing. It will not claim weapon, spell, PvP, or full TFS formula parity until each has its own tested runtime and profile-specific evidence.
