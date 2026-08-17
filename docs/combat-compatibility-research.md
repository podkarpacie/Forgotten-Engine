# Combat Compatibility Research Boundary

## Purpose

This note records the public behavioral context used to choose FE’s next combat milestones. It is not an implementation specification and does not authorize source-code reuse. Forgotten Engine remains a clean-room Rust implementation.

## Public observations

The public [The Forgotten Server repository][1] identifies the upstream project as an MMORPG server emulator and exposes a conventional `data` / `src` project structure. FE uses that project only as a compatibility-research reference; it does not copy source code, data packs, maps, item databases, or client assets.

FE’s existing vocation compatibility research records that public TFS-style vocation metadata includes an attack-interval concept alongside progression and regeneration fields.[2] That observation supports a **typed, profile-neutral timing contract** in FE. It does not prove a numeric formula for Tibia 7.4, 8.0, or TFS 1.2 / Tibia 10.98.

## FE scope decision

The current FE foundation therefore supports only typed physical adjacent-melee events, bounded requested damage, deterministic server-tick cooldowns, and profile-neutral flat physical mitigation. Defensive values are not inferred from TFS armor, shielding, equipment, skills, vocations, or PvP rules. Weapon, spell, ranged, resistance, client-effect, and legacy-formula behavior remain deferred until profile-specific public evidence and independent tests exist.

| Topic | Current FE decision | Compatibility status |
|---|---|---|
| Attack timing | Deterministic server-tick cooldown state | Foundation only; not a profile formula claim |
| Physical mitigation | Explicit flat reduction supplied by authoritative state | Foundation only; not armor/shielding parity |
| Weapon definitions | Registry paths are audited but script behavior is not executed | Deferred runtime |
| Spells, projectiles, PvP | No runtime claim | Deferred |

## References

[1]: https://github.com/otland/forgottenserver "otland/forgottenserver public repository"
[2]: ./progression-compatibility-research.md "Forgotten Engine progression compatibility research"
