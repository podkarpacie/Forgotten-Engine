# Concurrency Model

Forgotten Engine uses Rust threads for independent service listeners and bounded concurrent client
sessions. This is intentionally distinct from allowing arbitrary concurrent mutation of gameplay
state. The engine treats **throughput** and **authoritative determinism** as separate requirements:
parallel work must not change the observable result of a world tick.

| Work category | Current execution boundary | Authoritative-state rule |
| --- | --- | --- |
| Listener accept loops | One service thread per enabled listener | Never mutate a world directly. |
| Client sessions | One bounded worker thread per accepted active session | Submit only validated actions; session parsing and frame writes remain isolated. |
| Shared native world | Shared through a synchronized authoritative world handle | Registration, movement, vitals, progression, conditions, and removal mutate through one protected state transition at a time. |
| Database connections | Short-lived per-operation SQLite access with a busy timeout | No database connection is shared across worker threads. |
| Static configuration, map, and item catalogs | Immutable `Arc`-shared data | Read concurrently without locks once validation completes. |

> The current mutex protects memory safety but is **not itself a global deterministic scheduler**:
> simultaneous sessions can acquire the lock in host scheduling order. FE must therefore preserve
> a single authoritative mutation boundary now, then introduce explicit tick/sequence ordering
> before moving simulation mutation to a multi-worker implementation.

## Approved parallelization boundary

The following work can be parallelized without changing world state, provided it consumes immutable
snapshots and returns a bounded result to the authoritative boundary:

| Parallel-safe work | Required hand-off |
| --- | --- |
| Config, XML, OTB, OTBM, and map companion parsing | Fully validated immutable catalog or a typed load error. |
| Client-frame decoding and input validation | Typed action with a session-local monotonic sequence number. |
| Packet preparation from a captured world snapshot | Bounded encoded frame; never hold the world lock during socket I/O. |
| Conversion audit and content inventory | Stable sorted report generated from immutable input. |
| Per-session network waiting and frame writes | Session-local I/O result only. |

## Required deterministic simulation design

Before parallel worker pools execute gameplay simulation, FE will introduce a command queue with an
explicit tick number and stable tie-breaker `(tick, player_id, session_sequence)`. The authoritative
world will apply a sorted batch at each tick, publish an immutable snapshot, and release the lock
before packet encoding or network writes. This preserves reproducible outcomes while allowing
decoding, encoding, and disconnected-content work to use available CPU cores.

## Benchmark and regression gate

The multithreaded rollout must be measured rather than assumed to be faster. Each implementation
slice will record, with a fixed local scenario, worker count, active-session count, accepted actions
per second, p50/p95 authoritative lock hold time, p50/p95 action latency, peak resident memory, and
snapshot/packet preparation time. The regression suite must additionally prove that the same
ordered input batch produces byte-identical world snapshots and equivalent authoritative revisions
under one and multiple worker threads.

No performance or scalability claim is made until those deterministic and measured gates pass.
