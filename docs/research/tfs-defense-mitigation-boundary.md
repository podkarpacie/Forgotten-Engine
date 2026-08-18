# Public TFS Defense and Mitigation Boundary

## Purpose

This note records the public-reference findings used to plan a future **profile-specific** physical mitigation slice. It is a behavior map only. Forgotten Engine must implement any accepted behavior independently in Rust and must not copy upstream implementation code.

## Observed public reference inputs

The public TFS reference first checks combat immunity. When a damage event permits defense or armor, it consumes a limited defense-block count, conditionally applies a randomized defense reduction, and then conditionally applies randomized armor reduction. Defense is skipped after a full defense block. The public source is `src/creature.cpp`, `Creature::blockHit`, around lines 633–676.

For players, armor is accumulated from head, necklace, armor, legs, feet, and ring slots, then multiplied by a vocation armor multiplier. Defense selects a weapon and/or shield from hand slots, combines item defense and extra-defense according to the selected hand setup, uses the applicable weapon or shielding skill, then applies fight-mode, recent-attack, and vocation-defense multipliers. The public source is `src/player.cpp`, `Player::getArmor`, `Player::getShieldAndWeapon`, `Player::getDefense`, and `Player::getDefenseFactor`, around lines 321–434.

## Current FE boundary

FE currently owns only a profile-neutral `PlayerCombatDefense { physical_flat_reduction }`. It deliberately does not interpret legacy armor, shielding, equipment, skill, vocation, fight-mode, PvP, or randomized formula behavior. See `crates/forgotten-core/src/lib.rs`, around lines 1722–1742.

The existing FE legacy item catalog validates server/client IDs, groups, subtype flags, and selected path-blocking XML attributes. It does **not** retain armor, defense, extra-defense, weapon type, or attack-speed metadata. See `crates/forgotten-config/src/items.rs`.

## Required prerequisites before compatibility claims

| Required input | FE status | Reason it is required |
|---|---|---|
| Item armor, defense, extra-defense, weapon type, and attack-speed metadata | Missing | Public aggregation chooses and evaluates equipped hand items and armor slots. |
| Slot compatibility and complete equipped item semantics | Partial | Current equipment is a bounded ownership model, not a verified legacy slot/weapon model. |
| Fight-mode state and native request semantics | Missing | The public defense factor depends on fight mode and elapsed time since attack. |
| Bounded deterministic random source with profile evidence | Missing | The public defense and armor reductions use random ranges. |
| Vocation armor/defense multipliers | Missing | Current vocation parsing does not populate these combat values. |
| Profile-specific client and runtime evidence | Missing | FE 7.4 must not infer modern TFS formula behavior as classic 7.4 parity. |

## Decision

No formula implementation is authorized from this audit alone. The next safe independent slice is **catalog-only ingestion and validation of the required item and vocation combat metadata**, with no change to live damage, combat packets, or compatibility claim. Any later mitigation algorithm must be profile-gated, deterministic, independently tested, and accompanied by exact source/evidence notes.
