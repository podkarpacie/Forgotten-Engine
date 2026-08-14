# Forgotten Engine

**Forgotten Engine** is an original Rust implementation pursuing versioned, clean-room behavioral and operational compatibility with The Forgotten Server. It currently provides original foundations for direct **Tibia 7.4** and **Tibia 8.0** targets and **TFS 1.2 / Tibia 10.98**, including TFS-style configuration discovery, content validation, embedded SQLite storage, persistent game/status listeners, migrations, diagnostics, backups, bounded protocol contracts, and a versioned parity roadmap.

> This project is not a port or source translation of The Forgotten Server. It uses independently implemented Rust code and treats upstream projects only as behavior and terminology references. No Tibia client assets, maps, item databases, or copyrighted game data are included.

## Compatibility-profile scope

| Area | Delivered foundation |
|---|---|
| TFS-style startup | World-local `config.lua` discovery, typed required-value diagnostics, original `data/` content-skeleton validation, database initialization, and startup phase output. |
| Local server lifecycle | Persistent configuration-driven game, status, and opt-in session-foundation TCP listeners with connection limits, timeouts, session logging, and orderly in-process shutdown. |
| Persistence | SQLite auto-creation, migrations, Argon2 account verification, account-owned character lookup, and an event ledger. |
| Operations | `init`, `validate`, `run`, `status`, `generate-key`, `backup`, `command`, `compatibility`, and `version` commands via the `forgotten-engine` executable. |
| Protocol | Explicit FE 7.4.0 / Tibia 7.4, FE 8.0.0 / Tibia 8.0, and FE 1.2.0 / TFS 1.2 / Tibia 10.98 profiles; an FE diagnostic probe; TFS-style XML/binary status queries; bounded opt-in 7.4 RSA/XTEA login/character-list contracts; a separate FE-aware OTClient endpoint with an original empty-world viewport, tick, and cardinal-movement contract; and a profile-driven normal OTClientV8 740 login/character-list/empty-world path that requires no FE client module. |
| Scripting | Honest capability matrix for a narrow TFS 1.2 Lua compatibility surface. |
| Safety | Content validation, backup manifests, structured diagnostics, and no unsafe Rust. |

## Quick start

```bash
cargo run -p forgotten-engine-cli -- init ./my-engine-world --profile fe-7.4
cargo run -p forgotten-engine-cli -- validate ./my-engine-world
cargo run -p forgotten-engine-cli -- generate-key ./my-engine-world
cargo run -p forgotten-engine-cli -- run ./my-engine-world
cargo run -p forgotten-engine-cli -- command ./my-engine-world broadcast "Welcome to Forgotten Engine"
cargo run -p forgotten-engine-cli -- backup ./my-engine-world
```

The `init` command creates a world-local `config.lua`, an original TFS-style `data/` directory skeleton, and an embedded `data/forgotten-engine.db`. It defaults to `fe-1.2`; `--profile fe-8.0` selects direct Tibia 8.0, and `--profile fe-7.4` selects direct Tibia 7.4. `run` binds `gameProtocolPort` and `statusProtocolPort` from `config.lua` and keeps both listeners online until Ctrl+C. `generate-key` writes an original local 1024-bit RSA key at the configured `rsaPrivateKey` path; `legacyLoginEnabled = true` enables the bounded login foundation, while `gameSessionEnabled = true` separately enables the challenge/session foundation on `gameSessionPort` for an `fe-7.4` world.

For the first **unmodified OTClientV8** target, configure a normal native client profile explicitly rather than relying on an FE profile name:

```lua
otclientV8NativeEnabled = true
otclientV8ProtocolVersion = 740
otclientV8NumericAccountIds = true
otclientV8LoginPacketEncryption = false
otclientV8ProtocolChecksum = false
otclientV8ChallengeOnLogin = false
advertisedOtClientV8Host = "203.0.113.24" -- public IPv4 address

-- These are operator-owned identifiers from lawful matching 740 .dat/.spr data.
otclientV8NativeEmptyWorldEnabled = true
otclientV8EmptyWorldGroundThingId = 102
otclientV8PlayerLookType = 128
otclientV8PlayerSpeed = 220
otclientV8ServerBeat = 50
```

This starts native login and game listeners on ports `7174` and `7175` by default. A stock OTCv8 build configured for protocol `740` can authenticate with a numeric FE account ID and receive its ordinary legacy character list without an FE-specific module or extended opcode. With the explicit empty-world values above, the selected character then receives normal player-login and full-map packets with a ground fixture, one visible local player, and bounded cardinal movement acknowledgements. Operators must supply lawful matching `.dat`/`.spr` thing data in their own OTCv8 installation; FE neither includes nor generates client assets. The example identifiers are configuration values, not FE-distributed game data, and must be replaced if they do not match the operator’s client data.

> This is a renderable **fixture**, not a real map server. Movement is accepted only inside the initial 18×14 viewport; FE does not yet stream map rows, load maps, validate collision from item data, populate other creatures, or implement gameplay.

> The FE host and status endpoint are real TCP services. The FE 7.4 foundations include original RSA/XTEA account authentication, character-list handling, challenge issuance, challenge-bound session authentication, an FE-aware custom OTClient capability acknowledgement, encrypted identity/start-position/advertised-endpoint metadata, a deterministic FE-owned empty-world viewport with cardinal movement acknowledgements, and a separate normal OTCv8 740 credential/character-selection/full-map fixture. However, FE 7.4.0, FE 8.0.0, and FE 1.2.0 do **not** yet claim broad playable-client interoperability or drop-in replacement status. Real maps and map deltas, item and creature systems, collision, Lua VM parity, combat, and broad world simulation remain feature-gated pending independent specifications and acceptance tests.

## Architecture

See [TFS_COMPATIBILITY_MATRIX.md](TFS_COMPATIBILITY_MATRIX.md) for the complete upstream-surface mapping, [PARITY_ROADMAP.md](PARITY_ROADMAP.md) for the versioned acceptance sequence, [NETWORK_MILESTONE.md](NETWORK_MILESTONE.md) for the diagnostic-session contract, [RUNTIME_READINESS.md](RUNTIME_READINESS.md) for deployment and client limits, [OTCLIENT_INTEGRATION.md](OTCLIENT_INTEGRATION.md) for the custom-client capability contract, and [OTCLIENTV8_740_NATIVE_CONTRACT.md](OTCLIENTV8_740_NATIVE_CONTRACT.md) for the normal native-client contract. See [VERSIONING.md](VERSIONING.md) for the published FE-to-TFS/Tibia mappings.

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
