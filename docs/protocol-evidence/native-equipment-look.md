# Native Equipment Look

## Bounded implemented behavior

Stock OTCv8 sends ordinary LookMap for non-creatures. FE accepts one additional classic 740 form:
`x = 0xFFFF`, an unflagged fixed equipment-slot code in `y`, `z = 0`, and stack position zero.
The caller must currently own an item in that exact slot and the item’s validated native catalog
record must contain the exact requested client thing ID.

When all checks pass, FE returns the existing parser-shaped classic status-text record. The text
contains only the fixed slot code, FE server item ID, and item count. The request changes no
equipment, map, container, persistence, or combat state.

## Deliberate exclusions

Container-flagged inventory positions, nonzero stack positions, map-ground items, descriptions,
attributes, weight, generated names, writable text, generic container inspection, and full TFS
item-description behavior remain deferred.
