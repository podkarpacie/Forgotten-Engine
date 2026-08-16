# Progression Compatibility Research

This note records **behavioral interoperability observations** for the upcoming Forgotten Engine player-progression work. It is not a source-code translation and it does not authorize copying upstream implementation details. Forgotten Engine will retain its own Rust data model, parsing, timing, persistence, and tests.

## Observed configuration contract

The public Forgotten Server repository exposes a configurable `vocations.xml` registry in which vocation records describe a numeric identifier, client-visible identifier, label, level-based health/mana/capacity gains, health and mana regeneration cadence/amount, magic-level multiplier, attack interval, base movement speed, soul parameters, combat modifiers, and seven skill multipliers.[1]

| Observed behavior | Forgotten Engine design implication | Delivery state |
| --- | --- | --- |
| Vocation properties are data-configurable rather than intrinsically tied to a binary version. | Add a bounded, validated vocation registry loaded from operator-owned data; do not embed a release-to-vocation mapping in the runtime. | Planned |
| The common base IDs are None, Sorcerer, Druid, Paladin, and Knight; promoted entries are also represented in the public registry. | Treat the five named base identities as a small typed foundation while retaining numeric IDs and registry extensibility for configured promotion/custom vocation entries. | Planned |
| Seven distinct skill multiplier positions are exposed alongside a magic-level multiplier. | Model all seven skills as typed data with tries and percentage; do not substitute a generic untyped map in authoritative state. | Planned |
| Health and mana regeneration configuration has an interval and amount for each resource. | Represent regeneration as elapsed-time-safe rules with bounded catch-up, rather than assuming a fixed host heartbeat equals a regeneration tick. | Planned |

## Timing interpretation

An OTLand discussion about TFS 1.2 vocation configuration identifies the health cadence and amount fields as the number of seconds between gains and the amount gained, respectively.[2] This is supporting operational evidence, not a normative specification. Forgotten Engine will make intervals explicit in seconds, validate nonzero values, persist the necessary runtime state for reconnect safety, and expose the result in its capability matrix.

## Observed progression requirement behavior

The public TFS vocation implementation exposes a seven-entry skill base table and computes a required skill-try count from the selected skill base multiplied by the configured vocation multiplier raised to the target-level offset. It also exposes required magic mana from a base of 1,600 multiplied by the configured magic multiplier raised to the target magic-level offset.[3] Forgotten Engine treats this as **behavioral research**, not reusable implementation text: the Rust core stores exact tries and spent mana separately from visible percentage fields, takes validated fixed-point rules as input, uses saturating deterministic calculations, and performs no automatic weapon-hit, spell-cast, offline-training, or Lua gain event.

> FE must not describe the resulting progression behavior as profile-compatible until its requirements, rounded percentage updates, persistence transitions, and client refreshes have been validated against the operator’s selected profile and content configuration.

## Scope boundary for this milestone

The initial implementation will cover typed skills, typed vocation configuration, persistence/migration, and profile-gated stat delivery. It will **not** claim formula combat, weapons runtime, spells, conditions, death/respawn, or full Lua event behavior. Those remain separate future milestones and must remain reported as deferred until verified.

## References

[1]: https://github.com/otland/forgottenserver/blob/master/data/XML/vocations.xml "The Forgotten Server: vocation configuration"
[2]: https://otland.net/threads/tfs-1-2-vocations-xml-fast-regeneration.243428/ "OTLand discussion: TFS 1.2 vocation regeneration configuration"
[3]: https://raw.githubusercontent.com/otland/forgottenserver/master/src/vocation.cpp "The Forgotten Server: public vocation progression requirement behavior"
