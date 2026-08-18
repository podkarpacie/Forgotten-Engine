# Compatibility Evidence Register

This register records externally observable format and architecture evidence used to guide original FE behavior. It is deliberately a **reference index**, not a copy of upstream implementation, game data, maps, scripts, or client assets.

## Weapon content boundary

| Evidence | Clean-room interpretation for FE | Implementation consequence |
|---|---|---|
| TFS publishes a legacy `data/weapons/weapons.xml` registry.[1] | A legacy TFS directory can declare weapon entries independently of OTBM map placement. | `tfs-audit` discovers and validates the registry/reference paths, but labels it `deferred weapon runtime` until typed weapon semantics exist. |
| TFS documentation describes a modern RevScript registration approach and identifies `Weapon` among supported event/metatable families.[2] | Modern TFS-derived packs can register weapon behavior through scripts rather than only legacy XML. | FE must eventually create one typed weapon-registration model with legacy XML and modern Lua adapters; parsing either source must not execute scripts. |
| The historical 7.4 configuration contract separates map location, item-use intervals, combat/weapon exhaustion, rates, spawns, houses, persistence, and operational saves.[3] | A map file cannot be treated as a complete weapon or gameplay definition. | FE stages item registry, inventory/equipment, player skills, combat rules, timing, PvP policy, and script hooks as separate dependencies before claiming working weapons. |

## Evidence handling rules

1. Public sources may guide a format/behavior observation, but FE code and fixtures are authored independently.
2. Private operator maps, items, scripts, client assets, packet captures, and server directories remain local. They are never committed to FE.
3. Every future source-format parser records accepted structure, bounded limits, unsupported attributes, and a regression fixture before enabling runtime behavior.
4. Content discovery is not permission to execute content. Audit and parser paths must remain side-effect free.

## Inventory protocol boundary

The current FE work has added authoritative/persistent equipment foundations but has not introduced a native 740 inventory or container encoder. The local public OTClientV8 parser establishes the classic inventory dispatch IDs: `OpenContainer` (`0x6E`), `CloseContainer` (`0x6F`), container add/update/remove (`0x70`–`0x72`), set inventory (`0x78`), and remove inventory (`0x79`).[4] For the non-pagination branch applicable to the classic profile, the parser consumes an inventory slot byte followed by an item record for `0x78`, or just a slot byte for `0x79`. It consumes a container ID, container item, name, capacity, parent flag, item count, and item records for `0x6E`; pagination-only fields are gated separately. FE will now document the independently implemented item-record encoder and write parser-aligned golden fixtures before enabling this output. This avoids repeating the earlier class of client parser failures caused by guessed classic-protocol records.

## Quest Log protocol boundary

The public client protocol declarations identify both the classic Quest Log request and response as `0xF0`; the parser consumes a little-endian `u16` quest count before attempting to read entries.[5] FE therefore accepts only the zero-payload request and responds with an independently authored three-byte record containing `0xF0` and a zero count. This is a session-safety acknowledgement only. It does not claim quest storage, mission lines, Lua hooks, reward logic, or client confirmation beyond the automated wire contract.

## Player-stats and inspect protocol boundaries

For the selected 740 profile, the public client parser consumes a `u16` free-capacity field, then a `u32` experience field, `u16` level, level percent, `u16` mana, `u16` maximum mana, magic level, magic-level percent, and a soul byte.[6] The public feature map enables neither double free capacity before 840 nor total capacity before 910, so no additional capacity field belongs in FE’s 740 record.[11] FE’s native `0xA0` regression therefore includes a realistic persisted `32,000` capacity with separate `42 / 50` mana bytes; it does not claim later-protocol total-capacity, stamina, or soul gameplay systems. Public OTClient declarations identify classic map look and creature look as client requests `0x8C` and `0x8D` respectively.[22] The public sender describes map look as position, thing ID, and stack position.[7] FE continues to tolerate these requests without a visible result because a profile-specific, parser-confirmed 740 response mode must be established before enabling one. It must not reuse the previously invalid generic mode value.

The maintained OTCv8 project reports an open 7.4 defect: its message-mode map is not constructed for protocol versions below 760, causing both talk and text-message modes to resolve as invalid.[8] The cited public mode-map implementation has its final map-construction branch at `version >= 760`, and its server-mode translator returns `MessageInvalid` when the map contains no matching byte.[7] This independently explains the earlier `unknown message mode` client errors. FE therefore keeps native 740 visible chat and visible look output explicitly deferred. Treating a server-side mode byte as a substitute would require a client change, which conflicts with FE’s unmodified-OTClientV8 compatibility target.

