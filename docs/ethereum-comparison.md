# Base Era and Ethereum's history roadmap

Base Era is an experimental Base client architecture for **version-isolated historical
execution**. It is adjacent to Ethereum's history-expiry work, but it is not state expiry,
not an Era/Era1 archive format, and not a deployed change to Base or Ethereum consensus.

## The short comparison

| Topic | What it concerns | What it does **not** provide | Status and relationship to Base Era |
|---|---|---|---|
| **History expiry / EIP-4444** | Availability and retention of old execution **headers, block bodies and receipts**. The draft says clients should stop serving, and may prune, data older than 33,024 epochs over execution-layer p2p.[^eip4444] | It does not expire current account balances, contract code or storage. Nor does an archive of old blocks by itself execute them. | EIP-4444 remains a **Draft**. Separately, all major Ethereum execution clients supported a partial, pre-Merge history drop by July 2025; full rolling expiry was still ongoing.[^partial] Base Era explores one execution-versioning direction already named by EIP-4444, rather than claiming to invent it. |
| **State expiry** | Making infrequently used **account/storage state** inactive and resurrectable, so ordinary nodes need not retain all active state indefinitely.[^statelessness] | It is not pruning old blocks or receipts. Inactive state is not simply destroyed. | Research, not a deployed Ethereum consensus feature in the cited roadmap. Base Era does not implement it. |
| **Era archives (`.era`)** | Cold-storage groups of beacon-chain blocks plus a beacon state, normally spanning 8,192 slots. Post-Merge Era files also carry execution block contents, but not execution states.[^era] | They do not contain pre-Merge execution history, and do not package an execution engine. | A consensus-layer archive format. Base Era neither reads nor writes it. |
| **Era1 archives (`.era1`)** | Pre-Merge execution-layer history: compressed RLP headers, bodies and receipts, total difficulty, an accumulator and an index, in batches of at most 8,192 blocks.[^era1] | They do not contain current or historical execution state, or the fork-rule implementation needed to replay a block. | A history container and distribution input, not Base Era's worker protocol. Era1 could be an upstream data source for a future full-history workflow. |
| **Historical execution versioning** | Selecting the rules that applied at a block and retaining executable implementations capable of reproducing its transition. Ethereum's executable specification (EELS), for example, publishes fork-aware versions in which the hardfork count is encoded in the version.[^eels] | Version labels, old blocks and state snapshots do not prove that the selected implementation is correct. | EIP-4444 explicitly suggests a full-sync shim that pieces together releases of execution engines.[^eip4444] Base Era is a concrete local Base spike in this design space: it routes selected operations to approved worker executables and keeps validation and canonical storage in the host.[^spike][^worker] |

## Three things that must not be conflated

**History data** is the old chain record: headers, transaction-bearing bodies and receipts.
**State** is the account/storage view against which execution reads and writes. Current state is
still needed for current execution; answering arbitrary past-state queries generally requires
specialized archive-node indexes, not merely a directory of old blocks.[^partial] **Rules** are the
executable fork implementations that transform a parent state and block into an output state and
receipts.

An Era1 file can preserve authentic block inputs without preserving the parent state needed for
replay. Even when both input and state are available, bytes in an archive do not execute
themselves: replay still needs the correct historical gas schedule, transaction and receipt rules,
precompiles, system transitions and fork selection. Consequently, archiving data alone neither
reexecutes history nor justifies deleting old rule implementations. EIP-4444 itself says clients
that retain archive support must import history obtained out of band **and retain support for each
historical network upgrade**; its proposed multi-engine shim is the complementary option.[^eip4444]

## What this demo actually verifies

In the source spike, the host owns the canonical database, forkchoice, validation, commit and
unwind. For the tested pre-Isthmus range it launches a SHA-256-approved executable worker. The
worker derives the precise fork from a frozen chain specification, requests immutable reads from a
parent-bound state view, executes the block, and returns explicit account/code/storage deltas and
canonical receipt bytes. The host checks request/result bindings and then performs its ordinary
commitment and consensus validation. Missing, crashed, incompatible or malformed workers fail
closed rather than falling back to current rules.[^spike][^worker]

That pinned executable identity is why the demo can make a narrow, testable statement about which
code verified historical execution. It does **not** establish that the host binary is free of all
legacy branches: historical source and dependencies remain, and some compatibility work remains
host-side. Nor does it turn worker output into consensus merely because it came from an approved
binary; normal roots and commitments are still checked.[^spike]

## Proof and performance caveats

