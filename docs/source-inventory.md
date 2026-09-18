# Base Era source-copy inventory

## Historical status

This document records the **pre-migration source inventory** used to select Base Era's source
closure. Its counts describe the old checkout and were not rerun here. The migration
uses those results: host crates are at their normal paths, frozen worker source is committed under
`etc/history-worker/historical/base`, the independent reference under `historical/reference`, and
integrated reth under `vendor/reth`. Independent builds and fresh full acceptance succeeded from
clean source `d4ae34c6345174757a27a60688a1caae569d96ec`; see
[`evidence/final/provenance.json`](../etc/history-devnet/evidence/final/provenance.json).

## Recommendation (as measured before migration)

Copy a **bounded source fork**, not a rolling patch overlay and not a handful of
EVM files. Initially preserve Base's relative paths and its existing devnet
launcher. The measured host/demo closure is **98 Base packages**. Copy those
packages, the worker/protocol, and the harness. Keep historical and reference
sources independently frozen. Integrate the reth changes into owned source, not
an install-time patch against a moving upstream checkout.

Upstream updates then become deliberate source merges plus parity testing;
updating `base/base` cannot silently change this demo. Copying does not eliminate
future integration work, but it removes patch application from the demo's setup.

This inventory itself was not a migration or clean-copy build result. The migration was completed
later; its acceptance status must not be inferred from these measurements.

## What was measured

