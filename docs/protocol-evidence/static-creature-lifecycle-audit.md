# Static Creature Lifecycle Audit

## Current typed behavior

FE already supports one bounded deterministic static-creature lifecycle rule. A defeated static
creature is reactivated only after its parsed respawn interval becomes due, and only when its
original tile is valid and not occupied by a player or another creature. The shared native heartbeat
uses the existing authoritative reactivation summary to refresh visibility after an actual state
change.

This is a tile-occupancy reactivation policy. It is not a general TFS spawn system.

| Candidate behavior | Current FE model | Decision |
|---|---|---|
| Interval-gated same-tile reactivation | Typed static identity, original position, respawn interval, tick, map walkability, and occupancy checks | Already supported and regression-covered. |
| Spawn-zone selection | No typed spawn-zone geometry or eligible-tile contract | Deferred. |
| Spectator/player proximity suppression | No bounded spectator query or range policy | Deferred. |
| Spawn flags, dynamic rates, or event hooks | No parsed authoritative contracts | Deferred. |
| Monster AI, loot, formulas, scripts, or NPC behavior | No required gameplay models or safe Lua authority | Deferred. |

## Decision

No additional static-creature lifecycle behavior is enabled by this audit. Adding a new rule would
either guess missing legacy semantics or overstate compatibility. The existing same-tile
reactivation remains the sole supported slice until FE gains typed spawn-zone, spectator,
spawn-flag, rate-cap, and event-hook contracts with dedicated deterministic tests.
