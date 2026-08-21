# Native 740 Explicit Attack-Target Clear Audit

## Scope

This record covers one narrow classic 740 combat-control behavior. It does not claim player combat,
damage formulas, target authorization, movement, effects, loot, PvP, scripts, or TFS combat parity.

## Client and reference evidence

Public OTCv8 development sources define `ClientAttack` as opcode `161` (`0xA1`) and write one
unsigned 32-bit creature ID in `ProtocolGame::sendAttack`. The optional attack-sequence extension
is feature-gated and is not selected for FE’s bounded classic 740 profile.

The local TFS reference parser reads one creature ID for `parseAttack` and schedules the server’s
authoritative attack-target transition. That transition removes the current target and sends its
cancel-target record when it receives a zero ID or cannot resolve a requested creature. FE uses
this behavior only as an independent reference map; it does not copy upstream source.

## FE contract

FE’s existing profile-gated decoder maps the exact `0xA1 + u32` request to `SelectTarget`. For a
zero target ID, the native session clears only the authenticated player’s existing target through
the shared authoritative world and emits the existing parser-verified zero-payload `ClearTarget`
record (`0xA3`). A rejected nonzero target retains its established identical response.

The regression `native_explicit_target_clear_emits_classic_clear_target_record` opens a live native
740 session, sends the exact five-byte action record, and asserts the complete one-byte response.
No raw packet diagnostics are recorded.

## Deferred behavior

The route has no attack execution, cooldown, fight-mode formula, target-permission rule, movement,
visual effect, loot, corpse, PvP, Lua, or real-client compatibility claim.
