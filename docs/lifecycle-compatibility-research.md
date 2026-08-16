# Player Lifecycle Compatibility Research

This note captures observable configuration and public-interface evidence for Forgotten Engine's upcoming player-lifecycle work. It is a **clean-room behavioral reference**, not a translation of upstream implementation code. FE will retain independent Rust structures, timing rules, persistence, and tests.

## Observed operator-facing behavior

The public TFS configuration template exposes a `deathLosePercent` setting and explains that `-1` selects a default formula, `10` selects an older formula, and `0` disables skill/experience loss.[1] The same template separates experience stages from a flat `rateExp` value and documents stage records containing mandatory minimum level and multiplier fields with an optional maximum level.[1]

The public legacy TFS configuration template exposes the same `deathLosePercent` conventions and additionally documents death-list retention settings, confirming that loss policy and death-record retention are distinct concerns.[4]

Public configuration behavior also shows that experience stages are ordered level ranges. When a matching stage exists it takes precedence over the flat experience rate; otherwise the flat rate applies.[5] This confirms that FE should model stages as an explicit validated data structure rather than reuse its current temporary square-root level curve as a compatibility formula.

## Observed death-loss accounting boundary

The public player lifecycle behavior measures lossable magic progression against cumulative required mana plus the player’s current spent mana, and lossable skills against cumulative required tries plus each skill’s current tries. It applies the resulting loss percentage to experience as well.[6] In its non-default configuration path, the public behavior adjusts the configured percentage for promoted status and blessings before using it; the default `-1` formula follows a distinct level/experience-based path.[6]

Forgotten Engine therefore has sufficient evidence for **exact accounting inputs** but not yet for full profile-compatible policy application: FE has no blessing, promotion-status, magic-spent event source, weapon/spell source, or client death delivery. The next bounded slice may expose an explicit fixed-percent loss model only if it takes a caller-supplied effective percentage and operates on persisted exact counters; it must leave the configured default formula and blessing/promotion adjustments deferred.

| Observed surface | Forgotten Engine design implication | Status |
| --- | --- | --- |
| Death loss is configurable and can be formula-selected or disabled. | Add a typed, validated death-loss policy only after experience and skill-try accounting exist; do not use the current level/percentage display state as a substitute for lossable tries. | Deferred |
| Experience may use flat rates or level stages. | Implement a bounded configuration model and deterministic arithmetic with an explicit overflow policy before accepting TFS-compatible experience claims. | Deferred |
| Vocation records expose health/mana cadence and amount fields, alongside soul cadence and amount.[2] | Use elapsed-time-safe regeneration schedules that can catch up in bounded fashion after delayed heartbeats or reconnects. | Planned |
| Public TFS scripting interfaces include creature death, global timed/interval events, condition, player, and vocation surfaces.[3] | Keep conditions, death events, and Lua dispatch independently gated. A lifecycle foundation must not claim scripted event compatibility before the sandbox runtime exists. | Deferred |

## Initial scope decision

The next FE slice should begin with a bounded operator-owned vocation registry parser and regeneration-rule model. It must not silently enable death loss, corpse drops, temple respawn, condition damage, or script callbacks. Those features have durable persistence and client-state implications and require separate tested delivery contracts.

## References

[1]: https://github.com/otland/forgottenserver/blob/master/config.lua.dist "The Forgotten Server configuration template"
[2]: https://github.com/otland/forgottenserver/blob/master/data/XML/vocations.xml "The Forgotten Server vocation configuration"
[3]: https://github.com/otland/forgottenserver/wiki/Script-Interface "The Forgotten Server scripting-interface overview"
[4]: https://github.com/otland/tfs-old-svn/blob/master/config.lua "Legacy The Forgotten Server configuration template"
[5]: https://github.com/otland/forgottenserver/blob/master/src/configmanager.cpp "The Forgotten Server public configuration-manager surface"
[6]: https://raw.githubusercontent.com/otland/forgottenserver/master/src/player.cpp "The Forgotten Server public player lifecycle behavior"
