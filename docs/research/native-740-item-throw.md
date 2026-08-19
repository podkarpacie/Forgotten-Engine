# Native 740 Item-Throw Parser Boundary

## Scope

This note records the bounded inbound classic item-throw request that Forgotten Engine can now decode safely. It does **not** claim an equipment, container, ground, capacity, ownership, or stack-transfer implementation.

## Verified packet boundary

The local read-only TFS reference dispatches client opcode `0x78` to its item-throw parser.[1] That parser reads the following payload sequence: source position, source sprite identifier, source stack position, target position, and count.[2] The resulting payload is 14 bytes after the opcode.

| Field | Width | FE parser behavior |
|---|---:|---|
| Source position | 5 bytes | Decoded as bounded native position data. |
| Source client thing ID | 2 bytes | Decoded but not trusted as authority. |
| Source stack position | 1 byte | Decoded. |
| Target position | 5 bytes | Decoded as bounded native position data. |
| Requested count | 1 byte | Decoded. |

> **Current boundary:** FE accepts only the exact 14-byte classic request. It records a metadata-only extended diagnostic when enabled and leaves the native session alive. It does not mutate server-owned items.

## Deferred behavior

The existing authoritative equipment and container models are not connected to the native throw request. The following remain deferred: client item-to-server item validation, equipment compatibility, slot swap, container paths, ground paths, stack splitting and merging, capacity, ownership, persistence coordination, cancellation messages, and client inventory/container deltas.

## References

[1] [TFS `ProtocolGame` client opcode dispatch](https://github.com/otland/forgottenserver/blob/master/src/protocolgame.cpp#L594-L599)

[2] [TFS `ProtocolGame::parseThrow` field order](https://github.com/otland/forgottenserver/blob/master/src/protocolgame.cpp#L1203-L1215)
