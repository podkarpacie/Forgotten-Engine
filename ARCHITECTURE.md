# Forgotten Engine Architecture

## Purpose

Forgotten Engine is an original Rust workspace for an OpenTibia-compatible server platform. The initial release targets a **Tibia 8.0-oriented foundation**: configuration, embedded persistence, deterministic world state, administration commands, diagnostics, backup primitives, and a deliberately narrow protocol seam. It is not a source translation of The Forgotten Server.

## Compatibility boundary

The upstream Forgotten Server is used only as an external behavioral reference for public domain vocabulary and future compatibility tests. This repository contains original Rust implementations, original documentation, and no copied source or data assets. Any distributed importers or protocol work must be independently implemented and tested against openly documented file/protocol behavior. The upstream project is GPL-2.0, so direct code incorporation would require satisfying its license conditions.[1]

## Workspace layout

| Crate | Responsibility | First-release acceptance criteria |
|---|---|---|
| `forgotten-core` | Domain entities, world state, validation, lifecycle state machine | Movement and player/account operations are deterministic and unit tested. |
| `forgotten-persistence` | SQLite schema, migrations, repository implementation, backup manifest | Starts from an empty data directory and migrates safely. |
| `forgotten-protocol` | Binary packet framing and Tibia 8.0 protocol seam | Rejects malformed frames and exposes explicit protocol capability flags. |
| `forgotten-scripting` | TFS Lua compatibility registry and command/event contracts | Reports supported API entries rather than claiming full Lua compatibility. |
| `forgotten-cli` | `init`, `validate`, `run`, `backup`, and `status` commands | Creates a usable local instance with safe defaults and structured output. |

## Runtime contract

The engine reads `server.toml`, prepares `data/server.db` by default, applies idempotent migrations, validates the content manifest, and runs an administration command loop. The initial command surface supports status inspection, controlled start/stop state transitions, broadcasts, and a deterministic simulation tick; it is the foundation for a real login/game networking layer rather than a claim of full gameplay compatibility.

## Future hosting-agent contract

Forgotten Cloud must not execute untrusted game processes in its web request runtime. A future `forgotten-agent` runs on an approved host node, owns engine processes and storage, and exposes an authenticated API for `create`, `start`, `stop`, `restart`, `command`, `backup`, `restore`, and metrics/log streaming. Cloud persists the requested state and audits every mutation; agents report observed state and append-only events.

## References

[1]: https://github.com/otland/forgottenserver/blob/master/LICENSE "The Forgotten Server GPL-2.0 license"
