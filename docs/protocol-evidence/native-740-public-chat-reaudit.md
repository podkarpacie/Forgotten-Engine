# Native 740 Public Chat Re-audit

The local OTCv8 protocol dispatch recognizes server Talk (`0xAA`) generally, but its
`buildMessageModesMap` creates the legacy Say/Whisper/Yell mappings only for protocol versions
`760` and later. The local version `740` path has no server message-mode entries. Translating a
mode therefore yields the client’s unknown sentinel rather than a safe Say mode.

FE keeps the existing bounded shared chat intake and queue, but suppresses outbound native 740
Talk frames. Sending `0xAA` with a guessed mode caused the documented client-side “unknown message
mode 255” parser error; no guessed replacement is safe.

No client-visible 740 chat packet is added by this audit. A future route requires independent
740-compatible parser evidence from the exact client build and a complete outbound field contract.
