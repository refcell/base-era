# Historical fork devnet

The disposable harness starts dedicated L1 services and a full L2 stack (sequencer, batcher,
derivation, and independent verifier) with Isthmus at block 20. From a clean checkout, first build
the artifacts as documented in `docs/history-spike-build.md`, then use a fresh run directory:

```sh
RUN="$PWD/target/history-devnet-run"
etc/history-devnet/start.sh "$RUN"
etc/history-devnet/exercise.sh "$RUN"
etc/history-devnet/verify.sh "$RUN"
```

Keep the devnet running for replay/import. Stop it when those consumers finish:

```sh
etc/history-devnet/stop.sh "$RUN"
```

`runtime.json` contains ephemeral Engine authentication and must not be published. Portable results
in `etc/history-devnet/evidence/` deliberately contain no JWT or runtime endpoint.

## Native stateless witness recording

Witness recording needs a closed, fully replayed node database and a node started with
`--rpc.eth-proof-window 128`. The reusable runner copies that database, allocates unique local HTTP,
Auth RPC, and P2P ports, and starts and stops only the node that it owns:

```sh
etc/history-devnet/stateless.py \
  "$RUN" "$RUN/replay-evidence/worker-datadir" target/history-host-node
```

Build `target/debug/examples/fixture` first if it is absent (the runner also accepts
`--fixture-binary`). The output at `$RUN/evidence/stateless-final` contains full RPC headers,
witness archives, command metadata, logs, and `results.json`. The runner checks every subprocess
exit and compares each fixture's expected full-header hash with RPC for blocks 19–21.

The original source spike passed these checks on 2026-09-18. Fresh migrated evidence must be
captured before claiming this checkout passed. This check neither authorizes zk proofs nor changes
a verifying program.

Captured `corpus/block-{19,20,21}.tar.gz` files can be replayed without a
running network using `target/debug/examples/fixture run <archive>`. Run
`etc/history-devnet/acceptance.sh "$RUN"` for the complete live/replay/import/failure/native/measurement
suite, and `python3 etc/history-devnet/collect.py "$RUN" --output "$RUN/portable-evidence"` to
collect shareable results without Engine credentials.
