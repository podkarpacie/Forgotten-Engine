# Native Progression and Mana Presentation Audit

## Existing supported boundary

FE already hydrates native 740 HUD records from one persisted authoritative player state. The login
snapshot reads level, experience, health, maximum health, mana, maximum mana, capacity, magic level,
and skills from the same authoritative registration and SQLite-backed values. Later bounded
condition damage and vocation regeneration use the vital epoch to send matching refreshed stats
records after persistence.

Vocation gains for health, mana, and capacity are applied only during the existing authoritative
level-up transaction. Magic-mana and skill progress remain separately typed progression paths with
their own exact attempt accounting. No new formula is inferred from a client HUD value.

| Concern | Current status |
|---|---|
| Login level, experience, mana, maximum mana, capacity, and magic-level HUD hydration | Supported and regression-covered. |
| Persisted vital refresh after bounded condition damage or regeneration | Supported and regression-covered. |
| Vocation gains during a real authoritative level-up | Supported and atomically persisted. |
| Full TFS formula parity, spell gameplay, Lua formulas, and real-client confirmation | Deferred. |

## Decision

No new mana or progression behavior is added by this audit. The reported historical HUD inconsistency
is covered by the current authoritative hydration and refresh regressions. A future correction
requires a reproducible current mismatch with a persisted character state and corresponding
profile-specific parser evidence; otherwise changing the calculation would risk replacing a tested
contract with an invented one.
