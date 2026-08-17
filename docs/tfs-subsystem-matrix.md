# Official TFS Source-Aided Subsystem Matrix

## Purpose and reading boundary

This matrix turns the official public TFS repository into a **feature and behavior inventory** for Forgotten Engine. It is not a source-code migration plan. FE keeps an independent Rust design, its own tests, and its own explicit support boundaries. No upstream C++ source, TFS data directory, maps, item databases, or client assets are copied into this repository.

The reference snapshot is the public `otland/forgottenserver` `master` revision `cbc4d7b` (“Fix item description to respect reading distance limits”, shallow clone reviewed on 2026-08-17). The source path references below identify the responsible upstream subsystem only. They are not a claim that FE reproduces its full behavior.

> **Completion rule:** a row is not supported merely because FE has a parser, data type, listener, or partial test. It requires an authoritative Rust runtime path, automated boundary and failure tests, accurate capability reporting, and profile-specific client evidence where a client is involved.

## Runtime and world systems

| Upstream subsystem and public reference paths | Intended responsibility | FE status on 2026-08-17 | Next independently implemented Rust boundary |
|---|---|---|---|
| `game.*`, `map.*`, `tile.*`, `thing.*`, `position.*`, `creature.*` | Authoritative world state, spatial occupancy, visible things, map operations, and game transitions. | **Partial.** FE has bounded authoritative map tiles, players, static entities, occupancy, movement validation, deterministic world clock, OTBM import/audit boundaries, and persistence. | Complete authoritative map interactions, dynamic thing ownership, visibility reconciliation, and profile-gated world updates. |
| `player.*`, `vocation.*`, `condition.*`, `combat.*` | Player progression, vocation rules, conditions, combat state, health/mana/capacity, death and recovery. | **Partial.** FE has checked classic experience thresholds, rates/stages, parsed vocation gain metadata, stable-CLI atomic vocation gains after a positive level increase, vitals, regeneration, typed skills, bounded conditions, death-state foundations, and selected-melee foundations. | Provide separately verified native HUD refresh behavior, then continue formula, promotion, and other advancement work through independent evidence. |
| `item.*`, `items.*`, `container.*`, `depotchest.*`, `depotlocker.*`, `inbox.*`, `storeinbox.*` | Item metadata, ownership, containers, depot/inbox storage, and transformations. | **Partial.** FE validates OTB-derived presentation data, models runtime items/equipment/owned containers, persists their core state, emits catalog-mapped equipment deltas, re-emits complete mapped top-level container windows after an authoritative shared replacement, and supports bounded complete-item moves between equipment and owned top-level containers through both stable operator and shared-world paths. | Implement authoritative ground/container/equipment transfers, stacks, capacity, slot compatibility, swaps, ownership rules, close/item container deltas, persistence routing, and real-client validation. |
| `actions.*`, `movement.*`, `weapons.*`, `spells.*`, `matrixarea.*` | Item actions, movement actions, weapon use, spells, areas, and triggered gameplay. | **Foundation.** FE parses bounded scriptless weapon/spell declarations and validates selected typed event paths without formula, effect, target, or script parity. | Add source-evidenced, profile-approved action and weapon semantics one narrow runtime path at a time. |
| `monster.*`, `monsters.*`, `npc.*`, `spawn.*` | Monster and NPC definitions, spawn timing, AI, target selection, combat, dialogue, shops, and movement. | **Partial.** FE materializes static spawn entities, persists bounded state, has deterministic reset/occupancy behavior, safe movement, player-selected static targets, a core-only nearest-player selector, an opt-in shared-heartbeat acquisition policy, and one caller-triggered distance-reducing target step with legal map/occupancy fallback. A real shared-world step emits the existing native full-map refresh. | Implement deterministic creature lifecycle, pursuit/path boundaries, combat participation, loot/corpses, NPC behavior, and client evidence before claiming dynamic-creature support. |

## Session, protocol, and operations systems

