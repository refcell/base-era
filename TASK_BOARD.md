# Demo publication task board

The migrated demo passed fresh acceptance on 2026-09-18. Portable evidence lives
in `etc/history-devnet/evidence/final`; it is not reused source-spike evidence.

| Workstream | Owner | Status / evidence |
|---|---|---|
| Source inventory | Orchestrator + inventory agents | Complete: `docs/source-inventory.md` and JSON allowlists |
| Host and harness migration | Orchestrator | Complete: selected source closure, committed sources, runnable `./demo` interface |
| Frozen worker/reference and reth packaging | Packaging agent | Independent locked builds passed; divergent Alloy versions and frozen-source hashes verified |
| Showcase website | Site agent + orchestrator | Real migrated stats integrated; desktop/mobile layout and timeline checked; final public inspection pending |
| Ethereum comparison | Research agent | Sourced EIP-4444, state-expiry/statelessness and Era/Era1 comparison integrated |
| Fresh builds and live acceptance | Orchestrator | PASS: 95 replay, 6 import, 7 failures, native 19–21; 11 host + 15 worker + 9 integrity tests |
| Independent correctness/reproducibility review | Review agents | Complete: source/artifact issues fixed and rerun; final evidence audit found no publication blocker |
| GitHub Pages publication | Orchestrator | Pending deployment and public verification; downloadable binary releases not published |

## Publication rules

- Push verified checkpoints directly to `main` roughly every ten minutes; PR #1
  is merged. Never publish secrets or claim incomplete acceptance as passing.
- Keep the source baseline, integration and any actual historical-code deletion
  distinguishable. Removing unrelated packages is not retiring fork logic.
- Preserve the original Base workspace and its existing local devnet.
- Published measurements identify their source run, build profiles and limits.
- Complete when the migrated demo runs reproducibly and the inspected Pages site
  is public, with runnable instructions, evidence, walkthrough and limitations.
