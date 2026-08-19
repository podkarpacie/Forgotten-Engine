# Lua Registry Adapter Audit

## Scope

This audit reviewed whether FE needs a new adapter from parsed TFS XML registry metadata to its existing sandboxed Lua callback runtime.

## Existing FE boundary

FE already provides the required safe boundary through the stable explicit command:

```text
script dispatch <directory> <category> <declared-relative-script> <callback-name> <event-kind> <subject-id> <value>
```

The command accepts only typed script-capable registry categories. It resolves the requested relative path against the already validated registry metadata, then passes that single declared file to the callback dispatcher. The sandbox has restricted libraries, bounded source size, bounded instructions, bounded memory, and primitive-only typed output.

| Behavior | Current status |
|---|---|
| TFS registry parsing and declared path validation | Partial and supported as a conversion boundary. |
| Explicit operator-selected callback execution | Partial and supported through `script dispatch`. |
| Automatically running imported registry events | Deferred. |
| TFS Lua APIs, global state, module loading, or gameplay mutation | Deferred. |
| General imported-script compatibility | Deferred. |

## Decision

No new registry adapter is added. The existing explicit adapter is the safe boundary and has a CLI regression proving a declared file can dispatch while undeclared paths are rejected. Automatic execution would cross the current safety boundary and requires separately modeled FE APIs, event contracts, determinism, and sandbox authority controls.
