# Native 740 Outfit Persistence Audit

## Existing supported boundary

FE already supports a bounded persisted native 740 outfit change. A client change must use an
operator-configured inclusive classic look-type range. An accepted change persists the selected
look type and head, body, legs, and feet colours through the schema-v11 player outfit fields, then
updates the current session and visible peer sessions using the existing parser-verified creature
outfit record.

On the next session, the stored outfit is hydrated. The configured fallback remains available only
when no valid stored appearance exists.

| Behavior | Status |
|---|---|
| Validated classic look type and four colour bytes | Supported. |
| SQLite persistence and relog hydration | Supported. |
| Current-session creature-outfit update | Supported. |
| Visible peer-session propagation | Supported. |
| Client asset validation, addons, mounts, or arbitrary appearance metadata | Deferred. |
| Real unmodified OTCv8 persistence confirmation | Deferred. |

## Decision

No new outfit persistence behavior is added. The requested Rust-side persistence path is already
implemented and tested. The remaining required evidence is a real unmodified OTCv8 740 test:
change an accepted look type and colours, relog, and confirm the stored appearance appears without
a parser error or disconnect.
