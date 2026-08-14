# Development Checklist

- [x] Define safe local `account create` and `player create` command behavior for native-world testing.
- [x] Implement Argon2-backed account creation and account-owned character creation through the CLI.
- [x] Add unit and CLI-level coverage for provisioning, duplicate handling, and native numeric account lookup.
- [x] Build and publish updated Linux and Windows binaries with the provisioning commands.
- [ ] Diagnose and correct the reported native OTCv8 login rejection after local account provisioning.
- [ ] Diagnose and correct the native OTCv8 game-session disconnect after successful character selection.
- [ ] Clarify the intentionally minimal character fixture and defer professions/vocations to the player-state milestone.
- [ ] Diagnose and correct the native OTCv8 client-side `ERROR 2` after successful character selection and retained server listener state.
