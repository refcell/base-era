# Historical execution performance

## Changes

The original demo spent 2,343.979 ms per repeated historical `eth_call`, versus 1.927 ms in the
in-process reference. Those were **debug host/reference builds**, with a release worker. The raw
baseline remains in [`evidence/final`](../etc/history-devnet/evidence/final/).

Protocol 2 reuses a sandboxed worker and its validated immutable chain specification. The first
request initializes the process with full genesis; subsequent requests bind configuration identities
and send `genesis: null`. Execution state, overrides and parent reads are rebuilt for every request.
The host caches parsed manifest and derived chain configuration, but rereads manifest bytes and
cryptographically verifies the current artifact bytes for every operation. Configuration changes
select a new generation. A failed request is never retried automatically or executed locally.

Host and reference now use the same optimized `profiling` build profile (opt-level 3, no LTO).
The historical worker still has its independent release build, lockfile, and divergent dependencies.
**Any speedup against the original demo includes both optimized compilation and session reuse.**
It must not be presented as the isolated effect of process reuse.

## Results (September 18, 2026)

Fresh acceptance ran from clean committed source
[`b346294`](https://github.com/refcell/base-era/commit/b346294b662fc29a872d8d65af664d580761fa18).
The [optimized evidence](../etc/history-devnet/evidence/optimized/) preserves artifact hashes,
configuration identity, sample distributions, routing and failure verdicts. The independent review
checked these against the raw run, including exact reference outputs and reused worker PIDs.

| Measurement | Original demo | Optimized session | Improvement |
|---|---:|---:|---:|
| Historical call, warm median | 2343.979 ms | 10.527 ms | 223× |
| Historical gas estimate, warm median | 2599.164 ms | 10.222 ms | 254× |
| Import 22 blocks | 49.409 s | 0.652 s | 75.8× |

The optimized in-process reference measured **0.207 ms** for historical calls and **0.248 ms** for
estimation. Isolation still costs roughly **51× / 41×** on these tiny workloads. This is not parity
with in-process latency and is not an EVM computation speedup.

The first historical call in a fresh host/worker took **164.419 ms**; warm call p95 was **12.668 ms**.
The first estimate ran after the call workload and is therefore **not** a cold session. One worker
PID served all **42** historical benchmark operations. Initialization sent **9,372,216 bytes**;
the next call sent **2,000 bytes**, with 12 state reads and 2,598 bytes of read-request frames.
These are directional frame counts, not total bidirectional traffic. Warm worker exchange took
about **0.6 ms**; most remaining wall time is host-side artifact/configuration handling.

Host CPU was **0.625 s**, with **226,592 KiB** peak RSS for the full benchmark. The persistent
worker separately reported **0.090 s** CPU, **44,024 KiB** current RSS and **51,136 KiB** peak RSS.
Reference host CPU was **0.442 s**, peak RSS **160,088 KiB**. The standalone startup/unsupported
request probe took **6.614 ms**, now including the worker's executable self-hash. Node readiness
was **107.605 ms** for the history host and **509.259 ms** for the reference; these single startup
observations are not stable throughput estimates.

## Verification board

| Responsibility | Status |
|---|---|
| Profiling | Repeated genesis/spec construction and large request serialization identified |
| Host integration | Bounded retained session; configuration cache; per-operation artifact verification |
| Worker/protocol | Immutable initialized configuration; fresh state and read sequence per operation |
| Independent review | Fixed artifact check before queueing; strengthened state-isolation test; bound malformed block verdict |
| Unit/subprocess tests | 13 host, 18 worker and 9 integrity tests pass |
| Optimized devnet acceptance and measurements | 95 replay/validation/RPC, 6 import, 9 failures pass; native blocks 19–21 match |

The unchanged reth integration remains 13 files (+300/−42); this performance change does not alter
reth or the frozen historical implementation sources. The new tests cover session initialization,
changed storage across warm operations, configuration drift, request binding, and cache lifetime.
The live failure harness additionally requires the same PID on warm calls, changed override values,
artifact removal/replacement with an unchanged manifest, and crash/recovery with a new PID.

## Measurement method and remaining costs

Environment: AMD Ryzen AI MAX+ 395, x86-64 Linux 7.1.8, Rust 1.96.0.

Run `just demo` (setup, build, tests, live network and evidence in a fresh run directory).
Benchmarking uses fresh host/reference
processes and owned database copies, records the first request separately, then takes 20 repeated
samples. The OS page cache is **not** dropped. Both results and real fork-dependent return values
must match the independent reference. Logs record request/PID identity, state-read traffic and
actual serialized request size. The harness fails if it cannot demonstrate process reuse.

Cold requests still transfer and validate the full genesis. Warm operations still read the manifest
and hash the executable; replacing these checks with pathname metadata would weaken the current
artifact failure contract. State-read round trips remain synchronous. One serialized session limits
concurrency; this is not yet a production-sized worker pool.

CPU and peak RSS from `wait4` cover the host and only descendants it reaped. A separate `/proc`
snapshot records persistent worker CPU, current RSS and peak RSS before shutdown. Do not sum RSS
peaks or claim host-only `wait4` figures include all persistent-worker cost. Small 22-block import
timings include import-process startup (database initialization is separately timed) and are not
steady-state large-chain replay throughput.

The accepted network is left running in `target/demo-performance` (launcher PID 514236): builder
`http://127.0.0.1:46593/`, verifier `http://127.0.0.1:34901/`, L1 `http://localhost:35240/`.
These are local-only, ephemeral endpoints. Inspect/stop with
`BASE_ERA_RUN_DIR="$PWD/target/demo-performance" ./demo status` or `./demo stop` using the same
environment variable. Restart using a fresh directory and the build/start/exercise commands above.

## Recorded call waterfall

The site renders [a real captured call](../site/data/historical-call.json), not a Grafana mockup.
It was captured from the successful `just demo` network on September 18, 2026, after warming
the historical session. `GasPriceOracle.isIsthmus()` at block 19 returns false; the sequencer and
independently following verifier agree on that block's hash and state root.

The single sample measured **10.879 ms HTTP round trip**, containing a **0.677 ms worker
exchange** beginning 9.397 ms after the client timer started. The correlated worker events record
PID 607443, request binding, artifact/configuration identities, 12 state-read requests, 2,598 bytes
of read-request frames, and a 2,000-byte operation request. Byte counts are directional, not total
traffic. The exchange includes IPC, state reads and execution; it is not EVM-only time. Remaining
round-trip time is deliberately unattributed. This sample is separate from the 20-sample benchmark.

Recreate the capture after `just demo`, using the run directory printed by that command:

```sh
RUN_DIR=target/demo-one-command-... # replace with the printed run directory
python3 tools/capture-history-call.py "$RUN_DIR" call.json
```

The capture script uses a monotonic HTTP timer and same-machine wall-clock log timestamps for the
worker offset, checks interval containment, correlates request identity, and publishes only selected
fields. It does not export the devnet's runtime credentials. A concurrent historical request causes
capture to fail rather than silently attribute another operation's measurements.
