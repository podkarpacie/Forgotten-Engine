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
| FE 8.0 native profile boundary | A protocol-800 configuration is classified as requiring RSA/XTEA and refuses native enablement with an actionable diagnostic. The protocol unit suite may encode a non-empty bounded outbound XTEA envelope and extract only a marker/four-word key prefix from an already decrypted fixed test block. | This proves only safe non-enablement and isolated transport/bootstrap primitives. It does not prove client-input or RSA decryption, credentials, encrypted login, game sessions, packet layouts, listener operation, or client interoperability. |

## Required manual unmodified-OTClientV8 checks

Use a lawful, unmodified OTClientV8 build configured for the selected 740 profile and your own matching data. Record the engine commit, client revision, profile configuration, map/item-data fingerprint, OS, and outcome for every run. Never include credentials or raw packet bodies in the report.

| Scenario | Expected evidence today | Failure rule |
|---|---|---|
| Login and character list | A numeric local account reaches its owned character list. | Any disconnect, parser error, or character mismatch is a release blocker. |
| Game entry and map | The selected character joins the native 740 map with valid position, visible player state, and non-negative HUD values. | A black viewport, missing local player, invalid HUD values, or return to character list is a blocker. |
| Player-stats framing | The initial `0xA0` player-stats record parses without end-of-input errors and reports the persisted level, health, mana, capacity, magic level, and bounded zero soul value. | Any `0xA0` parser EOF, shifted next opcode, or invalid HUD value is a release blocker. |
| Quest Log acknowledgement | Opening Quest Log sends one parser-shaped empty `0xF0` response and leaves the native session connected. | A parser error, disconnect, or claimed quest content is a release blocker. |
| Outfit dialog and persistence | Configure an inclusive valid classic range with `otclientV8OutfitFirstLookType` and `otclientV8OutfitLastLookType`, open Set Outfit, and verify the native `0xD2` request receives one classic `0xC8` dialog containing the current appearance and exactly that range. Select an in-range concrete look and colors, relog, and confirm the stored appearance is visible after map initialization. Attempt an out-of-range look and confirm the existing stored appearance remains unchanged. | A parser error, disconnect, missing dialog, range that excludes the current look, lost stored colors, accepted out-of-range look, unexpected newer-format fields, missing operator-owned assets, or acceptance of an unsupported look type is a release blocker. |
| Position persistence | Move through the supported native path, exit normally or close the client connection, relog, and confirm the map initializes at the saved tile. | A reset to spawn, stale coordinate, parser error, or disconnect is a release blocker. |
| Equipment bootstrap | Provision catalog-mapped equipment in more than one fixed slot, log in, and confirm every mapped item is visible in its matching equipment slot. | A parser error, disconnect, missing mapped item, or displayed item without a validated client mapping is a release blocker. |
| Container bootstrap | Provision one top-level persisted container whose container item and contents have validated client mappings, log in, and confirm it opens with the expected title, capacity, and items. | A parser error, disconnect, missing mapped window, a nested window, or a displayed item without a validated mapping is a release blocker. |
| Static due-reactivation | With one imported static entity configured with a nonzero legacy spawn interval, start one native game host, deactivate the entity, wait the exact interval, and confirm the host-owned shared heartbeat produces one map refresh at the unoccupied spawn tile. | Early reactivation, missed elapsed reactivation, a spawn-tile overlap, stale viewport, parser error, disconnect, or shared-time acceleration from additional sessions is a release blocker. |
| Static target selection | Select one visible active static entity, verify it becomes the player’s target, then deactivate it and verify the target clears. Attempting to follow a static entity must not create movement or follow state. | A parser error, disconnect, stale target after deactivation, static follow state, or any unclaimed combat/movement side effect is a release blocker. |
| Static health display | Start with a visible imported static entity, verify its configured percentage is sent after login, update the authoritative display percentage, and verify one visibility refresh carries the changed percentage. Verify zero health does not deactivate it and a legitimate reactivation restores the configured percentage. | A parser error, disconnect, stale health, an invalid percentage, unexpected deactivation, client-driven health mutation, or any damage/death/combat side effect is a release blocker. |
| Static selected melee | Select an active adjacent static entity and wait for the native heartbeat. Verify one fixed 10-point hit per authoritative world tick refreshes visible health, while rapid repeated input before the next tick causes no extra damage. A hit that reaches zero clears the target and removes the entity from the active map view. Confirm later configured reactivation restores the imported percentage. | A parser error, disconnect, accelerated damage, damage outside adjacent selected targets, missing visibility refresh, a stale target, client-supplied damage, loot/corpse/reward creation, AI behavior, or failure to restore configured health on reactivation is a release blocker. |
| Movement | Cardinal and diagonal movement, turns, click-walk replacement, manual interruption, map edges, and reconnects remain responsive under normal and rapid input. | Teleporting, desynchronization, unintended rotation, disconnect, or sustained input lag is a blocker. |
| Shared sessions | Two players can join, see movement/leave updates, and retain independent authoritative positions. | Missing, stale, or duplicate player state is a blocker. |
| Native 740 chat boundary | Send bounded public-chat input and confirm the session remains connected without a parser error. Client-visible server chat remains intentionally deferred because unmodified OTCv8 has no message-mode map below protocol 760. | Any parser-invalid `0xAA` or generic text-message output, disconnect, or claim that visible native 740 chat is supported is a release blocker. |
| Lifecycle foundations | Regeneration, selected melee, hydrated conditions, death-state activation with a valid town/temple, and persistence across expected supported boundaries behave exactly as documented. | Any behavior outside the documented boundary must be reported as a defect or deferred—not accepted as parity. |
| Imported private content | `tfs-audit` and `validate` identify imported map/config/item/registry findings without executing scripts. | Audit errors, missing required files, or a false “compatible” claim is a blocker. |
| Deferred script boundary | `tfs-audit` may report typed category/count metadata through the no-op dispatcher, but it must not read a script path or body, load Lua, execute a callback, or change world state. | Any script file access, runtime execution, private source disclosure, or gameplay side effect is a release blocker. |

