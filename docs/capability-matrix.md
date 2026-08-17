# Forgotten Engine Capability Matrix

This matrix is the source-controlled statement of **current behavior**, not an aspirational feature list. A capability marked **partial** is intentionally not equivalent to a supported gameplay system. Every release must update this document and the paired machine-readable JSON matrix before claiming broader compatibility.

## Status vocabulary

| Status | Meaning |
|---|---|
| `supported` | Implemented, tested, and available through the declared profile contract. |
| `partial` | A bounded foundation exists, but the complete gameplay or protocol behavior is not available. |
| `deferred` | Explicitly planned but not yet implemented. |
| `not-applicable` | Not part of the selected profile’s declared surface. |
| `unknown` | Requires compatibility research before implementation or claim. |

## Profile baseline

| Capability | `fe-7.4` — Tibia 7.4 | `fe-8.0` — Tibia 8.0 | `fe-1.2` — TFS 1.2 / Tibia 10.98 |
|---|---|---|---|
| Profile selection/configuration | supported | supported | supported |
| General frame/status service foundation | supported | supported | supported |
| Native unmodified OTClientV8 game path | partial — experimental 740 login, character selection, map, movement, shared-player visibility, bounded player-vitals updates, and a parser-shaped empty Quest Log acknowledgement. The 740 player-stats record has parser-shaped 16-bit level and soul fields, but unmodified-client confirmation remains required | deferred | deferred |
| Complete official-client protocol emulation | deferred | deferred | deferred |
| TFS-style `config.lua` discovery | partial — bounded assignment subset and safe defaults | partial — bounded assignment subset and safe defaults | partial — bounded assignment subset and safe defaults |
| OTBM map and companion discovery | partial — map, towns, houses, spawns, and legacy item metadata import boundary | partial — content audit only until profile mapping is verified | partial — content audit only until profile mapping is verified |
| Item type metadata | partial — load/validate map-facing legacy metadata | deferred | deferred |
| Runtime items, inventory, containers, equipment, depot, inbox | partial — bounded core item/equipment/container primitives, authoritative player equipment/container state, SQLite equipment/container persistence, native-session hydration, parser-verified catalog-mapped 740 equipment bootstrap, a non-emitting parser-verified container-open codec, and an operator command for atomic complete-item transfer from one equipment slot into one already owned bounded container; no native incremental inventory synchronization, map-ground transfer, recursive nesting, unsolicited live container windows, stack splitting, or item-use semantics | deferred | deferred |
| Map item use | partial — typed server-side validation accepts only same-tile or adjacent use of an exact top-level authoritative map item stack and returns immutable metadata without mutation; the same validation is available through synchronized host state. Action execution, doors, switches, containers, charges, Lua, persistence, and client item-use packets remain deferred | deferred | deferred |
| Accounts and character ownership | supported — local SQLite, Argon2 accounts, numeric native account flow | supported — local CLI/persistence foundation | supported — local CLI/persistence foundation |
| Player vitals | partial — persisted health, mana, capacity, and magic level; parser-shaped classic 740 HUD delivery with a 16-bit level and bounded soul byte. Unmodified-client confirmation remains required | deferred | deferred |
| Player outfit | partial — schema-v11 persists accepted matching classic-740 look type and color values, then hydrates them after map initialization with a configured look-type fallback for legacy or mismatched records. Look-type selection, addons, mounts, asset validation, and unmodified-client confirmation remain deferred | deferred | deferred |
| Death and respawn lifecycle | partial — server-side death/temple respawn/fixed-loss foundations plus schema-v9 persisted dead/respawn/loss state and strict native hydration; client death screens, automatic timers, teleport packets, default-loss formula, and blessings/promotions remain deferred | deferred | deferred |
| Typed skill progression, experience and vocation rules | partial — typed seven-skill levels/percentages and numeric vocation identity are authoritative, SQLite-persisted, locally provisionable, and delivered in classic 740 bootstrap/refresh records; operator-owned TFS-style `data/XML/vocations.xml` supplies profile-gated online health/mana regeneration and vocation skill multipliers, while bounded `data/XML/stages.xml` plus `rateExp` can drive a deterministic authoritative operator experience award with SQLite persistence; each successful fixed selected-player melee hit can award one configured fist try with exact persisted counters; hydrated poison/burning/energy schedules update native vitals, schema-v10 restores each schedule’s validated elapsed interval remainder across restart, and lethal bounded condition damage activates the existing persisted server-side death state only at a validated assigned town temple; weapon/spell formulas, combat/quest/monster reward sources, condition effect packets, client death screens/timers/respawn packets, soul, promotion runtime, and full TFS-compatible experience calculation remain deferred | deferred | deferred |
| Movement and collision | partial — authoritative native player movement, map bounds/walkability/occupancy, bounded click-walk scheduling, SQLite position persistence, and an orderly native-session relog regression that restores the saved tile. Unmodified-client confirmation remains required | deferred | deferred |
| Player interaction intent | partial — target/follow selection state and departure cleanup | deferred | deferred |
| Combat | partial — typed physical adjacent-melee events with bounded damage, deterministic per-player server-tick cooldowns, profile-neutral flat physical mitigation, and an optional scriptless FE declarative weapon catalog. On the current native selected-melee path, only a server-owned matching right-hand item may select a catalog weapon; otherwise no catalog event is created. A separate typed spell-cast foundation atomically enforces nonzero bounded mana costs and server-tick cooldowns; an optional scriptless FE declarative spell catalog may construct those events from ID, mana cost, and interval, is retained as immutable native-host configuration, and can be resolved through a synchronized server-only host helper. No spell layer is profile-gated, persistent, or client-routable. Persisted vitals and native selected-player health updates remain available, while the no-catalog fallback is fixed 10 HP with zero mitigation. TFS Lua weapon scripts, legacy XML formula semantics, map use, client weapon-use packets, armor, shielding, spell names/words/targets/formulas/effects/packets, ranged delivery, resistance, PvP, automatic attacks, and client combat effects remain deferred | deferred | deferred |
| Static creature rendering/occupancy | partial — catalog-derived static spawn representation, native visibility, optional deterministic movement, and caller-triggered reactivation at an unoccupied spawn position with a native map refresh; timed respawns, AI, combat, loot, corpses, and scripts remain deferred | deferred | deferred |
| Monster AI, respawn, combat, loot, corpses | deferred | deferred | deferred |
| NPC conversation, shops, travel | deferred | deferred | deferred |
| Lua execution and event dispatch | deferred — XML/script references are validated but never executed | deferred | deferred |
| Actions, movements, use/use-with, doors, levers | deferred — input is safely tolerated where implemented, not semantically executed. A native 740 Quest Log request returns an explicit empty response; client-visible look/inspect stays deferred until a verified message-mode contract exists | deferred | deferred |
| Chat and social systems | partial — bounded shared 740 delivery with parser-safe limitations | deferred | deferred |
| Guilds, parties, trade, mail, VIP | deferred | deferred | deferred |
| Houses, rent, beds, bank/economy | deferred | deferred | deferred |
| PvP skulls/frags/bans/death policy | deferred | deferred | deferred |
| Backups | supported — local SQLite copy plus manifest | supported | supported |
| TFS conversion audit | partial — safe registry/entity/spawn-reference inventory and missing/unsafe-path diagnostics | partial | partial |

