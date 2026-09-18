# Version-isolated historical execution — local spike report

The final 2026-09-18 run completed successfully on `/tmp/base-history-final-3`. This is a working
local-node spike, not a production client or complete repository-wide history retirement.
The reproducible entry point is [the build guide](history-spike-build.md); the portable final
evidence is [here](../etc/history-devnet/evidence/final/). Earlier evidence outside `final/` is
development history, not evidence for the final artifacts.

## Architecture and scope

The host owns its canonical database, forkchoice, commit and unwind. With `BASE_HISTORY_MANIFEST`
enabled, complete pre-Isthmus block execution goes to an approved worker subprocess; Isthmus and
later execution remains local. The selector uses the chain's configured activation, not the wall
clock. The worker independently derives its precise fork from the frozen chain specification.
The compressed devnet activates Isthmus at block 20's timestamp: blocks 1–19 use Holocene rules,
20 onward use Isthmus. This is a real fork: activation injects upgrade deposits, changes
GasPriceOracle's Isthmus flag, and introduces the requests/withdrawals commitment rules. Block 20
has a pre-Isthmus parent; changing only an arbitrary routing flag would not pass these checks.

Reth's complete-block hook, not a transaction-only EVM hook, covers Engine execution, the staged
import/sync executor, backfill/ExEx, and witness execution. Historical prewarming and the
locally-built-payload shortcut are disabled. Historical payload assembly remotely reexecutes
transaction prefixes and returns normal execution output; Engine validation still runs. RPC call,
estimate, and supported debug-trace paths route separately using their actual state/block context.

The [versioned protocol](history-worker.md) sends canonical input RLP and binds the operation,
configuration/genesis identities, exact parent, candidate, artifact digest, request and PID. The
worker asks for immutable parent-view reads, performs execution itself, and returns explicit
account/code/storage deltas and canonical receipt bytes. Before-values and storage-wipe semantics
allow the host to reconstruct reverts. Nothing shares writable state or Rust trait objects.
The host verifies result bindings and adapts the output before ordinary consensus/root validation
and persistence. Infrastructure failure is not INVALID and never falls back to local legacy code.

Executable bytes are SHA-256 verified and sealed in a memfd. Linux Landlock, seccomp, cleared
environment and close-on-exec inherited descriptors prevent direct host database access, writes,
network access and process-memory access. The worker remains a trusted client component; this is
not a defense against kernel exploits. Unsupported sandbox kernels fail closed.

## Acceptance board and evidence

| Criterion / ownership | Final evidence and outcome |
|---|---|
| A — worker/build | Separate workspace, lockfile and pinned source; independent host/reference/worker builds. Worker Alloy consensus/EIPs 2.4.2 and primitives 1.7.3 versus host 2.4.1/1.6.1. Worker uses original reth; host uses patched incompatible hooks. Pins/digests are in `final/provenance.json`. The reth patch applied to a fresh original checkout with locked metadata; the complete setup image built from scratch. |
| B — devnet | `final/live.json`: sequencer and independently deriving verifier agree across 19/20/21/22, including balances, contract storage `0x1234`, receipts and Isthmus flag false→true. `final/routing.json` joins block/parent/config identities to real worker PIDs and digests for 1–19, with no block-worker invocations for 20–22. |
| C — host replay | `final/replay.json`: **95 PASS, 0 FAIL, 0 SKIP**. Fresh empty nodes replay 1–22 through authenticated Engine APIs and match original reference commitments; graceful restart preserves historical RPC. Raw import independently exercises the staged executor. |
| D — state/reorg | Replay selects an alternate 19–22 branch across the boundary and restores canonical state. Unit/subprocess tests exercise creation, deletion, code/storage changes, wipe/recreation, block-by-block reverts and distinct parent views. Sandbox test denies canonical-file writes and inherited-FD access. |
| E — validation | `final/import.json`: **6 PASS** for canonical imports and malformed pre/post-cutover roots on host/reference; failed imports leave head at genesis. Replay compares six malformed Engine payload verdicts, not just roots. Worker fixtures cover Regolith deposit nonce, Canyon receipt version and CREATE2 deployment. |
| F — RPC | Replay compares exact reference JSON for pre/post-cutover calls, estimates, default/callTracer traces, transaction/block traces, and code/state overrides. Unsupported historical tracers and trace-call `txIndex` fail explicitly. |
| G — failures | `final/failures.json`: **7 scenarios pass**: real execution, missing/wrong artifact and recovery, in-flight SIGKILL and recovery, malformed/schema-incompatible/stale replies, and a partial-frame timeout with child reaping. Replay additionally requires Engine artifact failure to be an internal error with no commit. Real-worker tests reject incompatible protocol/configuration identities. |
| H — native proof | `final/stateless.json`: native witness capture/offline replay for **19, 20, 21** all exit 0 and match the full RPC header hash. The three self-contained archives are in `final/corpus/`. zkVM work is explicitly separate below. |
| I — isolation | Real workers perform historical execution; wrong/missing artifact tests establish causality. Source/dependency and residual-history inventory below distinguishes extracted execution from compatibility metadata and unextracted components. |
| J — review/measurements | Independent agents reviewed host/reth integration, protocol/state adaptation, sandbox and acceptance evidence. Verified findings were fixed and the complete final acceptance rerun passed. CPU/RSS/startup/traffic/RPC measurements are in `final/benchmark.json`; import throughput is in `final/import.json`. |

