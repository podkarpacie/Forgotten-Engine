# Forgotten Engine Validation Baseline

## Readiness status

**FE is not production-ready.** The project has a tested implementation baseline, not a complete TFS replacement. The validation process is intentionally split between deterministic local checks and operator-run checks with the lawful local OTCv8 build and data profile that will actually be deployed.

> Passing this baseline proves only the currently implemented paths. It is not evidence of complete Tibia gameplay compatibility, full TFS data-pack compatibility, performance at production load, or readiness for public hosting.

## Repeatable automated validation

Run the following command from a clean checkout on every candidate build and after any behavior-changing merge:

```bash
bash scripts/validate-supported.sh
```

The script validates formatting, strict linting, all workspace unit and integration tests, the debug build, profile/config/database initialization, local provisioning, player-state commands, container/equipment persistence commands, backups, validation, and a bounded host-startup smoke check. It creates and removes an isolated temporary world; it neither requires nor reads operator data, client assets, maps, scripts, accounts, credentials, or live production ports beyond the temporary local host start.

| Area | Automated evidence | Current boundary |
|---|---|---|
| Rust workspace | Formatting, strict warnings-as-errors linting, full test suite, debug build | Does not benchmark load or prove platform-specific deployment behavior. |
| TFS-style configuration and SQLite | Temporary `init`, `validate`, `status`, key generation, backup, and local persistence commands | Does not validate an operator’s entire private TFS directory. |
| Character administration | Account creation; character, vocation, town, skill, equipment, and container commands | Does not prove gameplay semantics for the stored state. |
| Native host | Bounded process-startup check and all protocol/unit regressions in the workspace | Does not operate a real client or claim complete protocol/gameplay compatibility. |

## Required manual unmodified-OTClientV8 checks

Use a lawful, unmodified OTClientV8 build configured for the selected 740 profile and your own matching data. Record the engine commit, client revision, profile configuration, map/item-data fingerprint, OS, and outcome for every run. Never include credentials or raw packet bodies in the report.

| Scenario | Expected evidence today | Failure rule |
|---|---|---|
| Login and character list | A numeric local account reaches its owned character list. | Any disconnect, parser error, or character mismatch is a release blocker. |
| Game entry and map | The selected character joins the native 740 map with valid position, visible player state, and non-negative HUD values. | A black viewport, missing local player, invalid HUD values, or return to character list is a blocker. |
| Movement | Cardinal and diagonal movement, turns, click-walk replacement, manual interruption, map edges, and reconnects remain responsive under normal and rapid input. | Teleporting, desynchronization, unintended rotation, disconnect, or sustained input lag is a blocker. |
| Shared sessions | Two players can join, see movement/leave updates, and retain independent authoritative positions. | Missing, stale, or duplicate player state is a blocker. |
| Supported chat | Public chat reaches active sessions through the supported client-visible path without parser errors. | A client parser error or host session failure is a blocker. |
| Lifecycle foundations | Regeneration, selected melee, hydrated conditions, death-state activation with a valid town/temple, and persistence across expected supported boundaries behave exactly as documented. | Any behavior outside the documented boundary must be reported as a defect or deferred—not accepted as parity. |
| Imported private content | `tfs-audit` and `validate` identify imported map/config/item/registry findings without executing scripts. | Audit errors, missing required files, or a false “compatible” claim is a blocker. |

## Production-release blockers

The following systems are still missing or only foundational and therefore block any claim that FE is a production-grade replacement for TFS:

| Blocking area | Current status |
|---|---|
| Full protocol coverage | Only the experimental profile-driven native 740 route is partially implemented; FE 8.0 and 1.2 remain foundations. |
| Core gameplay | Formula combat, weapons, spells, combat rules, skills awarded by gameplay, experience stages/rates, soul, PvP rules, and skull/frags are not complete. |
| World simulation | Monster AI, NPC behavior, spawns, respawns, loot, corpses, actions, doors, levers, housing, guilds, and economy systems are not complete. |
| Scripting | Lua is not a general runtime; referenced scripts are audited but not executed. |
| Lifecycle delivery | Condition visual effects, condition-driven death activation, death screens, automatic respawn packets, and the default loss formula remain deferred. Bounded condition schedule interval progress is persisted and restored, but its client-visible effect delivery is not. |
| Operational validation | Sustained multi-player load, soak, fault recovery, backup restoration under load, Windows release smoke tests, and independent real-client interoperability testing have not established a production SLO. |

## Decision rule

Do **not** market, publish, or deploy FE as a general-purpose production TFS replacement until every required gameplay and operational scope item has implementation evidence, automated regression coverage, profile-specific real-client tests, and documented performance/reliability acceptance thresholds. Until then, use it only for controlled local development and compatibility testing of the explicitly supported paths.
