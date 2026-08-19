# Native 740 Public-Chat Delivery Audit

## Scope

This note records the compatibility boundary for **unmodified OTCv8 configured for protocol 740**. It does not define a generic chat protocol or claim visual public-chat delivery.

## Parser evidence

The audited OTCv8 development source builds its server-message-mode map only for protocol versions **760 and newer**. The `parseTalk` and `parseTextMessage` handlers translate their incoming mode byte through that map before reading the relevant layout. For protocol 740, the map remains empty, so an invented speech or text-message mode becomes `MessageInvalid` and the client throws an unknown-message-mode parser error.

FE's existing `0xB4` encoder is separately limited to the verified `MSG_STATUS_DEFAULT` byte (`0x15`) and the fixed string layout used by bounded inspection responses. It is not evidence of a normal public-speech layout, does not contain speaker or position fields, and must not be relabeled as public chat.

## Current FE behavior

FE accepts and sanitizes bounded native 740 chat input, then distributes the resulting event through its bounded shared-world recipient queues. At the native-session boundary, `drain_shared_public_chat` consumes those events without emitting an outbound record. Extended diagnostics report only that delivery was suppressed and the text byte count; they never emit text contents or packet bodies.

| Area | Current state |
|---|---|
| Native 740 chat request decoding | Partial, input is safely consumed and bounded. |
| Shared event fanout | Partial, sanitized events are delivered through bounded in-memory queues. |
| Client-visible public speech | Deferred. No parser-verified 740 server-talk layout is available. |
| `0xB4` status text | Partial, reserved for verified status/inspection records; not used as public chat. |

## Decision

**No outbound public-chat feature is enabled for native 740.** This prevents the known unmodified-client parser failure and preserves the distinction between an internal event foundation and a supported client-visible gameplay capability. A future implementation requires new reproducible 740 parser evidence and an authenticated socket regression for the exact outbound layout.
