# Forgotten Engine

Forgotten Engine is an original Rust project pursuing clean-room behavioral and operational compatibility with selected Forgotten Server and Tibia protocol targets. It does not copy The Forgotten Server source code or distribute Tibia client assets, maps, item databases, or game data.

## Current profiles

| Profile | Compatibility target |
|---|---|
| `fe-7.4` | Tibia 7.4 |
| `fe-8.0` | Tibia 8.0 |
| `fe-1.2` | TFS 1.2 / Tibia 10.98 |

The current native 740 path is experimental. It supports numeric-account login, character selection, operator-supplied map loading with streamed native viewports, authoritative walkability and occupancy, bounded shared-player visibility, static spawned-creature representation, persisted player vitals, and a deliberately bounded selected-player melee foundation. It is not yet a general-purpose playable server: runtime items/inventory, weapon formulas, spells, monster AI/respawn/loot, NPC behavior, Lua execution, social/economy systems, and most gameplay semantics remain under development.

## Capability matrix

The source-controlled [capability matrix](docs/capability-matrix.md) is the authoritative statement of each profile’s supported, partial, and deferred features. Operators can also use `forgotten-engine compatibility` for the concise human-readable summary or `forgotten-engine compatibility --json` for the same profile baseline in a machine-readable form.

## Build

```bash
cargo build --release
```

For a Windows cross-build on a Linux build machine:

```bash
cargo build --release --target x86_64-pc-windows-gnu
```

## Basic use

```bash
forgotten-engine init ./my-world --profile fe-7.4
forgotten-engine validate ./my-world
forgotten-engine run ./my-world
```

## Docker

```bash
docker compose up --build
```

Provision the mounted world directory first (`forgotten-engine init ./my-world`), then point the
compose file at it. The image runs as a non-root user and exposes the classic status/login, game,
and native OTClient ports; all real port bindings come from your `config.lua`.

Operator documentation, compatibility notes, and changelogs will be maintained in the future Forgotten Engine GitBook rather than duplicated throughout this repository.