## Stable local-world CLI contract

The following commands are part of FE’s public local-first interface. Their basic syntax and concise default output are preserved. Future features must be additive through flags or subcommands; no command may be removed or renamed without an alias and two release cycles of deprecation.

| Command | Current purpose | Compatibility requirement |
|---|---|---|
| `init <directory> [--profile …]` | Creates a world directory, template config/content skeleton, and SQLite database. | Preserve the world-directory workflow and default profile behavior. |
| `validate <directory>` | Loads configuration, reconciles the content skeleton, validates map/content, and opens the database. | Remain non-destructive and concise. |
| `tfs-audit <directory>` | Audits selected private TFS-style content without executing Lua. | Continue to report conversion readiness and explicit runtime deferrals. |
| `run <directory> [--ed]` | Starts services; `--ed` enables bounded privacy-safe diagnostics. | Keep normal output concise; never log credentials or raw packet bodies. |
| `status <directory>` | Reports local server/profile/database status. | Preserve the no-panel local inspection path. |
| `generate-key <directory>` | Generates an FE-owned legacy key where applicable. | Keep overwrite protection and profile validation. |
| `backup <directory>` | Creates a local SQLite backup and manifest. | Preserve simple one-command recovery preparation. |
| `account create <directory> <account-name> <password>` | Creates a local account without database-console access. | Preserve Argon2-backed local provisioning and numeric account compatibility. |
| `player create <directory> <account-id> <character-name>` | Creates a local test character. | Preserve the current numeric account/character workflow. |
| `player vocation <directory> <player-id> <vocation-id>` | Stores a numeric vocation identity for an existing local character. | Additive only; custom IDs remain valid pending operator-loaded vocation-rule runtime. |
| `player skill <directory> <player-id> <skill> <level> [percent]` | Stores a bounded typed classic skill value for an existing local character. | Additive only; no skill-try or advancement-formula claim. |
| `command <directory> broadcast <message>` | Records the bounded administrator broadcast command. | Extend only through an allowlisted typed command family. |
| `compatibility`, `version`, `help` | Gives profile/capability/build guidance. | Remain stable, discoverable, and scriptable. |

## Change-control rules

1. A feature is not listed as `supported` unless its core behavior, persistence behavior where relevant, and profile-specific protocol delivery are all tested.
2. A content parser does not imply runtime execution. Script references, weapons, spells, and actions stay `deferred` until their runtime semantics exist.
3. The capability key is **profile + content adapter + declared feature**, not the FE release number alone.
4. Private maps, item data, client assets, and TFS data packs are never committed to this repository or used as FE fixtures.
5. Every new command or command option requires help/exit-code regression coverage and must preserve normal versus `--ed` privacy/output guarantees.
