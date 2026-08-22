# Static Spawn Lifecycle Research

The local TFS reference keeps a per-spawn interval and last-spawn time. Its periodic check skips already present entries, delays a missing entry until its interval expires, blocks ordinary respawns when a relevant player is nearby, and applies broader probability, rate-cap, event-hook, monster-type, and placement rules.

FE currently retains only an independently bounded model: each materialized static entity may reactivate after its own imported interval when its exact spawn tile is unoccupied. FE does not model spawn zones, player spectator flags, spawn-block bypasses, weighted monster alternatives, rate caps, script hooks, or full placement rules.

No lifecycle feature will be claimed as TFS spawn parity until those independent ownership and policy contracts exist.

The existing FE path can support a narrow next step without changing unbounded behavior: `LegacySpawnArea` already exposes a validated center and radius; static materialization assigns a stable static ID while iterating that area; `FeTfsStaticSpawnCollection` retains stable-ID metadata; and runtime reactivation already checks one due inactive entity at a time. A future slice can therefore retain only an ID-to-center/radius record for materialized monsters and defer an otherwise due reactivation if a live player lies within that rectangular area. It must leave NPC reactivation, spectator flags, chance alternatives, rate caps, hooks, and placement rules outside the slice.
