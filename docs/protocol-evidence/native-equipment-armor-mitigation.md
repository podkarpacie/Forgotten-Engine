# Native Equipment Armor Mitigation

## Bounded implemented behavior

FE retains validated positive `armor` metadata from operator-supplied `data/items/items.xml` for
known OTB server IDs. On the existing typed declarative selected-player weapon path, the target's
current authoritative equipment is read under the shared-world boundary before combat resolution.
FE sums only the head, neck, armor, legs, feet, and ring values. The saturated sum is multiplied
with truncating integer thousandths by the parsed vocation `armor` multiplier, or the safe `1.000`
default. The result becomes the existing profile-neutral physical flat reduction for a typed
declarative weapon event and for the existing opt-in fixed static-creature physical attack before
it evaluates a lethal transition.

The same derived value is applied when a native session hydrates its persisted equipment. It is
not separately persisted. Supported native transfers mutate authoritative equipment first; the
next selected weapon event recalculates from that current equipment snapshot.

## Deliberate exclusions

This is not a TFS armor or shielding formula implementation. FE does not use hands, shields,
`defense`, `extraDef`, the vocation defense multiplier, fight mode, active block, random armor, resistance,
or client combat effects in this bridge. The fixed native selected-player melee fallback remains
outside the mitigation path. General weapon and creature formula compatibility remains deferred.
