# Historical execution failure test

`etc/history-devnet/failures.py` runs live failure and causality checks against an owned host process
and a copy of an already-populated replay datadir. It does not modify the source replay manifest,
artifact, or datadir.

Run after the replay owner has stopped:

```console
python3 etc/history-devnet/failures.py --run-dir "$RUN"
```

The script copies the host binary, manifest, and datadir into a unique `target/failure-test*` directory,
chooses free HTTP, Engine, and P2P ports, and refuses to start if any process command line names the
source datadir. Pass `--use-existing-datadir` only when the supplied datadir is owned by the caller.
It records `evidence.json` and `host.log`. The checks cover real GasPriceOracle calls at blocks 19 and
20, wrong-digest and missing-worker causality, recovery, a SIGKILL of a uniquely verified direct memfd
worker child, and malformed, incompatible-terminal-schema, stale-request, and partial-frame timeout
fixture responses. Fixtures consume the complete request before replying, failure assertions reject a
generic pipe error, and the timeout check verifies that the worker is reaped. Fixture executables are
used only to exercise failure handling; successful historical execution always uses the real pinned
artifact. Protocol version negotiation is covered separately by the real worker spawn tests.

By default the script derives genesis and `replay-evidence/worker-datadir` from `--run-dir`; use
`--datadir` if replay used another output directory. The migrated run passed all [seven recorded
claims](../etc/history-devnet/evidence/final/failures.json): real historical/current execution,
digest/missing-artifact causality and recovery, in-flight crash recovery, malformed response,
incompatible terminal schema, stale request, and timeout/reaping. Every failure left `latest`
unchanged; current execution remained available while the historical worker was broken.