## Outfit-dialog protocol boundary

The public client parser consumes one classic outfit record, then—when the newer outfit-list feature is unavailable—one-byte start and end look types.[12] FE’s bounded native response to request `0xD2` is therefore an independently authored `0xC8` record containing only the current classic outfit and that inclusive one-byte range. It intentionally emits no newer named-outfit list, addons, mounts, wings, auras, shaders, or client-specific extensions. The existing native session regression covers request, response, accepted colors, and persisted appearance; real-client confirmation that the dialog opens remains required.

## Physical mitigation research boundary

Public historical OpenTibia discussion describes shield defense and armor as separate physical-mitigation stages, with defense considered before armor and each potentially reducing the same incoming hit.[9] Current public TFS combat code also exposes independent armor and shield block switches in combat parameters.[10] The available sources span different eras and do not establish a complete profile-740 formula, blocking count, skill interaction, stance effect, or random-range contract. FE therefore retains its existing bounded profile-neutral flat mitigation foundation and does not claim TFS 7.4 armor or shielding parity from this evidence alone.

## FE 8.0 encrypted transport boundary

The public Wireshark Tibia dissector marks client versions from 761 as using RSA and XTEA, and marks 780 and later as having additional outfit/stamina/message-level behavior.[13] FE’s public OTCv8 sender evidence independently shows a feature-gated login block whose leading byte is zero and whose encryption-enabled form carries four 32-bit XTEA words before RSA wrapping; it enables XTEA only after the encrypted login send.[14] Therefore the existing plain numeric-account 740 handshake cannot be widened to protocol 800 by changing only a version number. FE now has an independently authored outbound-only opaque XTEA envelope primitive bounded to the classified encrypted 8.0 profile and a marker/four-word parser over an already decrypted fixed block; neither accepts a client frame, performs RSA decryption, parses trailing bytes or credentials, or emits key material in diagnostics. It must not infer a complete protocol-800 login, game packet, checksum, or listener contract from this evidence. Native protocol-800 enablement remains refused until parser-backed session work exists.

## Vocation advancement metadata boundary

The public TFS vocation registry declares `gaincap`, `gainhp`, and `gainmana` alongside regeneration amounts/ticks, skill multipliers, combat formulas, speed, soul, and other fields.[15] The public player header exposes a pure classic experience threshold calculation, while the public progression routine compares total experience to those thresholds and applies vocation health, mana, and capacity gains when a level changes.[16] [17] FE may therefore parse those three nonzero bounded per-level gain values into immutable vocation metadata and independently test an overflow-safe threshold helper. The source alone does not establish an FE starting-stat baseline, promotion, rounding, maximum-vital, packet, or full runtime advancement transaction. Parsing and threshold calculation must not mutate player state until those dependencies have separate authoritative and profile-specific evidence.

## Classic death-notification boundary

The public OTClient protocol declarations identify `GameServerDeath` as opcode `0x28`.[18] The matching parser dispatches that opcode to `parseDeath`; it consumes a death-type byte only when the later `GameDeathType` feature is enabled, and consumes a penalty byte only when the later `GamePenalityOnDeath` feature is enabled for a regular death.[19] The selected FE 740 profile enables neither later feature. FE therefore emits an independently authored one-byte `0x28` record only after an already-authoritative server-side death transition. It must not attach guessed death type, penalty, redemption, re-login, teleport, or automatic-respawn fields. Manual unmodified-OTClientV8 confirmation remains required before this record clears any release blocker.

## Script-file loading boundary

The public TFS registry examples use XML `script` attributes relative to a subsystem directory, while its newer Revscriptsys model discovers scripts beneath `data/scripts/` and expects TFS metatables and registration methods.[20] [21] These sources establish that script location is a separate concern from the Lua API contract. FE therefore permits an operator to load only an explicit callback-function chunk from a caller-selected canonical root, using normal relative path components only and rejecting traversal or canonical symlink escape. The loaded source still executes only in FE’s fresh no-standard-library sandbox with existing source, memory, instruction, primitive-value, and callback-count limits. It does not load XML registries, resolve modules, expose TFS metatables, provide `Player`, `Game`, `Action`, `CreatureEvent`, or filesystem/network APIs, or mutate authoritative world state. Ordinary TFS Lua compatibility remains deferred.

