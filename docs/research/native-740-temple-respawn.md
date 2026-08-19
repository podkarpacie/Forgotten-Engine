# Native 740 Temple Respawn: Client Evidence and FE Boundary

## Evidence

The public OTCv8 parser handles server opcode `0x28` as the death record. Under the selected classic profile, the existing zero-payload FE death record is therefore the only accepted death notification used by this boundary. The parser also treats a later full-map record as a teleport when a map is already known.[1]

The public OTCv8 death-dialog module does not send a revive opcode. Its **Ok** callback invokes `CharacterList.doLogin()`. That helper first waits for `g_game.isOnline()` to become false after `g_game.safeLogout()`, then opens a new game login connection.[2]

| Client observation | FE implementation decision |
|---|---|
| The death dialog begins a new character login rather than sending a revive request. | Do not invent an in-session revival packet or client opcode. |
| The new game session receives normal initialization including a full-map description. | Before shared-world registration, respawn a persisted-dead owned character through the existing authoritative core transition, then use the normal native bootstrap. |
| The core transition resets current health and mana to the persisted maxima while retaining the other persisted vital fields. | Persist exactly the transition output, then rehydrate the normal native player-stats snapshot. |

## Bounded FE behavior

FE authenticates and verifies character ownership first. If the selected persisted character is marked dead, FE builds the existing isolated authoritative transition, checks that the stored temple destination is walkable in the active map, and atomically persists the returned position, vitals, and cleared lifecycle state. Only then does FE register the player in the shared world and emit the regular parser-shaped initialization record.

This behavior is covered by a live socket regression. It asserts that a persisted-dead character receives a full-map bootstrap centered on the temple and that SQLite contains the restored position, returned vitals, and a cleared respawn state.

> This is not a claim of complete death parity. FE does not add death-screen timing, automatic respawn timers, a direct revival packet, teleport effects, loss records, default loss formulas, blessing or promotion adjustments, or unmodified-client acceptance evidence.

## References

[1]: https://github.com/OTCv8/otclientv8/blob/master/src/client/protocolgameparse.cpp "OTCv8 protocol game parser"
[2]: https://github.com/OTCv8/otclientv8/blob/master/modules/game_playerdeath/playerdeath.lua "OTCv8 death dialog module"
