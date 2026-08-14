# FE 7.4.0 — Tibia 7.4 Compatibility Foundation

FE 7.4.0 adds the direct Tibia 7.4 compatibility profile to Forgotten Engine. It preserves the distinct FE 8.0.0 / Tibia 8.0 and FE 1.2.0 / TFS 1.2 / Tibia 10.98 profile lines.

The release adds the `fe-7.4` selector, profile-specific configuration and diagnostics, cross-profile regression coverage, and precompiled Linux and Windows release assets with SHA-256 checksums. The Linux asset is executed after packaging; the Windows asset is cross-compiled with the Rust GNU Windows target and MinGW-w64, then structurally verified as a 64-bit Windows PE executable. **The executable is not yet a connectable Tibia server:** it does not bind ports or implement the login/game protocol. See [RUNTIME_READINESS.md](RUNTIME_READINESS.md) for the dependency audit and missing network milestones.

## Runtime readiness correction

The Windows archive does not need the third-party DLL set commonly shipped beside the C++ The Forgotten Server executable. Its import table contains only Windows-supplied libraries: `KERNEL32.dll`, `USERENV.dll`, `WS2_32.dll`, `api-ms-win-core-synch-l1-2-0.dll`, `bcryptprimitives.dll`, `msvcrt.dll`, and `ntdll.dll`. SQLite is bundled into the Rust binary, so no `sqlite3.dll` is required.

The EXE is a local compatibility foundation, **not a connectable Tibia server**. It does not bind a login/game port, accept a client, or implement the login/game protocol. The detailed readiness audit and implementation roadmap are available in [RUNTIME_READINESS.md](https://github.com/podkarpacie/Forgotten-Engine/blob/main/RUNTIME_READINESS.md).

See [VERSIONING.md](VERSIONING.md) for profile mappings and [RELEASE_ASSETS.md](RELEASE_ASSETS.md) for binary installation and validation details.
