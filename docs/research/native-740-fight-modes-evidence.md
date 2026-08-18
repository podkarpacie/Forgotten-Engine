# Native 740 Fight-Mode Boundary

## Verified input shape

The classic game request uses opcode `0xA0` followed by three bytes: fight mode, chase mode, and secure mode. The public TFS reference maps raw fight mode `1` to attack, `2` to balanced, and every other value to defense. A nonzero chase or secure byte is treated as `true`. See the read-only public reference at `src/protocolgame.cpp`, `ProtocolGame::parseFightModes`, around lines 1267–1285.

## FE state boundary

FE now decodes this request into a typed native protocol value and replaces a synchronized core `PlayerFightModeState`. The authoritative default matches the public player defaults: attack mode, chase disabled, secure disabled. See the public reference at `src/player.h`, around lines 1320–1324.

The state is non-persistent and intentionally has no present effect on damage, defense, armor, targeting, movement, chase behavior, packets, or client UI. Those effects require separately verified profile-specific behavior and are not claimed by this milestone.

## Verified native 740 initialization delivery

The public OTClient protocol parser identifies `GameServerPlayerModes` as opcode `0xA7`. Its `parsePlayerModes` handler reads exactly three bytes—fight mode, chase mode, and safe mode—then reads a fourth PvP-mode byte only when the `GamePVPMode` feature is enabled. The selected native 740 profile does not enable that later feature. FE therefore appends one parser-shaped `0xA7` record with exactly those three values to native 740 initialization, sourced from authoritative fight-mode state. State-change packets, later PvP-mode variants, persistence, and formula effects remain deferred.
