# Native Static-Creature Respawn Audit

## Existing supported behavior

The native shared-world heartbeat already advances the deterministic world clock and invokes the
authoritative `reactivate_due_static_creatures` path on each elapsed-second step. A static creature
reactivates only when its imported nonzero interval is due and its original spawn tile is not
occupied by a player or another active static creature.

The core reactivation path restores the creature at its original tile with its configured health
percentage, clears its inactive/due state, refreshes occupancy, and advances world revision only
when a real reactivation occurs.

| Behavior | Status |
|---|---|
| Automatic interval-gated static-creature reactivation | Supported through the native heartbeat. |
| Player or active-creature occupancy deferral | Supported. |
| Deterministic same-tile state restoration | Supported. |
| Spawn zones, random tile selection, proximity suppression, rate controls, AI, loot, scripts, or NPC runtime | Deferred. |

## Decision

No new scheduler is added. The existing heartbeat is already the single automatic scheduling point,
and a second scheduler would risk duplicate reactivation or nondeterministic ordering. The remaining
work is broader spawn and creature runtime modeling, not basic timed reactivation.
