# Native Render Preparation Benchmark — FE 7.4.44

## Decision

The bounded worker path is **not faster** than direct single-threaded viewport encoding for one
isolated native 740 frame under this local scenario. Its median per-frame cost was **63.860 µs**,
compared with **27.594 µs** for direct encoding. The worker was therefore **2.314×** the direct
median cost, or **131.43%** additional median overhead.

This is expected for a small per-frame workload because the worker path also pays for a bounded
channel hand-off, snapshot ownership transfer, wake-up/scheduling, response transfer, and timeout
boundary. It does **not** measure or justify a full parallel gameplay engine. The staged worker
remains intentionally disconnected from the production listener.

## Method

The benchmark is an ignored Rust regression in `forgotten-host`:

```text
cargo +stable test --release -p forgotten-host \
  benchmark_native_render_preparation_direct_and_worker -- --ignored --nocapture
```

It builds one fixed native 740 viewport scenario with one local player and one visible peer. Every
iteration encodes the same already-captured immutable render snapshot. Both paths must produce a
byte-identical frame before a sample is accepted.

| Parameter | Value |
|---|---:|
| Build mode | `--release` |
| Samples per path | 45 |
| Iterations per sample | 1,000 |
| Total frames per path | 45,000 |
| Map/player scenario | Fixed native 740 viewport, one local player and one visible peer |
| Direct path | In-session synchronous encoder from the immutable snapshot |
| Worker path | One `NativeRenderPreparationWorker`, 32-request queue, one response channel |
| Mutation during benchmark | None; packet equivalence is asserted on every iteration |

The run used a local six-logical-CPU AMD EPYC sandbox with an observed load average of `0.52`,
`0.38`, and `0.15`. It was not CPU-pinned and should not be treated as a stable cross-machine
performance claim.

## Aggregate results

All timing values are microseconds for a **1,000-frame sample**. Per-frame medians divide the
sample median by 1,000.

| Path | Median sample | p95 sample | Min–max sample | Median per frame |
|---|---:|---:|---:|---:|
| Direct synchronous encoder | 27,594 µs | 30,889 µs | 26,908–33,105 µs | 27.594 µs |
| Bounded worker hand-off | 63,860 µs | 70,496 µs | 56,046–72,551 µs | 63.860 µs |

| Comparison | Result |
|---|---:|
| Worker/direct median ratio | 2.314× |
| Worker median overhead | 131.43% |
| Correctness result | 45,000 byte-identical frames per path |

## Raw samples

Each value is the duration of 1,000 frame preparations, in microseconds.

| Run | Direct samples | Worker samples |
|---|---|---|
| 1 | 28066, 27123, 31180, 29607, 27580, 27746, 27503, 33105, 27449 | 56046, 68497, 62918, 68292, 69784, 70496, 71464, 67310, 65490 |
| 2 | 27548, 27755, 27594, 27137, 27423, 27319, 27347, 27360, 27411 | 60341, 62987, 62888, 59844, 56707, 58410, 57100, 58399, 57851 |
| 3 | 28615, 27315, 27303, 26975, 27391, 28720, 30889, 29742, 29693 | 69215, 63731, 63860, 60383, 67839, 66980, 68994, 72551, 70390 |
| 4 | 28432, 27997, 27795, 28031, 28369, 28392, 28515, 27617, 27875 | 59011, 61920, 60963, 60177, 66391, 62672, 67493, 66489, 63628 |
| 5 | 28106, 27716, 27462, 27488, 27257, 27091, 27558, 27527, 26908 | 61205, 63443, 64850, 61846, 66957, 69681, 68965, 67996, 68658 |

## Limits and next measurement

This benchmark measures only pre-captured viewport packet preparation. It does not measure
authoritative lock hold time, action latency, network writes, database work, multi-session
throughput, queue saturation, peak resident memory, or a multi-worker pool. It must not be used to
claim an end-to-end server speedup.

