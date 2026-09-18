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

## Verification board

| Responsibility | Status |
|---|---|
| Profiling | Repeated genesis/spec construction and large request serialization identified |
| Host integration | Bounded retained session; configuration cache; per-operation artifact verification |
| Worker/protocol | Immutable initialized configuration; fresh state and read sequence per operation |
| Independent review | Fixed artifact check before queueing; strengthened state-isolation test; bound malformed block verdict |
| Unit/subprocess tests | 13 host, 18 worker and 9 integrity tests pass |
| Optimized devnet acceptance and measurements | Pending fresh integrated run |

The unchanged reth integration remains 13 files (+300/−42); this performance change does not alter
reth or the frozen historical implementation sources. The new tests cover session initialization,
changed storage across warm operations, configuration drift, request binding, and cache lifetime.
The live failure harness additionally requires the same PID on warm calls, changed override values,
artifact removal/replacement with an unchanged manifest, and crash/recovery with a new PID.

## Measurement method and remaining costs

Run `./demo build`, `./demo test`, then `./demo start && ./demo exercise`, `./demo verify`,
and `./demo evidence` with a fresh `BASE_ERA_RUN_DIR`. Benchmarking uses fresh host/reference
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
timings include node initialization and are not steady-state large-chain replay throughput.
