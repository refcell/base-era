# Version-isolated historical execution

This repository is the migrated Base Era demo. PR1 is merged, the host is the selected Base source
closure, and the worker and reference build independently from committed frozen sources. Fresh
acceptance is currently running in `target/demo-publication`.

> **Evidence pending:** do not treat the source-spike results below as results from this checkout.
> Migrated acceptance evidence will be published under
> [`etc/history-devnet/evidence/final/`](../etc/history-devnet/evidence/final/) when the run finishes.

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

Exact frozen-source metadata is in
[`sources/frozen-sources.json`](../sources/frozen-sources.json). Independent host, reference, and
worker builds have succeeded from this checkout; that does not substitute for the pending full
acceptance run.

Historical execution bodies still exist in selected host crates and dependency graphs even where
the external boundary bypasses them. Host header/env/receipt-root assembly, validation,
old-parent/new-child fee compatibility, derivation upgrade transactions, and transaction-pool
admission remain host-side. Reth/revm also retain historical branches. Pending simulation,
`eth_simulateV1`, unsupported custom tracers, and trace-call `txIndex` are outside the extracted
historical RPC boundary. There is no claim of a physically history-free binary.

## Original source-spike evidence (historical only)

The pre-migration source spike reported 95 replay passes, six import passes, seven failure-scenario
passes, and native witness replay parity for blocks 19–21. It also reported exact sequencer/verifier
agreement around blocks 19–22, reorg restoration, malformed payload parity, and worker causality.
Those observations motivated this migration but **have not yet been re-established by the fresh
migrated acceptance run**.

On an AMD Ryzen AI MAX+ 395 running x86-64 Linux 7.1.8 and Rust 1.96.0, that original spike measured:

| Operation | History host | Original reference |
|---|---:|---:|
| Full-state import, blocks 1–22 | 49.592 s; 0.444 blocks/s | 1.302 s; 16.897 blocks/s |
| Historical call, first / repeated median | 2524.066 / 2537.581 ms | 6.102 / 2.903 ms |
| Historical estimate, first / repeated median | 2698.938 / 2704.166 ms | 4.381 / 2.159 ms |
| Current call, repeated median | 4.241 ms | 3.062 ms |
| Fresh-process readiness | 2323.313 ms | 1655.564 ms |
| Benchmark peak RSS | 745704 KiB | 249364 KiB |

These were debug host/reference builds and a release worker. “Cold” meant fresh processes and
copied databases, not dropped page caches. One process was spawned per operation, and repeated
serialization of the roughly 9 MiB genesis/manifest dominated observed cost. The numbers are not
migrated-release benchmarks.

## Proof and production boundaries

The source spike's native witness check was not a zkVM proof. Some original proof guest directories
are intentionally absent from this selected checkout; the omitted range guest can be inspected in
the pinned upstream Base source at
[`crates/proof/zk/programs/succinct/range/ethereum/src/main.rs`](https://github.com/base/base/blob/1eda0f7f4cebb823522e62f34fc3e513b1c450b1/crates/proof/zk/programs/succinct/range/ethereum/src/main.rs).
A schedule ID commits configuration, not an external executable's semantics. zkVM deployment still
requires guest-compatible execution, independently built guest/VK identities, boundary proofs,
aggregation changes where needed, and verifier authorization. No new zkVM proof or on-chain
authorization is claimed.

Productionization also requires protocol-wide resource budgets (the current frame limit is 64 MiB
and invocation deadline is 30 seconds), batching or persistent workers, configuration caching, a
broader historical/custom-chain corpus, fuller historical API coverage, and artifact governance.
