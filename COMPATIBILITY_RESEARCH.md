# Compatibility Research Notes

## TFS 1.2 reference point

The official upstream `v1.2` release is described as the third stable release in The Forgotten Server 1.x series and was published on 22 October 2016. Its original release announcement identifies **Game protocol 10.98** as the target.

Forgotten Engine uses this record only to define an externally observable compatibility target. The Rust implementation is original; this repository does not copy or incorporate upstream The Forgotten Server source code.

## Initial FE mapping decision

| Forgotten Engine release | Compatibility reference | Tibia protocol target | Status |
|---|---|---:|---|
| FE 1.2.0 | TFS 1.2 | 10.98 | Planned clean-room compatibility foundation |

## Additional verified upstream references

The official TFS 1.4 release record identifies its updated map/data context as 10.98. The official TFS 1.6 release explicitly identifies protocol 13.10. These references are recorded for future FE planning only; FE does not yet claim support for either compatibility target.

| Future FE line | Upstream TFS reference | Tibia protocol reference | FE status |
|---|---|---:|---|
| Candidate future line | TFS 1.4 | 10.98 | Not implemented |
| Candidate future line | TFS 1.6 | 13.10 | Not implemented |

## Initial implementation finding

The initial Engine protocol crate labels itself as an `8.0` future-work foundation. That label conflicts with the requested FE 1.2 compatibility target. FE 1.2.0 must instead expose an explicit **Tibia 10.98** profile, while keeping framing code clearly separated from a claim of complete network-protocol emulation.

## Sources

1. [Official TFS v1.2 GitHub release](https://github.com/otland/forgottenserver/releases/tag/v1.2)
2. [Original TFS 1.2 release announcement](https://otland.net/threads/the-forgotten-server-1-2.246641/)
