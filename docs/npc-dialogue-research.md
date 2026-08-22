# Static NPC Dialogue Research

The current static-spawn materialization path resolves both monster and NPC definitions, but it intentionally collapses them into the same render-only `FeTfsStaticEntity` structure. Monster-only experience and direct-melee metadata are retained separately. NPC kind, script presence, and dialogue semantics are not carried into the active static-creature runtime.

The authoritative static-creature runtime can return a complete active collection for rendering and can identify entities by stable ID and current position. It cannot currently distinguish an active NPC from an active monster. A dialogue route must therefore preserve a validated NPC identity map keyed by the stable materialized ID; matching only an entity name would be unsafe when an operator configuration contains overlapping names.

The existing classic 740 public-speech encoder accepts a validated speaker name, position, and bounded text. A future NPC response can reuse that parser-backed record for the speaking player without inventing a separate wire format. The response route must remain limited to one deterministic nearby active NPC, exact sanitized public `Say` keywords, and bounded operator-owned text. Lua callbacks, focus state, parameters, shops, travel, quests, timing, and generic TFS NPC behavior remain outside this milestone.