Source: Base [1eda0f7](https://github.com/base/base/commit/1eda0f7f4cebb823522e62f34fc3e513b1c450b1)
plus the local historical-execution spike. Host reth starts at
[5877708](https://github.com/base/reth/commit/5877708bbf9219c44758cd2ce28a365f738661f7).

I traversed Cargo's selected compilation-unit graphs for the actual build
commands, not every dependency appearing in Cargo metadata. A second pass
followed declared local dependencies, including optional, target-specific,
build and dev dependencies, because Cargo still needs their manifests.

| Component | Base packages compiled | Base package directories to copy without editing dependency declarations |
|---|---:|---:|
| Host node alone, in the combined build's feature context | 52 | Included in the combined set below |
| Existing devnet launcher | 97 | Included in the combined set below |
| Host node + launcher + native fixture + history unit tests | **98** | **98** |
| Native/stateless fixture example | 15 | Already included in those 98 |
| Frozen historical implementation used by worker and worker tests | **18** | **29** |
| Independent reference node | **50** | **51** |

Counts overlap; do not add rows together. The protocol and worker packages are
additional to these Base-source counts. The host graph also compiles 103 local
reth packages, counted separately, not as Base packages. Native fixture and
worker tests do not expand their respective compiled Base sets.

The current Base workspace has 153 members; 55 are outside the measured host
closure. There are also manifests outside that workspace. Neither number is a
claim that every repository package was compiled or tested.

The 98 host package directories contain **1,772 tracked or non-ignored source
files, 53,724,852 bytes (51.2 MiB)**, including fixtures and documentation. They
contain 427,144 physical Rust lines, including tests and blank lines. The entire
original tracked tree is approximately 59.2 MiB. Thus this conservative extraction
is still most of Base's source bytes, not a tiny demo. Binary dependencies and
build output are excluded. None of these reductions measure retired fork logic.

Exact package paths, the host file allowlist, omitted workspace members, graph
hashes and lockfile hashes are in [source-inventory.json](source-inventory.json).

## Host copy list

Preserve **complete selected package directories**, including `Cargo.toml`,
`README.md`, `build.rs`, migrations, bytecode and fixtures. Do not copy only `src/`.
Use `sources.host.copy_packages` in the JSON, not whole-family globs:

| Family | Packages | Why retained |
|---|---:|---|
| `bin/node` | 1 | Engine/RPC node, replay and import |
| `crates/common` | 17 | Chain configuration, primitives, EVM, fees, precompiles, wire types |
| `crates/execution` | 24 | Node, history bridge, consensus, RPC, payloads, pool, trie and extensions |
| `crates/consensus` | 12 | Derivation, networking, engine control, sources, upgrade transactions |
| `crates/builder` | 4 | Sequencer construction, metering, multiplexing and publication |
| `crates/batcher` | 7 | Actual L1 batch submission and encoding |
| `crates/utilities` | 13 | Runtime, CLI, metrics, signing support, transaction manager and test utilities |
| `crates/proof` | 18 | Current launcher's proof dependencies plus the native parity path |
| `crates/infra/load-tests` | 1 | Unconditional dependency of existing systems library |
| `etc/systems` | 1 | Existing L1-backed sequencer/verifier launcher |

Root files: retain the license, guidance, toolchain, lockfile and relevant Cargo
workspace metadata/lints/profiles/dependencies. Replace workspace member and
default-member globs with the explicit retained set. Keep formatting/lint
configuration for development. `.config` currently contains nextest/zepter
configuration, not a hidden Cargo source override; it is not a runtime input.

Examples that can be omitted from the host: other CLI binaries, audit/telemetry/
snapshotter tooling, challenger/proposer services, Nitro/TEE packages,
`crates/common/evm2`, and unrelated CI/actions. See the full 55-member omit list.
Omitting those tools must not be presented as deleting historical semantics.

### A worthwhile second extraction

`etc/systems/src/lib.rs` unconditionally exposes benchmark, prover-service and
zk-host modules. Its manifest pulls in load tests, prover-service, SP1-related
host/backend code and PostgreSQL orchestration even for the history subcommand.

A dedicated history-only launcher should keep `HistoryArgs`, the stack builder,
L1 containers/setup, L2 builder/client/consensus/batcher, networking, configuration
and endpoint reporting. It can then remove benchmark/snapshot/prover/zk-host CLI
paths and their now-unused dependencies. This is a real module/manifest
extraction, not a directory deletion. Recompute the graph and rerun the live
acceptance suite afterward. No smaller package count has yet been demonstrated.

Do not remove all proof code: `crates/proof/executor` and `crates/proof/mpt` are
needed for native/stateless parity. The current launcher's ELF package has an
existing stub fallback for absent SP1 ELFs; that is not proof functionality and
must not be advertised as such.

## Historical source, protocol and reference

Copy `etc/history-worker/{Cargo.toml,Cargo.lock,rust-toolchain.toml,README.md}`
and both `crates/protocol` and `crates/worker`. The worker keeps its own workspace,
lockfile and original reth dependency graph. Host and worker may share the
protocol's source definition; they do not share Rust instances or execution code.

The migration implemented this recommendation at `etc/history-worker/historical/base`, copied from
the original Base revision rather than the modified host tree. The 18 compiled directories are
listed in the JSON. Preserving their
dependency declarations requires 11 more Base package manifests/sources:

```text
crates/common/bundles
crates/common/observability-events
crates/execution/exex
crates/execution/node
crates/execution/payload
crates/execution/rpc
crates/execution/trie
crates/execution/txpool
crates/utilities/retry
crates/utilities/test-utils
crates/utilities/upgrade-signal
```

Hence **29 was the conservative copy set**, not 18. The committed frozen workspace preserves the
required manifests and licensing without generated-source setup. These 29 directories measured
about 35.2 MiB in the source spike.

The reference is another independent source root: 50 compiled Base packages,
plus `crates/utilities/test-utils` for manifest resolution. It uses the original
Base lockfile and unmodified execution. Its only intentional adaptation is
exposing reth's standard import command in `crates/execution/cli/src/app.rs` and
`src/commands/mod.rs`. The migration commits that adaptation in `historical/reference`; the
retained `reference-cli.patch` is provenance, not a setup step. Those 51 directories total
about 43.3 MiB. Never build the reference from the implementation under test.

For a compact default checkout, frozen historical/reference sources can instead
be immutable, digest-verified source release archives paired with the compiled
artifacts. The sources and build inputs must remain available; publishing a
binary is not a substitute for provenance. Their bytes then live outside the
default tree, not disappear. No such release has been created yet.

## Reth and setup-image sources

Use an owned, pinned reth source tree or fork containing the integration. Do not
copy only the 13 modified source files as though they were a standalone crate.
The source delta is 300 insertions and 42 deletions; the local `.cargo-ok` marker
is not part of that source change. A full tracked reth checkout is about 41 MiB.
Its workspace/build dependencies are a separate inventory from the Base list.

The migration implemented this with committed `vendor/reth`. `tools/reth-config.py` emits Cargo
path overrides to that tree. `sources/reth-history.patch` is audit-only and is not applied during
setup. Keep the worker/reference on original reth. Do not edit Cargo caches.

The launcher binary `base-devnet` and reference node binary `base-reth-node` have different roles
and are built from different source graphs by design; their hashes are not expected to match.

There is a **second non-Base source modification**:
`etc/history-devnet/optimism-isthmus.patch` changes Base Optimism's offline genesis
generator at [0066b17](https://github.com/base/optimism/commit/0066b17c3fe0cbb5ea935de6d5b18d4fc86dc439).
For fully patch-free setup, integrate this change into pinned setup-tool source
or publish a digest-pinned setup image with that source provenance. Merely
removing the Base overlay would leave this Docker-time patch in place.

## Harness, non-Rust inputs and exclusions

Keep `etc/history-devnet` scripts for build, setup-image build, start, exercise,
verify, replay, import, failure injection, stateless comparison, benchmarking,
acceptance and stop. Keep `benchmark.py`: the acceptance script invokes it.
Replace reference/materialization/override assumptions described above.

Keep `etc/docker/Dockerfile.devnet`, its context rules, and the required
`etc/scripts/devnet` assets. The Dockerfile uses a directory COPY, but the current
`.dockerignore` already excludes Grafana, load and several monitoring/smoke files;
those are not all required. Copying the tracked directory conservatively is
small and simpler than claiming every file is a runtime dependency.

Keep package-local chain genesis JSON, EVM bytecode, genesis banners, builder
test templates and selected SQL migrations. `base-test-utils` is compiled
**without its `contracts` feature** in the measured build, so its ignored Foundry
`contracts/out` output is not a prerequisite. `evm2` bytecode, audit migrations
and TEE registrar fixtures are outside the selected set.

Retain a reviewed evidence bundle and `docs/history-*.md` for the showcase;
acceptance does not depend on the old evidence to produce new results.

Do not copy `.devnet`, target directories, databases, runtime JWT/key material,
local Cargo caches, artifact symlinks, process files or unsanitized logs. The migration instead
commits selected original sources in explicitly named immutable roots.

External setup inputs include pinned eth-beacon-genesis and eth2-val-tools
sources, Go/Alpine base images, L1 reth and Lighthouse images. Runtime image tags
still need conversion to digest pins for stronger release reproducibility.
Host tools include Rust 1.96.0, C/C++/Clang build tooling, Docker/BuildKit, Git,
Python, Bash, jq, Foundry cast and common archive/hash utilities. Failure fixtures
use gcc. Worker isolation requires Linux, Landlock ABI 3+ and seccomp.

## Migration and comparison sequence

1. Import the selected **original** Base package directories at their original
   paths as a source baseline, with upstream revision and file provenance.
2. Integrate the spike in a separate commit, including new history packages,
   protocol/worker, owned dependency sources and harness. Keep historical and
   reference workspaces separate from the host workspace.
3. Prune manifests and fix every old-root/build-output assumption. Verify
   `cargo metadata`, locked independent builds, unit/subprocess tests and the
   full live acceptance suite from a fresh checkout.
4. Slim the launcher as a separate refactor and rerun acceptance.
5. Make and measure actual legacy-body deletions separately. Present the
   baseline-to-integration diff independently of unrelated directory omissions.

The current spike bypasses selected historical host paths but still retains
legacy bodies in host source. This inventory does not turn that into a deletion
demonstration. A vendored reth tree plus multiple frozen Base trees can make this
repository larger overall even when the active implementation gets smaller.

## Inventory reproducibility note

The old inventory used Cargo nightly 1.100.0 unit graphs in the source-spike checkout. Its obsolete
generated paths and setup commands are intentionally omitted here. `source-inventory.json` preserves
the selected package paths, allowlist, omitted members, graph hashes, and lockfile hashes. Graph
digests may include old absolute paths and are historical local evidence identifiers, not portable
artifact identities or proof that migrated acceptance passed.
