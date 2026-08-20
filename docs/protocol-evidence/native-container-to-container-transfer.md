# Native Container-to-Container Transfer

## Bounded implemented behavior

Native 740 ThrowItem now accepts one caller-owned mapped item from a current non-nested top-level
container window to a distinct current non-nested top-level container window. The source address,
source item index, requested count, and requested client thing ID must exactly match the current
authoritative state.

The authoritative core splits the requested count, merges only an identical item instance into the
destination or appends one bounded new stack, then commits both container windows atomically. FE
persists the complete resulting container snapshot and advances its existing container refresh
epoch only after the successful transition.

## Deliberate exclusions

Same-container reordering, nested containers, closed views, swaps, destination placement rules,
ground and depot movement, capacity/weight, arbitrary stackability, scripts, and generic inventory
behavior remain deferred.
