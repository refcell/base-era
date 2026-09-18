# Historical execution worker protocol

The standalone workspace is `etc/history-worker`; its protocol crate intentionally links only
Serde. Frames are a four-byte big-endian unsigned length followed by UTF-8 JSON, with a 64 MiB
limit. Protocol version 2 supports serialized sessions. For each `ExecuteRequest`, the worker may send ordered `ReadRequest`s and requires
the matching `request_id` and strictly increasing `sequence` in each `ReadResponse`, then emits one
`Outcome`, then waits for another request. Clean EOF ends the process; partial EOF, malformed/truncated frames, unavailable witness/provider reads, and
provider errors are infrastructure failures—not block invalidity. A nonexistent account and a zero
storage word are legitimate state values, not missing-witness errors.

Hex values are lower-case, `0x` prefixed, fixed-width for hashes/addresses/words, and minimal-width
for bytes. Decimal quantities are strings. Header and block fields contain exact consensus RLP.
`genesis_identity` is Keccak-256 of the request's exact canonical genesis JSON bytes;
`config_identity` is Keccak-256 of its canonical effective config object. An adapter must recompute
both, parse `BaseChainSpec::try_from_genesis`, check the decimal chain ID, derive the fork from that
frozen spec at the child number/timestamp, and only then compare the advisory `era`. It must decode
parent and child canonically, verify child parent hash/number/timestamp, and reject trailing RLP.
Thus an era label can never select execution rules.

The executable SHA-256 is checked on every operation, including warm requests and after transport
lock contention. On launch the verified bytes are copied into a fully sealed memfd, closing pathname
replacement races. The worker independently hashes its own executable once per process.
Deployment approves a digest out of band; the request repeats it to bind the invocation. Four
terminal outcomes exist: `success`, consensus `invalid-input`, unavailable protocol/fork
`unsupported`, and `infrastructure`. Configuration, artifact, era, protocol binding and provider
failures are never consensus-invalid verdicts.

The first request supplies full genesis JSON; later requests send `genesis: null`. This omits only
previously validated immutable configuration, not state or block input. Every request's binding
covers the actual full/null field, PID, request ID, configuration identities, parent, candidate,
and operation. Null before initialization or incompatible identities fail closed. Every operation
creates fresh execution state and a fresh parent-read sequence. No account, storage, code, block
hash, result, or override cache crosses requests.

The host rereads complete manifest bytes each time, retaining one cached parsed generation and
one serialized worker session. Effective chain-spec derivation is cached; comparison with the
active chain's genesis, chain ID and fork conditions is not. A manifest change creates a new
generation; in-flight callers keep their own immutable generation. Artifact bytes are never trusted
from pathname metadata alone. Transport, binding and infrastructure failures evict the session;
the current operation fails without retry or fallback, and a later operation may start a new child.
The 30-second deadline includes queueing and transport. Concurrency currently serializes rather
than growing an unbounded process pool.

Before exec, the host clears the environment, marks inherited descriptors close-on-exec, installs
Landlock (ABI 3 or newer), sets `NO_NEW_PRIVS`, and installs an architecture-checked seccomp filter.
Only runtime-library paths are readable/executable; no filesystem writes are allowed. Network,
ptrace/process-memory, process signaling, namespace/mount, and io_uring creation syscalls are
denied. The worker receives neither a database path nor an inherited database descriptor. Unsupported
kernels fail worker launch closed. This is Linux client isolation, not a claim of protection against
arbitrary kernel exploits; the approved worker remains part of the trusted consensus implementation.

## Delta semantics

Every changed account includes exact before/after nonce, balance, and code bytes. `None` means
nonexistent, distinct from an existing empty account. Storage contains every changed slot with its
original and final word. `storage_wiped` means all unspecified prior slots are deleted. Destruction
followed by recreation has both before and after plus `storage_wiped=true`; destruction has no
after; creation has no before. Loaded-only accounts and unchanged loaded slots are omitted. Applying
the before values (and restoring storage covered by a wipe from the host snapshot) is the inverse.
`block_reversions` are per-block inverse deltas in oldest-to-newest block order, not serialized reth
`BundleState`. Receipts include canonical Base receipt RLP because deposit fields and commitment
representation are era-sensitive, plus explicit deposit nonce/version. Gas and requests commitment
are block fields. No dependency status enum crosses the wire.

## Reproducibility and integration

The committed workspace at `etc/history-worker/historical/base` is frozen from Base revision
`1eda0f7f4cebb823522e62f34fc3e513b1c450b1`. That revision pins reth tag `base-v2.5.2.6`
(commit `5877708bbf9219c44758cd2ce28a365f738661f7`). An execution adapter's concrete call is:

```rust,ignore
let output = BasicBlockExecutor::new(
    BaseEvmConfig::base(Arc::new(BaseChainSpec::try_from_genesis(genesis)?)),
    rpc_database,
).execute(&recovered_block)?;
```

The RPC database implements revm `Database`; each method (`basic`, `storage`, `code_by_hash`, and
`block_hash`) writes a read frame and synchronously validates its response. It has no DB path or
Engine API. Build independently with `cargo build --manifest-path etc/history-worker/Cargo.toml
--locked`. The workspace lockfile is independent of the node lockfile.

The concrete lock divergence is recorded in `docs/history-spike-build.md`: worker
`alloy-consensus`/`alloy-eips` are 2.4.2 and primitives 1.7.3, versus host 2.4.1 and primitives
1.6.1. The worker also uses original reth rather than the host's patched execution traits. The
independently built processes exchange Serde DTOs, not Rust dependency types.

## Current implementation status

The worker now decodes canonical parent/header RLP, derives the era from a frozen `BaseChainSpec`,
executes recovered Base blocks through `BaseEvmConfig`, serves every revm database read over the
framed channel, and converts bundle state, receipts, requests, and gas totals to protocol DTOs. The
invocation binding is `keccak256(ExecuteRequest::binding_payload_bytes())`. The payload is compact
UTF-8 JSON in declaration order containing every request field (including genesis header hash,
chain ID, era, and protocol version); only `binding_hash` itself is omitted. Hosts should call the
protocol method rather than duplicate this serialization.

The migrated worker subprocess suite passed 15 tests covering Canyon receipt/CREATE2 transitions,
Regolith account creation, code/storage deletion, separate parent views, calls, estimation, tracing,
protocol/configuration failures, and provider failures. Host unit tests passed 11 and the Python
suite passed 9. Live cutover, replay/reorg, import, and failure evidence is indexed in
[`history-spike.md`](history-spike.md).
`block_reversions` is reserved and empty in this one-block protocol: the host reconstructs inverse
entries from validated before-values and retains the parent snapshot to restore slots omitted by
a wipe. Canonical receipt RLP is authoritative; duplicated deposit fields are informational. The
host checks receipt order, status, bloom, cumulative/terminal gas, transaction count, optional request
commitments and pre-Isthmus output restrictions. Normal Engine/import consensus checks still verify
header fields, receipt commitments and computed state roots before committing.

Supported historical debug tracers are the default struct logger and built-in `callTracer`; their
JSON is compared exactly with the pinned reference. Other tracers and trace-call `txIndex` are
explicitly unsupported, with no local fallback. Pending-block simulation and `eth_simulateV1`
remain host operations, outside the extracted historical RPC boundary.
