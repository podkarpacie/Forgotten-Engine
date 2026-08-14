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

## References

[1]: https://github.com/OTCv8/otcv8-dev/tree/3d32139512cc4576b105682c3579f18fe0d534e4 "OTClientV8 development reference revision"
[2]: https://github.com/OTCv8/otclientv8 "OTClientV8 official distribution and server-owner asset guidance"
[3]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/game_features/features.lua "OTClientV8 740 feature thresholds"
[4]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/src/client/protocolcodes.h "OTClientV8 normal opcode declarations"
[5]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/gamelib/protocollogin.lua "OTClientV8 legacy character-list parser"
