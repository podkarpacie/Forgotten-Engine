# Native Static-Creature Pursuit

## Supported boundary

`staticCreatureTargetPursuitRange` is an explicit native-host `config.lua` setting. Its default
value is `0`, which preserves disabled behavior. A value from `1` through `8` enables one
deterministic pursuit pass per native heartbeat.

For every active static creature, FE selects the nearest living same-floor player within the
configured range, resolves ties through the existing deterministic selection contract, and attempts
at most one existing legal distance-reducing cardinal step. The movement uses the authoritative map
and occupancy checks. A real move advances the native visibility epoch once; no movement produces
no visibility change.

| Area | Supported boundary | Deferred |
|---|---|---|
| Scheduling | One opt-in pursuit pass per native heartbeat | Independent creature timers and rate controls |
| Target choice | Existing nearest-living-player range up to 8 tiles | Protection zones, PvP rules, threat tables, and events |
| Movement | One legal cardinal step under map/occupancy checks | General pathfinding, retries, diagonal policy, and routing |
| Gameplay | Visibility refresh after a real move | Combat, loot, corpses, scripts, spells, NPCs, and AI |

## Regression evidence

Configuration tests cover the disabled default, accepted range `4`, and rejection of range `9`.
The native heartbeat regression verifies that an enabled 4-tile policy moves a creature exactly one
tile toward the player, advances visibility once, and leaves attack behavior disabled.
