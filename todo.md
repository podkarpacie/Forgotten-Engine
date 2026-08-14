# Development Checklist

- [x] Define safe local `account create` and `player create` command behavior for native-world testing.
- [x] Implement Argon2-backed account creation and account-owned character creation through the CLI.
- [x] Add unit and CLI-level coverage for provisioning, duplicate handling, and native numeric account lookup.
- [x] Build and publish updated Linux and Windows binaries with the provisioning commands.
- [ ] Diagnose and correct the reported native OTCv8 login rejection after local account provisioning.
- [ ] Diagnose and correct the native OTCv8 game-session disconnect after successful character selection.
- [ ] Clarify the intentionally minimal character fixture and defer professions/vocations to the player-state milestone.
- [ ] Diagnose and correct the native OTCv8 client-side `ERROR 2` after successful character selection and retained server listener state.
- [ ] Add wire-level native game-session diagnostics and correct the remaining 740 post-selection initialization mismatch.
- [ ] Correct the native 740 game-login request decoder after the live client exposed `StringTooLong(1536)` before initialization.
- [ ] Correct the native 740 game-start transition after the client receives login/map frames but returns to character selection.
- [ ] Correct the outbound native 740 initialization stream after the real client emits no post-map control frame.
- [ ] Remove the invalid zero extended-outfit player creature from the asset-free native map diagnostic fixture.
- [ ] Correct remaining native 740 game-start packet sequencing after map-only initialization still returns to character selection.
- [ ] Treat Windows nonblocking `WouldBlock` during a native session as a transient condition rather than a rejected game session.
