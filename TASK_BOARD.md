# Demo publication task board

The source spike in `base/base` has passed its local acceptance suite. This
repository's migration is not complete until a fresh run here passes too.

| Workstream | Owner | Status / evidence |
|---|---|---|
| Source inventory | Orchestrator + inventory agents | Complete: `docs/source-inventory.md` and JSON allowlists |
| Host and harness migration | Orchestrator | In progress: preserve upstream paths, remove moving patch setup |
| Frozen worker/reference and reth packaging | Packaging agent | Pending assignment; separate immutable source workspaces |
| Showcase website | Site agent | Pending assignment; accessible static site and honest evidence-driven claims |
| Ethereum comparison | Research agent | Pending assignment; distinguish history expiry, state expiry and Era files |
| Fresh builds and live acceptance | Orchestrator | Pending migrated source |
| Independent correctness/reproducibility review | Review agent | Pending integrated implementation and fresh evidence |
| GitHub Pages and release publication | Orchestrator | Pending verified demo, site inspection and artifact provenance |

## Publication rules

- Push coherent checkpoints roughly every ten minutes; never publish secrets or
  claim incomplete acceptance as passing.
- Keep the source baseline, integration and any actual historical-code deletion
  distinguishable. Removing unrelated packages is not retiring fork logic.
- Preserve the original Base workspace and its existing local devnet.
- Published measurements identify their source run, build profiles and limits.
- Complete when the migrated demo runs reproducibly and the inspected Pages site
  is public, with runnable instructions, evidence, walkthrough and limitations.
