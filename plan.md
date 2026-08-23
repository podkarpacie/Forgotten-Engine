# Forgotten Engine — Master Plan & Roadmap to 100%

> **Document status:** Authoritative roadmap. Every subsystem below carries a current-completion
> estimate and the exact work required to reach production-ready 100%. Update this file whenever a
> milestone lands. Companion documents: `docs/capability-matrix.md` (per-feature truth),
> `todo.md` (short-term checklist), `docs/completion-ledger.md` (landed-work ledger).
>
> **Last updated:** ledger re-weighted to **30%** after corpse opening/decay (schema v27), owned-inventory ground drops, whisper/yell delivery, and the durable runtime tile-item registry.
> (schema v26), defeated-creature roll fix, whisper/yell delivery, and socket-test
> stabilization; pushed to `podkarpacie/Forgotten-Engine`.

---

## 0. Mission Statement

Forgotten Engine (FE) is an original, clean-room **Rust** MMORPG server engine pursuing behavioral
and operational compatibility with The Forgotten Server (TFS) and classic Tibia protocol targets.
It must become a **production replacement** for TFS that is:

1. **Easier** — single self-contained binary, zero-config SQLite by default, auto-migrations,
   `init → validate → run` in under 10 minutes.
2. **Faster** — Rust, typed authoritative world state, bounded lock contention, no scripting VM
   on the hot path unless explicitly enabled.
3. **More compatible** — loads operator-supplied TFS worlds (OTBM/XML/monsters/spawns/houses),
   TFS-style `config.lua`, and eventually TFS Lua scripts through a sandboxed host.
4. **Safer** — no UB (`unsafe_code = "forbid"`), atomic persistence boundaries, validated inputs
   at every layer, deterministic replayable transitions.
5. **Universal** — profile-based protocol support: standalone profiles for 7.4 / 8.0 / 8.6, then
   a universal edition spanning 7.4 → latest (13.x+).

### Target editions

| Edition | Tag | Scope |
|---|---|---|
| **fe-7.4** | `fe-v7.4.x` | Tibia 7.4 native path — first production target |
| **fe-8.0** | `fe-v8.0.x` | Tibia 8.0 protocol foundation |
| **fe-8.6** | `fe-v8.6.x` | Tibia 8.6 protocol foundation |
| **fe-universal** | `universal-v1` | One binary, all supported clients 7.4→13.x, per-version feature gating |

---

## 1. Overall Completion

**Current overall estimate: ~22% of the full TFS-replacement vision.**

The engine has a strong, well-tested skeleton (341 tests green): networking, persistence,
protocol codecs for 7.4, movement, chat, combat foundations, progression, inventory transfers,
static spawns with bounded AI, and a clean-room import pipeline. What remains is the long tail of
gameplay semantics, script compatibility, social/economy systems, multi-version protocols, and
operational hardening.

### Completion by layer

| Layer | Crate(s) | Est. complete | Notes |
|---|---|---|---|
| Build / CI / packaging | root, `.github`, `scripts` | **85%** | Missing: release automation polish, benchmarks in CI, MSRV check |
| Configuration | `forgotten-config` | **70%** | TFS config.lua subset parsed; missing many keys, hot reload |
| Core domain model | `forgotten-core` | **35%** | Strong primitives; missing containers-on-map semantics depth, houses runtime, guilds, trade |
| Persistence | `forgotten-persistence` | **55%** | Schema v24; missing players-full round-trip breadth, house items, guild storage, migrations UX |
| Protocol (7.4) | `forgotten-protocol` | **45%** | Login/game/status + map/movement/chat/inventory codecs verified against OTCv8 evidence |
| Protocol (other versions) | `forgotten-protocol` | **5%** | 8.0/8.6/10.x/12.x+ foundations only |
| Host / session runtime | `forgotten-host` | **40%** | Native 7.4 sessions deep; legacy FE-OTC extended path thinner |
| Scripting | `forgotten-scripting` | **10%** | Sandboxed expression/callback dispatcher exists; no gameplay event surface |
| CLI / tooling | `forgotten-engine-cli` | **60%** | init/validate/run/tfs-audit/provisioning/bank/etc. present |
| Docs & evidence | `docs/` | **75%** | Excellent research/evidence culture; needs operator guides |

---

## 2. Phase Roadmap

Phases are ordered by dependency and value. Each phase lists workstreams with their own
completion estimates. A phase is "done" when every workstream hits 100% **and** its acceptance
criteria pass.

---

### PHASE 0 — Foundation & Hygiene ✅ (100%)

