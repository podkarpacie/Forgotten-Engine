# Forgotten Engine Network Host Milestone

The first network-capable milestone adds a persistent, configuration-driven TCP service to Forgotten Engine. It is deliberately an **engine probe/session foundation**, not a claim of official Tibia 7.4 client compatibility.

## Initial vertical slice

| Component | Initial behavior | Boundary |
|---|---|---|
| Listener | Binds a configured IPv4/IPv6 socket and accepts connections while the process remains active. | No login/game ports are claimed yet. |
| Framing | Uses the existing little-endian, length-prefixed bounded frame type. | It is not presented as the Tibia 7.4 wire format. |
| Handshake | Accepts a `FEHS` engine-probe payload and returns a structured profile/session response. | It is a synthetic diagnostic handshake for integration tests and host-agent connectivity. |
| Limits | Enforces bounded frames, read/write timeouts, connection cap, and orderly shutdown. | It does not implement account authentication, character selection, encryption, or game state. |
| Persistence | Records accepted session events in the engine database. | It does not persist players or game sessions. |

## Completion criteria

The milestone is complete only when a separate process can establish a TCP connection, send a valid framed probe, receive a valid framed response, observe malformed-frame rejection, and request an orderly server shutdown in automated integration tests.

> This milestone makes Forgotten Engine a real network service. It does not make it a playable Tibia 7.4 server. Official-client interoperability begins only after independently specified login and game session codecs are implemented and verified.
