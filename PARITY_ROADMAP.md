# Forgotten Engine Clean-Room Parity Roadmap

Forgotten Engine targets **behavioral and operational compatibility**, not a direct source translation. Every compatibility claim is tied to a target version, a documented external contract, independently written Rust code, and acceptance tests.

## Rust architecture

| Rust crate or surface | Responsibility | Upstream-reference boundary |
|---|---|---|
| `forgotten-config` | Typed `config.lua` subset loader, defaults, validation, and diagnostic output. | TFS-style configuration categories and errors; no upstream parser code. |
| `forgotten-host` | Process lifecycle, TCP service registration, bounded sessions, shutdown, connection limits, and logs. | Observable service lifecycle and external ports. |
| `forgotten-protocol` | Version-specific independent codecs, framing, session negotiation, encryption boundaries, and fixtures. | Publicly observable client/server behavior only. |
| `forgotten-persistence` | Migrations, accounts, players, map/content state, transactions, and backup metadata. | Schema and operational requirements, not copied SQL. |
| `forgotten-world` | Map/entity model, ticks, creatures, combat, conditions, inventory, and visibility. | Game behavior under independently written tests. |
| `forgotten-scripting` | Lua-compatible capability host, script loading, permissions, errors, and deterministic callbacks. | Script-interface contracts, not upstream script/source reuse. |
| `fe-data` | Original content manifest and loader contracts for actions, events, monsters, NPCs, spells, movements, talkactions, weapons, XML, and world data. | Directory-level operational expectations; no upstream data copied. |

## Versioned acceptance sequence

| Milestone | TFS-style operator behavior | Acceptance evidence | Status |
|---|---|---|---|
| P0 — startup/config/content | `config.lua` discovery, typed settings, startup banners, content-directory validation, error/no-service diagnostics. | Unit tests plus CLI fixture runs. | Implemented bounded foundation. |
| P1 — host/status | Long-running service lifecycle, configured bind address/ports, safe connection limits, probe/status endpoints, graceful shutdown. | TCP integration tests and CLI service smoke test. | Implemented for FE probe plus TFS-style XML/binary status query families. |
| P2 — persistence/scheduler | Schema migration, account/player lifecycle, periodic tasks, save/backup, deterministic events. | Transaction, migration, and task tests. | Account/password and character-list persistence foundation implemented; scheduler remains planned. |
| P3 — scripting/content | Original Lua-compatible runtime and FE content-manifest loaders. | Script capability tests and malformed-content diagnostics. | Planned. |
| P4 — 7.4 login/game | Independently specified 7.4 login and game sessions, character list, encryption boundary, version-safe packet codecs. | Synthetic fixtures and authorized interoperability test harnesses. | Login/character-list, RSA, XTEA, challenge-bound game-session bootstrap, FE-aware OTClient capability negotiation, and initial identity/position metadata foundations implemented; normal client world initialization remains planned. |
| P5 — world/gameplay | Original map/entity simulation, movement, combat, conditions, NPC/monster behavior, and persistence. | Deterministic simulation tests. | Planned. |
| P6 — deployment parity | Reproducible Linux/Windows packages, Docker image, secure production host guidance, status/metrics, backups, and release verification. | Package, container, and host acceptance tests. | Partial packaging exists. |

## P0 configuration contract

The first TFS-style FE configuration contract uses a world-local `config.lua`. It recognizes a deliberately bounded set of literals: quoted strings, integer values, booleans, and comments. The initial required keys are `ip`, `gameProtocolPort`, `statusProtocolPort`, `maxPlayers`, `serverName`, `mapName`, `worldType`, `mysqlHost`, `mysqlUser`, `mysqlDatabase`, `feProfile`, and `tibiaProtocol`. `legacyLoginEnabled` and `rsaPrivateKey` are optional and default to a disabled login foundation and the local `key.pem` path, respectively.

The parser must reject missing or malformed required values with a clear configuration diagnostic. It must not execute arbitrary Lua during the initial milestone. A real Lua runtime will enter under P3 with sandboxing and capability tests.

## P0 content contract

FE creates an original empty `data/` skeleton with TFS-style operational directories: `actions`, `creaturescripts`, `events`, `globalevents`, `items`, `lib`, `migrations`, `monster`, `movements`, `npc`, `spells`, `talkactions`, `weapons`, `world`, and `XML`. The first loader validates the structure and an FE-owned manifest; it does not bundle upstream scripts, maps, XML, item data, or client assets.

## P1 status and P4 login foundation

The status endpoint accepts the legacy XML `info` request and the bounded binary request family. Its output declares **Forgotten Engine** rather than The Forgotten Server, reports FE release/profile information, and explicitly exposes zero online/peak players until a world-session registry exists.

The initial `fe-7.4` login envelope is intentionally gated. It expects an independently documented `0x01` envelope containing a little-endian 7.4 version field and a fixed raw 1024-bit RSA block. The decrypted block supplies a zero marker, four XTEA words, and bounded account/password strings. Account passwords are verified with Argon2, and character-list/error responses are XTEA-wrapped.

The game-session foundation is also intentionally separate and opt-in. It uses `gameSessionPort` rather than claiming that the production game port is client-compatible. On connection, FE sends an original challenge; the client foundation must return a raw-RSA bootstrap containing an XTEA key, account name, password, character name, and that exact challenge. FE verifies account/character ownership, negotiates the documented FE-aware custom OTClient capability acknowledgement, and returns the stored character position plus an explicit empty-world gate. The custom-client boundary and public endpoint advertisement settings are documented in [OTCLIENT_INTEGRATION.md](OTCLIENT_INTEGRATION.md). This is not an assertion that an unmodified OTClient or official 7.4 client uses or accepts the current sequence.

## Compatibility safeguards

The FE command line will use TFS-style startup phases and actionable diagnostics, but it will label feature gates explicitly. A successful run can state that status service is listening and that the bounded 7.4 login foundation is disabled or enabled, while still stating that official login/game compatibility remains unverified. A release may not label itself playable or official-client compatible until P4 acceptance criteria pass.

## References

1. [Official The Forgotten Server repository](https://github.com/otland/forgottenserver)
2. [Official The Forgotten Server configuration template](https://github.com/otland/forgottenserver/blob/master/config.lua.dist)
3. [Official The Forgotten Server scripting reference](https://github.com/otland/forgottenserver/wiki/Script-Interface)