- [x] Rust workspace, crate boundaries, strict lints (`unsafe_code = "forbid"`)
- [x] CI: test + clippy `-D warnings` + fmt + JSON matrix validation
- [x] Windows/Linux builds; cross-compile toolchain
- [x] Capability matrix as source-controlled truth (MD + JSON)
- [x] Clean-room policy docs and protocol-evidence discipline
- [x] Windows accepted-socket non-blocking bug fixed (real-client sessions no longer rejected)
- [x] Atomic inventory persistence boundary (`replace_player_inventory`) — closes item-duplication race

---

### PHASE 1 — fe-7.4 Playable Core 🔄 (~45% → target 100%)

*Goal: an operator can download FE, import a TFS 7.4-style world, create accounts/characters, and
players can log in with stock OTCv8 7.4 and play the core loop: walk, fight monsters, gain XP,
loot corpses, equip gear, chat, die and respawn.*

#### 1.1 World & Map Runtime — ~65%
- [x] OTBM reader (bounded clean-room), `.femap` editable format, import/export workflow
- [x] Tile layers, ground/items, walkability, occupancy, streamed viewports
- [x] Spawns/houses/vocations XML companion loading; towns from OTBM
- [ ] **Runtime tile-item mutation journal hardening (90%)** — corpse placement currently writes
      runtime-only items without source identity; need explicit runtime-item registry so restart
      recovery distinguishes imported vs spawned items *(est. 2–3 days)*
- [ ] **Map item decay/cleanup (0%)** — corpses expire, dropped items despawn per configured TTL
      *(est. 3 days)*
- [ ] **Doors & levers as map state (0%)** — open/closed door tiles, lever toggles *(est. 4 days)*
- [ ] **Teleport pads full parity (80%)** — chained hops capped; add effect packets *(est. 1 day)*
- [ ] **Multi-floor viewport correctness audit (70%)** — verify floor-stack rendering against real
      client screenshots *(est. 2 days)*

#### 1.2 Items & Inventory — ~50%
- [x] Equipment slots, stack merges/splits, container↔equipment transfers (atomic persistence)
- [x] Depot/inbox/bank schema + storage APIs; house ownership/access-list schema
- [ ] **Item definitions completeness (40%)** — weight, stackable, slotType, defense, attack,
      requirements fully wired into combat/equipment admission *(est. 5 days)*
- [ ] **Container open/close via UseItem (25%)** — open backpack-in-hand/backpack-on-tile as
      nested window (depth ≤ 2), up-arrow parent navigation *(est. 4 days)*
- [ ] **Ground pickup/drop (30%)** — throw-to-ground creates map item; pick-up returns to
      container with capacity checks *(est. 3 days)*
- [ ] **Depot & inbox client windows (20%)** — open depot in town, move items in/out *(est. 5 days)*
- [ ] **Money handling (10%)** — gold coin stacks ↔ bank balance conversion, buy/sell plumbing
      *(est. 3 days)*
- [ ] **Item attributes client-visible (15%)** — action/unique IDs drive doors/quests; text/description
      look output *(est. 3 days)*

#### 1.3 Combat & Monsters — ~40%
- [x] Adjacent melee (player↔player, player↔static monster), armor mitigation, cooldowns
- [x] Declarative weapon catalog (scriptless), spell-cast foundation (mana/timing)
- [x] Static spawn lifecycle: reactivation intervals, spawn-area blockers, pursuit, direct melee
- [x] **Loot rolls + corpse spawn on defeat (new)** — deterministic seeded rolls, flat `<loot>`
      parsing, corpse as runtime map item *(just landed; needs persistence + client-open wiring)*
- [x] **Loot persistence across restart (new)** — durable revision-bound runtime tile-item
      registry (schema v26); corpses re-materialize on startup and fail closed on incompatible
      state; defeated-roll fix so loot tables roll after deactivation *(client corpse opening
      remains deferred)*
- [ ] **Monster health/condition effects (10%)** — poison/fire/burn conditions on players from
      monster attacks *(est. 4 days)*
- [ ] **Death list & frags (0%)** — kill tracking, skull system, unjustified kills *(est. 5 days)*
- [ ] **PvP zones & protection-zone enforcement (15%)** — `worldType` exists; add PZ tile semantics,
      no-logout zones *(est. 4 days)*
- [ ] **Spells v1 (20%)** — declarative spell catalog executes: damage/heal/area effects with
      cooldowns and mana; no Lua yet *(est. 8 days)*
