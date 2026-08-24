# Native 760 Visible-Text Evidence (and the 740 Mode-Map Gap)

## Why the classic-760 runnable profile exists

An unmodified OTClientV8 selecting protocol **740** cannot display *any* server text — chat,
whispers, yells, Look replies, status/failure messages, or broadcasts. This is a client-side
fact, not an FE encoding defect:

- `src/client/protocolcodes.cpp:29` (`buildMessageModesMap`) has no branch below `version >= 760`
  (line 148). At 740 the map stays empty.
- `translateMessageModeFromServer` (`protocolcodes.cpp:178`) therefore returns
  `Otc::MessageInvalid` for every incoming mode byte.
- `parseTalk` (`protocolgameparse.cpp:2145`, throw at 2179) and `parseTextMessage`
  (`protocolgameparse.cpp:2278`) both throw "unknown message mode", and `parseMessage`
  discards the rest of that packet (`:564`). Outgoing `sendTalk` likewise emits mode byte 255.

FE's earlier decision to suppress visible chat on 740 was correct: no server-side encoding can
make a stock 740-selected client render text through 0xAA/0xB4. The opcode-0x00 Lua-injection
hook only compiles injected chunks (`loadBuffer` without `safeCall`,
`luainterface.cpp:803`), so it is not an execution path either.

The 760 profile resolves this without touching the client:

- OTCv8's entergame version dropdown offers 760; feature flags are identical to 740
  (`modules/game_features/features.lua`: nothing new until 770).
- The login packet is byte-identical (`protocolgamesend.cpp:61-136`): plaintext numeric account,
  character name, password, trailing "OTCv8" + version string; RSA/XTEA stay off below 770.
- `buildMessageModesMap` gains its first real branch at 760 (`protocolcodes.cpp:148`),
  mapping Say=1, Whisper=2, Yell=3, PrivateFrom/To=4, Channel=5, GM Broadcast=9, Login=20,
  Status=21, Look=22, Failure=23.

FE therefore accepts 740 or 760 wherever it previously required exactly 740
(`NativeOtClientProfile::supports_classic_740_inventory_records`), while gating genuinely
client-visible text on the new `supports_visible_text_messages()` (760-only). Operators should
select **760** in their client's version list; every record FE already emitted becomes visible.

## New encoder layouts (verified against the local parser source)

All records use FE's standard frame wrapper. Payloads:

| Record | Opcode | Layout |
|---|---|---|
| GM broadcast | 0xAA | name(string) + mode 0x09 + text |
| Look message | 0xB4 | class 0x16 + text |
| Failure message | 0xB4 | class 0x17 + text |
| Login message | 0xB4 | class 0x14 + text |
| Animated text | 0x84 | x(u16) y(u16) z(u8) + color(u8) + text |
| Magic effect | 0x83 | x(u16) y(u16) z(u8) + effect id(u8) |

Animated text is parsed with no mode translation on every classic protocol
(`protocolgameparse.cpp:1523`), giving spatial feedback even on mode-less profiles.

## Operator gamemaster system

- Schema v32 adds `players.gm_level INTEGER NOT NULL DEFAULT 0`; levels are bounded 0-3.
- Chat talkactions (say-mode messages starting with `/`) resolve before spell/bank/shop
  routers when the speaker holds gm_level > 0:
  `/spawn <entity>`, `/give <player> <item-id> [count]`, `/tp <player> <player>`
  (`me` = self), `/kick <player>`, `/gm <online|offline> <player> [level]`,
  `/broadcast <message>`; replies arrive as status-message records.
- Dynamic summons clone an installed creature template by case-insensitive name and allocate
  IDs from 0x7000_0000 upward, keeping imported spawn IDs collision-free across restarts;
  despawn is limited to that range.
- A loopback-only JSON-line bridge (`crates/forgotten-host/src/operator.rs`) runs beside the
  game listener when started via `run`. Requests: `{"op":"broadcast"|"gm"|"give"|"tp"|"spawn"|
  "kick"|"status", ...}`. Non-loopback peers are rejected before parsing. `run` publishes the
  bound port in `<world>/.fe-operator-port` and removes it at shutdown, letting the CLI
  (`forgotten-engine command <world> ...`) and Forgotten Cloud act on the live world.

## Outfit and appearance corrections

- Config now defaults `otclientV8PlayerLookType` to citizen look type 128 (chooser range
  128..=134) whenever native clients are enabled without an explicit appearance and the world
  is not the deliberate asset-free fixture. Zero look types rendered players as client-side
  invisibility effect #13 (`creature.h:148`, `getOutfit` at `protocolgameparse.cpp:3317`).
- RequestOutfit / ChangeOutfit handlers degrade encoder failures to failure-text records;
  a missing chooser range can no longer tear down the session.

## Remaining release blockers

Real-client confirmation of the 760 path remains outstanding: select 760 in an unmodified
OTCv8 build, log in, verify public say/whisper/yell echo, Look green text, GM broadcast from
the bridge, animated combat feedback, and outfit persistence across relog.
