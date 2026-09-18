# Historical execution spike build

## Recorded inputs

- Worker materialization archives Base commit `1eda0f7f4cebb823522e62f34fc3e513b1c450b1`.
- Fork remote is `https://github.com/base/reth`, original commit
  `5877708bbf9219c44758cd2ce28a365f738661f7` (`base-v2.5.2.6`).
- `etc/history-reth/0001-external-complete-block-hook.patch` is generated with `git diff
  5877708bbf9219c44758cd2ce28a365f738661f7`, not `git diff HEAD`; it includes committed spike
  changes and current working-tree changes, including fresh RPC builder/debug routing.
- Worker and host use `etc/history-worker/Cargo.lock` and root `Cargo.lock`, respectively.

The actual resolved locks intentionally diverge: worker Alloy consensus/EIPs 2.4.2 and primitives
1.7.3 versus host consensus/EIPs 2.4.1 and primitives 1.6.1. Worker reth is the original upstream
revision; host reth has the external execution/RPC interface patch. No Rust types cross the framed
JSON boundary. Both workspaces pin Rust 1.96.0. The worker is built in release mode; the measured
host/reference are debug builds with default features disabled.

## Commands

```sh
# Run from a clean checkout. No path below depends on a particular home directory.
etc/history-reth/setup.sh
etc/history-devnet/build.sh
etc/history-devnet/build-reference.sh
RUN="$PWD/target/history-devnet-run"
BASE_HISTORY_APPROVAL="$PWD/target/history-artifacts/APPROVAL.json" \
  etc/history-devnet/start.sh "$RUN"
etc/history-devnet/exercise.sh "$RUN"
etc/history-devnet/acceptance.sh "$RUN"
python3 etc/history-devnet/collect.py "$RUN" --output "$RUN/portable-evidence"
etc/history-devnet/stop.sh "$RUN"
```

Setup refuses an existing checkout rather than clobbering edits. The build runs the worker
independently with `etc/history-worker/Cargo.lock`, materializing Base commit
`1eda0f7f4cebb823522e62f34fc3e513b1c450b1` first. It then builds the host with the independent
root `Cargo.lock` and generated pinned-reth overrides. Both complete executables are copied to
`target/history-artifacts/sha256/<digest>/`; `APPROVAL.json` names the immutable worker and `DIGEST`
records its digest for simple auditing. Existing source materializations are byte-compared and are
never replaced. The checkout, materialization, Cargo targets, override config, and approval artifacts
are generated data and must not be committed.

`start.sh` uses that approval automatically when present and does not rebuild the history host. A
caller may set `BASE_HISTORY_APPROVAL` explicitly to use another reviewed approval.

Run from the repository root. Required tools are Rust 1.96, a Rust-capable native toolchain
(C/C++, clang/libclang, pkg-config), Python 3, Git, jq, ripgrep, Foundry `cast`, and Docker/BuildKit.
The supported host is Linux with Landlock ABI 3+ and seccomp; unsupported sandbox setup fails closed.
Internet access is needed to fetch pinned dependencies/images on a cold machine. The spike always
requires the explicit reth override, including ordinary builds without the history feature. No
dependency cache is modified. The complete reth patch was applied to a fresh original-revision
checkout and its generated overrides passed locked Cargo metadata resolution.

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

The script archives Base commit `1eda0f7f4cebb823522e62f34fc3e513b1c450b1`, including that
commit's own lockfile, into the owned `target/history-reference-source`. It applies only the recorded
`etc/history-devnet/reference-cli.patch`, which exposes reth's `import` command, and refuses to
replace a changed materialization. It uses no local reth override and builds
`base-reth-node --no-default-features` in the separate `target/history-reference-target`, avoiding
the concurrently built host executable. The resulting `target/history-reference-node` is also
published under `target/history-reference-artifacts/sha256/<digest>/`; exact inputs, toolchain,
digest, and artifact path are recorded in `target/history-reference-build.json`.

The digest is only a content-addressed approval input. Producing it is not deployment or acceptance;
the fixture and host integration limitations in `docs/history-worker.md` remain.

## Build image prerequisites

Historical devnet startup requires Docker with BuildKit support and builds
`devnet-setup:local-v3` from `etc/docker/Dockerfile.devnet` when that tag is absent:

```sh
etc/history-devnet/build-setup-image.sh
```

No pre-existing `devnet-setup:local-v2` image is required. The Dockerfile constructs the complete
image from Alpine 3.21.3
(`sha256:a8560b36e8b8210634f77d9f7f9efd7ffa463e380b75e2e74aff4511df3ef88c`) and Go 1.26
Alpine (`sha256:51a7c389a5ddaf82f527191a1e9bff9928655130a44e4975dd1d7e0acf59f1ae`), and pins `base/optimism` to
`0066b17c3fe0cbb5ea935de6d5b18d4fc86dc439`, and applies `etc/history-devnet/optimism-isthmus.patch`
against source blob `8e544e408`. It also pins `eth-beacon-genesis` to
`f57c0fb4606f9a28e5eecb61546efa9b658183b2` and `eth2-val-tools` to tag `v0.2.0`, commit
`db4f5fa6b8a6096a5f22c0a60f6494bf489c407b`.