## References

[1] [TFS legacy weapon registry path](https://github.com/otland/forgottenserver/blob/master/data/weapons/weapons.xml).

[2] [TFS Revscriptsys documentation](https://github.com/otland/forgottenserver/wiki/Revscriptsys).

[3] [Historical 7.4 configuration contract available in the local reference environment](file:///tmp/avesta-74/Avesta%207.4%20(OTServ_SVN%200.6.3)/config-74.lua).

[4] [Local public OTClientV8 parser reference](file:///tmp/otcv8-dev-source/src/client/protocolgameparse.cpp) and [opcode definitions](file:///tmp/otcv8-dev-source/src/client/protocolcodes.h). Read-only behavioral evidence; no client source is copied into FE.

[5] [Public OTClientV8 parser](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgameparse.cpp) and [opcode declarations](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolcodes.h). Read-only behavioral evidence; no client source is copied into FE.

[6] [Public OTClientV8 player-stats parser](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgameparse.cpp). Read-only behavioral evidence; no client source is copied into FE.

[7] [Public OTClientV8 look sender](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgamesend.cpp) and [message-mode map](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolcodes.cpp). Read-only behavioral evidence; no client source is copied into FE.

[8] [OTCv8 issue #218: 7.4 protocol cannot parse creature talk/speak](https://github.com/OTCv8/otclientv8/issues/218). Read-only compatibility evidence; no client source is copied into FE.

[9] [OTLand discussion: Defense and armour](https://otland.net/threads/defense-and-armour.287950/). Community historical analysis; used only to identify research questions and stage ordering, not as a complete formula specification.

[10] [Public TFS combat parameter declarations](https://raw.githubusercontent.com/otland/forgottenserver/master/src/combat.cpp). Read-only behavioral evidence from a later TFS codebase; no source is copied into FE.

[11] [Public OTCv8 feature map](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/game.cpp). Read-only classic-profile feature evidence; no client source is copied into FE.

[12] [Public OTCv8 outfit-dialog parser](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgameparse.cpp). Read-only behavioral evidence; no client source is copied into FE.

[13] [Wireshark Tibia dissector protocol feature thresholds](https://github.com/wireshark/wireshark/blob/master/epan/dissectors/packet-tibia.c). Read-only public packet-analysis evidence; no dissector source is copied into FE.

[14] [Public OTCv8 game-login sender](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgamesend.cpp). Read-only behavioral evidence; no client source is copied into FE.

[15] [Public TFS vocation registry](https://raw.githubusercontent.com/otland/forgottenserver/master/data/XML/vocations.xml). Read-only XML attribute evidence; no registry data is copied into FE.

[16] [Public TFS player progression logic](https://raw.githubusercontent.com/otland/forgottenserver/master/src/player.cpp). Read-only behavior evidence; no player source is copied into FE.

[17] [Public TFS player threshold helper](https://raw.githubusercontent.com/otland/forgottenserver/master/src/player.h). Read-only formula evidence; no player source is copied into FE.

[18] [Public OTClient game-server opcode declarations](https://github.com/edubart/otclient/blob/master/src/client/protocolcodes.h). Read-only wire-identifier evidence; no client source is copied into FE.

[19] [Public OTClient death parser](https://github.com/edubart/otclient/blob/master/src/client/protocolgameparse.cpp). Read-only classic feature-gating evidence; no client source is copied into FE.

[20] [Public TFS talkactions XML registry](https://github.com/otland/forgottenserver/blob/master/data/talkactions/talkactions.xml) and [creaturescripts XML registry](https://github.com/otland/forgottenserver/blob/master/data/creaturescripts/creaturescripts.xml). Read-only script-reference evidence; no script source is copied into FE.

[21] [Public TFS Revscriptsys documentation](https://github.com/otland/forgottenserver/wiki/Revscriptsys). Read-only discovery and API-surface evidence; no script source is copied into FE.

[22] [Public OTClient classic client-opcode declarations](https://raw.githubusercontent.com/edubart/otclient/master/src/client/protocolcodes.h) and [message-mode translation table](https://raw.githubusercontent.com/edubart/otclient/master/src/client/protocolcodes.cpp). Read-only classic request and response-mode evidence; no client source is copied into FE.