Focused checks also passed: **11 host history tests**, **15 worker subprocess tests**, scoped
Clippy, ordinary no-history-feature compilation, script syntax and patch whitespace checks.
This is not a claim that every test in the full Base monorepo ran.

Independent review led to fixes for deposit receipt metadata, empty-receipt terminal gas,
request commitments, artifact pathname races, partial-frame deadlines, inherited process authority,
configuration-error classification, and post-result transaction/output checks. A suspected payload
prefix double-application was investigated and rejected: its read cache stores parent reads;
execution deltas are not committed into it. The retained protocol limit is 64 MiB per frame and
a 30-second absolute invocation deadline; an aggregate read/byte budget is still production work.

## Measurements

Environment: AMD Ryzen AI MAX+ 395, x86-64 Linux 7.1.8-arch1-3, Rust 1.96.0, Python 3.14.7.
Host/reference are comparable debug builds; the independently built worker is release. Five
repeated samples follow the observed first request. “Cold” means fresh processes and owned database
copies, **not** dropped OS page caches. The live devnet remained active during measurements.

| Operation | History host | Original reference |
|---|---:|---:|
| Full-state import, blocks 1–22 | 49.592 s; 0.444 blocks/s | 1.302 s; 16.897 blocks/s |
| Historical call, first / repeated median | 2524.066 / 2537.581 ms | 6.102 / 2.903 ms |
| Historical estimate, first / repeated median | 2698.938 / 2704.166 ms | 4.381 / 2.159 ms |
| Current call, repeated median | 4.241 ms | 3.062 ms |
| Readiness of fresh node process | 2323.313 ms | 1655.564 ms |
| Benchmark CPU, user + system | 32.564 + 1.085 s | 1.604 + 0.221 s |
| Benchmark peak RSS | 745704 KiB | 249364 KiB |

CPU/RSS use Linux `wait4` after graceful shutdown and include waited-for descendants; peak RSS is
not a sum of simultaneously resident processes. A bare release-worker startup plus unsupported
protocol parse took 0.890 ms; that probe excludes host verification, sandbox setup and real
execution. Real historical requests each made 12 state reads, with 2562–2574 bytes of serialized
read **requests**, not total bidirectional traffic, and 206–221 ms inside worker transport.
The dominant observed end-to-end cost is repeated processing/serialization of the roughly 9 MB
genesis and manifest plus one process per operation. This spike is intentionally not optimized;
metadata-only block retrieval is not presented as an execution benchmark.

## Source isolation, policy and remaining history

