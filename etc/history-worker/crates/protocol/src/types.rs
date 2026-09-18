use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A complete, immutable request to execute one block.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteRequest {
    /// Wire protocol version.
    pub version: u16,
    /// Host-selected invocation identifier.
    pub request_id: String,
    /// SHA-256 of the invoked worker executable.
    pub executable_sha256: String,
    /// PID of the worker process.
    pub worker_pid: u32,
    /// Keccak-256 of [`Self::binding_payload_bytes`].
    pub binding_hash: String,
    /// Advisory, independently checked fork name.
    pub era: String,
    /// Decimal chain identifier.
    pub chain_id: String,
    /// Keccak-256 of the serialized genesis value.
    pub genesis_identity: String,
    /// Consensus genesis header hash.
    pub genesis_header_hash: String,
    /// Keccak-256 of the serialized genesis config.
    pub config_identity: String,
    /// Full chain genesis JSON.
    pub genesis: Value,
    /// Canonical parent header RLP.
    pub parent_header_rlp: String,
    /// Canonical child block RLP.
    pub child_block_rlp: String,
    /// Optional RPC operation. Absent means consensus block execution.
    #[serde(default)]
    pub operation: Option<Value>,
}

impl ExecuteRequest {
    /// Returns canonical binding bytes: compact JSON of this request in declaration order,
    /// excluding only `binding_hash`.
    pub fn binding_payload_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(&BindingPayload {
            version: self.version,
            request_id: &self.request_id,
            executable_sha256: &self.executable_sha256,
            worker_pid: self.worker_pid,
            era: &self.era,
            chain_id: &self.chain_id,
            genesis_identity: &self.genesis_identity,
            genesis_header_hash: &self.genesis_header_hash,
            config_identity: &self.config_identity,
            genesis: &self.genesis,
            parent_header_rlp: &self.parent_header_rlp,
            child_block_rlp: &self.child_block_rlp,
            operation: &self.operation,
        })
    }
}

/// Borrowed request fields covered by an [`ExecuteRequest`] binding hash.
#[derive(Serialize)]
pub struct BindingPayload<'a> {
    /// Wire protocol version.
    pub version: u16,
    /// Host-selected invocation identifier.
    pub request_id: &'a str,
    /// SHA-256 of the invoked worker executable.
    pub executable_sha256: &'a str,
    /// PID of the worker process.
    pub worker_pid: u32,
    /// Advisory fork name.
    pub era: &'a str,
    /// Decimal chain identifier.
    pub chain_id: &'a str,
    /// Keccak-256 of the serialized genesis value.
    pub genesis_identity: &'a str,
    /// Consensus genesis header hash.
    pub genesis_header_hash: &'a str,
    /// Keccak-256 of the serialized genesis config.
    pub config_identity: &'a str,
    /// Full chain genesis JSON.
    pub genesis: &'a Value,
    /// Canonical parent header RLP.
    pub parent_header_rlp: &'a str,
    /// Canonical child block RLP.
    pub child_block_rlp: &'a str,
    /// Optional RPC operation.
    pub operation: &'a Option<Value>,
}

/// A synchronous parent-state read requested by the worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReadRequest {
    /// Reads an account.
    Account {
        /// Invocation identifier.
        request_id: String,
        /// Monotonic read sequence.
        sequence: u32,
        /// Hex-encoded account address.
        address: String,
    },
    /// Reads bytecode by hash.
    Code {
        /// Invocation identifier.
        request_id: String,
        /// Monotonic read sequence.
        sequence: u32,
        /// Hex-encoded bytecode hash.
        code_hash: String,
    },
    /// Reads a storage word.
    Storage {
        /// Invocation identifier.
        request_id: String,
        /// Monotonic read sequence.
        sequence: u32,
        /// Hex-encoded account address.
        address: String,
        /// Hex-encoded storage key.
        key: String,
    },
    /// Reads a historical block hash.
    BlockHash {
        /// Invocation identifier.
        request_id: String,
        /// Monotonic read sequence.
        sequence: u32,
        /// Decimal block number.
        number: String,
    },
}

/// Host response to exactly one [`ReadRequest`].
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadResponse {
    /// Matching invocation identifier.
    pub request_id: String,
    /// Matching request sequence.
    pub sequence: u32,
    /// Typed JSON value, or `None` for an absent account.
    pub value: Option<Value>,
    /// Provider failure; mutually exclusive with `value`.
    pub error: Option<String>,
}

/// Before and after values for a changed account.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccountDelta {
    /// Hex-encoded account address.
    pub address: String,
    /// Account state before execution, if it existed.
    pub before: Option<AccountState>,
    /// Account state after execution, if it exists.
    pub after: Option<AccountState>,
    /// Whether execution cleared the account's storage.
    pub storage_wiped: bool,
    /// Changed storage words.
    pub storage: Vec<StorageDelta>,
}
/// Version-neutral account state.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccountState {
    /// Decimal account nonce.
    pub nonce: String,
    /// Decimal account balance.
    pub balance: String,
    /// Hex-encoded account bytecode.
    pub code: String,
}
/// Before and after values for a storage word.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StorageDelta {
    /// Hex-encoded storage key.
    pub key: String,
    /// Hex-encoded value before execution.
    pub before: String,
    /// Hex-encoded value after execution.
    pub after: String,
}
/// Version-neutral execution receipt.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Receipt {
    /// Zero-based transaction index.
    pub transaction_index: u32,
    /// Whether execution succeeded.
    pub success: bool,
    /// Decimal cumulative gas used.
    pub cumulative_gas_used: String,
    /// Hex-encoded logs bloom.
    pub logs_bloom: String,
    /// Hex-encoded canonical receipt RLP.
    pub canonical_rlp: String,
    /// Decimal deposit nonce, when applicable.
    pub deposit_nonce: Option<String>,
    /// Decimal deposit receipt version, when applicable.
    pub deposit_receipt_version: Option<String>,
}

/// Terminal worker result.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Outcome {
    /// Execution succeeded.
    Success {
        /// Invocation identifier.
        request_id: String,
        /// Changed accounts.
        accounts: Vec<AccountDelta>,
        /// Per-block reverted account changes.
        block_reversions: Vec<Vec<AccountDelta>>,
        /// Execution receipts.
        receipts: Vec<Receipt>,
        /// Decimal gas used.
        gas_used: String,
        /// Decimal blob gas used.
        blob_gas_used: String,
        /// Hex-encoded execution requests.
        requests: Vec<String>,
        /// Hex-encoded execution requests hash, when present.
        requests_hash: Option<String>,
    },
    /// Input is malformed or violates consensus.
    InvalidInput {
        /// Invocation identifier, when available.
        request_id: Option<String>,
        /// Failure description.
        error: String,
    },
    /// The worker does not support the requested protocol/fork.
    Unsupported {
        /// Invocation identifier.
        request_id: String,
        /// Failure description.
        error: String,
    },
    /// Transport, provider, or deployment failure.
    Infrastructure {
        /// Invocation identifier, when available.
        request_id: Option<String>,
        /// Failure description.
        error: String,
    },
}

/// Successful terminal RPC response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcSuccess {
    /// Invocation identifier.
    pub request_id: String,
    /// JSON-RPC result value.
    pub result: Value,
}

/// Failed terminal RPC response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcError {
    /// Invocation identifier.
    pub request_id: String,
    /// JSON-RPC error code.
    pub code: i32,
    /// JSON-RPC error message.
    pub message: String,
    /// Optional JSON-RPC error data.
    pub data: Option<Value>,
}
