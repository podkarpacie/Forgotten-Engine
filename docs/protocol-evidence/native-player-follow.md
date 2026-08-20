# Native Player Follow

## Bounded implemented behavior

FE already retains a validated player follow intent through the native interaction route. On each
native shared-world heartbeat, every living source with a current living player follow target is
considered in stable player-ID order. A source may make at most one direct cardinal step that
reduces distance toward the target. Ties prefer the horizontal axis, then the vertical axis.

The candidate destination must be walkable and free of every other player and active static
creature. A real batch move advances the existing native visibility epoch once and the heartbeat
persists every resulting authoritative player position through the existing database update path.

## Deliberate exclusions

This is not pathfinding. FE does not route around blocked tiles, retry alternative movement after
the two direct axes, move diagonally, change the follow selection, alter player facing, attack,
or add combat automation. Formations, speed calculations, client route feedback, and full TFS
follow behavior remain deferred.
