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
| Player-stats framing | The initial `0xA0` player-stats record parses without end-of-input errors and reports the persisted level, health, mana, capacity, magic level, and bounded zero soul value. | Any `0xA0` parser EOF, shifted next opcode, or invalid HUD value is a release blocker. |
| Quest Log acknowledgement | Opening Quest Log sends one parser-shaped empty `0xF0` response and leaves the native session connected. | A parser error, disconnect, or claimed quest content is a release blocker. |
| Outfit persistence | Change accepted color values through the native outfit dialog, relog the same character, and confirm the stored look is visible after map initialization. | A parser error, disconnect, lost stored colors, or acceptance of an unsupported look type is a release blocker. |
| Position persistence | Move through the supported native path, exit normally or close the client connection, relog, and confirm the map initializes at the saved tile. | A reset to spawn, stale coordinate, parser error, or disconnect is a release blocker. |
| Equipment bootstrap | Provision catalog-mapped equipment in more than one fixed slot, log in, and confirm every mapped item is visible in its matching equipment slot. | A parser error, disconnect, missing mapped item, or displayed item without a validated client mapping is a release blocker. |
| Container bootstrap | Provision one top-level persisted container whose container item and contents have validated client mappings, log in, and confirm it opens with the expected title, capacity, and items. | A parser error, disconnect, missing mapped window, a nested window, or a displayed item without a validated mapping is a release blocker. |
| Static due-reactivation | With one imported static entity configured with a nonzero legacy spawn interval, deactivate it, advance the authoritative shared world by the exact interval, invoke the explicit due-reactivation path, and confirm one map refresh makes it visible at its unoccupied spawn tile. | Early reactivation, missed elapsed reactivation, a spawn-tile overlap, stale viewport, parser error, or disconnect is a release blocker. |
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
| Native 740 player-stats confirmation | The local encoder now has a parser-shaped 16-bit level and soul byte regression contract. A real unmodified-OTClientV8 run has not yet confirmed the reported initial `0xA0` EOF is resolved. |
| Native 740 Quest Log confirmation | The local decoder and host emit a parser-shaped empty response with a zero quest count. A real unmodified-OTClientV8 run has not yet confirmed the Quest Log window opens without disconnecting. |
| Native 740 outfit confirmation | Schema-v11 migrations, persistence, matching-look hydration, and host-session regression tests are present. A real unmodified-OTClientV8 relog has not yet confirmed the stored appearance is rendered without a parser error. |
| Native 740 position confirmation | Automated movement plus orderly and abrupt-disconnect native-session relog coverage restores the persisted destination. A real unmodified-OTClientV8 relog has not yet confirmed the reported spawn reset is resolved. |
| Native 740 container confirmation | Parser-shaped top-level mapped container bootstrap coverage is present. A real unmodified-OTClientV8 run has not yet confirmed the window opens without a parser error. |
| Static due-reactivation confirmation | Legacy per-spawn interval propagation, core due-timer/occupancy coverage, and synchronized visibility-epoch coverage are present. A shared authoritative clock invocation policy and real unmodified-client observation are still required before claiming timed respawn support. |
| Core gameplay | Formula combat, weapons, spells, combat rules, skills awarded by gameplay, experience stages/rates, soul, PvP rules, and skull/frags are not complete. |
| World simulation | Monster AI, NPC behavior, spawns, respawns, loot, corpses, actions, doors, levers, housing, guilds, and economy systems are not complete. |
| Scripting | Lua is not a general runtime; referenced scripts are audited but not executed. |
| Lifecycle delivery | Condition visual effects, death screens, automatic respawn packets, and the default loss formula remain deferred. Bounded condition schedule interval progress is persisted and restored; lethal condition damage activates and persists server-side death state only for a validated assigned town temple, but client-visible effect and death delivery are not implemented. |
| Operational validation | Sustained multi-player load, soak, fault recovery, backup restoration under load, Windows release smoke tests, and independent real-client interoperability testing have not established a production SLO. |

## Decision rule

Do **not** market, publish, or deploy FE as a general-purpose production TFS replacement until every required gameplay and operational scope item has implementation evidence, automated regression coverage, profile-specific real-client tests, and documented performance/reliability acceptance thresholds. Until then, use it only for controlled local development and compatibility testing of the explicitly supported paths.
