# Native Dead-Player Target Cleanup Audit

FE already sends the parser-verified zero-payload ClearTarget (`0xA3`) record to the native
attacker that actually defeats its selected player target. The helper is deliberately session
scoped and documents that it does not notify other sessions or implement generic combat
cancellation.

The authoritative death transition preserves other players' target and follow intents. A safe
cross-session cleanup would need explicit ownership of affected sessions, target-versus-follow
semantics, a client delivery order, and broader PvP/lifecycle policy. No broadcast cleanup is
added by this audit.
