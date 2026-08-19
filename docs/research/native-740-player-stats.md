# Native 740 Player-Stats Parser Audit

## Scope

This note checks Forgotten Engine's fixed-width native `PlayerStats` (`0xA0`) encoder against the public OTCv8 development source. It is a clean-room compatibility audit. It does not copy client code into FE and does not replace a real unmodified-client observation.

## Source reviewed

The audit reviewed public OTCv8 development-source revision `3d32139512cc4576b105682c3579f18fe0d534e4`.[1] The `parsePlayerStats` reader selects widths through feature gates, while the selected feature table enables several optional fields only at later protocol versions.[2]

| Field order for protocol 740 | OTCv8 parser gate | Required 740 width | FE native encoder |
|---|---|---:|---|
| Health, maximum health | `GameDoubleHealth` | `u16`, `u16` | `u16`, `u16` |
| Free capacity | `GameDoubleFreeCapacity` begins at 840 | `u16` | `u16` |
| Experience | `GameDoubleExperience` begins at 870 | `u32` | `u32` |
| Level, level percentage | `GameDoubleLevel` is not enabled at 740 | `u16`, `u8` | `u16`, `u8` |
| Mana, maximum mana | `GameDoubleHealth` | `u16`, `u16` | `u16`, `u16` |
| Magic level, magic percentage | `GameDoubleMagicLevel` is not enabled at 740 | `u8`, `u8` | `u8`, `u8` |
| Soul | `GameDoubleSoul` is not enabled at 740 | `u8` | `u8` |

The 740 feature table also leaves `GamePlayerStamina`, `GameSkillsBase`, `GameTotalCapacity`, and later regeneration/training fields disabled because their thresholds begin after 740.[2] FE therefore must not append those fields to its declared 740 record.

> **Audit result:** FE's current 21-byte `0xA0` frame, including its opcode, matches the public parser-selected 740 field order and widths. Existing byte-level protocol regression coverage keeps persisted capacity distinct from mana and maximum mana.

## Boundary retained

The audit proves public parser compatibility evidence only. It does **not** prove that a user-selected OTCv8 build, its feature configuration, client assets, or complete initialization stream renders correctly. The checklist item requiring an unmodified-client observation without an EOF error remains open.

## References

[1] [OTCv8 development source, `protocolgameparse.cpp`, revision `3d32139512cc4576b105682c3579f18fe0d534e4`](https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/src/client/protocolgameparse.cpp)

[2] [OTCv8 development source, `features.lua`, revision `3d32139512cc4576b105682c3579f18fe0d534e4`](https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/game_features/features.lua)
