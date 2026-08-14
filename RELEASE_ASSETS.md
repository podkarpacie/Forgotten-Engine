# Forgotten Engine Precompiled Release Assets

Forgotten Engine publishes executable archives for a specific tagged source revision. Archives contain the `forgotten-engine` executable, a profile matrix, installation instructions, and a SHA-256 checksum manifest. They do not contain Tibia client assets, maps, item databases, or copyrighted game content.

> **Readiness notice:** The archives are local compatibility foundations, not network-ready Tibia servers. The executable does not currently bind a login/game port or accept a Tibia client. See [RUNTIME_READINESS.md](RUNTIME_READINESS.md) before deploying an archive.

## FE 7.4.0 asset matrix

| Archive | Build environment | Executable | Compatibility profiles in the binary | Verification |
|---|---|---|---|---|
| `forgotten-engine-fe-v7.4.0-linux-x86_64.zip` | Ubuntu x86_64 | `forgotten-engine` | FE 7.4.0, FE 8.0.0, FE 1.2.0 | Release build, workspace tests, profile initialization, profile validation, and `--version` smoke test. |
| `forgotten-engine-fe-v7.4.0-windows-x86_64.zip` | Ubuntu x86_64 cross-compile with Rust `x86_64-pc-windows-gnu` target and MinGW-w64 | `forgotten-engine.exe` | FE 7.4.0, FE 8.0.0, FE 1.2.0 | Release build, workspace tests, target-specific linker build, PE-header verification, and archive checksum validation. |
| `SHA256SUMS-fe-v7.4.0-linux-x86_64.txt` and `SHA256SUMS-fe-v7.4.0-windows-x86_64.txt` | Platform-specific packaging steps | N/A | N/A | SHA-256 value for the corresponding attached precompiled archive. |

## Installation

Extract the archive, then initialize a local world with an explicit profile:

```bash
./forgotten-engine init ./my-world --profile fe-7.4
./forgotten-engine validate ./my-world
```

On Windows, use `forgotten-engine.exe` in PowerShell or Command Prompt. The generated `forgotten-engine.toml` names the chosen profile, compatibility reference, protocol target, and SQLite data location.

The Windows archive needs no bundled Boost, OpenSSL, MariaDB/MySQL, zlib, bz2, pugixml, or SQLite DLLs. It does retain normal Windows operating-system imports, including `KERNEL32.dll`, `msvcrt.dll`, and `WS2_32.dll`; these are supplied by Windows rather than the archive.

> An executable archive is a reproducible compatibility foundation, not a full production multiplayer server. Do not treat the binary as claiming complete client protocol, game content, map, scripting, combat, or login encryption compatibility.
