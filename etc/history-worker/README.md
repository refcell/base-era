# Base historical worker protocol

Serde-only DTOs for the version-neutral framed protocol. It deliberately has no Base, reth,
revm, or Alloy dependency.

Protocol `VERSION` 2 supports sessions: a worker reads request frames until clean EOF and emits one
terminal response per request (with any state-read exchange occurring before that response). The
first request in a process must carry the full `genesis` JSON. Later requests may send
`"genesis":null` to reuse that process's validated immutable genesis and chain specification. Null
before initialization is an infrastructure failure. Every request, including a cached request,
binds the actual full-or-null genesis field and must repeat matching deployment/configuration
identities. Supplying a different full genesis or identities never replaces session configuration.
