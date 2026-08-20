# Native Map-Action Audit

## Existing supported actions

The native 740 `UseItem` path already validates an exact catalog-mapped authoritative map item
before executing the only two imported metadata-backed action types FE currently models.

| Imported metadata | Existing native result | Limits |
|---|---|---|
| Teleport destination | Revalidates the destination under the authoritative world lock, persists player relocation, cancels click-walk, resolves bounded direct hop chains, and sends the established viewport refresh | No effects, scripts, generic tile behavior, or arbitrary use semantics |
| Non-empty item text | Sends the parser-verified read-only text window | No editing, writer/date data, mutation, or generic text behavior |
| Action ID / unique ID / charges | Validated and diagnostic-only | No action registry, scripts, switches, doors, levers, or generic item semantics |

`UseItemEx`, `UseItemOnCreature`, and `RotateItem` remain deliberately validation-only because
the imported map model does not define a safe authoritative action contract for those requests.

## Decision

No additional map action is enabled by this audit. The next valid implementation requires a typed
FE-owned action registry or independently validated imported action metadata with deterministic
state, persistence, packet, and failure contracts. Adding behavior from an action ID alone would
invent compatibility rather than implement it.
