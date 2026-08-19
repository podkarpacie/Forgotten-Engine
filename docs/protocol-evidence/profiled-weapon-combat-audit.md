# Profiled Weapon Combat Audit

## Scope

This audit considers a possible extension of the selected native 740 player-melee path. The goal is to determine whether FE can safely apply its imported legacy item weapon metadata to live combat without inventing or copying broad TFS combat behavior.

## Existing FE boundary

FE already has one explicit, operator-owned combat boundary. A `fe-weapons` declarative catalog defines only a server item ID, fixed physical damage, and a validated tick interval. The host resolves that declaration only when the attacking player has the matching item equipped in the approved slot. The core then applies the existing typed adjacent physical combat event with its established range, cooldown, mitigation, persistence, and death-state handling.

This path is deliberately independent from the imported legacy item metadata. The latter retains armor, defense, extra-defense, attack-speed, and weapon-type labels as conversion metadata only.

| Candidate input | Evidence available | Safe live use now |
|---|---|---|
| FE declarative weapon ID, fixed damage, interval ticks | Typed FE schema and existing selected-player regressions | Already supported within the selected-player adjacent-melee boundary. |
| Legacy `items.xml` weapon type and attack speed | Bounded conversion metadata only | Deferred. Profile-specific precedence and server-tick conversion are not proven. |
| TFS melee/distance/wand behavior | Read-only reference shows weapon classes, skill/level formulas, modifiers, ammunition, and target/tile variants | Deferred. FE does not yet model the full required inputs or client-visible outcomes. |
| Profile 740 target request | Existing FE input and typed selected-player transition | Partial. It drives only the existing declared event, not generic TFS weapon use. |

## Decision

No additional weapon-combat behavior is enabled by this audit. In particular, FE will not derive fixed damage or cooldowns from legacy `items.xml` metadata, and will not claim melee, distance, wand, ammunition, formula, skill, resistance, or target/tile parity.

A future profile-driven extension needs reproducible evidence for profile-specific timing conversion and weapon precedence, plus typed FE models for the relevant player skills, formulas, weapon class, modifiers, and client delivery. It must introduce separate regressions for normal, cooldown, range, missing-equipment, and persistence boundaries.
