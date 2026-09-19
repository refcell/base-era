# Base Era

**Version-isolated historical execution for Base, using pinned worker binaries.**

Base Era is an experimental demo built on [base/base](https://github.com/base/base).
The current node executes current-era blocks locally and delegates historical execution to
independently built, pinned worker processes. The node remains responsible for canonical state,
forkchoice, validation, commits and reorgs.

This explores the versioned-execution direction described in
[EIP-4444](https://eips.ethereum.org/EIPS/eip-4444#full-syncing-from-genesis).
It is **not** an Era/Era1 archive-format implementation or an official Base project.

## Why

Every fork leaves historical rules in the active client. The goal is a repeatable retirement
path: freeze a reviewed implementation, route its history to a pinned worker, migrate remaining
consumers, then remove superseded host code. History stays executable; the current implementation
has less historical behavior to maintain.

A [fresh-clone deletion experiment](docs/retirement.md) removes **5,907 net Rust lines** from Base:
**2,743 production + 3,164 test/benchmark lines**, including comments and blanks. The actual diff,
per-file counts and checks are published. This Beryl-and-later candidate retires historical
execution **and derivation**, with a conditional native-proof reduction; realizing it requires
additional historical dispatch beyond this demo. It is not safe for historical replay and is
**not applied to the running demo**. Source-copy pruning is not counted as history retirement.

## Status

**The migrated demo passed its complete local acceptance run on September 18, 2026.** This
repository contains the selected host source, independently frozen historical/reference sources,
vendored reth, and the devnet harness—no enclosing Base checkout or moving patch overlay required.

[Explore the showcase](https://refcell.github.io/base-era/) · [Read the report](docs/history-spike.md)
· [Inspect the evidence](etc/history-devnet/evidence/optimized/) ·
[Review the integration diff](https://github.com/refcell/base-era/compare/00cca4b99fad0c68901521cedd32b5a13d283c21...main)

Historical calls now take **10.53 ms instead of 2.34 s (223× faster)**; estimation improves **254×**.
These gains combine persistent isolated workers, configuration caching and optimized compilation.
The optimized in-process reference is still faster. See [measurements and limitations](docs/history-performance.md).

**Operational screenshots:** [actual node/worker logs](site/media/execution-logs-cutover.png)
and [live Base Control Grafana capture](site/media/base-control-dashboard.png).
The [capture notes](site/media/README.md) include raw evidence, reproduction commands and scope.

## Run it

Requires x86-64 Linux with Landlock ABI 3+, Docker, Rust 1.96, Foundry `cast`, `just`, and a native build
toolchain. Allow at least 70 GiB for build outputs. See [the build guide](docs/history-spike-build.md)
for prerequisites, pins, profiles and fresh-run handling.

```sh
git clone https://github.com/refcell/base-era.git
cd base-era
just demo
```

One command builds, tests, starts, exercises and verifies a fresh network, then exports evidence.
It leaves the devnet running and prints its endpoints, evidence directory and exact stop command.
Set `BASE_ERA_RUN_DIR` to choose a fresh output directory; otherwise each invocation creates one.

The harness creates content-addressed local executables; no downloadable binary release is
published yet. Runtime images and package downloads are not fully hermetic. The selected upstream
baseline is a separate commit; omitted tooling is not removed historical execution logic.

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

## What this checkout demonstrated

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

The optimized local run recorded 95 replay/validation/RPC checks, six import checks, nine
failure/recovery scenarios, 13 host tests, 18 worker subprocess tests and nine artifact-integrity
tests passing. These are local results, not GitHub CI results. Portable evidence binds each stage
to its tested executable hashes and approved configuration; an independent reviewer checked the
results, routing, failure causality and publication safety.

## Boundaries and limitations

- This is experimental, not production-ready or a security audit.
- Historical implementation bodies remain in this host tree; the selected execution
  paths bypass them. Complete latest-only source extraction is not demonstrated.
- Native stateless parity is not a zkVM proof. Guest execution and verifier authorization need
  separate work.
- The worker supports specific historical RPC operations and tracers, not every simulation API.
- Warm historical calls take 10.53 ms versus 0.207 ms in the optimized reference. Cold initialization
  takes 164 ms; per-operation artifact verification and serialized state reads remain costs.
- The tested isolation mechanism requires Linux with Landlock ABI 3+ and seccomp.

## Upstream inputs

- Base: [`1eda0f7`](https://github.com/base/base/commit/1eda0f7f4cebb823522e62f34fc3e513b1c450b1)
- Base's reth fork: [`5877708`](https://github.com/base/reth/commit/5877708bbf9219c44758cd2ce28a365f738661f7)
  (`base-v2.5.2.6`), plus the host integration patch.

The spike's reth patch changes 13 files: 300 added and 42 removed lines. Worker execution uses
the original pinned reth revision, not the host's patched interfaces.

Upstream source retains its applicable licenses and notices. Source/dependency details are in
[the inventory](docs/source-inventory.md) and [frozen-source manifest](sources/frozen-sources.json).
