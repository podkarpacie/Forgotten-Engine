# Native 740 UseItem Evidence

This note records the public behavior reference used for a bounded Forgotten Engine compatibility slice. It does not copy or adapt upstream implementation code.

The official TFS `ProtocolGame::parseUseItem` reader consumes a position, a 16-bit sprite ID, a stack-position byte, and an index byte for opcode `0x82`. It forwards those values to its server-side item-use path. The matching game entry point distinguishes the client sprite ID from the server-owned item identity and applies broader action, cooldown, pathfinding, script, and effect semantics outside this packet layout.[1] [2]

Forgotten Engine may therefore decode the full fixed request shape, but must resolve a client sprite ID through its own validated item mapping before asking its own server-owned map validator about an item. An absent or ambiguous mapping must not be guessed. This evidence does **not** establish support for action execution, doors, levers, containers, scripts, cooldowns, pathfinding, messages, or client effects.

## References

[1]: https://github.com/otland/forgottenserver/blob/master/src/protocolgame.cpp
[2]: https://github.com/otland/forgottenserver/blob/master/src/game.cpp