- [ ] **Ranged/weapons distance (0%)** — bows/wands, ammo consumption *(est. 5 days)*
- [ ] **Blocking/shield/defense formulas (25%)** — shield defense value, block chance *(est. 3 days)*

#### 1.4 Player Lifecycle — ~70%
- [x] Numeric-account login, character select, session bootstrap, HUD stats/skills/outfit
- [x] Death → temple respawn, fixed-percent death loss, level-up vitals gains
- [x] Regeneration schedules, condition advancement with death transition
- [x] Relog safety (orderly + abrupt disconnect), position persistence
- [ ] **Default-formula death loss (50%)** — implement the audited default formula path alongside
      fixed-percent *(est. 3 days)*
- [ ] **Blessings & promotion (0%)** — bless charges reduce loss; promotion vocation tier *(est. 5 days)*
- [ ] **Vocation attackspeed/basespeed enforcement (30%)** — metadata exists; wire into combat
      cadence *(est. 2 days)*
- [ ] **Outfit change window (40%)** — outfit frame accepted; add server-side outfit storage +
      addon gating *(est. 2 days)*

#### 1.5 Chat & Social — ~35%
- [x] Public Say with parser-backed 0xAA layout; NPC proximity keyword dialogue
- [x] Shared chat queue, VIP presence fan-out, party shields snapshot
- [ ] **Whisper/yell range modes (0%)** — mode 2/3 with distance filtering *(est. 2 days)*
- [ ] **Private messages (10%)** — sender ack + recipient delivery *(est. 2 days)*
- [ ] **Default channel UI (25%)** — join/leave channel records, channel tab messages *(est. 4 days)*
- [ ] **Party mechanics (30%)** — invite/join/leave/shared-exp eligibility exist; wire client
      packets for invite dialogs *(est. 4 days)*
- [ ] **Guilds v1 (0%)** — create/disband/ranks/titles/guild chat *(est. 10 days)*
- [ ] **Trade between players (0%)** — offer/accept/atomic swap with anti-dupe guarantees *(est. 6 days)*
- [ ] **Friends/VIP list management (40%)** — add/remove/edit exist; persist edits *(est. 2 days)*

#### 1.6 NPCs — ~25%
- [x] Static materialization, NPC-not-attackable guards, keyword dialogue XML
- [ ] **NPC shops (0%)** — buy/sell windows, stock, currency conversion *(est. 8 days)*
- [ ] **Travel/transport (0%)** — passage destinations, fare payment *(est. 3 days)*
- [ ] **Focus & conversation state (0%)** — NPC turns to speaker, timeout clears focus *(est. 3 days)*

#### 1.7 Quests & Interaction — ~10%
- [x] Quest-log request returns explicit empty response
- [ ] **Quest log v1 (0%)** — quest state storage, mission lines, client quest window *(est. 6 days)*
- [ ] **Action/use-with semantics v1 (5%)** — doors, levers, food, potions, runes *(est. 10 days)*

**Phase 1 exit criteria:** a 2-player smoke test completes: login → walk → kill rat → loot corpse →
open corpse → take gold → buy potion from NPC shop → drink potion → die to monster → respawn at
temple → relog retains everything. All under stock OTCv8 7.4.

---

### PHASE 2 — Scripting & TFS Compatibility Layer 🔄 (~10% → target 100%)

*Goal: existing TFS servers migrate with minimal script rewrites.*

#### 2.1 Sandboxed Lua Host — ~15%
- [x] Vendored Lua VM, no stdlib, instruction/memory budgets, callback file registration with
      traversal rejection, primitive-only results
- [ ] **Event dispatch surface (5%)** — creature events, talkactions, actions, movements,
      globalevents registered from TFS XML registries *(est. 15 days)*
- [ ] **TFS-compatible API subset (0%)** — `doCreatureSay`, `doPlayerAddItem`, `getThingPos`,
      teleport, storage functions mapped onto typed core calls *(est. 20 days)*
- [ ] **Script hot-reload (0%)** — reload command with safe swap *(est. 4 days)*
- [ ] **Perf guardrails (0%)** — budget enforcement under load, script timeouts don't stall world
      tick *(est. 4 days)*

#### 2.2 Data Migration Tooling — ~55%
- [x] `tfs-audit`: config/world/items/spawns/houses/registries/entities inventory + diagnostics
- [x] Conversion-readiness report distinguishing importable vs deferred content
- [ ] **Full TFS MySQL → FE SQLite importer (0%)** — accounts, players, depots, houses, guilds
      *(est. 10 days)*