- **Historical source:** `etc/history-worker/scripts/materialize-base.sh` independently archives
  pinned Base into `etc/history-worker/generated/base`. Its `crates/common/{evm,consensus}` and
  `crates/execution/{evm,chainspec,...}` supply historical implementation bodies, with original
  pinned reth/revm; the worker adapter lives in `etc/history-worker/crates/worker`. Generated
  source is reproducible, ignored data, not an unexplained cache modification.
- **Host:** `crates/execution/history` owns protocol transport, sandbox and delta adaptation;
  `crates/execution/evm/src/history.rs` owns selection/binding. Historical bodies remain in the
  host source tree/dependency graph but are bypassed for the extracted execution/RPC boundary.
  Host header/env/receipt-root assembly and validation, and old-parent/new-child fee compatibility,
  remain. This does not claim a physically history-free binary.
- **Reth:** the complete fork delta is `etc/history-reth/0001-external-complete-block-hook.patch`.
  Its executor, Engine, stage, ExEx, witness and RPC hooks preserve normal validation and storage
  ownership. Reth/revm dependencies still contain historical branches.
- **Derivation/pool/RPC:** derivation still injects protocol upgrade transactions; transaction-pool
  admission remains host-side. Pending simulation and `eth_simulateV1` are outside the extracted
  historical RPC boundary. Supported debug traces have exact-reference compatibility; this is
  not a promise to support every custom JavaScript/native tracer.
- **Policy:** absent Isthmus means all selected history remains remote; genesis-active Isthmus
  means no pre-Isthmus route. Worker spec selection retains block-based rules. Requests freeze
  their manifest/configuration; chain schedule drift fails closed, rather than reinterpreting an
  in-flight request. There is no timestamp-only result cache. A consensus-preserving security fix
  is a new reviewed immutable worker digest and approval, not mutation of an approved artifact.

## Proof boundary

Native proof behavior is unchanged and was verified on the boundary corpus. The actual zkVM
range guest is `crates/proof/zk/programs/succinct/range/ethereum/src/main.rs`; it invokes
`run_range_program` and the witness executor. Aggregation verifies against `multi_block_vkey`
and commits the image hash alongside schedule/config identifiers. Backend contract bindings
authorize aggregation/range verification keys and the rollup configuration hash.

A schedule ID commits configuration, **not** an external worker's executable semantics. Deploying
this architecture in proofs still requires a guest-compatible execution design, independently
built guest program/VK identities, boundary proofs under those programs, aggregation changes as
needed, and verifier authorization of those identities. No new zkVM proof, guest deployment or
on-chain authorization is claimed. A native process worker is not a zk proof.

## Reproduce and operate

Follow [history-spike-build.md](history-spike-build.md) for clean setup/build commands, then:

```sh
RUN="$PWD/target/history-devnet-run"
etc/history-devnet/start.sh "$RUN"
etc/history-devnet/exercise.sh "$RUN"
etc/history-devnet/acceptance.sh "$RUN"
python3 etc/history-devnet/collect.py "$RUN" --output "$RUN/portable-evidence"
etc/history-devnet/stop.sh "$RUN"
```

The successful network was left running: L1 `http://localhost:35221/`, sequencer
`http://127.0.0.1:34919/`, verifier `http://127.0.0.1:39747/`, owned stack PID **3774267**.
Stop it with `etc/history-devnet/stop.sh /tmp/base-history-final-3`. Ports/PIDs are runtime values;
a fresh start records its own endpoints. Never publish `runtime.json` or JWT files.
Subordinate replay/import/fault/benchmark/proof nodes have been stopped; their data and logs remain.

All Base changes are local and uncommitted on `main`, based on
`1eda0f7f4cebb823522e62f34fc3e513b1c450b1`. The owned reth checkout is on local `master` at
`d201e8932a613e6ec3c1c89d610c47cbc1684f7b` plus working changes; the recorded patch is relative to
upstream `5877708bbf9219c44758cd2ce28a365f738661f7` and therefore includes both. No push,
publication or shared-network deployment was performed. Productionization requires protocol
resource budgets, batching/persistent workers and config caching, a broader historical/custom-chain
corpus, complete historical API coverage, artifact governance and the proof work above.