| Upstream subsystem and public reference paths | Intended responsibility | FE status on 2026-08-17 | Next independently implemented Rust boundary |
|---|---|---|---|
| `connection.*`, `protocol.*`, `protocolgame.*`, `protocolstatus.*`, `networkmessage.*`, `outputmessage.*`, `rsa.*`, `xtea.*` | Network transport, game/status protocol frames, packet limits, encryption, and connection lifecycle. | **Partial for native 740 only.** FE has a profile-driven native 740 route with real-client blockers still open. FE 8.0 has only pure XTEA envelope/bootstrap foundations and no runnable listener. | Validate native 740 state refresh/inventory behavior against an unmodified client; implement FE 8.0 only after parser-backed encrypted session evidence exists. |
| `server.*`, `scheduler.*`, `tasks.*`, `lockfree.*`, `signals.*`, `main.*` | Server lifecycle, scheduled work, task execution, concurrency primitives, shutdown, and process entry. | **Foundation.** FE uses a central heartbeat and listener boundaries, but has not earned full deterministic multithreaded execution or load/soak evidence. | Define ownership, ordering, snapshot, and contention contracts before adding concurrent gameplay domains. |
| `configmanager.*`, `fileloader.*`, `definitions.h`, `const.h`, `enums.h` | Configuration and content loading, shared identifiers, and operator-visible policy. | **Partial.** FE supports bounded TFS-style configuration discovery and selected XML catalogs. | Expand only by parser-to-runtime validation pairs, keeping unsupported assignments and data fields visible in validation output. |
| `database.*`, `databasemanager.*`, `databasetasks.*`, `iologindata.*`, `iomap.*`, `iomapserialize.*` | Database access, schema work, asynchronous database tasks, account/player persistence, and world serialization. | **Partial.** FE uses versioned SQLite migrations and typed persistence for its implemented state. It is not a full legacy TFS database implementation. | Add audited migration adapters, restore/upgrade regressions, and safe aggregate compatibility reports before claiming database-conversion parity. |
| `http/*`, `base64.*`, `protocolstatus.*` | HTTP/status surfaces and server information responses. | **Partial.** FE has a bounded status service and diagnostic surfaces. | Add only documented, rate-limited, security-reviewed control/status behavior. |

## Scripting, social, economy, and administration systems

| Upstream subsystem and public reference paths | Intended responsibility | FE status on 2026-08-17 | Next independently implemented Rust boundary |
|---|---|---|---|
| `luascript.*`, `luavariant.h`, `script.*`, `scriptmanager.*`, `baseevents.*`, `events.*` | Lua execution, bindings, script registration, and event dispatch. | **Foundation.** FE has a typed no-op aggregate audit dispatcher for imported references plus a separate direct expression sandbox with no standard libraries, typed primitive-only results, and bounded source/memory/instruction limits. It never loads or executes imported Lua or event callbacks. | Build permissioned event execution, typed APIs, safe script loading, compatibility adapters, and deterministic failures one event family at a time. |
| `creatureevent.*`, `globalevent.*`, `talkaction.*`, `movement.*`, `actions.*` | Player/creature/global events, talk actions, and triggered hooks. | **Deferred.** No legacy event behavior is claimed. | Use the sandboxed runtime after its security boundary and typed event contracts are verified. |
| `chat.*`, `party.*`, `guild.*`, `groups.*`, `ban.*` | Chat, groups, parties, guilds, privileges, bans, and moderation. | **Deferred.** Native 740 visible chat remains intentionally suppressed because its message-mode contract is unresolved. | Resolve the supported profile’s public message-mode contract first; then add persistence and moderation semantics with explicit packet/runtime evidence. |
| `iomarket.*`, `house.*`, `housetile.*`, `bed.*`, `mailbox.*`, `teleport.*`, `trashholder.*`, `podium.*`, `outfit.*`, `mounts.*` | Market, housing, beds, mail, map holders, teleports, outfits, and mounts. | **Mostly deferred.** FE has a tested native classic outfit persistence range boundary, but no complete asset, addon, mount, housing, market, or holder behavior. | Add feature-specific authoritative models and client evidence individually; do not infer support from a related item/map type. |

## Work sequencing

The next bounded implementation is **deterministic creature runtime**. It follows the completed native 740 player-stats refresh, mapped equipment synchronization, and bounded item-location transfers. The creature slice will extend only authoritative static-spawn timing, movement, selection, and path boundaries before any broader AI, loot, or client-parity claim.

The rest of the matrix is sequenced by runtime dependency: authoritative items and creatures first, sandboxed scripting second, social/economy/housing later, then profile and operational acceptance work. The weighted completion ledger remains the project’s production-readiness measure; this matrix is a source-guided catalog of work, not a new completion percentage.

## References

[1]: https://github.com/otland/forgottenserver/tree/cbc4d7b/src "Official TFS source directory at reviewed snapshot"
[2]: https://github.com/otland/forgottenserver/blob/cbc4d7b/src/player.cpp "Official TFS player subsystem at reviewed snapshot"
[3]: https://github.com/otland/forgottenserver/blob/cbc4d7b/src/protocolgame.cpp "Official TFS game protocol subsystem at reviewed snapshot"
[4]: https://github.com/otland/forgottenserver/blob/cbc4d7b/src/luascript.cpp "Official TFS Lua subsystem at reviewed snapshot"
