# Dead Player Interaction Cleanup

## Supported boundary

When FE records an authoritative player death, it now removes every other active player's
`target_player_id` and `follow_player_id` reference to that dead player. The target player remains
in the world with its existing dead and temple-respawn state; FE does not remove, teleport, or
otherwise alter the player through this cleanup.

The rule reuses the existing removal-path invalidation shape and is applied inside the central core
`apply_player_death` transition. It therefore covers both selected-player melee and typed condition
damage paths that record a real death state.

| Behavior | Status |
|---|---|
| Clear authoritative player target referencing the newly dead player | Supported. |
| Clear authoritative player follow referencing the newly dead player | Supported. |
| Clear the dead player's own interaction state | Unchanged; a dead source already cannot set a new target or follow. |
| Emit a `ClearTarget` frame to every affected native session | Deferred. Session ownership and cross-session notification routing remain incomplete. |
| Broader PvP cancellation, effects, death UI, loot, corpse, or respawn behavior | Deferred. |

## Regression evidence

The core regression records a real temple-backed player death and verifies that another player's
target and follow intent become the default empty intent. The host selected-player melee regression
also verifies that a lethal configured melee event clears the attacking player's target and follow
while retaining its existing persistence and fixed-loss assertions.
