# Smoke-test checklist (run before every tag/release)

Goal: a deterministic ~10-minute pass over the core 7.4 loop using the CLI and one stock
OTCv8 7.4 client, before cutting a `fe-v7.4.*` tag. Items still blocked on *unmodified*-client
confirmation are called out explicitly rather than skipped silently.

## Prereqs
- `cargo build --release` succeeds; `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings`, and `cargo fmt --all -- --check` are all green.
- A world directory (created with `init`) plus operator-supplied client assets in
  `data/things/740/` (no assets are bundled — see the no-redistribution contract).

## Provisioning (CLI)
1. `forgotten-engine init <dir>` — world skeleton created; `validate` reports no import errors.
2. `forgotten-engine account create <dir> <name> <password>` — account persisted.
3. `forgotten-engine player create <dir> <account-id> <character-name> [vocation-id]` — character persisted.
4. `forgotten-engine player town <dir> <player-id> <town-id>` — temple assigned for respawn.
5. (optional) `forgotten-engine player equip <dir> <player-id> <slot> <server-item-id>` — starting gear.

## Run
6. `forgotten-engine run <dir>` — listener up; `status`/`fe-metrics` responds; `.fe-operator-port` written.

## In-client (stock OTCv8 7.4)
7. **login** — account/character select reaches the game viewport; HUD shows level/exp/health/mana/capacity.
8. **move** — click-to-walk and cardinal numpad moves stream smoothly; no teleport/hang on spam.
9. **fight** — `command spawn <dir> <monster>` then melee a nearby creature; health drops on both sides.
10. **loot** — defeat drops a corpse; open it and take an item into inventory.
11. **trade/inventory** — equip/unequip and container moves persist (CLI `player equip` / `container-*`).
12. **GM command** — `command broadcast` / `give` / `tp` / `kick` route through the running world's bridge.
13. **relog** — disconnect, reconnect; position, outfit, vitals, and inventory are retained.

## Open (unmodified-client confirmation still required)
- Real-client visual confirmation of the `0xA0` stats frame (no EOF error) and outfit persistence.
- Public chat rendering below protocol 760 (the message-mode map is empty; currently suppressed by design).

Any failure gets a tracking entry; do not tag until the loop above is green end-to-end.
