# Classic 7.4 Item-Use Packet Evidence

## Scope

This note records only packet-shape observations used as a clean-room behavior map. It does not import, copy, execute, or redistribute upstream source code.

## UseItemEx (`0x83`)

On 2026-08-19, the public legacy 7.4 protocol reference below was inspected in a browser. Its `parseUseItemEx` parser reads the inbound payload in this field order:

| Order | Field | Encoded size |
|---:|---|---:|
| 1 | Source position | 5 bytes (`u16 x`, `u16 y`, `u8 z`) |
| 2 | Source client sprite/item identifier | 2 bytes |
| 3 | Source stack position | 1 byte |
| 4 | Target position | 5 bytes (`u16 x`, `u16 y`, `u8 z`) |
| 5 | Target client sprite/item identifier | 2 bytes |
| 6 | Target stack position | 1 byte |

The resulting payload length after the `0x83` opcode is **16 bytes**. Forgotten Engine independently implements only a bounded parser and server-owned validation path. It does not execute actions, consume charges, invoke Lua, mutate item state, persist state, or emit an item-use response.

## Battle window (`0x84`)

The same public reference dispatches `0x84` to its battle-window parser. That parser reads a source position, a source client sprite/item identifier, a source stack position, and a creature ID. The resulting payload length after the opcode is **12 bytes**: `5 + 2 + 1 + 4`.

Any Forgotten Engine support must remain a clean-room, validation-only implementation until an authoritative server-owned creature-target model is explicitly wired to the input path. It must not imply rune, wand, combat, Lua, targeting, charge, message, effect, or client-output behavior.

## Rotate item (`0x85`)

The same public reference dispatches `0x85` to its rotate-item parser. That parser reads one position, one client sprite/item identifier, and one stack position. The resulting payload length after the opcode is **8 bytes**: `5 + 2 + 1`.

Any Forgotten Engine support must remain validation-only until a complete authoritative item-rotation model exists. It must not imply that an item can rotate, mutate, run scripts, persist, or emit a client record.

## Source

1. [Podkal/Avesta-1, `protocol76.cpp` — public legacy 7.4 reference](https://github.com/Podkal/Avesta-1/blob/master/Avesta%207.4%20(OTServ_SVN%200.6.3)/source%207.4/protocol76.cpp)
