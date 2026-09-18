# Building and running Base Era

## Recorded inputs

- The committed worker snapshot derives from Base commit `1eda0f7f4cebb823522e62f34fc3e513b1c450b1`.
- Fork remote is `https://github.com/base/reth`, original commit
  `5877708bbf9219c44758cd2ce28a365f738661f7` (`base-v2.5.2.6`).
- `vendor/reth` contains the integrated host hooks. `sources/reth-history.patch` records the full
  delta against the original pin for review; setup does not apply it or fetch a moving Base tree.
- `sources/frozen-sources.json` pins the frozen worker, reference and reth source trees. Hashing is
  byte-sorted with `LC_ALL=C`, independent of the user's locale.
- Worker and host use `etc/history-worker/Cargo.lock` and root `Cargo.lock`, respectively.

The actual resolved locks intentionally diverge: worker Alloy consensus/EIPs 2.4.2 and primitives
1.7.3 versus host consensus/EIPs 2.4.1 and primitives 1.6.1. Worker reth is the original upstream
revision; host reth has the external execution/RPC interface patch. No Rust types cross the framed
JSON boundary. Both workspaces pin Rust 1.96.0. The worker is built in release mode; the measured
host/reference are debug builds with default features disabled.

## Commands

```sh
git clone https://github.com/refcell/base-era.git
cd base-era
./demo setup
./demo build
./demo test
# Exercise immediately after startup so the pre-cutover transaction lands in time.
./demo start && ./demo exercise
./demo verify
./demo evidence
./demo stop
```

All paths are checkout-relative. Setup verifies frozen source and builds the local setup image.
Build uses the worker's independent lockfile and committed historical subset, then the host's
lockfile with Cargo path overrides into committed `vendor/reth`. These overrides are dependency
locations, not source patches. No enclosing Base checkout or upstream Git history is needed.
Executables are copied to `target/history-artifacts/sha256/<digest>/`; `APPROVAL.json` names the
worker and `DIGEST` records its identity. Generated approvals, databases and build outputs stay
ignored. Source snapshots and lockfiles are committed.

The default run is `target/demo-run`. To repeat, use a fresh directory, for example
`export BASE_ERA_RUN_DIR="$PWD/target/demo-run-2"`. Startup refuses existing directories; stop
verifies process ownership and does not delete chain data. `./demo status` prints the local RPC
endpoints. These are disposable local test accounts, not production keys or funds.

`start.sh` uses that approval automatically when present and does not rebuild the history host. A
caller may set `BASE_HISTORY_APPROVAL` explicitly to use another reviewed approval.

Run from the repository root. Required tools are Rust 1.96, a Rust-capable native toolchain
(C/C++, clang/libclang, pkg-config), Python 3.11+, Git, jq, ripgrep, Foundry `cast`, and Docker/BuildKit.
The supported host is Linux with Landlock ABI 3+ and seccomp; unsupported sandbox setup fails closed.
Internet access is needed to fetch pinned dependencies/images on a cold machine. The spike always
requires the explicit reth override, including ordinary builds without the history feature. No
dependency source cache is patched. A cold build downloads dependencies and needs substantial disk
space: allow at least 70 GiB for the separate debug host/reference, release worker and native proof
fixture builds. `CARGO_BUILD_JOBS` defaults to 3 and can be reduced on smaller machines.

`acceptance.sh` requires a fresh evidence directory and runs live parity, fresh Engine replay/reorg,
raw import/rejection, worker fault injection, native stateless replay and reference measurements.
The source devnet remains running; all subordinate nodes are stopped by their respective harnesses.

Focused protocol/state/sandbox and real subprocess tests can be rerun independently:

```sh
cargo --config target/history-reth-overrides.toml test --locked -p base-execution-history
cargo test --manifest-path etc/history-worker/Cargo.toml --locked --all-targets
```

## Reference baseline

Build the import-capable reference node independently of the host fork with:

```sh
etc/history-devnet/build-reference.sh
```

The committed `historical/reference` subset retains original execution and original reth. Only
the standard import command is exposed; the workspace is pruned and unused lock entries are
removed without adding dependency versions. The builder validates its source digest before
building `base-reth-node --no-default-features` into separate `target/reference` output.
The executable is stored under `target/history-reference-artifacts/sha256/<digest>/`; exact
inputs, toolchain, digest and artifact path are recorded in `target/history-reference-build.json`.
Replay, import and measurement validate the approval and executable hash, then execute that
content-addressed artifact. A stale convenience copy is rejected rather than silently compared.

The digest is only a content-addressed approval input. Producing it is not deployment or acceptance;
the fixture and host integration limitations in `docs/history-worker.md` remain.

## Build image prerequisites

Historical devnet startup requires Docker with BuildKit support and builds
`base-era-setup:local-v1` from the committed `etc/docker/Dockerfile.devnet` and context on every
start. Docker can reuse matching layers, but an existing image tag alone is not trusted. The
resulting image ID and live executable digests are recorded before launch. Each acceptance stage
records its inputs before execution, launches frozen binary copies, and checks them again on
completion; evidence collection rejects missing or mismatched stage identities.

```sh
etc/history-devnet/build-setup-image.sh
```

No pre-existing setup image is required. The Dockerfile constructs the complete
image from Alpine 3.21.3
(`sha256:a8560b36e8b8210634f77d9f7f9efd7ffa463e380b75e2e74aff4511df3ef88c`) and Go 1.26
Alpine (`sha256:51a7c389a5ddaf82f527191a1e9bff9928655130a44e4975dd1d7e0acf59f1ae`), and pins `base/optimism` to
`0066b17c3fe0cbb5ea935de6d5b18d4fc86dc439`, and applies `etc/history-devnet/optimism-isthmus.patch`
against source blob `8e544e408`. It also pins `eth-beacon-genesis` to
`f57c0fb4606f9a28e5eecb61546efa9b658183b2` and `eth2-val-tools` to tag `v0.2.0`, commit
`db4f5fa6b8a6096a5f22c0a60f6494bf489c407b`.