## Production-release blockers

The following systems are still missing or only foundational and therefore block any claim that FE is a production-grade replacement for TFS:

| Blocking area | Current status |
|---|---|
| Full protocol coverage | Only the experimental profile-driven native 740 route is partially implemented; FE 8.0 and 1.2 remain foundations. |
| Native 740 player-stats confirmation | The local encoder has a parser-shaped 16-bit level and soul byte contract, including a regression that keeps persisted `42 / 50` mana separate from `32,000` capacity. A real unmodified-OTClientV8 run has not yet confirmed the reported initial `0xA0` EOF and implausible mana display are resolved. |
| Native 740 Quest Log confirmation | The local decoder and host emit a parser-shaped empty response with a zero quest count. A real unmodified-OTClientV8 run has not yet confirmed the Quest Log window opens without disconnecting. |
| Native 740 outfit confirmation | Schema-v11 migrations, persistence, matching-look hydration, and host-session regression tests are present. A real unmodified-OTClientV8 relog has not yet confirmed the stored appearance is rendered without a parser error. |
| Native 740 position confirmation | Automated movement plus orderly and abrupt-disconnect native-session relog coverage restores the persisted destination. A real unmodified-OTClientV8 relog has not yet confirmed the reported spawn reset is resolved. |
| Native 740 container confirmation | Parser-shaped top-level mapped container bootstrap coverage is present. A real unmodified-OTClientV8 run has not yet confirmed the window opens without a parser error. |
| Static due-reactivation confirmation | Legacy per-spawn interval propagation, one native host-owned shared-clock step, core due-timer/occupancy coverage, and synchronized visibility-epoch coverage are present. End-to-end service-lifecycle testing and real unmodified-client observation are still required before claiming timed respawn support. |
| Core gameplay | Formula combat, weapons, spells, combat rules, skills awarded by gameplay, experience stages/rates, soul, PvP rules, and skull/frags are not complete. |
| World simulation | Monster AI, NPC behavior, spawns, respawns, loot, corpses, actions, doors, levers, housing, guilds, and economy systems are not complete. |
| Scripting | Lua is not a general runtime; referenced scripts are audited but not executed. |
| Lifecycle delivery | Condition visual effects, death screens, automatic respawn packets, and the default loss formula remain deferred. Bounded condition schedule interval progress is persisted and restored; lethal condition damage activates and persists server-side death state only for a validated assigned town temple, but client-visible effect and death delivery are not implemented. |
| Operational validation | Sustained multi-player load, soak, fault recovery, backup restoration under load, Windows release smoke tests, and independent real-client interoperability testing have not established a production SLO. |

## Decision rule

Do **not** market, publish, or deploy FE as a general-purpose production TFS replacement until every required gameplay and operational scope item has implementation evidence, automated regression coverage, profile-specific real-client tests, and documented performance/reliability acceptance thresholds. Until then, use it only for controlled local development and compatibility testing of the explicitly supported paths.
