# Native Fixed Selected-Player Melee

## Bounded implemented behavior

The existing native selected-player fallback keeps its stable request route and fixed requested
damage. It now constructs the existing server-owned adjacent physical combat event with a one-tick
interval instead of bypassing typed combat. The authoritative shared heartbeat advances world time
in elapsed whole seconds, so a one-tick event preserves the established no-more-than-once-per-
heartbeat cadence while preventing duplicate same-tick resolution.

The typed event therefore uses the existing current equipment-derived profile-neutral armor
reduction and existing death handling. The result is persisted through the existing native
selected-player path only after real applied damage.

## Deliberate exclusions

This does not implement TFS weapon speed, attack speed, skills, shielding, fight modes, random
blocking, PvP legality, resistance, client combat effects, automatic targeting, or generic combat
formula parity. The fixed requested damage remains an FE bounded fallback, not a TFS formula.
