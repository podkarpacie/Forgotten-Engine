# The Forgotten Server Compatibility Matrix

Forgotten Engine (FE) uses the official The Forgotten Server (TFS) repository as an **observable behavior and interface reference**. FE is an original Rust implementation: this matrix records compatibility targets and does not authorize source translation, copied code, copied data, client assets, or upstream credentials.

## Repository and operational surface

| Upstream surface | Original Forgotten Engine equivalent | Current state |
|---|---|---|
| `.github/` CI, lint, packaging, and release workflows | Rust `cargo fmt`, `cargo test`, release-asset scripts, and FE-focused CI definitions. | Partial: local release scripts implemented; full CI planned. |
| `cmake/`, `CMakeLists.txt`, `CMakePresets.json` | Cargo workspace, crate manifests, feature flags, Cargo profiles, and reproducible target builds. | Implemented baseline; optional feature matrix planned. |
| `Dockerfile`, `.dockerignore` | Rust multi-stage container image and container smoke test. | Planned. |
| `vc17/`, `vcpkg.json` | Cargo Windows target support, GNU/MSVC target profiles, packaging scripts, and documented OS imports. | Partial: GNU x86_64 Windows archive built; MSVC target planned. |
| `.env.example` | FE environment template for sensitive runtime settings only; public gameplay configuration remains file based. | Planned. |
| `config.lua.dist` | `config.lua` compatibility contract plus an original, bounded Rust configuration loader. | Planned; temporary `forgotten-engine.toml` remains available during migration. |
| `schema.sql` | Versioned FE database migrations, SQLite baseline, and documented MySQL/MariaDB schema strategy. | Partial: SQLite baseline implemented; relational compatibility schema planned. |
| `key.pem` | Per-world private-key management with secure generation/loading rules for a future official-client login path. | Planned; no upstream key is used. |
| `LICENSE`, `AUTHORS`, README | FE-owned licensing, attribution, security, build, and compatibility documentation. | Partial. |

## Runtime, content, and service surface

| Upstream surface | Original FE boundary | Current state |
|---|---|---|
| `src/main`, `otserv`, `server`, `signals` | `forgotten-host`: process lifecycle, signal-driven shutdown, listener supervision, startup diagnostics. | In progress: bounded TCP probe host implemented; TFS-style startup pending. |
| `src/configmanager` | `forgotten-config`: typed config schema, defaults, validation, compatibility diagnostics. | Planned. |
| `src/connection`, `protocol*`, `networkmessage`, `outputmessage`, `rsa`, `xtea` | `forgotten-protocol`: bounded framing, independent packet codecs, session negotiation, encryption boundary, and test fixtures. | Foundation only; no official Tibia codec claimed. |
| `src/game`, `map`, `tile`, `thing`, `item*`, `creature*`, `player`, `monster*`, `npc`, `spells`, `combat`, `condition` | `forgotten-world`: original world/entity simulation and deterministic game rules. | Planned. |
| `src/database*`, `iologindata`, `iomap*`, `iomarket` | `forgotten-persistence`: migrations, account/player storage, map/content persistence, and transaction boundaries. | SQLite event/account/player foundation implemented. |
| `src/luascript`, `script*`, `actions`, `events`, `talkaction`, `weapons`, `movement` | `forgotten-scripting`: bounded Lua-compatible scripting host and capability matrix. | Inventory only; execution runtime planned. |
| `src/scheduler`, `tasks`, `spawn`, `globalevent` | `forgotten-scheduler`: deterministic tick, timed events, spawn, and task supervision. | Planned. |
| `src/protocolstatus` | `forgotten-status`: status protocol and monitoring endpoint with rate controls. | Planned. |
| `data/` actions, creaturescripts, events, globalevents, items, lib, migrations, monster, movements, npc, spells, talkactions, weapons, world, XML | `fe-data/` versioned content manifests, original loader interfaces, validation reports, and migration contracts. | Planned; no upstream data is copied. |

## Compatibility status vocabulary

| Status | Meaning |
|---|---|
| **Implemented** | Available in FE, covered by tests, and documented with its exact boundary. |
| **Partial** | Interface exists but is not feature complete or not official-client interoperable. |
| **Planned** | Accepted target with a specified Rust boundary but no production behavior yet. |
| **Reference-only** | Observable upstream surface informing behavior, with no direct FE source/data reuse. |
| **Not applicable** | Upstream toolchain mechanics replaced by an FE-native Rust equivalent. |

## Versioned priority order

The first implementation target is a TFS-style startup/configuration/content contract because it determines how operators run, diagnose, validate, and deploy the service. The next targets are service registration, bounded session negotiation, scheduler, persistence migration, scripting capability host, and original world simulation. Official client interoperability must wait for a complete independently tested version-specific codec and login/game session contracts.

## References

1. [Official The Forgotten Server repository](https://github.com/otland/forgottenserver)
2. [Official The Forgotten Server configuration template](https://github.com/otland/forgottenserver/blob/master/config.lua.dist)
