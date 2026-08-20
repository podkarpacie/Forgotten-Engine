# Native Ground-Item Ownership Audit

FE currently provides the native listener an immutable `Arc<WorldMap>`. The map holds imported
tile-item data used for rendering, while player equipment and top-level containers are mutable
authoritative state inside `SharedNativeWorld`.

Ground-to-inventory transfer cannot safely mutate either side alone. A correct later design must
give ground items one synchronized mutable owner, define a single lock order with the authoritative
player world, produce short-lived immutable map snapshots for rendering, and persist any supported
ground-item transition atomically with its inventory destination.

No ground-item transfer, map mutation, or protocol routing is enabled by this audit.
