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

## Tibia 8.0 reference point

Tibia 8.0 is an independent compatibility target, not a TFS 1.2 target. The public YATC project documents support beginning at Tibia-compatible protocol 8.0, which confirms that 8.0 is a distinct protocol line suitable for a separately versioned FE profile. FE will implement this profile from published behavior specifications and original tests rather than copied server or client code.

| Forgotten Engine release | Compatibility reference | Tibia protocol target | Status |
|---|---|---:|---|
| FE 8.0.0 | Tibia 8.0 protocol | 8.0 | Planned clean-room compatibility foundation |

## Additional source

3. [YATC protocol-support statement](https://github.com/opentibia/yatc)
4. [Tibia Specifications Initiative](https://otland.net/threads/tibia-specifications-initiative.241415/)

## Tibia 7.4 reference point

Tibia 7.4 is another direct protocol target rather than a TFS 1.2 derivative. Public OpenTibia projects identify 7.4 as a supported protocol line, including one distribution that names 7.4 as its default protocol and another independently written server that documents experimental support beginning at 7.40. These projects are used only as interoperability references; Forgotten Engine remains an original Rust implementation.

| Forgotten Engine release | Compatibility reference | Tibia protocol target | Status |
|---|---|---:|---|
| FE 7.4.0 | Tibia 7.4 protocol | 7.4 | Planned clean-room compatibility foundation |

## Additional sources

5. [Avesta 7.4 protocol statement](https://github.com/peonso/avesta74)
6. [OpenTibia 7.40 experimental-support statement](https://github.com/mtanksl/OpenTibia)

## Sources

1. [Official TFS v1.2 GitHub release](https://github.com/otland/forgottenserver/releases/tag/v1.2)
2. [Original TFS 1.2 release announcement](https://otland.net/threads/the-forgotten-server-1-2.246641/)
