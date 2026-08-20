# Native UseItem Container-Open Audit

The native 740 UseItem route is intentionally map-only. It requires a loaded world map and then
uses the exact authoritative map-item validator before its two supported actions: imported
teleport metadata and read-only OTBM text.

FE's persisted top-level container record retains a root `container_item`, but the current model
does not establish an unambiguous identity between that record and a particular equipment or map
position for UseItem. Matching by server item ID alone could open the wrong container when a player
owns multiple identical roots.

No open-container action is added. A future slice needs a stable root location/identity model,
exact classic packet evidence for the intended source position, and session view-state rules before
reusing the existing open-container frame.
