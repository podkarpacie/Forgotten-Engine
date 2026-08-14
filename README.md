# Forgotten Engine

**Forgotten Engine** is an original Rust foundation for a modern, local-first OpenTibia server platform. It provides versioned clean-room foundations for the direct **Tibia 8.0** protocol target and for **TFS 1.2 / Tibia 10.98**: deterministic world state, embedded SQLite storage, migrations, diagnostics, backups, binary packet framing, a compatibility inventory, and a command-line workflow.

> This project is not a port or source translation of The Forgotten Server. It uses independently implemented Rust code and treats upstream projects only as behavior and terminology references. No Tibia client assets, maps, item databases, or copyrighted game data are included.

## Compatibility-profile scope

| Area | Delivered foundation |
|---|---|
| Local server lifecycle | Offline/starting/online/stopping state machine with audited transitions. |
| Persistence | SQLite auto-creation, migrations, account/player records, and event ledger. |
| Operations | `init`, `validate`, `run`, `status`, `backup`, `command`, `compatibility`, and `version` commands via the `forgotten-engine` executable. |
| Protocol | Explicit FE 8.0.0 / Tibia 8.0 and FE 1.2.0 / TFS 1.2 / Tibia 10.98 profiles plus bounded, length-prefixed packet framing with malformed-frame rejection. |
| Scripting | Honest capability matrix for a narrow TFS 1.2 Lua compatibility surface. |
| Safety | Content validation, backup manifests, structured diagnostics, and no unsafe Rust. |

## Quick start

```bash
cargo run -p forgotten-engine-cli -- init ./my-engine-world --profile fe-8.0
cargo run -p forgotten-engine-cli -- validate ./my-engine-world
cargo run -p forgotten-engine-cli -- run ./my-engine-world
cargo run -p forgotten-engine-cli -- command ./my-engine-world broadcast "Welcome to Forgotten Engine"
cargo run -p forgotten-engine-cli -- backup ./my-engine-world
```

The `init` command creates a `forgotten-engine.toml` configuration and an embedded `data/world.db` automatically. It defaults to `fe-1.2`, and `--profile fe-8.0` selects the direct Tibia 8.0 line. The generated configuration always records its profile, compatibility reference, protocol target, and SQLite storage selection.

> FE 8.0.0 and FE 1.2.0 do not claim complete client-protocol emulation or drop-in replacement status. Login encryption, opcode coverage, map/datapack ingestion, Lua VM parity, combat, and production multiplayer are intentionally outside their supported scope.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for crate boundaries, the Cloud host-agent contract, and the clean-room compatibility policy. See [VERSIONING.md](VERSIONING.md) for the published FE-to-TFS/Tibia mappings and [RELEASE_NOTES_FE_1_2_0.md](RELEASE_NOTES_FE_1_2_0.md) for release-specific changes. The roadmap is intentionally incremental: full client protocol, map loaders, content ingestion, combat, and a real Lua VM require independent behavior specifications and test suites before they can be advertised as supported.

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
