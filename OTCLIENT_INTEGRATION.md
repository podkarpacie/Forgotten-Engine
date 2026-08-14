# Forgotten Engine OTClient Integration Boundary

## Purpose and support boundary

**Forgotten Engine supports an original, opt-in custom-client session foundation for `fe-7.4`.** It is designed as a practical integration target for an OTClient-derived build that deliberately implements the FE capability contract. It is not a claim that an unmodified OTClient distribution, the official Tibia client, or any other client will connect successfully.

The selected transport follows the public extended-opcode pattern used by an older OTClient interoperability patch: that patch shows a game-protocol extended-opcode envelope with opcode `0x32`, a one-byte subopcode, and a string payload, while also distinguishing OTClient operating-system identifiers.[1] FE uses its own reserved subopcode and FE-specific payload vocabulary; it does not copy the referenced implementation or its server patch.

## Operator configuration

An initialized `fe-7.4` world keeps all client-foundation features disabled by default. Generate a local FE-owned key, then enable only the required foundations in `config.lua`.

```lua
legacyLoginEnabled = true
gameSessionEnabled = true
gameSessionPort = 7173

-- This is what the custom OTClient module receives after negotiation.
-- It can be a public IP, DNS name, reverse proxy, tunnel, or IP-changing endpoint.
advertisedGameSessionHost = "play.example.net"
advertisedGameSessionPort = 443
```

| Setting | Purpose | Safe operator expectation |
|---|---|---|
| `ip` and `gameSessionPort` | Local address and port to which FE binds. | Bind to a private or public interface according to the server deployment. |
| `advertisedGameSessionHost` | Hostname or address sent in the FE custom-client world payload. | Set this to the stable public address exposed by a proxy, tunnel, DNS record, or IP-changing service. |
| `advertisedGameSessionPort` | Port sent with the advertised host. | Match the public forwarding/proxy port, which may differ from the local listener port. |

> FE does **not** patch clients, alter executable files, evade endpoint restrictions, or provide an IP-changing service. It merely advertises a deployment-selected endpoint to an FE-aware custom module. The operator remains responsible for DNS, port forwarding, firewall rules, proxying, and any third-party service configuration.

## FE capability contract `v1`

After the existing challenge-bound RSA/XTEA bootstrap and account-owned character check, the FE session foundation sends an XTEA-wrapped ready response followed by an FE extended-opcode capability offer. A custom OTClient module must send the encrypted acknowledgement before FE returns initial-world data.

| Step | Direction | Envelope | Contract |
|---|---|---|---|
| 1 | FE → custom client | Game-session challenge | Timestamp plus random byte. |
| 2 | Custom client → FE | Raw-RSA bootstrap | XTEA key, account, password, character, and exact challenge. |
| 3 | FE → custom client | XTEA session-ready response | Authenticated session foundation; no playable map is claimed. |
| 4 | FE → custom client | Extended opcode `0x32`, subopcode `0xf0` | Capability offer: `fe.capabilities.v1`. |
| 5 | Custom client → FE | XTEA-wrapped extended opcode | Required acknowledgement string: `fe.otclient.v1`. |
| 6 | FE → custom client | Extended opcode `0x32`, subopcode `0xf0` | Text payload: `fe.world.v1`, character identity, start position, advertised endpoint, and `world=empty-gated`. |

> The initial-world payload deliberately reports an empty, feature-gated world. It is not a Tibia map window, a creature list, movement stream, or world-state synchronization packet.

## Custom module requirement

The OTClient-derived build must explicitly recognize the FE session endpoint and implement the acknowledgement above. A module should only treat the connection as established after it validates the `fe.capabilities.v1` offer and receives `fe.world.v1`. Any unknown FE payload should produce a visible module error rather than silently assuming gameplay compatibility.

## Current limits

The implemented path proves a bounded encrypted session flow in FE’s Rust integration tests. It does **not** yet provide a normal Tibia game protocol login, actual map serialization, tiles, creatures, movement, chat, combat, Lua game content, or a packaged OTClient module. Those are subsequent engineering milestones.

## References

[1]: https://github.com/edubart/otclient/blob/master/tools/tfs_extendedopcode.patch "OTClient extended-opcode interoperability patch (public reference)"
