# Base Era

**Version-isolated historical execution for Base, using pinned worker binaries.**

Base Era is an experimental demo built on [base/base](https://github.com/base/base).
The current node executes current-era blocks locally and delegates historical execution to
independently built, pinned worker processes. The node remains responsible for canonical state,
forkchoice, validation, commits and reorgs.

This explores the versioned-execution direction described in
[EIP-4444](https://eips.ethereum.org/EIPS/eip-4444#full-syncing-from-genesis).
It is **not** an Era/Era1 archive-format implementation or an official Base project.

## Status

The source spike has run successfully on a disposable local devnet. **Migration is in progress:**
this repository now contains the selected host source, independently frozen historical/reference
sources, vendored reth, the devnet harness and a static showcase site. Fresh builds and acceptance
are being run from this checkout; the earlier spike's results below are not yet migration results.

The local entry point is `./demo help`. Follow [the task board](TASK_BOARD.md) for current evidence.
There is no published binary release yet. The source baseline is a separate commit so integration
changes can be reviewed without mistaking omitted tooling for removed historical logic.

## How it works

```text
Current Base node
  |
  +-- Select execution rules using chain configuration and block context
  |
  +-- Current era ------> In-process execution
  |
  +-- Historical era ---> Pinned worker executable
                          | Read-only access to a parent-bound state view
                          | Execute with its own source/dependency versions
                          + Return state changes, receipts and execution result
  |
  +-- Validate results and commitments; commit or unwind in the host
```

Workers exchange versioned serialized messages, not Rust objects. They cannot write the host's
canonical database. Requests and results are bound to the parent, chain configuration, operation
and approved executable identity. Missing artifacts, crashes and malformed responses fail closed;
they do not silently fall back to the current execution rules.

## What the local spike demonstrated

The tested boundary was **pre-Isthmus versus Isthmus and later**, with Isthmus scheduled at block
20 on the devnet. This is a real protocol activation, not an artificial switch between identical
implementations.

- An L1-backed sequencer and independently deriving verifier agreed across the cutover.
- Transfers, contract storage and receipts matched expected results.
- Fresh replay, restart and cross-cutover reorgs matched a reference implementation.
- Historical calls, gas estimation and supported debug traces executed through workers.
- Artifact failures, worker termination, malformed replies and timeouts failed safely and recovered.
- Native stateless execution matched the full headers for blocks 19–21.
- Host and worker built with separate lockfiles and different resolved dependency versions.

The final local run recorded 95 replay/validation/RPC checks, six import checks, seven
failure/recovery scenarios, 11 host tests and 15 worker subprocess tests passing.
These are results from the source spike, **not CI results for this repository**. Reproducible
commands and supporting evidence will accompany the migration.

## Boundaries and limitations

- This is experimental, not production-ready or a security audit.
- Historical implementation bodies remain in the source spike's host tree; the selected execution
  paths bypass them. Complete latest-only source extraction is not demonstrated.
- Native stateless parity is not a zkVM proof. Guest execution and verifier authorization need
  separate work.
- The worker supports specific historical RPC operations and tracers, not every simulation API.
- Historical calls in the unoptimized spike took roughly 2.54 seconds versus 2.9 milliseconds in
  the reference. Repeated genesis/configuration processing and per-request processes need work.
- The tested isolation mechanism requires Linux with Landlock ABI 3+ and seccomp.

## Upstream inputs

- Base: [`1eda0f7`](https://github.com/base/base/commit/1eda0f7f4cebb823522e62f34fc3e513b1c450b1)
- Base's reth fork: [`5877708`](https://github.com/base/reth/commit/5877708bbf9219c44758cd2ce28a365f738661f7)
  (`base-v2.5.2.6`), plus the host integration patch.

The spike's reth patch changes 13 files: 300 added and 42 removed lines. Worker execution uses
the original pinned reth revision, not the host's patched interfaces.

Any migrated upstream source and distributed artifacts must retain their applicable licenses and
notices. Release packaging and attribution will be established before binaries are published.
