# Forgotten Engine Completion Ledger

## Purpose

This is a **scope-progress ledger**, not a production-readiness score. A system receives credit only when it has an authoritative runtime path, regression coverage, clear capability reporting, and profile-specific evidence where a client or legacy format is involved.

The current conservative estimate is **18% complete by total replacement scope**. This matches the present state: FE has strong foundations, but many of the main TFS gameplay systems do not exist yet.

> Reaching more than 50% here will mean that more than half of the defined FE 7.4 replacement scope has a tested runtime implementation. It will **not** mean that FE is production-ready or a complete TFS replacement.

## Current weighted position

| System category | Weight | Current credited scope | Why it is not higher |
|---|---:|---:|---|
| Core architecture, configuration, persistence, and conversion | 18% | 12% | FE has Rust workspace foundations, SQLite persistence, bounded TFS-style config/content discovery, and map import boundaries. Complete TFS configuration and data runtime behavior is not present. |
| Native 7.4 client/session path | 12% | 3% | The native OTCv8 740 route covers the experimental login/session, maps, movement, visibility, and bounded chat path. Full protocol support is missing. |
| Player progression, lifecycle, and combat | 13% | 2% | Vitals, regeneration, conditions, selected-player fixed melee, death-state, loss, and respawn foundations exist. Formula gameplay and client delivery are incomplete. |
| Items and world interaction | 17% | 0% | Item data, equipment, containers, and maps are stored or parsed, but runtime item transfer, actions, and interaction behavior are not complete. |
| Monsters, NPCs, spawns, loot, and corpses | 13% | 0% | Catalogs and static-render foundations exist. Runtime AI, combat, spawn, loot, and corpse behavior do not. |
| Lua, social systems, economy, housing, and administration | 12% | 0% | Script references are audited. Safe execution and the main social/economy systems are absent. |
| Production operations and compatibility evidence | 15% | 1% | Releases, checksums, a validation script, and concurrency boundaries exist. Production load, reliability, security, migration, and independent client evidence are absent. |
| **Total** | **100%** | **18%** | **Foundation stage; not production-ready.** |

## Remaining checklist to reach more than 50%

### Milestone A — Player progression and lifecycle delivery: target 23%

- [ ] Apply `rateExp` and bounded `stages.xml` values when FE grants experience.
- [ ] Award vocation-based skill tries from supported melee events.
- [ ] Add client-visible health updates for condition ticks.
- [ ] Deliver verified death, temple-respawn, and loss state to the native client.
- [ ] Persist only lifecycle data whose exact reload behavior is defined and tested.

### Milestone B — Basic combat runtime: target 29%

- [ ] Build typed damage types, attack timing, defensive values, and bounded combat events.
- [ ] Add data-driven weapon use and vocation formulas without silently executing scripts.
- [ ] Add profile-gated spell definitions, cooldowns, mana use, target validation, and effects only after packet evidence.
- [ ] Add PvP legality, skull/frags, death/corpse ownership rules, and explicit policy configuration.

### Milestone C — Items and map actions: target 36%

- [ ] Synchronize inventory, equipment, and containers in native 740 sessions.
- [ ] Implement authoritative item transfer and stack rules.
- [ ] Implement item use, move, look, doors, levers, switches, teleporters, and simple map actions.
- [ ] Add depot and ground/container ownership rules.

### Milestone D — Creature and NPC world runtime: target 44%

- [ ] Add deterministic monster state, spawn timing, movement, targeting, and combat participation.
- [ ] Add death, corpse, loot, and respawn runtime behavior.
- [ ] Add bounded NPC dialogue, shops, travel, and scripts through typed events.
- [ ] Validate imported private monster/NPC/spawn content against active runtime capabilities.

### Milestone E — Scripted, social, and administrative core: target 48%

- [ ] Add a sandboxed Lua runtime with permission limits, typed API contracts, time/resource limits, and deterministic failure handling.
- [ ] Add parties, channels, private messages, and moderation controls.
- [ ] Add guild baseline, banking/trading baseline, houses/access lists, and operator administration APIs.

### Milestone F — Evidence and reliability gates: target 51%+

- [ ] Run repeatable two-player and multi-player OTCv8 tests for every supported 740 feature.
- [ ] Add command-batch ordering tests and concurrency stress tests.
- [ ] Establish load, soak, memory, fault-recovery, backup/restore, and security acceptance thresholds.
- [ ] Smoke-test Windows and Linux release artifacts before every release.
- [ ] Run migration tests against lawful private TFS-style directories and record only safe summaries.

## Rule for counting progress

A checklist item can be marked complete only after all of the following are true:

1. The runtime behavior is implemented in FE.
2. Automated regression tests pass.
3. The capability matrix states the exact supported boundary.
4. Any affected profile has the required protocol/client evidence.
5. Deferred behavior remains visible in the documentation and does not pretend to work.

## Current next slice

The next implementation slice is **authoritative experience award processing**. It will use the existing configuration parser for flat experience rates and ordered stages. It will not claim weapon, spell, or full TFS formula compatibility until those event paths exist and are tested.
