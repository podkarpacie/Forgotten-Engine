# Native Ground-Item Composite Transfer Boundary

## Scope

This note records the **server-side foundation only** for one source-bound imported map item moving into one empty player equipment slot. It does not claim native 740 packet routing, client inventory deltas, or a usable client-facing pickup action.

| Boundary | Current rule |
|---|---|
| Source identity | A top-level source item is identified by immutable source-map revision, tile position, and source ordered index. |
| Runtime binding | The synchronized mutable map keeps an index from each current runtime item position to its source index. Whole-tile replacement removes the affected bindings rather than inferring new identities. |
| Item shape | Only plain top-level items are eligible. Children, text, descriptions, teleport destinations, duration, and charges are rejected because the current `ItemInstance` contract cannot preserve them. |
| Destination | One empty fixed equipment slot only. Slot compatibility, swaps, stacking, capacity, and containers are not part of this slice. |
| Lock order | Map owner, shared player world, source-index state, removal-journal state. No code in this slice acquires these locks in the reverse order. |
| Durable boundary | The full authoritative equipment/container snapshot and full revision-bound removal journal commit in one SQLite transaction before in-memory map/player state is published. |
| Publication | After a successful commit, FE replaces the runtime map, updates equipment, removes the source binding, records the identity, increments map revision, and increments the existing equipment epoch. |

## Evidence

The host regression `source_map_item_pickup_persists_map_inventory_and_journal_together` verifies a successful transfer with preserved server ID, count, action ID, and unique ID. It asserts the runtime map removal, map revision, authoritative equipment, equipment epoch, SQLite inventory, and durable removal journal.

The host regression `source_map_item_pickup_rejects_occupied_slot_without_mutating_map_or_persistence` verifies that an occupied destination leaves map content, map revision, authoritative equipment, equipment epoch, SQLite equipment, and the journal unchanged. The persistence regression `atomically_replaces_player_inventory_and_map_item_removal_journal` verifies the combined SQLite write and rejects a duplicated journal identity without replacing prior durable state.

## Deferred Work

The native game service now creates one synchronized map owner from its immutable startup map. Each accepted session receives a detached owner snapshot, and the heartbeat obtains a detached owner snapshot per tick. Native classic `ThrowItem` source-position decoding must still be routed to this foundation. That future route also needs exact catalog client-item validation, parser-verified inventory/map delta delivery, and a recovery bootstrap that applies the durable journal only after the current source revision has been validated. Container destinations, stack splitting and merging, capacity, slot compatibility, ground-item ownership rules beyond the source map, map actions, and real unmodified OTClientV8 confirmation remain deferred.
