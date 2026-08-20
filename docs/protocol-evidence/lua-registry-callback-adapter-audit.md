# Lua Registry Callback Adapter Audit

FE's sandboxed dispatcher requires an explicitly registered callback function accepting exactly
`(event_kind, subject_id, value)` in a fresh restricted VM. Legacy TFS registry entries provide
script references but do not establish a matching event identity, argument shape, or supported
server API.

Automatically registering registry scripts would therefore imply arbitrary legacy Lua behavior.
No registry-backed callback adapter is added by this audit.
