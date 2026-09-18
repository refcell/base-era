# Version-isolated historical execution

This repository is the migrated Base Era demo. The host is the selected Base source closure, and
the worker and reference build independently from committed frozen sources. Fresh acceptance passed
from clean source [`d4ae34c`](https://github.com/refcell/base-era/commit/d4ae34c6345174757a27a60688a1caae569d96ec); portable results are published in
[`etc/history-devnet/evidence/final/`](../etc/history-devnet/evidence/final/).

**Update:** the protocol-2 session implementation passed a new full acceptance run from clean
[`b346294`](https://github.com/refcell/base-era/commit/b346294b662fc29a872d8d65af664d580761fa18).
Historical calls improved 223× and gas estimation 254× versus the original debug demo, combining
session reuse with optimized builds. See [the performance report](history-performance.md) and
[new evidence](../etc/history-devnet/evidence/optimized/). The original measurements below remain
as a baseline, not the current performance claim.

## Architecture and scope

The host owns its canonical database, forkchoice, commit, and unwind. With
`BASE_HISTORY_MANIFEST` enabled, complete pre-Isthmus block execution goes to an approved worker
subprocess; Isthmus and later execution remains local. Selection uses the configured chain
activation, while the worker independently derives the precise fork from its frozen chain spec.
The compressed devnet activates Isthmus at block 20: blocks 1–19 use Holocene rules and block 20
onward uses Isthmus. This exercises upgrade deposits, the GasPriceOracle flag, and
requests/withdrawals commitments rather than an arbitrary routing switch.

Reth's complete-block hook covers Engine execution, staged import/sync, backfill/ExEx, and witness
execution. Historical payload assembly remotely reexecutes transaction prefixes; ordinary Engine
validation remains host-side. RPC call, estimate, and supported trace paths route using their real
state and block context.

The [versioned protocol](history-worker.md) binds operation, configuration/genesis identities,
parent, candidate, artifact digest, request, and PID. The worker requests immutable parent-view
reads and returns account/code/storage deltas and canonical receipt bytes. Before-values and
storage-wipe semantics support host reverts. Infrastructure failure is not `INVALID` and never
falls back to local legacy execution.

Executable bytes are SHA-256 verified and run from a sealed memfd. Linux Landlock, seccomp, a
cleared environment, and close-on-exec descriptors deny direct database, write, network, and
process-memory access. The worker remains trusted consensus code; this is not protection against
kernel exploits, and unsupported sandbox kernels fail closed.

## Migrated source layout and provenance

- The host uses the selected crates in this repository.
- The worker's frozen Base source is committed at
  [`etc/history-worker/historical/base`](../etc/history-worker/historical/base), pinned to Base
  `1eda0f7f4cebb823522e62f34fc3e513b1c450b1`.
- The independent reference is committed at
  [`historical/reference`](../historical/reference) at the same Base pin.
- Integrated reth is committed at [`vendor/reth`](../vendor/reth). Upstream reth is pinned to
  `5877708bbf9219c44758cd2ce28a365f738661f7` (`base-v2.5.2.6`); the imported integration source
  records local commit `d201e8932a613e6ec3c1c89d610c47cbc1684f7b`.
- [`tools/reth-config.py`](../tools/reth-config.py) generates Cargo path overrides for the committed
  reth tree. [`sources/reth-history.patch`](../sources/reth-history.patch) is an audit artifact,
  not a setup-time patch.

Exact pinned source-set metadata is in
[`sources/frozen-sources.json`](../sources/frozen-sources.json). Independent host, reference, and
worker builds and the full acceptance run succeeded from this checkout. The worker intentionally
uses Alloy consensus/EIPs 2.4.2 and primitives 1.7.3, independently of the host's 2.4.1/1.6.1.

Historical execution bodies still exist in selected host crates and dependency graphs even where
the external boundary bypasses them. Host header/env/receipt-root assembly, validation,
old-parent/new-child fee compatibility, derivation upgrade transactions, and transaction-pool
admission remain host-side. Reth/revm also retain historical branches. Pending simulation,
`eth_simulateV1`, unsupported custom tracers, and trace-call `txIndex` are outside the extracted
historical RPC boundary. There is no claim of a physically history-free binary.

## Migrated acceptance results (2026-09-18)

The fresh suite recorded [95 replay PASS, 0 FAIL, 0 SKIP](../etc/history-devnet/evidence/final/replay.json),
[six import passes](../etc/history-devnet/evidence/final/import.json), [seven failure/recovery
claims](../etc/history-devnet/evidence/final/failures.json), and [native witness parity for blocks
19–21](../etc/history-devnet/evidence/final/stateless.json). Live evidence confirms exact
builder/verifier parity across the block-20 Isthmus cutover. Host routing used the worker through
block 19 and local execution from block 20. Host unit tests passed 11, worker subprocess tests passed
15, and the Python suite passed 9.

On an AMD Ryzen AI MAX+ 395 running x86-64 Linux 7.1.8 and Rust 1.96.0, the migrated debug
host/reference and release worker measured:

| Operation | History host | Original reference |
|---|---:|---:|
| Full-state import, blocks 1–22 | 49.409 s; 0.445 blocks/s | 1.356 s; 16.224 blocks/s |
| Historical call, first / warm median | 2342.176 / 2343.979 ms | 8.507 / 1.927 ms |
| Historical estimate, first / warm median | 2631.316 / 2599.164 ms | 5.694 / 2.102 ms |
| Current call, first / warm median | 6.099 / 2.137 ms | 5.286 / 2.159 ms |
| Fresh-process readiness | 1453.390 ms | 2306.753 ms |
| Node CPU user/system; peak RSS | 30.374/0.709 s; 333572 KiB | 1.914/0.366 s; 247688 KiB |

CPU and peak RSS use Linux `wait4` and include waited-for descendants; peak RSS is not the sum
of concurrently resident processes. Medians use five samples after the first request.
“Cold” means fresh processes and copied databases, not dropped page caches. Each measured historical
operation performed 12 host-served state reads; request frames were 2550 or 2562 bytes. Those byte
counts are requests only, not bidirectional traffic. The release-worker startup/framed-parse probe
was 0.708 ms and 55588 KiB peak RSS. Full samples and measurement definitions are in
[`benchmark.json`](../etc/history-devnet/evidence/final/benchmark.json).

## Independent review and publication

Review covered the migrated source closures, consensus boundary, sandbox, artifact approval and
acceptance harness. Verified findings led to byte-sorted frozen-source hashing, reference artifact
verification, rebuilding setup from committed context rather than trusting a tag, and per-stage
pre-launch artifact records. The complete acceptance run was repeated after these changes.

A final independent audit joined canonical hashes, parents, worker PIDs/digests, exact receipt
pairs, malformed verdicts, reorg/restart results and native fixture hashes. It found no blocker to
publishing this scoped demo. The live `base-devnet` executable and standalone `base-reth-node`
have distinct hashes by design; both are recorded. This review is not a security audit or a claim
of broad production coverage. Only the credential-free portable evidence was published.

The [Pages showcase](https://refcell.github.io/base-era/) was deployed and checked over public
HTTPS, including its compact technical layout. The optimized accepted local network remains
in `target/demo-performance`; see [operation commands](history-devnet.md). Builds produce local
approved artifacts; no downloadable binary release is published. Runtime image tags and package
downloads remain non-hermetic inputs, documented in the build guide.

## Proof and production boundaries

The native witness check is not a zkVM proof. Some original proof guest directories
are intentionally absent from this selected checkout; the omitted range guest can be inspected in
the pinned upstream Base source at
[`crates/proof/zk/programs/succinct/range/ethereum/src/main.rs`](https://github.com/base/base/blob/1eda0f7f4cebb823522e62f34fc3e513b1c450b1/crates/proof/zk/programs/succinct/range/ethereum/src/main.rs).
A schedule ID commits configuration, not an external executable's semantics. zkVM deployment still
requires guest-compatible execution, independently built guest/VK identities, boundary proofs,
aggregation changes where needed, and verifier authorization. No new zkVM proof or on-chain
authorization is claimed.

Productionization also requires protocol-wide resource budgets (the current frame limit is 64 MiB
and invocation deadline is 30 seconds), batching and bounded concurrent worker pools, a
broader historical/custom-chain corpus, fuller historical API coverage, and artifact governance.