The spike's **native stateless checks** captured self-contained witnesses for blocks 19–21 and
replayed them offline to matching headers. This tests the existing native witness/execution path.
It is not a zkVM proof and does not authorize a new program. A zkVM deployment would separately
need a guest-compatible historical design, independently built guest program and verification-key
identities, proofs across the boundary, any required aggregation changes, and verifier/onchain
authorization. A schedule or configuration identifier does not commit an external native worker's
semantics.[^spike]

The measured worker numbers are costs of this experimental implementation, not performance gains.
In the final local run, repeated historical calls were about 2.54 seconds versus about 2.9
milliseconds on the reference client, and full 22-block import was about 49.6 seconds versus 1.30
seconds. The spike starts a process per operation and repeatedly processes roughly 9 MB of genesis
and manifest data. Persistent workers, batching and configuration caching are future optimization
work; metadata retrieval should not be compared with execution.[^spike]

## Publication caveats

- EIP-4444's normative page is still labelled **Draft**. Do not describe its full rolling policy as
  a deployed Ethereum consensus feature. The narrower 2025 pre-Merge client support is real, but
  client-specific and explicitly described as “partial history expiry.”[^eip4444][^partial]
- Base Era is a local devnet spike, not an official Base project, production client, security audit,
  Era/Era1 implementation, state-expiry implementation, or new proof system.[^spike]
- The demonstrated cutover is pre-Isthmus versus Isthmus at compressed devnet block 20. It is
  evidence for that corpus and those operations, not every historical Base fork or RPC method.[^spike]

## Notes for the current README/site owner

1. The README's statement that the project explores EIP-4444's versioned-execution direction is
   supportable, but it should say that EIP-4444 already proposes the multi-release execution-engine
   shim; Base Era must not claim novelty for that idea.[^eip4444]
2. Any site copy saying “Ethereum history expiry is only research” is stale/overbroad. The EIP is
   still Draft and full rolling expiry was unfinished, while partial pre-Merge expiry had shipped
   across execution clients by July 2025.[^partial]
3. Avoid wording such as “Era stores Ethereum state,” “archives make history executable,” “old
   rules can be deleted once blocks are archived,” or “stateless replay is a zk proof.” Each merges
   a distinct boundary documented above.
4. Performance copy should present the reported latency and resource figures as experimental
   overhead. The current spike demonstrated isolation and parity, not a speedup.[^spike]

## Sources

[^eip4444]: Ethereum Improvement Proposals, [EIP-4444: Bound Historical Data in Execution Clients](https://eips.ethereum.org/EIPS/eip-4444) (status: Draft; accessed 2026-09-18), especially Abstract, Specification, “Full syncing from genesis,” and JSON-RPC/Security considerations.
[^partial]: Ethereum Foundation Blog, [“Partial history expiry announcement”](https://blog.ethereum.org/2025/07/08/partial-history-exp) (2025-07-08), including the explicit distinction between chain history and current account state and the statement that full rolling expiry remained ongoing.
[^statelessness]: ethereum.org, [“Statelessness, state expiry and history expiry”](https://ethereum.org/en/roadmap/statelessness) (page last updated 2024-08-11), especially “History expiry” and “State expiry.” Its progress labels predate the 2025 partial-history rollout, so the later Foundation announcement controls for that deployment fact.
[^era]: Ethereum client format specifications, [“Era files”](https://github.com/eth-clients/e2store-format-specs/blob/main/formats/era.md) (accessed 2026-09-18), especially Structure and “What happens after the merge?”.
[^era1]: Ethereum client format specifications, [“Era1 files”](https://github.com/eth-clients/e2store-format-specs/blob/main/formats/era1.md) (accessed 2026-09-18).
[^eels]: Ethereum Execution Layer Specifications, [“Spec Releases”](https://github.com/ethereum/execution-specs/blob/forks/amsterdam/docs/specs/spec_releases.md) (accessed 2026-09-18). EELS is an example of fork-aware executable specification versioning, not the Base Era worker format or proof of client deployment.
[^spike]: Base source spike, [`docs/history-spike.md`](https://github.com/base/base/blob/1eda0f7f4cebb823522e62f34fc3e513b1c450b1/docs/history-spike.md), local report dated 2026-09-18. This document is source-grounded in that local report; the link will resolve only if/when those spike docs are published at that revision or an integration revision.
[^worker]: Base source spike, `docs/history-worker.md`, “Historical execution worker protocol,” local report dated 2026-09-18. This protocol document is not yet available at a stable public URL.
