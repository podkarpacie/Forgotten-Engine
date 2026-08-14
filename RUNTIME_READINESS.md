# Forgotten Engine Runtime Readiness

## Windows package dependencies

The FE 7.4.0 Windows archive contains one program, `forgotten-engine.exe`, plus documentation. Its PE import table was inspected after packaging. The executable does **not** import Boost, OpenSSL, MariaDB/MySQL, zlib, bz2, pugixml, or a separate SQLite DLL. SQLite is compiled into the binary through the Rust `rusqlite` bundled feature.

| Imported runtime library | Source | Needs to ship beside the EXE? | Purpose at this stage |
|---|---|---:|---|
| `KERNEL32.dll`, `ntdll.dll`, `USERENV.dll`, `api-ms-win-core-synch-l1-2-0.dll` | Windows operating system | No | Standard Windows process, environment, and synchronization APIs. |
| `msvcrt.dll` | Windows operating system | No | C runtime imported by the GNU Windows target. |
| `bcryptprimitives.dll` | Windows operating system | No | Windows cryptographic primitive support used by the Rust runtime. |
| `WS2_32.dll` | Windows operating system | No | Winsock import available to the Rust/Windows runtime. It does not mean that FE currently opens a game socket. |

> The Windows build is therefore a single application **with normal operating-system DLL imports**, not a completely static PE file. It should not need the collection of third-party DLLs bundled by the C++ The Forgotten Server distribution shown in the comparison image.

## What the EXE can do today

The release binary can initialize a local Forgotten Engine world, create its SQLite database, validate profile-specific configuration, record lifecycle/administration events, create backups, and report the supported compatibility profiles. The `fe-7.4`, `fe-8.0`, and `fe-1.2` selectors produce different explicit configuration targets.

The `run` command currently simulates the local state transition to `online` and then exits. It does **not** bind a login port, accept a Tibia client, speak the login or game protocol, perform RSA/XTEA negotiation, load a map/datapack, run combat, or keep a server process alive. A Tibia client therefore cannot connect to this FE release yet.

## Path to a connectable server

| Milestone | Required implementation | Current status |
|---|---|---|
| Long-running host | TCP listener, graceful shutdown, world tick loop, structured server logs, and port configuration. | Not implemented |
| 7.4 login path | Login packet parsing, account/character-list response, and protocol tests based on an independently written specification. | Not implemented |
| 7.4 game path | Game-session handshake, bounded opcode decoding/encoding, movement, visibility, chat, and client-state synchronization. | Not implemented |
| World content | Original-compatible map/item loaders, validation, spawn management, and data migration rules. | Not implemented |
| Gameplay | Combat, conditions, creatures, inventories, NPC/monster behavior, and persistence transactions. | Not implemented |
| Script/runtime integration | A deliberately specified Lua or alternative scripting API with capability tests. | Planned only |
| Network release validation | Native Windows and Linux integration tests with a legal test client or synthetic protocol harness. | Not implemented |

Until the first two network milestones exist, the correct description is **local compatibility foundation**, not "ready Tibia server." See [RELEASE_ASSETS.md](RELEASE_ASSETS.md) for archive contents and [VERSIONING.md](VERSIONING.md) for the profile boundaries.
