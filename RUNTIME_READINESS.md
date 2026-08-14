# Forgotten Engine Runtime Readiness

## Windows package dependencies

The FE 7.4.0 Windows archive contains one program, `forgotten-engine.exe`, plus documentation. Its PE import table was inspected after packaging. The executable does **not** import Boost, OpenSSL, MariaDB/MySQL, zlib, bz2, pugixml, or a separate SQLite DLL. SQLite is compiled into the binary through the Rust `rusqlite` bundled feature.

| Imported runtime library | Source | Needs to ship beside the EXE? | Purpose at this stage |
|---|---|---:|---|
| `KERNEL32.dll`, `ntdll.dll`, `USERENV.dll`, `api-ms-win-core-synch-l1-2-0.dll` | Windows operating system | No | Standard Windows process, environment, and synchronization APIs. |
| `msvcrt.dll` | Windows operating system | No | C runtime imported by the GNU Windows target. |
| `bcryptprimitives.dll` | Windows operating system | No | Windows cryptographic primitive support used by the Rust runtime. |
| `WS2_32.dll` | Windows operating system | No | Winsock support for the current persistent FE diagnostic TCP host and future protocol services. |

> The Windows build is therefore a single application **with normal operating-system DLL imports**, not a completely static PE file. It should not need the collection of third-party DLLs bundled by the C++ The Forgotten Server distribution shown in the comparison image.

## What the EXE can do today

The current binary can initialize a world-local TFS-style `config.lua`, create an original `data/` content skeleton, create its SQLite database, validate profile-specific configuration/content contracts, record lifecycle/administration events, create backups, and report the supported compatibility profiles. The `fe-7.4`, `fe-8.0`, and `fe-1.2` selectors produce different explicit configuration targets.

The `run` command now binds both the configured `gameProtocolPort` and `statusProtocolPort`, remains active until Ctrl+C, applies connection caps and timeouts, and owns both listener lifecycles. The game endpoint accepts the bounded FE diagnostic probe (`FEHS`) and returns a profile-specific `FEOK` response. The status endpoint answers the TFS-style XML `info` request and the bounded binary basic/player/map/software query family with configured name, ports, uptime, and current foundation counters.

For the `fe-7.4` profile only, an operator may enable an **original, bounded legacy-login foundation** by setting `legacyLoginEnabled = true` and providing a locally generated 1024-bit RSA private key at `rsaPrivateKey`. The foundation validates a 7.4 version field, raw-RSA transport block, XTEA session key, bounded account/password strings, Argon2-backed account authentication, and an encrypted character-list-or-error response. These contracts are covered by Rust codec and persistence tests.

An operator can separately set `gameSessionEnabled = true` to expose the **fe-7.4 game-session foundation** on `gameSessionPort` (default `7173`). This deliberately nonconflicting, opt-in endpoint sends a challenge, validates a challenge-bound raw-RSA bootstrap, authenticates that the named character belongs to the supplied account, and returns an XTEA-wrapped explicit world/map feature-gate response. It is a session-boundary test harness, not a playable game endpoint.

When `gameSessionEnabled` is active, FE can advertise a separately configured public route to a custom OTClient module through `advertisedGameSessionHost` and `advertisedGameSessionPort`. These fields support a domain, proxy, tunnel, or IP-changing route without changing the local bind address. The exact FE custom-client acknowledgement and feature boundary are documented in [OTCLIENT_INTEGRATION.md](OTCLIENT_INTEGRATION.md).

FE also has a separate **profile-driven native OTClientV8 service**. When an operator explicitly sets `otclientV8NativeEnabled = true` and selects the currently supported plain numeric-account profile (`otclientV8ProtocolVersion = 740`, numeric account IDs enabled, and login encryption/checksum/challenge disabled), `run` starts normal client-facing login and game listeners on `otclientV8LoginPort` and `otclientV8GamePort` (defaults `7174` and `7175`). The login listener authenticates the numeric SQLite account ID with Argon2 and returns the standard legacy character-list layout. It does not need an FE client module or extended opcode. The advertised game endpoint must currently be an IPv4 address because that legacy character-list layout carries an IPv4 value rather than a hostname.

The matching native game listener accepts the normal pending-game selection request and verifies account ownership of the chosen character. It deliberately returns a normal client-understood game-login error stating that map initialization is pending. FE has **not** yet sent a full-map packet, a player creature, movement acknowledgements, or game-state initialization through that path. The native profile is selected from explicit `config.lua` behavior fields rather than being inferred from an FE release label, and unsupported profile combinations are rejected with an explicit configuration diagnostic.

It is still **not** an official Tibia login/game service or a playable normal OTClientV8 world. The older FE-aware custom-client endpoint returns only initial identity, stored position, endpoint metadata, and an empty-world gate; the native endpoint currently proves normal credential/character-selection interoperability only. Neither provides normal map/datapack loading, combat, scripting, or continuous world simulation.

## Path to a connectable server

| Milestone | Required implementation | Current status |
|---|---|---|
| Long-running host/status | Game and status TCP listeners, graceful shutdown, connection caps/timeouts, session logs, `config.lua` port configuration, and status responses. | Implemented for the diagnostic game endpoint and TFS-style status query families; world tick loop remains planned. |
| 7.4 login path | Login packet parsing, account/character-list response, RSA/XTEA boundary, and protocol tests based on an independently written specification. | Foundation implemented behind `legacyLoginEnabled`; authorized packet fixtures and official-client acceptance remain pending. |
| Native OTClientV8 740 login path | Profile-selected normal login/pending-game decoding, numeric account authentication, legacy character-list encoding, and client-understood errors. | Implemented behind `otclientV8NativeEnabled` for the selected plain numeric 740 profile; normal player/map initialization remains blocked on full-map serialization. |
| 7.4 game path | Game-session handshake, bounded opcode decoding/encoding, movement, visibility, chat, and client-state synchronization. | Challenge-bound bootstrap, FE-aware OTClient capability acknowledgement, stored identity/position metadata, FE-owned empty-world manifest, deterministic viewport/tick, one-step cardinal movement acknowledgement, and position persistence implemented on opt-in `gameSessionPort`; normal world-session behavior remains unimplemented. |
| World content | Original-compatible map/item loaders, validation, spawn management, and data migration rules. | Not implemented |
| Gameplay | Combat, conditions, creatures, inventories, NPC/monster behavior, and persistence transactions. | Not implemented |
| Script/runtime integration | A deliberately specified Lua or alternative scripting API with capability tests. | Planned only |
| Network release validation | Native Windows and Linux integration tests with a legal test client or synthetic protocol harness. | Linux synthetic probe host integration validated; native Windows execution remains pending. |

The correct current description is **network-capable compatibility foundation with status, bounded 7.4 login contracts, and a profile-driven normal OTClientV8 740 character-list path**, not "ready Tibia server." See [NETWORK_MILESTONE.md](NETWORK_MILESTONE.md), [OTCLIENTV8_740_NATIVE_CONTRACT.md](OTCLIENTV8_740_NATIVE_CONTRACT.md), [PARITY_ROADMAP.md](PARITY_ROADMAP.md), [RELEASE_ASSETS.md](RELEASE_ASSETS.md), and [VERSIONING.md](VERSIONING.md) for exact boundaries.
