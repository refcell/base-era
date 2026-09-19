# Latest-only Base: measured deletion experiment

**About 6k lines, not 60.** A fresh, full clone of `base/base` was edited into a
Beryl-and-later source candidate, then measured against its untouched upstream revision.
This replaces the earlier six-file pre-Isthmus range audit as the sizing evidence.

| Physical Rust lines | Removed | Added | Net reduction |
|---|---:|---:|---:|
| Production, including comments and blank lines | 3,003 | 260 | **2,743** |
| Unit/integration tests, test utilities and benchmarks | 3,375 | 211 | **3,164** |
| **Total Rust** | **6,378** | **471** | **5,907** |

Separately: 27 one-line embedded bytecode artifacts removed, plus 33 net lines of documentation
and build metadata. The full Git diff is **6,457 deletions / 490 additions = 5,967 net lines**.
Bytecode hex strings are counted as lines, not expanded into imaginary source lines.

These are **source-candidate savings**, not production-ready or integration-adjusted savings.
The missing historical derivation/proof dispatch and lifecycle machinery will add code. Tests
belong with their historical implementation, not in the trash. The runnable Base Era demo is
unchanged and still retains historical bodies; this candidate is not its devnet binary.

## Source and reviewable artifacts

- Fresh upstream: [`c870155`](https://github.com/base/base/commit/c8701553d151e3baf6bbe15712c8b95f2bfe7bbe).
- Local checkout: `/home/refcell/dev/base-era-retirement`, branch `spike/latest-only-retirement`.
- [Complete deletion diff](retirement/latest-only.patch), pinned to that upstream revision.
- [Machine-readable per-file counts, candidate tree and patch SHA-256](retirement/counts.json).
- [Test log](retirement/tests.log) and [host compilation log](retirement/host-check.log).
- Original upstream Rust toolchain and dependency versions retained: Rust 1.96.0, reth
  `base-v2.5.2.6`, revm 42.0.1. The lockfile only drops eight dependency edges from changed
  manifests. **No reth/revm source patch or shared-cache modification in this experiment.**

The diff is a frozen audit artifact, not a moving patch-overlay architecture for the demo.
The demo continues to build from the source already committed in this repository.

## What was removed

| Scope | Net production Rust | Net tests/benchmarks |
|---|---:|---:|
| Derivation and upgrade construction | 2,057 | 1,897 |
| Execution and shared configuration | 402 | 1,059 |
| Alternate EVM2 scaffold | 166 | 208 |
| Conditional native-proof reduction | 118 | 0 |

| Area | Historical implementation retired |
|---|---|
| Derivation pipeline | Pre-Holocene `ChannelBank` and `BatchQueue`, switching multiplexers, old batch stream/frame handling and their dedicated benchmarks/tests |
| Upgrade construction | Ecotone, Fjord, Isthmus and Jovian transaction builders and embedded bytecode; L1-info generation now uses the mature Jovian layout |
| Main execution | Canyon deployment hook, Bedrock/Ecotone fee math, older receipt commitments, pre-Jovian assembly/validation alternatives and historical execution tests |
| Alternate EVM2 scaffold | Canyon transition, older precompile cap variants and registration, legacy fee/receipt gates and old parity cases; counted separately in per-file data |
| Native proof executor | Pre-Jovian header/environment assembly and the Regolith receipt commitment workaround; historical proof execution requires a separate program |
| Integration | Flashblocks' obsolete Canyon hook and callers/re-exports; retained latest state-cache regression coverage |

Most removable code is in **derivation**, not an EVM transaction hook. An execution-only worker
does not earn those savings. A historical derivation implementation must own the old L1 scanning,
channel/batch processing and upgrade-transaction construction before these deletions can ship.

## What stays, and why

- Historical block, receipt, transaction and L1-info **decoders**, getters and roundtrip coverage:
  a current client still reads old stored data. Format names alone do not make a codec obsolete.
- Upgrade enums, activation schedules, chain identities and configuration commitments: routing
  still needs them. Cobalt, Denim and Zenith behavior is retained, not removed as "legacy."
- Current Fjord compression/fees and current Jovian storage/precompile rules, despite their names.
- RPC and transaction-pool adapters, ancestry/state/storage access, forkchoice and unwind: these
  remain necessary. Historical calls/receipt recomputation need external execution, not latest-rule
  substitution.
- L1 Pectra blob-schedule compatibility and historical data interpretation needed by present paths.
- Reth/revm/EVM2 dependency history and zkVM guest/verifier authorization infrastructure. These are
  not Base-owned deletions and are not counted as potential repository reduction.
- Additional fork gates and shared compatibility APIs remain where no safe deletion was established.

This is a broad **measured candidate**, not a proof that every remaining historical line can go
or a maximum-savings estimate. In particular, deleting all files mentioning an old fork would
remove live consensus rules and inflate the result.

## Assumptions and unsupported operations

The horizon is **Beryl**, `BaseUpgrade::LATEST` at the pinned revision—not the demo's Isthmus
cutover. Earlier upgrades, including Jovian, must already have activated before the horizon;
relevant L1 derivation origins must also be past the retired pipeline eras. Custom schedules with
simultaneous, disabled or later activation of those old upgrades are not supported by this patch.
The retained old parent at a Beryl boundary already has Jovian fields.

**Do not run this patch as a historical node.** It has no historical derivation dispatcher, no
complete historical execution routing and no versioned proof-program selection. Some remaining
public APIs still accept older spec IDs although their bodies are latest-only. Unsupported history
is not uniformly rejected at ingress. Pre-Beryl replay/import, crossing reorgs and historical RPC
are therefore **not verified or safe in this candidate**. The existing demo's acceptance results
must not be attributed to it.

Likewise, narrowing native proof code changes executable semantics. A real deployment needs
pinned historical guest programs, correct program/configuration identity and verifier authorization.
This experiment neither deploys guests nor changes on-chain verification.

## Verification and independent review

The combined nine-package test command passed **648 tests**, with **0 failures** and **8 existing
ignored documentation examples**. Coverage includes latest execution, protocol codecs, derivation,
native executor tests, and EVM2 integration/parity tests. This is not a full-workspace test run or a
latest-only devnet/replay parity claim.

Compilation also passed for `base-reth-node`, `base-builder-bin` and `base-consensus`, with
`--locked --no-default-features`. That covers the main host integrations, not every feature
combination. Reapplying the exported patch to untouched upstream reproduces the candidate tree.

Independent review found and prompted two fixes: restore current L1-info cache invalidation coverage
under Beryl (including Jovian DA scalars), and remove a blanket historical-upgrade implementation
that panicked at runtime. Retired upgrade types now have no transaction-builder implementation.
The current Denim same-second stale-batch regression was moved to `BatchValidator`, not discarded
along with the retired mux. The obsolete Flashblocks call was removed during integration.

## Reproduce the diff and checks

From a Base Era checkout, use a separate directory; never apply this to a working node checkout:

```sh
git clone https://github.com/base/base.git ../base-retirement-check
git -C ../base-retirement-check switch --detach c8701553d151e3baf6bbe15712c8b95f2bfe7bbe
git -C ../base-retirement-check apply --index "$PWD/docs/retirement/latest-only.patch"
git -C ../base-retirement-check diff --cached --stat
git -C ../base-retirement-check write-tree # compare with candidate_tree in counts.json

python3 -m venv ../base-retirement-check/target/loc-env
../base-retirement-check/target/loc-env/bin/pip install tree-sitter==0.25.2 tree-sitter-rust==0.24.0
../base-retirement-check/target/loc-env/bin/python tools/measure-retirement.py \
  ../base-retirement-check --base c8701553d151e3baf6bbe15712c8b95f2bfe7bbe \
  --output /tmp/retirement-counts.json --patch /tmp/retirement.patch
```

Inside that separate checkout:

```sh
CARGO_BUILD_JOBS=3 cargo test --locked \
  -p base-common-evm -p base-common-evm2 -p base-common-l1-fees \
  -p base-execution-evm -p base-execution-consensus -p base-consensus-derive \
  -p base-consensus-upgrades -p base-protocol -p base-proof-executor
CARGO_BUILD_JOBS=3 cargo check --locked --no-default-features \
  -p base-reth-node -p base-builder-bin -p base-consensus
```

The counter parses Rust syntax to separate `#[cfg(test)]` blocks and classifies explicit test,
test-utility and benchmark files separately. It counts actual added/deleted physical lines, so
comments and blank lines count; it does not label all `.rs` files "production". Per-file categories
sum to `git diff --numstat`. The patch and candidate tree bind the numbers to exact bytes.