The next valid experiment is a **batched, multi-session** preparation scenario that keeps all world
mutation serialized, reports queue depth and saturation, and compares end-to-end frame preparation
after snapshot publication. Listener integration remains deferred until that scenario demonstrates
a benefit without changing byte-level packet order or authoritative outcomes.

## Concurrent three-stream follow-up

A second local release-mode experiment ran three synchronized immutable request streams. Each
stream prepared 500 copies of the same captured native 740 viewport per sample. The direct case
used three concurrent encoder threads. The worker case used three independent bounded
`NativeRenderPreparationWorker` instances, one per stream. Each of the resulting 1,500 frames per
sample was required to be byte-identical to the direct reference frame.

| Parameter | Value |
|---|---:|
| Samples per path | 35 |
| Streams / workers | 3 / 3 |
| Frames per sample | 1,500 |
| Total frames per path | 52,500 |
| Direct median per frame | 17.008 µs |
| Worker median per frame | 21.625 µs |
| Worker/direct median ratio | 1.271× |
| Worker median overhead | 27.14% |

| Path | Median sample | p95 sample | Min–max sample |
|---|---:|---:|---:|
| Direct concurrent encoder | 25,512 µs | 29,465 µs | 15,584–30,058 µs |
| Three bounded render workers | 32,437 µs | 36,566 µs | 25,137–38,192 µs |

The direct parallel path remained faster even after amortizing setup across three concurrent
streams. This result confirms that the staged worker hand-off is a correctness and isolation
foundation, not a demonstrated throughput improvement. It remains disconnected from the production
listener. A future benchmark must include a bounded shared queue, realistic heterogeneous snapshots,
queue-depth/saturation telemetry, and end-to-end post-publication work before any listener
integration is considered.

## Ordered publication-pool follow-up

The next local release-mode experiment measured the bounded `NativeRenderPreparationPool`. The
pool owns no authoritative-world, database, socket, action queue, or mutation capability. It sorts
caller-owned publication sequence numbers before scheduling, rejects duplicates, uses three workers
for the run, and returns frames in that sorted order after each detached snapshot has been encoded.

```text
cargo +stable test --release -p forgotten-host \
  benchmark_native_render_preparation_ordered_publication_pool -- --ignored --nocapture
```

| Parameter | Value |
|---|---:|
| Build mode | `--release` |
| Samples per path | 9 |
| Batches per sample | 500 |
| Publications per batch | 3 |
| Frames per sample | 1,500 |
| Total frames per path | 13,500 |
| Direct path | Three direct encodes in established publication sequence |
| Pool path | Three workers, ordered batch fan-out and ordered collection |
| Mutation during benchmark | None; every output batch is byte-identical to direct encoding |

All timing values are microseconds for one 500-batch sample. Per-frame medians divide the sample
median by 1,500 frames.

| Path | Median sample | p95 sample | Min–max sample | Median per frame |
|---|---:|---:|---:|---:|
| Direct ordered encoding | 43,796 µs | 46,562 µs | 43,133–46,562 µs | 29.197 µs |
| Three-worker ordered publication pool | 47,577 µs | 54,422 µs | 44,892–54,422 µs | 31.718 µs |

| Comparison | Result |
|---|---:|
| Pool/direct median ratio | 1.086× |
| Pool median overhead | 8.633% |
| Correctness result | 13,500 byte-identical frames per path, returned in publication-sequence order |

Raw sample durations were direct: `43796, 44030, 43133, 43202, 43389, 43303, 46454, 46562,
46182`; pool: `44892, 46879, 53379, 54422, 49094, 47861, 47317, 45515, 47577`.

The ordered pool is a validated bounded snapshot-publication boundary, **not** a demonstrated
server-speed improvement. It remains disconnected from the listener. Future work must still measure
real queue saturation, lock holds, action latency, memory, socket writes, and authoritative command
application under a production-like workload.
