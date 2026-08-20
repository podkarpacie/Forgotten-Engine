# Native Ground-Item Ownership Audit

FE currently provides the native listener an immutable `Arc<WorldMap>`. The map holds imported
tile-item data used for rendering, while player equipment and top-level containers are mutable
authoritative state inside `SharedNativeWorld`.

Ground-to-inventory transfer cannot safely mutate either side alone. A correct later design must
give ground items one synchronized mutable owner, define a single lock order with the authoritative
player world, produce short-lived immutable map snapshots for rendering, and persist any supported
ground-item transition atomically with its inventory destination.

No ground-item transfer, map mutation, or protocol routing is enabled by this audit.

## Required composite transfer boundary

The existing SQLite inventory replacement transaction covers only player equipment and containers.
It does not persist map tile items. A ground-to-inventory transfer therefore needs durable map-item
state (or an equivalent recoverable journal), a defined lock order for map then world state, one
combined rollback strategy for map and inventory state, and only then native inventory/map refresh
delivery. The present map-owner foundation is intentionally insufficient for that composite commit.