- [ ] **Round-trip verification suite (0%)** — import → export diff = zero *(est. 4 days)*

**Phase 2 exit criteria:** a representative public TFS 8.x distribution's data directory imports;
its simple Lua scripts (talkaction hello, action door) run unmodified through the sandbox.

---

### PHASE 3 — Production Hardening 🔄 (~30% → target 100%)

#### 3.1 Performance — ~35%
- [x] Bounded queues, epoch-based refresh, detached render snapshots, worker render pool
- [ ] **Benchmark harness in CI (0%)** — headless bot swarm: N clients walk/fight/chat; track
      p99 latency, memory, CPU *(est. 5 days)*
- [ ] **Lock profiling & sharding (20%)** — shared-world mutex is global; evaluate per-region
      shards or actor model *(est. 10 days)*
- [ ] **Memory audit (0%)** — arena/pool allocations for frames, tile snapshots *(est. 4 days)*
- [ ] **Target:** 1,000 simulated players, p99 input-ack < 50ms, < 2GB RSS *(est. ongoing)*

#### 3.2 Reliability & Ops — ~45%
- [x] Backups (SQLite copy + manifest), event log, graceful shutdown, crash-safe transactions
- [ ] **Auto-save cadence (0%)** — periodic world snapshot flush *(est. 2 days)*
- [ ] **Watchdog/self-heal (0%)** — session leak detection, stuck-lock breaker *(est. 3 days)*
- [ ] **Metrics endpoint (0%)** — Prometheus-style counters *(est. 3 days)*
- [ ] **Docker image (0%)** — official image + compose example *(est. 2 days)*

#### 3.3 Security — ~50%
- [x] Argon2 passwords, bounded inputs everywhere, no unsafe, path-traversal rejection
- [ ] **Rate limiting (0%)** — login attempts, chat spam, packet floods *(est. 3 days)*
- [ ] **Ban/mute system (0%)** — account/IP bans, mutes with expiry *(est. 4 days)*
- [ ] **Fuzzing (0%)** — cargo-fuzz targets for decoders *(est. 5 days)*

#### 3.4 Documentation — ~60%
- [x] Capability matrix, research notes, protocol evidence, contract docs
- [ ] **Operator guide (0%)** — install, configure, import, run, backup, upgrade *(est. 4 days)*
- [ ] **Scripting guide (0%)** — sandbox API reference *(est. 5 days)*
- [ ] **Migration guide from TFS (0%)** — step-by-step *(est. 2 days)*

---

### PHASE 4 — fe-7.4 Production Release 📦 (target)

- [ ] Full regression suite green on Win+Linux, release + debug
- [ ] Real-client manual test matrix (login, movement, combat, loot, death, chat, party)
- [ ] Load test at target numbers documented in `docs/benchmarks/`
- [ ] Release artifacts: signed binaries + SHA256SUMS + INSTALL.txt
- [ ] Git tag `fe-v7.4.100`, GitHub Release, announcement post
- **Estimated additional effort: 3–4 weeks**

---

### PHASE 5 — fe-8.0 & fe-8.6 Profiles (~5% → target 100%)

- [x] Profile scaffolding (`fe-8.0` exists as protocol foundation)
- [ ] **8.0 codec set (5%)** — login RSA differences, game protocol deltas vs 7.4 *(est. 15 days)*
- [ ] **8.6 codec set (0%)** — adds outfits v2, more channels, party improvements *(est. 15 days)*
- [ ] **Per-version feature gating (0%)** — capability flags per profile so one codebase serves all
      *(est. 5 days)*
- [ ] **Real-client verification per version (0%)** — same smoke matrix as 7.4 *(est. 5 days each)*
- **Estimated effort: 8–10 weeks**

---

### PHASE 6 — Modern Clients (10.x → 13.x) (~5% → target 100%)

Required for the universal edition and parity with current TFS master (13.10+).

- [ ] **10.x protocol (0%)** — session keys, OTServList/OTSID login flow *(est. 15 days)*
- [ ] **11/12.x asset pipeline (0%)** — dat/pack/spr hashing, thing type resolution *(est. 20 days)*
- [ ] **12.x+ features (0%)** — imbuements? store? (scope decision needed) *(est. 20+ days)*
- [ ] **13.x protocol (0%)** — current TFS-master parity target *(est. 20 days)*
- [ ] **Feature-gate matrix expansion (0%)** — modern-only systems hidden from old clients *(est. 5 days)*
- **Estimated effort: 12–16 weeks** (largest remaining chunk)

---

### PHASE 7 — Universal Edition (`universal-v1`) (~0% → target 100%)

