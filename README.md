# Forgotten Engine

**Forgotten Engine** is an original Rust foundation for a modern, local-first OpenTibia server platform. The initial release establishes the safe operational seams needed before a full Tibia 8.0 implementation: deterministic world state, embedded SQLite storage, migrations, diagnostics, backups, binary packet framing, a TFS Lua compatibility inventory, and a command-line workflow.

> This project is not a port or source translation of The Forgotten Server. It uses independently implemented Rust code and treats upstream projects only as behavior and terminology references. No Tibia client assets, maps, item databases, or copyrighted game data are included.

## Current 0.1 scope

| Area | Delivered foundation |
|---|---|
| Local server lifecycle | Offline/starting/online/stopping state machine with audited transitions. |
| Persistence | SQLite auto-creation, migrations, account/player records, and event ledger. |
| Operations | `init`, `validate`, `run`, `status`, `backup`, `command`, and `compatibility` CLI commands. |
| Protocol | Bounded, length-prefixed packet framing with malformed-frame rejection. |
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

The `init` command creates a `server.toml` configuration and an embedded `data/server.db` automatically. The generated configuration deliberately defaults to `protocol = "8.0"` and `database.driver = "sqlite"`.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for crate boundaries, the Cloud host-agent contract, and the clean-room compatibility policy. The roadmap is intentionally incremental: full client protocol, map loaders, content ingestion, combat, and a real Lua VM require independent behavior specifications and test suites before they can be advertised as supported.

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
