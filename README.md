# Forgotten Engine

Forgotten Engine is an original Rust project pursuing clean-room behavioral and operational compatibility with selected Forgotten Server and Tibia protocol targets. It does not copy The Forgotten Server source code or distribute Tibia client assets, maps, item databases, or game data.

## Current profiles

| Profile | Compatibility target |
|---|---|
| `fe-7.4` | Tibia 7.4 |
| `fe-8.0` | Tibia 8.0 |
| `fe-1.2` | TFS 1.2 / Tibia 10.98 |

The current native OTCv8 740 path is experimental. It supports numeric-account login, character selection, and an opt-in generated empty-world fixture. It is not yet a general-purpose playable server: real map loading, map streaming, collision, items, creatures, scripting, and gameplay are still under development.

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

Operator documentation, compatibility notes, and changelogs will be maintained in the future Forgotten Engine GitBook rather than duplicated throughout this repository.
