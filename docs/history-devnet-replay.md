# History Engine replay

`etc/history-devnet/replay.py` copies blocks 1 through 22 from the isolated history devnet into a
worker-enabled EL and an unmodified reference EL. It uses authenticated Engine V3 before Isthmus,
V4 (including execution requests) at and after Isthmus, advances forkchoice, tests a reorg to block
18 and restoration (including canonical `latest` checks), and submits six malformed payloads whose
hashes are recomputed from modified RLP headers. Malformed payloads pass only when both clients
return the same consensus-invalid result; transport failures fail. It never sends Engine requests
to the source devnet.

The harness also compares worker and reference JSON for historical calls across the Isthmus
boundary, call and transaction/block traces at blocks 19 and 20, state/code overrides, and bounded gas
estimation. In launch mode it gracefully restarts both nodes with the same datadirs and repeats a
historical call. It then corrupts the executable digest in the replay-owned worker manifest,
requires the historical call to fail without changing `latest`, restores the manifest, and requires
the call to succeed.

The launch mode requires a common genesis and creates empty, harness-owned datadirs. The source
manifest is copied into the evidence directory; only that private copy is ever modified. The worker
alone receives `BASE_HISTORY_MANIFEST`; it is explicitly removed from the reference environment.
Each node uses a unique P2P port with discovery disabled and exposes `eth,net,web3,debug` over HTTP.
Additional flags can be repeated with `--node-arg`.

```sh
python3 etc/history-devnet/replay.py \
  --run-dir "$RUN" --through 22 --output "$RUN/replay-evidence"
```

If nodes were started separately, avoid assumptions about their CLI and pass their authenticated
Engine endpoints directly:

```sh
python3 etc/history-devnet/replay.py --run-dir "$RUN" \
  --worker-engine http://127.0.0.1:8551 --worker-rpc http://127.0.0.1:8545 \
  --reference-engine http://127.0.0.1:8552 --reference-rpc http://127.0.0.1:8546
```

The launch defaults are the artifacts under `target/`, and genesis is derived from `--run-dir`.
The JWT is read from `runtime.json`. `replay-evidence/transcript.json` records every JSON-RPC
request and response; `summary.json` records exact PASS, FAIL, and SKIP outcomes. `cast` must be on
`PATH` to compute Keccak-256 for malformed headers. The passed count excludes SKIP. The migrated
run recorded [95 PASS, 0 FAIL, and 0 SKIP](../etc/history-devnet/evidence/final/replay.json),
including alternate-branch builder parity, canonical restoration, restart, and manifest-digest
causality. The six
malformed cases span both sides of Isthmus, testing state/receipt roots and gas accounting;
Engine artifact failure is required to be internal, not INVALID.
