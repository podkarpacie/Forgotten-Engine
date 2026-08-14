# Forgotten Engine

**Forgotten Engine** is an original Rust foundation for a modern, local-first OpenTibia server platform. **FE 1.2.0** establishes a versioned clean-room compatibility foundation for **TFS 1.2** and the **Tibia 10.98** protocol target: deterministic world state, embedded SQLite storage, migrations, diagnostics, backups, binary packet framing, a TFS Lua compatibility inventory, and a command-line workflow.

> This project is not a port or source translation of The Forgotten Server. It uses independently implemented Rust code and treats upstream projects only as behavior and terminology references. No Tibia client assets, maps, item databases, or copyrighted game data are included.

## FE 1.2.0 scope

| Area | Delivered foundation |
|---|---|
| Local server lifecycle | Offline/starting/online/stopping state machine with audited transitions. |
| Persistence | SQLite auto-creation, migrations, account/player records, and event ledger. |
| Operations | `init`, `validate`, `run`, `status`, `backup`, `command`, `compatibility`, and `version` CLI commands. |
| Protocol | Explicit FE 1.2.0 / TFS 1.2 / Tibia 10.98 profile plus bounded, length-prefixed packet framing with malformed-frame rejection. |
| Scripting | Honest capability matrix for a narrow TFS 1.2 Lua compatibility surface. |
| Safety | Content validation, backup manifests, structured diagnostics, and no unsafe Rust. |

## Quick start

```bash
cargo run -p forgotten-cli -- init ./my-server
cargo run -p forgotten-cli -- validate ./my-server
cargo run -p forgotten-cli -- run ./my-server
cargo run -p forgotten-cli -- command ./my-server broadcast "Welcome to Forgotten Engine"
cargo run -p forgotten-cli -- backup ./my-server
```

The `init` command creates a `server.toml` configuration and an embedded `data/server.db` automatically. The generated configuration declares `engine_version = "1.2.0"`, `tfs_reference = "1.2"`, `protocol = "10.98"`, and `database.driver = "sqlite"`.

> FE 1.2.0 does not claim complete Tibia 10.98 protocol emulation or drop-in TFS replacement status. Login encryption, opcode coverage, map/datapack ingestion, Lua VM parity, combat, and production multiplayer are intentionally outside this release’s supported scope.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for crate boundaries, the Cloud host-agent contract, and the clean-room compatibility policy. See [VERSIONING.md](VERSIONING.md) for the published FE-to-TFS/Tibia mappings and [RELEASE_NOTES_FE_1_2_0.md](RELEASE_NOTES_FE_1_2_0.md) for release-specific changes. The roadmap is intentionally incremental: full client protocol, map loaders, content ingestion, combat, and a real Lua VM require independent behavior specifications and test suites before they can be advertised as supported.

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
