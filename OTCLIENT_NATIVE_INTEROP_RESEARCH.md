# OTClient-Native Interoperability Research Notes

## Public reference observations

The upstream OTClient game protocol connects to a selected host and port, initializes its receive path, and either sends a standard login packet immediately or waits for a configured game challenge feature.[1] Its enter-game module exposes host, port, and selectable supported client versions, sets the selected client/protocol version, chooses an RSA key, and proceeds only after client “things” data is loaded.[2]

The upstream project’s 7.4 issue history also shows a `things/740/Tibia.dat` data-path expectation. That material is client data, not Forgotten Engine content; FE must not redistribute it. An operator who uses a 7.4 OTClient profile needs to supply client assets lawfully and configure the matching profile independently.[3]

The public send codec shows the normal game-login ordering at a high level: a pending-game opcode, operating-system value, protocol version, feature-gated version/revision fields, an RSA block beginning with a zero marker, optional XTEA words, credentials or session identity, optional challenge fields, RSA encryption, and then XTEA enablement.[4] The public parse codec separately recognizes normal login, full-map, map-row, tile, creature-move, and walk-control messages; extended opcodes are only one optional parser case, not the normal map/session protocol.[5]

## Implication for Forgotten Engine

The current `gameSessionPort` path is intentionally **not** native OTClient interoperable. It uses an FE-specific raw-RSA bootstrap, extended-opcode capability acknowledgement, and `fe.*` world payloads. These were useful server-side foundations but diverge before a normal unmodified OTClient has parsed a standard game login and world packet.

To satisfy the unmodified-OTClient target, FE needs an independently specified normal 7.4-compatible path that uses the selected OTClient’s standard connection/login/character-selection/game-session framing. The current FE-specific path should remain feature-gated only as an internal diagnostic harness until native fixtures prove the normal path. The first native milestone must cover packet bytes, packet ordering, encryption/checksum policy, character-list format, normal initial map/player data, and movement acknowledgement semantics—not merely a custom module response.

## Research limits

These observations are limited to public OTClient behavior and do not authorize copying source, assets, maps, item databases, or proprietary client data. They establish a clean-room test target rather than an implementation source.

## OTClientV8 target observations

OTClientV8’s official distribution repository identifies the ready-to-run client separately from its development source repository and states that server owners must add their own sprite and data files under `data/things`.[6] It also documents custom server entries in `init.lua`, including a direct `ip:port:version` form and a user-visible custom-server option. Those are client-side distribution settings, not FE protocol extensions.[6]

Its public development send codec retains the normal pending-game, OS, selected custom protocol version, RSA-zero-marker, feature-gated XTEA/credential/challenge flow. It additionally may append OTCv8-identification text when no extended login data is set, so FE must accept or deliberately bound this tail based on the exact selected feature profile rather than assume the older upstream packet shape.[7] The public parser still treats ordinary login, full map, map-row, tile, creature, and walking messages as normal game opcodes; custom extended opcodes are not a replacement for those messages.[8]

For the first FE target, the acceptance definition will be a **stock OTCv8 binary from `OTCv8/otclientv8`, configured through its documented direct custom-server entry for protocol 7.4, using operator-supplied lawful 7.4 thing data**. The exact immutable client revision and its enabled feature profile must be captured before FE claims a passing interoperability result.

The public `OTCv8/otcv8-dev` reference was inspected at commit `3d32139512cc4576b105682c3579f18fe0d534e4`. Its public enter-game profile list includes `740`, and its game library also enumerates `740` among supported client values.[9] This confirms that FE can make the first acceptance target concrete: an unmodified OTCv8 build configured with the listed `740` profile—not an FE module and not the current custom-session endpoint.

The same public revision declares normal client pending-game and enter-game opcodes, then normal cardinal walking opcodes (`101` through `104`) and normal full-map/movement server opcode families. Those ordinary messages are distinct from the client’s optional extended-opcode range.[10] For the FE native path, packet fixtures will therefore begin with standard pending-game/login and normal walking/full-map messages, retaining FE extended opcodes only for the separate diagnostic path.

The public 740 feature rules show that the stock 740 profile does **not** enable the later login-packet XTEA/RSA encryption, protocol checksum, account-name strings, or challenge-on-login switches; those begin at later version thresholds. Its normal 740 login fields therefore use a numeric account identifier and legacy response form.[11] FE must add a separately gated numeric-account native path rather than force its existing encrypted FE-specific login foundation onto OTCv8 740.

The public login module shows that a legacy 740 character-list request contains the client-enter-account opcode, OS, selected protocol value, client data signatures, numeric account identifier, password string, OTCv8 tail data, and profile-dependent padding/encryption behavior. Its legacy character-list response parser expects opcode `100`, character count, each character name/world name/IPv4/port, then a 16-bit premium-day field.[12] These are the first native login fixtures; FE’s existing custom login response is not sufficient.

The public OTCv8 IPv4 helper converts the parsed `u32` through network-to-host byte order before rendering the address. Native FE character-list encoding must therefore serialize a normal IPv4 `u32` in the protocol’s little-endian integer representation so that the stock client sees the configured game endpoint correctly.[13]

## References

[1]: https://github.com/edubart/otclient/blob/master/src/client/protocolgame.cpp "OTClient ProtocolGame connection and first-message behavior"
[2]: https://github.com/edubart/otclient/blob/master/modules/client_entergame/entergame.lua "OTClient host, port, client-version, RSA, and thing-data selection flow"
[3]: https://github.com/edubart/otclient/issues/721 "Public issue showing the 7.4 `things/740/Tibia.dat` loading path"
[4]: https://github.com/edubart/otclient/blob/master/src/client/protocolgamesend.cpp "OTClient public game-login send codec"
[5]: https://github.com/edubart/otclient/blob/master/src/client/protocolgameparse.cpp "OTClient public normal game-message parser"
[6]: https://github.com/OTCv8/otclientv8 "Official OTClientV8 distribution and server-owner configuration guidance"
[7]: https://github.com/OTCv8/otcv8-dev/blob/master/src/client/protocolgamesend.cpp "OTClientV8 public game-login send codec"
[8]: https://github.com/OTCv8/otcv8-dev/blob/master/src/client/protocolgameparse.cpp "OTClientV8 public normal game-message parser"
[9]: https://github.com/OTCv8/otcv8-dev/tree/3d32139512cc4576b105682c3579f18fe0d534e4 "OTClientV8 development source revision with declared 740 profile"
[10]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/src/client/protocolcodes.h "OTClientV8 normal client/server opcode declarations"
[11]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/game_features/features.lua "OTClientV8 feature thresholds for legacy client profiles"
[12]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/modules/gamelib/protocollogin.lua "OTClientV8 legacy login request and character-list parser"
[13]: https://github.com/OTCv8/otcv8-dev/blob/3d32139512cc4576b105682c3579f18fe0d534e4/src/framework/stdext/net.cpp "OTClientV8 IPv4 conversion helper"
