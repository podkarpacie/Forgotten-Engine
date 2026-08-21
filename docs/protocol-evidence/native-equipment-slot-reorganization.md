# Native 740 occupied equipment-slot reorganization audit

## Scope

This note records the evidence boundary for one **bounded native 740 `ThrowItem` equipment-to-equipment exchange**. It does not claim full TFS item movement or generic inventory compatibility.

## Reference observations

The read-only OTCv8 reference at `otcv8-dev/src/client/game.cpp` serializes an ordinary move request from a source position, item ID, stack position, destination position, and count. It does not provide a separate client opcode for equipment-slot reorganization.

The read-only TFS reference at `forgotten-server/src/game.cpp` routes an item movement through `Game::internalMoveItem`. When destination admission returns `RETURNVALUE_NEEDEXCHANGE`, it first checks whether the destination item can return to the source. Only then does it remove the destination item, add it to the source, and continue with the original move. This is behavior evidence for an exchange concept, not permission to copy TFS logic or to claim its complete rules.

## FE implementation boundary

FE accepts the existing parser-backed classic `ThrowItem` record only when all of the following conditions hold:

| Check | Required FE behavior |
|---|---|
| Source and destination address | Both are exact fixed equipment positions (`x = 0xFFFF`) decoded by the existing native 740 path. |
| Source authority | The authenticated player owns a mapped source item whose catalog client ID and count match the request. |
| Destination authority | The destination is a **distinct, occupied** fixed equipment slot. |
| Count | The request count equals the complete source item count. |
| Mutation | `WorldState::swap_equipment_items` prepares both items in cloned equipment state and publishes one authoritative replacement only after both exist. |
| Persistence and refresh | The native route persists the updated equipment collection and advances the existing equipment epoch only after core acceptance. |

Core coverage verifies the successful swap, same-slot rejection, empty-source rejection, unchanged revision on rejection, and unchanged authoritative slots. A live native socket regression verifies that a parsed `0x78` request exchanges two mapped occupied slots and that the SQLite equipment record contains the two swapped complete items.

## Explicit deferrals

FE deliberately does **not** infer slot compatibility, capacity, item stackability, partial exchanges, generic cancellation messages, events, scripts, nested containers, ground/depot/inbox/trade movement, packet deltas beyond the existing equipment refresh flow, or full TFS `internalMoveItem` parity. Real unmodified-client confirmation remains a release blocker.
