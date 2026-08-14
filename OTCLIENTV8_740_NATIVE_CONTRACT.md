# Forgotten Engine Native OTClientV8 740 Contract

## Scope

This contract defines the first **native, unmodified OTClientV8** interoperability target for `fe-7.4`: a stock OTClientV8 build configured to protocol `740` through its documented custom-server configuration. It is a clean-room behavioral contract written from public client observations. It does not copy client code, Tibia assets, TFS code, maps, or item data.

The reference point is the public `OTCv8/otcv8-dev` revision `3d32139512cc4576b105682c3579f18fe0d534e4`, whose supported-client list includes `740`.[1] The operator must provide lawful, matching 7.4 thing data in the client’s own `data/things` installation; FE will not distribute or generate those files.[2]

## 740 feature profile

| Behavior | FE native 740 requirement | Public observation |
|---|---|---|
| Packet framing | Two-byte little-endian payload length. | Normal OTClient protocol transport. |
| Account identity | Numeric `u32` account identifier. | OTCv8 enables account-name strings only from 8.40. [3] |
| Login RSA/XTEA | Not assumed for 740. | OTCv8 enables login packet encryption only from 7.70. [3] |
| Protocol checksum | Not assumed for 740. | OTCv8 enables checksums only from 8.40. [3] |
| Challenge-on-login | Not assumed for 740. | OTCv8 enables it only from 8.41. [3] |
| Game movement request | Normal one-byte cardinal opcodes, not an FE extended opcode. | Client walk north/east/south/west opcodes are `101`–`104`. [4] |

## Native login-server request

The stock 740 client’s login-server request begins with normal `ClientEnterAccount` (`0x01`), then OS and selected protocol (`740`) fields, the client’s data signatures, numeric account ID, password string, and OTCv8 client-tail data. Because the 740 feature profile does not activate login encryption, FE treats the credential portion as an unencrypted, bounded normal 740 request. It accepts a documented OTCv8 tail and ignores padding after the bounded fields.

The native path maps the numeric account ID to the FE database account primary key. A database account whose ID is `42` is entered as account `42` in the client. This is separate from the existing FE custom account-name session foundation.

## Native character-list response

After successful numeric-account authentication, FE returns one normal legacy character-list frame:

| Field | Encoding |
|---|---|
| Opcode | `0x64` (`LoginServerCharacterList`) |
| Character count | `u8` |
| Per character | Name string, world-name string, IPv4 `u32`, game port `u16` |
| Account tail | Premium days `u16` (zero for the initial FE native contract) |

This is the legacy layout used by the public OTCv8 parser for protocol versions at or below 10.10.[5]

## Native game request and current boundary

Character selection creates a separate normal game connection. The first request begins with `ClientPendingGame` (`0x0a`), OS, protocol `740`, numeric account ID, character-name string, password string, and bounded OTCv8 tail. FE must accept this normal request on the game endpoint rather than require the current FE-specific RSA challenge or extended-opcode acknowledgement.

The native game response required for a truly connected client includes normal player-login data, full-map serialization, a player creature, and ordinary walking/map delta messages. A valid full-map packet depends on a carefully specified tile/thing encoding and matching lawful client thing data. That serialization is **not** implemented by this first native-login contract. Until the map fixture acceptance criteria are completed, FE must use a normal native login-error frame for an authenticated game selection instead of sending FE-only payloads or pretending that a normal OTCv8 world loaded.

## Acceptance criteria for the first native slice

The first native slice passes only when independent Rust fixtures prove the following:

1. FE accepts a bounded normal 740 login-server request with numeric account identity.
2. FE emits a normal legacy character-list frame that the documented OTCv8 parser layout can consume.
3. FE accepts a bounded normal 740 pending-game request for an account-owned character.
4. Before map serialization exists, FE emits a normal client-understood login-error message rather than an FE custom packet.
5. The native port path does not require an FE client module or extended opcode.

## Native empty-world extension contract

The next native slice is intentionally a **data-free world-session fixture**, not a claim of general map compatibility. It is enabled only when the selected native profile is explicitly configured for the observed classic-world behavior and the operator supplies lawful matching client thing identifiers. FE does not ship the associated `.dat`, `.spr`, map, or item database.

| Response or request | Normal opcode | Selected classic 740 layout | Initial FE boundary |
|---|---:|---|---|
| Player login | `0x0a` | Player ID `u32`, server beat `u16`, `canReportBugs` `u8`. | Uses the profile-selected no-new-speed-law layout. |
| Full map | `0x64` | Center position, then floors 7 down to 0; each floor visits an 18×14 viewport in x-major/y-minor order. | Emits only operator-configured ground things and one local-player creature; no FE extended payload. |
| Empty tile/thing boundary | `0xff00..0xffff` | A tile is finished by a classic marker; a marker also represents a run of following empty tiles. | Initial serializer may use a zero-run terminator per ground tile for clarity, while retaining an independently tested skip-marker codec. |
| Player creature | `0x0061` | Remove ID `u32`, creature ID `u32`, name string, health, direction, classic outfit, light, speed, skull, shield. | Uses no later creature-emblem, add-on, new-walking, or walkthrough fields. |
| Cardinal walk request | `0x65`–`0x68` | One normal opcode with no body. | Bounded to the original empty-world state. |
| Creature move response | `0x6d` | Mapped creature (`0xffff`, player ID) and a new position. | Returns the normal classic layout with no new-walking duration field. |

The first map fixture uses a configurable ground thing ID and a configurable player look type. These values are **operator-owned client-data identifiers**, not bundled Forgotten Engine data. The configuration must reject a world fixture that is enabled without nonzero values. A client whose installed thing data does not define the selected identifiers is outside this fixture’s acceptance boundary.

The first movement response acknowledges cardinal movement only while the player remains within the initially described viewport. It deliberately does not claim map-row delta streaming, loading of a real map, collision derived from item data, creatures beyond the local player, or gameplay. Full standard viewport deltas and world-content loading are separate follow-on milestones.

### Empty-world acceptance criteria

1. Given the selected classic profile and operator-supplied thing IDs, FE emits normal `0x0a` player-login and `0x64` full-map frames after an owned-character selection.
2. The full map contains exactly the selected classic viewport traversal, with no FE extended opcode or custom-client acknowledgement.
3. The local player is represented by a normal unknown-creature fixture whose ID lies in the normal player-ID range and whose field layout matches the selected profile.
4. FE decodes normal cardinal walk opcodes and replies with normal `0x6d` creature-move packets while the bounded fixture can safely maintain the described viewport.
5. Independent Rust codec and socket fixtures prove byte order, map traversal, player placement, and movement response fields without relying on proprietary client assets.

## References

[1]: https://github.com/OTCv8/otcv8-dev/tree/3d32139512cc4576b105682c3579f18fe0d534e4 "OTClientV8 development reference revision"
[2]: https://github.com/OTCv8/otclientv8 "OTClientV8 official distribution and server-owner asset guidance"
[3]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/game_features/features.lua "OTClientV8 740 feature thresholds"
[4]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/src/client/protocolcodes.h "OTClientV8 normal opcode declarations"
[5]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/gamelib/protocollogin.lua "OTClientV8 legacy character-list parser"
[6]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/game_features/features.lua "OTClientV8 version feature thresholds"
[7]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/src/client/protocolgameparse.cpp "OTClientV8 public game packet parser"
