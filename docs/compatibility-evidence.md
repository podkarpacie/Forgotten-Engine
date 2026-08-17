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

For the selected 740 profile, the public client parser consumes a `u16` level, level percent, `u16` mana, `u16` maximum mana, magic level, magic-level percent, and a soul byte after the common health, capacity, and experience fields.[6] FE’s native `0xA0` regression contract now reflects that observed classic record boundary; it does not claim the later-protocol total-capacity, stamina, or soul gameplay systems. The public client sender describes a classic map look request as `0x8C`, position, thing ID, and stack position.[7] FE continues to tolerate that request without a visible result because a profile-specific, parser-confirmed 740 text-response mode must be established before enabling one. It must not reuse the previously invalid generic mode value.

## References

[1] [TFS legacy weapon registry path](https://github.com/otland/forgottenserver/blob/master/data/weapons/weapons.xml).

[2] [TFS Revscriptsys documentation](https://github.com/otland/forgottenserver/wiki/Revscriptsys).

[3] [Historical 7.4 configuration contract available in the local reference environment](file:///tmp/avesta-74/Avesta%207.4%20(OTServ_SVN%200.6.3)/config-74.lua).

[4] [Local public OTClientV8 parser reference](file:///tmp/otcv8-dev-source/src/client/protocolgameparse.cpp) and [opcode definitions](file:///tmp/otcv8-dev-source/src/client/protocolcodes.h). Read-only behavioral evidence; no client source is copied into FE.

[5] [Public OTClientV8 parser](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgameparse.cpp) and [opcode declarations](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolcodes.h). Read-only behavioral evidence; no client source is copied into FE.

[6] [Public OTClientV8 player-stats parser](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgameparse.cpp). Read-only behavioral evidence; no client source is copied into FE.

[7] [Public OTClientV8 look sender](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolgamesend.cpp) and [message-mode map](https://raw.githubusercontent.com/OTCv8/otcv8-dev/master/src/client/protocolcodes.cpp). Read-only behavioral evidence; no client source is copied into FE.
