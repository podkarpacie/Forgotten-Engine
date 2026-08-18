# Native 740 UseItemEx Research Boundary

## Observation

The read-only TFS protocol reference maps inbound opcode `0x83` to a UseItemEx parser. The parser reads a source position, source client sprite ID, source stack position, destination position, destination client sprite ID, and destination stack position before scheduling a server item-use-with operation.

## Forgotten Engine boundary

This observation is a format lead, not an interoperability claim. Forgotten Engine must first establish profile-specific request bounds and a safe authoritative mapping for both client sprite IDs. Any initial FE path must remain no-mutation and must reject missing or ambiguous client-to-server mappings. Item actions, use-with semantics, containers, Lua, map mutation, messages, effects, and client-visible results remain deferred.