One binary serving 7.4 → latest simultaneously:

- [ ] **Multi-version listener negotiation (0%)** — version detection at handshake *(est. 5 days)*
- [ ] **Cross-version world consistency (0%)** — same world, per-client rendering adapters *(est. 10 days)*
- [ ] **Admin controls (0%)** — per-version min/max client allowlist *(est. 2 days)*
- [ ] **Universal test matrix (0%)** — automated multi-client soak *(est. 5 days)*
- **Estimated effort: 5–6 weeks** (after Phases 5–6)

---

### PHASE 8 — Mod / Plugin SDK (~5% → target 100%)

Folder-based mods with manifests, plus optional native plugins:

- [x] Backup manifest already anticipates `plugins,scripts` includes
- [ ] **Mod format spec (0%)** — `mods/<name>/mod.toml` (scripts, xml overrides, assets, deps)
      *(est. 3 days)*
- [ ] **Mod loader (0%)** — discovery, dependency resolution, load order, conflict detection
      *(est. 8 days)*
- [ ] **`fe mod` CLI (0%)** — new/build/pack/install/list/enable/disable *(est. 5 days)*
- [ ] **Typed Lua defs + templates (0%)** — LSP-friendly stubs, example mods *(est. 5 days)*
- [ ] **Native plugin ABI (0%)** — stable C ABI for compiled extensions, loaded via `libloading`
      with capability negotiation *(est. 15 days)*
- [ ] **Sandboxing for untrusted mods (0%)** — capability grants, resource limits *(est. 8 days)*
- [ ] **SDK docs + 3 showcase mods (0%)** *(est. 5 days)*
- **Estimated effort: 7–9 weeks**

---

## 3. Cumulative Effort Estimate

| Phase | Remaining effort (solo + AI-assisted) |
|---|---|
| Phase 1 (7.4 core) | 8–10 weeks |
| Phase 2 (scripting/TFS compat) | 8–10 weeks |
| Phase 3 (hardening) | 4–5 weeks |
| Phase 4 (release) | 3–4 weeks |
| Phase 5 (8.0/8.6) | 8–10 weeks |
| Phase 6 (modern clients) | 12–16 weeks |
| Phase 7 (universal) | 5–6 weeks |
| Phase 8 (mod SDK) | 7–9 weeks |
| **Total to full vision** | **~55–70 weeks** (~1 year solo pace) |

Milestones can ship independently: **fe-7.4 production (Phase 4)** is achievable in ~4 months and
is itself a useful, releasable product.

---

## 4. Immediate Next Steps (this week)

1. **Loot completion** — persist corpses across restart via runtime-item registry; wire
   client-visible corpse opening (UseItem on corpse → container window). *(registry, opening,
   decay, and ground drops all landed; remaining half is loot taking from opened windows)*
2. **Container UseItem opening** — backpack-in-hand nested windows (Phase 1.2); the corpse
   window machinery (window IDs, close handling, catalog rendering) now provides the base.
3. ~~Whisper/yell/private chat modes~~ — whisper and yell landed (`d98ecc5`).
4. ~~Update capability matrix / ledger / commit / push~~ — ledger re-weighted to **30%** with
   evidence-backed credits for whisper/yell, the durable runtime registry, corpse
   opening/decay, and owned-inventory ground drops.
5. **Next:** loot taking from opened corpse windows → then pickup of dropped runtime items.

---

## 5. Definition of Done (project-wide)

- All phases above at 100% with acceptance criteria met
- `cargo test --workspace` green on Windows + Linux (debug & release)
- `cargo clippy -- -D warnings` clean; `cargo fmt --check` clean
- Real-client smoke matrix passes for every shipped profile
- Load-test targets documented and met
- Operator/scripting/migration guides published
- Capability matrix reflects reality (no aspirational claims)
- Versioned releases with checksums for every edition

---

## 6. Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| Global world mutex limits scale | High | Benchmark early (Phase 3.1); design shard boundary before Phase 6 |
| Lua compat scope creep | High | Strict API whitelist; ship "compatibility tiers" (bronze/silver/gold) |
| Modern-client protocol complexity | High | Timebox Phase 6; consider shipping universal without 13.x if needed |
| Solo-dev burnout | Medium | Ship small milestones; keep changelog + ledger updated for morale |
| Clean-room contamination | Critical | Maintain evidence-doc discipline; never read upstream C++ while implementing |
| Test flakiness regressions | Medium | Keep socket tests retry-hardened like the relog tests; run suites 5× before tagging |
