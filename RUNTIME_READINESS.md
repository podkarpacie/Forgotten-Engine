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

The `run` command now binds the `gameProtocolPort` configured in `config.lua`, remains active until Ctrl+C, applies a connection cap and timeouts, and accepts a bounded FE diagnostic probe (`FEHS`). A valid probe gets a profile-specific response (`FEOK`). This is a real persistent TCP service and is covered by unit and CLI-level integration tests.

It is **not** an official Tibia login/game service. It cannot accept an official Tibia client because it does not yet implement version-specific login or game packet codecs, RSA/XTEA negotiation, character lists, map/datapack loading, combat, scripting, or continuous world simulation.

## Path to a connectable server

| Milestone | Required implementation | Current status |
|---|---|---|
| Long-running host | TCP listener, graceful shutdown, connection cap/timeouts, session logs, and `config.lua` port configuration. | Implemented for the FE diagnostic service; world tick loop remains planned. |
| 7.4 login path | Login packet parsing, account/character-list response, and protocol tests based on an independently written specification. | Not implemented |
| 7.4 game path | Game-session handshake, bounded opcode decoding/encoding, movement, visibility, chat, and client-state synchronization. | Not implemented |
| World content | Original-compatible map/item loaders, validation, spawn management, and data migration rules. | Not implemented |
| Gameplay | Combat, conditions, creatures, inventories, NPC/monster behavior, and persistence transactions. | Not implemented |
| Script/runtime integration | A deliberately specified Lua or alternative scripting API with capability tests. | Planned only |
| Network release validation | Native Windows and Linux integration tests with a legal test client or synthetic protocol harness. | Linux synthetic probe host integration validated; native Windows execution remains pending. |

The correct current description is **network-capable compatibility foundation**, not "ready Tibia server." See [NETWORK_MILESTONE.md](NETWORK_MILESTONE.md), [PARITY_ROADMAP.md](PARITY_ROADMAP.md), [RELEASE_ASSETS.md](RELEASE_ASSETS.md), and [VERSIONING.md](VERSIONING.md) for exact boundaries.
