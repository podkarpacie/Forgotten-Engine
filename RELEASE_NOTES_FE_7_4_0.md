# FE 7.4.0 — Tibia 7.4 Compatibility Foundation

FE 7.4.0 adds the direct Tibia 7.4 compatibility profile to Forgotten Engine. It preserves the distinct FE 8.0.0 / Tibia 8.0 and FE 1.2.0 / TFS 1.2 / Tibia 10.98 profile lines.

The release adds the `fe-7.4` selector, profile-specific configuration and diagnostics, cross-profile regression coverage, and precompiled Linux and Windows release assets with SHA-256 checksums. The Linux asset is executed after packaging; the Windows asset is cross-compiled with the Rust GNU Windows target and MinGW-w64, then structurally verified as a 64-bit Windows PE executable. It does not claim complete Tibia 7.4 login, encryption, opcode, map, content, combat, scripting, or production-networking emulation.

See [VERSIONING.md](VERSIONING.md) for profile mappings and [RELEASE_ASSETS.md](RELEASE_ASSETS.md) for binary installation and validation details.
