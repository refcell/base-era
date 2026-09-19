# Conditional pre-Isthmus extraction candidates

> Archived narrow audit. The [fresh-clone deletion experiment](retirement.md) supersedes this
> as the repository-wide sizing exercise. These old counts cover a smaller horizon and scope;
> do not add them to the new diff totals.

## Result

At the time of this range audit, no source had been removed: **actual removed = 0 lines**.

Against the frozen revision below, this bounded audit found **60 gross production lines** that are
plausible pre-Isthmus-only extraction candidates. It separately identifies **105 gross lines of
dedicated tests/fixtures** that could move or retire only with equivalent worker replay coverage.
Those figures are separate inventories, not a combined savings claim: a refactor must add
replacement routing/API code, so neither is a net-diff forecast.

An additional **96 production + 392 test lines** are recorded only as a **conditional boundary
expansion**, not as currently deletable candidates. They require resolving host consumers and
activation-boundary behavior first.

Source is immutable Base revision
[`1eda0f7f4cebb823522e62f34fc3e513b1c450b1`](https://github.com/base/base/commit/1eda0f7f4cebb823522e62f34fc3e513b1c450b1),
read from committed `etc/history-worker/historical/base`; `.base-revision` records the same hash.
Counts are inclusive physical lines, including blanks and comments.

## Counted extraction candidates

| File | Production ranges | Lines | Dedicated test ranges | Lines | Basis |
|---|---|---:|---|---:|---|
| `crates/execution/evm/src/l1.rs` | 76–107 | 32 | 378–395 | 18 | Bedrock L1-info parser and fixture |
| `crates/common/evm/src/l1block.rs` | 190–197, 304–307, 329–337 | 21 | 445–531 | 87 | Pre-Ecotone storage and Bedrock/Ecotone fee alternatives/helpers |
| `crates/execution/evm/src/build.rs` | 77–81, 125–126 | 7 | — | 0 | Pre-Isthmus withdrawals header/body alternatives |
| **Gross totals (kept separate)** | | **60** | | **105** | |

These become candidates only if every block before Isthmus is consensus-routed to digest-pinned,
independently buildable workers, with replay/import parity and defined artifact-failure behavior.
The inventory is an audit of these files and ranges only. It is **not a lower bound** on net savings,
not a whole-repository estimate, and not a claim about current trunk, which may have changed.

## Required code deliberately retained

- `l1.rs:109–162` and its Ecotone/Fjord tests remain. The first Isthmus block's L1-info transaction
  precedes the upgrade and uses the Ecotone-format selector, so the latest host must retain the
  predecessor parser. Consequently `l1.rs:69–73` dispatch is not counted either.
- `l1block.rs:533–625` tests the Fjord formula that remains the current Isthmus fee basis. It is not
  historical-only. Mixed tests at 351–442 also exercise current Fjord or current empty/deposit
  behavior and are omitted rather than partially inflated.
- `handler.rs:87–90` still rejects system deposits. A latest-only refactor may simplify its fork
  gate, but the check remains and no exact deletion is counted.
- `build.rs:88–92` supplies zero `excess_blob_gas`/`blob_gas_used` values required from Ecotone
  through Isthmus, before Jovian repurposes the field.
- The underlying formulas in `crates/common/l1-fees/src/params.rs` were inspected but not added:
  they are shared engine-neutral API bodies and this audit does not prove their host consumers can
  be removed.

## Conditional boundary expansion (not deletable now)

| File | Production ranges | Lines | Test ranges | Lines | Blocker |
|---|---|---:|---|---:|---|
| `crates/common/evm/src/canyon.rs` | 1–52 | 52 | — | 0 | Canyon deployment must account for activation at genesis, simultaneous/custom fork activation, irregular state transition replay, and proof/stateless consumers. |
| `crates/execution/consensus/src/proof.rs` | 18–38, 51–73 | 44 | 90–481 | 392 | Receipt quirks are still called by host consensus validation, block assembly, and proof paths. They move only if the extraction boundary expands to all those consumers. |
| **Conditional totals** | | **96** | | **392** | |

Other chain-spec, derivation, RPC, txpool, proof, genesis, upgrade, and vendored-reth surfaces were
not audited into the number. Imports/re-exports and signatures are also omitted unless represented
by an exact range.

## Reproduce

```sh
python3 tools/count-history-deletions.py
```

The script verifies the frozen revision, complete-file SHA-256 hashes, non-overlapping ranges,
lexical counts, declared totals, and explicit zero actual removal. It performs no discovery and
fails on source or inventory drift.
