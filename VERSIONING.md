# Forgotten Engine Versioning and Compatibility

Forgotten Engine (FE) uses its own semantic versioning. A matching The Forgotten Server (TFS) number is a **compatibility reference**, not an assertion that FE contains, derives from, or reproduces upstream source code. Each FE release declares exactly which client/protocol target is tested and which parts of that target remain outside the supported scope.

## FE 1.2.0

**FE 1.2.0** is the first tagged clean-room compatibility foundation. It takes **TFS 1.2** as its reference point and targets the **Tibia 10.98** game protocol. The upstream TFS 1.2 announcement identifies game protocol 10.98, while the upstream project released its `v1.2` tag on 22 October 2016.[1][2]

| FE release | TFS reference | Tibia target | Supported in this release |
|---|---|---:|---|
| FE 1.2.0 | TFS 1.2 | 10.98 | Profile identification, bounded packet framing, deterministic world state, SQLite persistence, diagnostics, and local lifecycle/backup commands. |

> FE 1.2.0 is not a drop-in, full-game TFS 1.2 replacement yet. It does not claim complete login encryption, RSA/XTEA negotiation, opcode coverage, map/datapack loading, Lua API parity, or production multiplayer operation.

## Roadmap references

The table below separates known upstream references from FE implementation commitments. A row marked **not implemented** is a planning record only.

| Future FE line | Upstream reference | Tibia protocol | Status |
|---|---|---:|---|
| FE 1.4 | TFS 1.4 | 10.98 | Not implemented; the official 1.4 release includes 10.98 datapack/map updates.[3] |
| FE 1.6 | TFS 1.6 | 13.10 | Not implemented; the official 1.6 release describes protocol 13.10.[4] |

The official upstream release list contains `v1.0`, `v1.1`, `v1.2`, `v1.4`, `v1.4.1`, `v1.4.2`, and `v1.6`; it does not publish a `v1.3` or `v1.5` release tag. FE will not invent mappings for untagged upstream releases.[5]

## Tag format

The Git tag format is `fe-vMAJOR.MINOR.PATCH`. For this release, the tag is **`fe-v1.2.0`**. GitHub release titles use a readable form, such as **FE 1.2.0 — TFS 1.2 / Tibia 10.98 Compatibility Foundation**.

## References

1. [The Forgotten Server 1.2 release announcement](https://otland.net/threads/the-forgotten-server-1-2.246641/)
2. [Official TFS v1.2 GitHub release](https://github.com/otland/forgottenserver/releases/tag/v1.2)
3. [Official TFS v1.4 GitHub release](https://github.com/otland/forgottenserver/releases/tag/v1.4)
4. [Official TFS v1.6 GitHub release](https://github.com/otland/forgottenserver/releases/tag/v1.6)
5. [Official TFS release list](https://github.com/otland/forgottenserver/releases)
