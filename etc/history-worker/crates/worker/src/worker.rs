use std::{
    io::{self, Read, Write},
    sync::{Arc, Mutex},
};

use alloy_consensus::{Block, Header, TxReceipt};
use alloy_evm::{
    Evm,
    env::BlockEnvironment,
    overrides::{apply_block_overrides, apply_state_overrides},
};
use alloy_genesis::Genesis;
use alloy_primitives::{Address, B256, Bytes, KECCAK256_EMPTY, TxKind, U256, keccak256};
use alloy_rlp::Decodable;
use alloy_rpc_types_eth::{BlockOverrides, TransactionRequest, state::StateOverride};
use alloy_rpc_types_trace::geth::{
    GethDebugBuiltInTracerType, GethDebugTracerType, GethDebugTracingOptions,
};
use base_common_chains::Upgrades;
use base_common_consensus::{BaseReceipt, BaseTxEnvelope};
use base_common_evm::BaseTransaction;
use base_execution_chainspec::BaseChainSpec;
use base_execution_evm::BaseEvmConfig;
use base_history_protocol::{
    AccountDelta, AccountState, ExecuteRequest, Outcome, ReadRequest, ReadResponse, Receipt,
    RpcError, RpcSuccess, StorageDelta, VERSION,
};
use reth_evm::{
    ConfigureEvm,
    execute::{BlockExecutor, Executor},
};
use reth_primitives_traits::{RecoveredBlock, SealedBlock};
use revm::{
    Database, DatabaseCommit,
    bytecode::Bytecode,
    context::{TransactionType, TxEnv, result::ExecutionResult},
    database::{EmptyDB, State},
    state::AccountInfo,
};
use revm_inspectors::tracing::{DebugInspector, TransactionContext};
use sha2::{Digest, Sha256};

const MAX_FRAME: usize = 64 * 1024 * 1024;
/// Error returned by the worker's remote state database.
#[derive(Debug)]
pub struct DbError(pub io::Error);
impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for DbError {}
impl revm::database_interface::DBErrorMarker for DbError {}
impl From<io::Error> for DbError {
    fn from(value: io::Error) -> Self {
        Self(value)
    }
}

/// Entrypoint for one framed worker invocation.
pub struct Worker;

impl Worker {
    /// Reads one request, services execution reads, writes one outcome, and exits.
    pub fn run() {
        let outcome = read_frame()
            .and_then(|body| serde_json::from_slice(&body).map_err(invalid_data))
            .and_then(execute)
            .unwrap_or_else(|error| {
                serde_json::to_value(Outcome::Infrastructure {
                    request_id: None,
                    error: error.to_string(),
                })
                .expect("serializable outcome")
            });
        let _ = write_json(&outcome);
    }
}

/// Creates an invalid-data I/O error.
pub fn invalid_data(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
/// Decodes a `0x`-prefixed hexadecimal value.
pub fn decode_hex(value: &str) -> io::Result<Vec<u8>> {
    hex::decode(value.strip_prefix("0x").ok_or_else(|| invalid_data("hex value lacks 0x"))?)
        .map_err(invalid_data)
}
/// Reads one length-prefixed frame from standard input.
pub fn read_frame() -> io::Result<Vec<u8>> {
    let mut size = [0; 4];
    io::stdin().read_exact(&mut size)?;
    let n = u32::from_be_bytes(size) as usize;
    if n > MAX_FRAME {
        return Err(invalid_data("frame exceeds 64 MiB"));
    }
    let mut body = vec![0; n];
    io::stdin().read_exact(&mut body)?;
    Ok(body)
}
/// Writes one JSON-encoded, length-prefixed frame to standard output.
pub fn write_json<T: serde::Serialize>(value: &T) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME {
        return Err(invalid_data("outgoing frame exceeds 64 MiB"));
    }
    let mut out = io::stdout().lock();
    out.write_all(&(body.len() as u32).to_be_bytes())?;
    out.write_all(&body)?;
    out.flush()
}
/// Returns the SHA-256 identity of the running executable.
pub fn executable_identity() -> io::Result<String> {
    Ok(format!("0x{:x}", Sha256::digest(std::fs::read("/proc/self/exe")?)))
}

/// Executes a validated worker request.
pub fn execute(request: ExecuteRequest) -> io::Result<serde_json::Value> {
    let id = Some(request.request_id.clone());
    if request.version != VERSION {
        return terminal(Outcome::Unsupported {
            request_id: request.request_id,
            error: format!("protocol version {}", request.version),
        });
    }
    if request.worker_pid != std::process::id() {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: "worker pid mismatch".into(),
        });
    }
    if executable_identity()?.as_str() != request.executable_sha256 {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: "executable identity mismatch".into(),
        });
    }
    let binding = keccak256(request.binding_payload_bytes().map_err(invalid_data)?);
    if format!("{binding:#x}") != request.binding_hash {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: "request binding mismatch".into(),
        });
    }
    let genesis_bytes = serde_json::to_vec(&request.genesis).map_err(invalid_data)?;
    if format!("{:#x}", keccak256(&genesis_bytes)) != request.genesis_identity {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: "genesis identity mismatch".into(),
        });
    }
    let genesis: Genesis = serde_json::from_value(request.genesis.clone()).map_err(invalid_data)?;
    let spec = BaseChainSpec::try_from_genesis(genesis).map_err(invalid_data)?;
    if spec.chain().id().to_string() != request.chain_id {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: "chain id mismatch".into(),
        });
    }
    if format!("{:#x}", spec.genesis_hash()) != request.genesis_header_hash {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: "genesis header hash mismatch".into(),
        });
    }
    if format!(
        "{:#x}",
        keccak256(serde_json::to_vec(&request.genesis.get("config")).map_err(invalid_data)?)
    ) != request.config_identity
    {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: "config identity mismatch".into(),
        });
    }
    let mut parent_bytes = &decode_hex(&request.parent_header_rlp)?[..];
    let parent = Header::decode(&mut parent_bytes).map_err(invalid_data)?;
    if !parent_bytes.is_empty() {
        return terminal(Outcome::InvalidInput {
            request_id: id,
            error: "trailing parent RLP".into(),
        });
    }
    if let Some(operation) = &request.operation {
        let semantic_timestamp = if request.child_block_rlp.is_empty() {
            parent.timestamp
        } else {
            let bytes = decode_hex(&request.child_block_rlp)?;
            Block::<BaseTxEnvelope>::decode(&mut bytes.as_slice())
                .map_err(invalid_data)?
                .header
                .timestamp
        };
        if era(&spec, semantic_timestamp) != request.era {
            return terminal(Outcome::Infrastructure {
                request_id: id,
                error: "RPC semantic era mismatch".into(),
            });
        }
        return rpc(&request, &spec, &parent, operation);
    }
    let child_raw = decode_hex(&request.child_block_rlp)?;
    let mut child_bytes = &child_raw[..];
    let child = Block::<BaseTxEnvelope>::decode(&mut child_bytes).map_err(invalid_data)?;
    if !child_bytes.is_empty() {
        return terminal(Outcome::InvalidInput {
            request_id: id,
            error: "trailing block RLP".into(),
        });
    }
    let equal_timestamp_allowed = spec.is_denim_active_at_timestamp(child.header.timestamp)
        && spec.is_denim_active_at_timestamp(parent.timestamp);
    if child.header.parent_hash != parent.hash_slow()
        || child.header.number != parent.number + 1
        || child.header.timestamp < parent.timestamp
        || (child.header.timestamp == parent.timestamp && !equal_timestamp_allowed)
    {
        return terminal(Outcome::InvalidInput {
            request_id: id,
            error: "child/parent relationship mismatch".into(),
        });
    }
    let era = era(&spec, child.header.timestamp);
    if era != request.era.to_ascii_lowercase() {
        return terminal(Outcome::Infrastructure {
            request_id: id,
            error: format!("era mismatch: expected {era}"),
        });
    }
    let child_hash = child.header.hash_slow();
    let sealed = SealedBlock::new_unchecked(child, child_hash);
    let recovered = match RecoveredBlock::try_recover_sealed(sealed) {
        Ok(v) => v,
        Err(e) => return terminal(Outcome::InvalidInput { request_id: id, error: e.to_string() }),
    };
    let provider_error = Arc::new(Mutex::new(None));
    let mut database = RpcDatabase::new(request.request_id.clone(), provider_error.clone());
    let output =
        match BaseEvmConfig::base(Arc::new(spec)).executor(&mut database).execute(&recovered) {
            Ok(v) => v,
            Err(e) => {
                let provider_error =
                    provider_error.lock().expect("provider error mutex poisoned").take();
                return terminal(if let Some(error) = provider_error {
                    Outcome::Infrastructure { request_id: id, error }
                } else {
                    Outcome::InvalidInput { request_id: id, error: e.to_string() }
                });
            }
        };
    let accounts = convert_accounts(&output.state, &mut database)?;
    let receipts =
        output.result.receipts.iter().enumerate().map(|(i, r)| convert_receipt(i, r)).collect();
    let requests = output.result.requests.iter().map(|r| format!("0x{}", hex::encode(r))).collect();
    terminal(Outcome::Success {
        request_id: request.request_id,
        accounts,
        block_reversions: vec![],
        receipts,
        gas_used: output.result.gas_used.to_string(),
        blob_gas_used: output.result.blob_gas_used.to_string(),
        requests,
        requests_hash: recovered.header().requests_hash.map(|h| format!("{h:#x}")),
    })
}

/// Serializes a terminal protocol value.
pub fn terminal(value: impl serde::Serialize) -> io::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(invalid_data)
}

/// Executes a supported JSON-RPC operation.
pub fn rpc(
    request: &ExecuteRequest,
    spec: &BaseChainSpec,
    header: &Header,
    operation: &serde_json::Value,
) -> io::Result<serde_json::Value> {
    let method = operation
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid_data("RPC operation method must be a string"))?;
    let params = operation
        .get("params")
        .and_then(|v| v.as_array())
        .ok_or_else(|| invalid_data("RPC operation params must be an array"))?;
    if matches!(method, "debug_traceCall" | "debug_traceBlock" | "debug_traceTransaction") {
        return debug_trace(request, spec, header, method, params);
    }
    if !matches!(method, "eth_call" | "eth_estimateGas") {
        return terminal(RpcError {
            request_id: request.request_id.clone(),
            code: -32601,
            message: format!("historical worker does not support {method}"),
            data: None,
        });
    }
    if params.is_empty() || params.len() > 3 {
        return terminal(RpcError {
            request_id: request.request_id.clone(),
            code: -32602,
            message: "expected call, optional state override, and optional block override".into(),
            data: None,
        });
    }
    let call: TransactionRequest =
        serde_json::from_value(params[0].clone()).map_err(invalid_data)?;
    if call.max_fee_per_blob_gas.is_some() || call.authorization_list.is_some() {
        return terminal(RpcError {
            request_id: request.request_id.clone(),
            code: -32602,
            message: "blob and authorization-list calls are not supported".into(),
            data: None,
        });
    }
    let state_override: Option<StateOverride> = params
        .get(1)
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(invalid_data)?;
    let block_override: Option<BlockOverrides> = params
        .get(2)
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(invalid_data)?;
    let mut tx = TxEnv::default();
    tx.caller = call.from.unwrap_or_default();
    tx.kind = call.to.unwrap_or(TxKind::Create);
    tx.gas_limit = call.gas.unwrap_or(header.gas_limit);
    tx.gas_price = call.max_fee_per_gas.or(call.gas_price).unwrap_or_default();
    tx.gas_priority_fee = call.max_priority_fee_per_gas;
    tx.value = call.value.unwrap_or_default();
    tx.data = call.input.input().cloned().unwrap_or_default();
    tx.nonce = 0;
    tx.chain_id = Some(call.chain_id.unwrap_or(spec.chain().id()));
    tx.access_list = call.access_list.unwrap_or_default().into();
    tx.tx_type = u8::from(if tx.gas_priority_fee.is_some() {
        TransactionType::Eip1559
    } else if !tx.access_list.is_empty() {
        TransactionType::Eip2930
    } else {
        TransactionType::Legacy
    });
    let provider_error = Arc::new(Mutex::new(None));
    let mut database = State::builder()
        .with_database(RpcDatabase::new(request.request_id.clone(), provider_error.clone()))
        .build();
    let config = BaseEvmConfig::base(Arc::new(spec.clone()));
    let mut env = config.evm_env(header).map_err(invalid_data)?;
    env.cfg_env.disable_base_fee = true;
    env.cfg_env.disable_balance_check = true;
    env.cfg_env.disable_eip3607 = true;
    env.cfg_env.disable_fee_charge = true;
    env.cfg_env.disable_nonce_check = true;
    if method == "eth_call" {
        let cap =
            operation.get("call_gas_limit").and_then(|v| v.as_u64()).unwrap_or(header.gas_limit);
        tx.gas_limit = call.gas.unwrap_or(cap);
        if cap != 0 {
            tx.gas_limit = tx.gas_limit.min(cap);
        }
        env.cfg_env.disable_block_gas_limit = true;
        env.cfg_env.tx_gas_limit_cap = Some(u64::MAX);
    }
    if let Some(overrides) = block_override {
        apply_block_overrides(overrides, &mut database, env.block_env.inner_mut());
    }
    if let Some(overrides) = state_override
        && let Err(error) = apply_state_overrides(overrides, &mut database)
    {
        return terminal(RpcError {
            request_id: request.request_id.clone(),
            code: -32602,
            message: error.to_string(),
            data: None,
        });
    }
    let is_basic_transfer = if tx.data.is_empty()
        && let TxKind::Call(to) = tx.kind
    {
        match database.basic(to) {
            Ok(Some(account)) => account.code_hash == KECCAK256_EMPTY,
            Ok(None) => true,
            Err(error) => {
                let provider = provider_error.lock().expect("provider error mutex poisoned").take();
                return rpc_error(
                    &request.request_id,
                    -32000,
                    provider.unwrap_or_else(|| error.to_string()),
                );
            }
        }
    } else {
        false
    };
    let mut maximum = if method == "eth_call" {
        tx.gas_limit
    } else {
        tx.gas_limit.min(env.block_env.inner_mut().gas_limit)
    };
    if tx.gas_price > 0 {
        match database.basic(tx.caller) {
            Ok(Some(account)) => {
                let allowance = account
                    .balance
                    .saturating_sub(tx.value)
                    .checked_div(U256::from(tx.gas_price))
                    .unwrap_or_default();
                maximum = maximum.min(u64::try_from(allowance).unwrap_or(u64::MAX));
            }
            Ok(None) => maximum = 0,
            Err(error) => {
                let provider = provider_error.lock().expect("provider error mutex poisoned").take();
                return rpc_error(
                    &request.request_id,
                    -32000,
                    provider.unwrap_or_else(|| error.to_string()),
                );
            }
        }
    }
    let execute_tx = |gas_limit: u64, database: &mut State<RpcDatabase>| {
        let mut candidate = tx.clone();
        candidate.gas_limit = gas_limit;
        let mut transaction = BaseTransaction::new(candidate);
        transaction.enveloped_tx = Some(Bytes::new());
        config.evm_with_env(database, env.clone()).transact(transaction)
    };
    let result = execute_tx(maximum, &mut database);
    match result {
        Ok(result) if result.result.is_success() => {
            let value = if method == "eth_estimateGas" {
                const MIN_TRANSACTION_GAS: u64 = 21_000;
                const CALL_STIPEND_GAS: u64 = 2_300;
                const ESTIMATE_GAS_ERROR_RATIO: f64 = 0.015;

                match is_basic_transfer.then(|| execute_tx(MIN_TRANSACTION_GAS, &mut database)) {
                    None => {}
                    Some(Ok(candidate)) if candidate.result.is_success() => {
                        return terminal(RpcSuccess {
                            request_id: request.request_id.clone(),
                            result: serde_json::json!(format!("{MIN_TRANSACTION_GAS:#x}")),
                        });
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        let provider =
                            provider_error.lock().expect("provider error mutex poisoned").take();
                        if let Some(provider) = provider {
                            return rpc_error(&request.request_id, -32000, provider);
                        }
                        if maximum <= MIN_TRANSACTION_GAS {
                            return rpc_error(&request.request_id, -32000, error.to_string());
                        }
                    }
                }

                let gas_refund = match &result.result {
                    ExecutionResult::Success { gas, .. } => gas.final_refunded(),
                    _ => unreachable!("successful result checked above"),
                };
                let mut gas_used = result.result.tx_gas_used();
                let mut low = gas_used.saturating_sub(1);
                let mut high = maximum;

                let optimistic = (gas_used + gas_refund + CALL_STIPEND_GAS) * 64 / 63;
                if optimistic < high {
                    match execute_tx(optimistic, &mut database) {
                        Ok(candidate) => {
                            gas_used = candidate.result.tx_gas_used();
                            if candidate.result.is_success() {
                                high = optimistic;
                            } else {
                                low = optimistic;
                            }
                        }
                        Err(error) => {
                            let provider = provider_error
                                .lock()
                                .expect("provider error mutex poisoned")
                                .take();
                            return rpc_error(
                                &request.request_id,
                                -32000,
                                provider.unwrap_or_else(|| error.to_string()),
                            );
                        }
                    }
                }

                let mut middle = (gas_used * 3).min(((high as u128 + low as u128) / 2) as u64);
                while low + 1 < high {
                    if (high - low) as f64 / (high as f64) < ESTIMATE_GAS_ERROR_RATIO {
                        break;
                    }
                    match execute_tx(middle, &mut database) {
                        Ok(candidate) if candidate.result.is_success() => high = middle,
                        Ok(_) => low = middle,
                        Err(error) => {
                            let provider = provider_error
                                .lock()
                                .expect("provider error mutex poisoned")
                                .take();
                            return rpc_error(
                                &request.request_id,
                                -32000,
                                provider.unwrap_or_else(|| error.to_string()),
                            );
                        }
                    }
                    middle = ((high as u128 + low as u128) / 2) as u64;
                }
                serde_json::json!(format!("{high:#x}"))
            } else {
                serde_json::json!(format!(
                    "0x{}",
                    hex::encode(result.result.output().unwrap_or_default())
                ))
            };
            terminal(RpcSuccess { request_id: request.request_id.clone(), result: value })
        }
        Ok(result) => terminal(RpcError {
            request_id: request.request_id.clone(),
            code: 3,
            message: "execution reverted".into(),
            data: result
                .result
                .output()
                .map(|v| serde_json::json!(format!("0x{}", hex::encode(v)))),
        }),
        Err(error) => {
            if let Some(error) =
                provider_error.lock().expect("provider error mutex poisoned").take()
            {
                terminal(RpcError {
                    request_id: request.request_id.clone(),
                    code: -32000,
                    message: error,
                    data: None,
                })
            } else {
                terminal(RpcError {
                    request_id: request.request_id.clone(),
                    code: -32000,
                    message: error.to_string(),
                    data: None,
                })
            }
        }
    }
}

/// Builds a terminal JSON-RPC error response.
pub fn rpc_error(
    request_id: &str,
    code: i32,
    message: impl Into<String>,
) -> io::Result<serde_json::Value> {
    terminal(RpcError { request_id: request_id.into(), code, message: message.into(), data: None })
}

/// Parses and validates debug tracing options.
pub fn tracing_options(value: Option<&serde_json::Value>) -> io::Result<GethDebugTracingOptions> {
    let options: GethDebugTracingOptions = value
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(invalid_data)?
        .unwrap_or_default();
    if let Some(tracer) = &options.tracer
        && !matches!(
            tracer,
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer)
        )
    {
        return Err(invalid_data(format!("unsupported tracer {}", tracer.as_str())));
    }
    Ok(options)
}

/// Executes a supported debug trace operation.
pub fn debug_trace(
    request: &ExecuteRequest,
    spec: &BaseChainSpec,
    state_header: &Header,
    method: &str,
    params: &[serde_json::Value],
) -> io::Result<serde_json::Value> {
    let expected = if method == "debug_traceCall" { 1..=2 } else { 0..=2 };
    if !expected.contains(&params.len()) {
        return rpc_error(&request.request_id, -32602, "invalid debug trace parameters");
    }
    let options_value = if method == "debug_traceCall" { params.get(1) } else { params.last() };
    let options = match tracing_options(options_value) {
        Ok(options) => options,
        Err(error) => return rpc_error(&request.request_id, -32602, error.to_string()),
    };
    if options_value.and_then(|value| value.get("txIndex")).is_some() {
        return rpc_error(&request.request_id, -32602, "txIndex is not supported");
    }
    let provider_error = Arc::new(Mutex::new(None));
    let mut database = State::builder()
        .with_database(RpcDatabase::new(request.request_id.clone(), provider_error.clone()))
        .build();
    let config = BaseEvmConfig::base(Arc::new(spec.clone()));

    let result = if method == "debug_traceCall" {
        let call: TransactionRequest =
            serde_json::from_value(params[0].clone()).map_err(invalid_data)?;
        let mut option_object =
            options_value.and_then(|value| value.as_object()).cloned().unwrap_or_default();
        let state_override: Option<StateOverride> = option_object
            .remove("stateOverrides")
            .map(serde_json::from_value)
            .transpose()
            .map_err(invalid_data)?;
        let block_override: Option<BlockOverrides> = option_object
            .remove("blockOverrides")
            .map(serde_json::from_value)
            .transpose()
            .map_err(invalid_data)?;
        if let Some(overrides) = state_override {
            apply_state_overrides(overrides, &mut database).map_err(invalid_data)?;
        }
        let mut env = config.evm_env(state_header).map_err(invalid_data)?;
        env.cfg_env.disable_base_fee = true;
        env.cfg_env.disable_balance_check = true;
        env.cfg_env.disable_eip3607 = true;
        env.cfg_env.disable_fee_charge = true;
        env.cfg_env.disable_nonce_check = true;
        if let Some(overrides) = block_override {
            apply_block_overrides(overrides, &mut database, env.block_env.inner_mut());
        }
        let mut tx = call_tx(&call, spec, state_header);
        let cap = request
            .operation
            .as_ref()
            .and_then(|op| op.get("call_gas_limit"))
            .and_then(|v| v.as_u64())
            .unwrap_or(state_header.gas_limit);
        tx.gas_limit = call.gas.unwrap_or(cap);
        if cap != 0 {
            tx.gas_limit = tx.gas_limit.min(cap);
        }
        env.cfg_env.disable_block_gas_limit = true;
        env.cfg_env.tx_gas_limit_cap = Some(u64::MAX);
        let mut transaction = BaseTransaction::new(tx.clone());
        transaction.enveloped_tx = Some(Bytes::new());
        let mut inspector = DebugInspector::new(options).map_err(invalid_data)?;
        let mut evm = config.evm_with_env_and_inspector(&mut database, env.clone(), &mut inspector);
        let execution = evm.transact(transaction.clone()).map_err(invalid_data)?;
        drop(evm);
        let mut trace_database = EmptyDB::default();
        inspector
            .get_result(None, &tx, &env.block_env, &execution, &mut trace_database)
            .map(|trace| serde_json::to_value(trace).expect("trace is serializable"))
            .map_err(invalid_data)
    } else {
        let child_raw = decode_hex(&request.child_block_rlp)?;
        let mut child_bytes = &child_raw[..];
        let child = Block::<BaseTxEnvelope>::decode(&mut child_bytes).map_err(invalid_data)?;
        if !child_bytes.is_empty() || child.header.parent_hash != state_header.hash_slow() {
            return rpc_error(&request.request_id, -32602, "target block does not bind its parent");
        }
        let child_hash = child.header.hash_slow();
        let recovered =
            RecoveredBlock::try_recover_sealed(SealedBlock::new_unchecked(child, child_hash))
                .map_err(invalid_data)?;
        let target = if method == "debug_traceTransaction" {
            let Some(index) = params.first().and_then(parse_index) else {
                return rpc_error(
                    &request.request_id,
                    -32602,
                    "transaction index must be a quantity",
                );
            };
            if index >= recovered.transactions_recovered().count() {
                return rpc_error(&request.request_id, -32602, "transaction index out of bounds");
            }
            Some(index)
        } else {
            None
        };
        let mut executor = config
            .executor_for_block(&mut database, recovered.sealed_block())
            .map_err(invalid_data)?;
        executor.apply_pre_execution_changes().map_err(invalid_data)?;
        drop(executor);
        let env = config.evm_env(recovered.header()).map_err(invalid_data)?;
        let mut traces = Vec::new();
        for (index, transaction) in recovered.transactions_recovered().enumerate() {
            let tx_env = config.tx_env(transaction);
            let mut inspector = DebugInspector::new(options.clone()).map_err(invalid_data)?;
            let mut evm =
                config.evm_with_env_and_inspector(&mut database, env.clone(), &mut inspector);
            let execution = evm.transact(tx_env.clone()).map_err(invalid_data)?;
            drop(evm);
            if target.is_none() || target == Some(index) {
                let context = TransactionContext {
                    block_hash: Some(child_hash),
                    tx_hash: Some(B256::from(*transaction.tx_hash())),
                    tx_index: Some(index),
                };
                let mut trace_database = EmptyDB::default();
                let trace = inspector
                    .get_result(
                        Some(context),
                        &tx_env,
                        &env.block_env,
                        &execution,
                        &mut trace_database,
                    )
                    .map_err(invalid_data)?;
                let trace = serde_json::to_value(trace).expect("trace is serializable");
                traces.push(if target.is_some() {
                    trace
                } else {
                    serde_json::json!({ "result": trace, "txHash": context.tx_hash })
                });
            }
            database.commit(execution.state);
            if target == Some(index) {
                break;
            }
        }
        Ok(if target.is_some() {
            traces.pop().expect("target index checked")
        } else {
            serde_json::Value::Array(traces)
        })
    };
    match result {
        Ok(value) => terminal(RpcSuccess { request_id: request.request_id.clone(), result: value }),
        Err(error) => {
            let provider = provider_error.lock().expect("provider error mutex poisoned").take();
            rpc_error(&request.request_id, -32000, provider.unwrap_or_else(|| error.to_string()))
        }
    }
}

/// Parses a transaction index from a JSON quantity.
pub fn parse_index(value: &serde_json::Value) -> Option<usize> {
    value.as_u64().map(|value| value as usize).or_else(|| {
        value.as_str().and_then(|value| {
            usize::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16).ok()
        })
    })
}

/// Converts an RPC transaction request into an EVM transaction environment.
pub fn call_tx(call: &TransactionRequest, spec: &BaseChainSpec, header: &Header) -> TxEnv {
    let mut tx = TxEnv::default();
    tx.caller = call.from.unwrap_or_default();
    tx.kind = call.to.unwrap_or(TxKind::Create);
    tx.gas_limit = call.gas.unwrap_or(header.gas_limit);
    tx.gas_price = call.max_fee_per_gas.or(call.gas_price).unwrap_or_default();
    tx.gas_priority_fee = call.max_priority_fee_per_gas;
    tx.value = call.value.unwrap_or_default();
    tx.data = call.input.input().cloned().unwrap_or_default();
    tx.nonce = 0;
    tx.chain_id = Some(call.chain_id.unwrap_or(spec.chain().id()));
    tx.access_list = call.access_list.clone().unwrap_or_default().into();
    tx.tx_type = u8::from(if tx.gas_priority_fee.is_some() {
        TransactionType::Eip1559
    } else if !tx.access_list.is_empty() {
        TransactionType::Eip2930
    } else {
        TransactionType::Legacy
    });
    tx
}

/// Returns the active Base fork name at a timestamp.
pub fn era(s: &BaseChainSpec, t: u64) -> String {
    for (name, on) in [
        ("zenith", s.is_zenith_active_at_timestamp(t)),
        ("denim", s.is_denim_active_at_timestamp(t)),
        ("cobalt", s.is_cobalt_active_at_timestamp(t)),
        ("beryl", s.is_beryl_active_at_timestamp(t)),
        ("azul", s.is_azul_active_at_timestamp(t)),
        ("jovian", s.is_jovian_active_at_timestamp(t)),
        ("isthmus", s.is_isthmus_active_at_timestamp(t)),
        ("holocene", s.is_holocene_active_at_timestamp(t)),
        ("granite", s.is_granite_active_at_timestamp(t)),
        ("fjord", s.is_fjord_active_at_timestamp(t)),
        ("ecotone", s.is_ecotone_active_at_timestamp(t)),
        ("canyon", s.is_canyon_active_at_timestamp(t)),
        ("regolith", s.is_regolith_active_at_timestamp(t)),
    ] {
        if on {
            return name.into();
        }
    }
    "bedrock".into()
}
/// Converts an EVM account into its protocol representation.
pub fn account(
    info: &AccountInfo,
    state: &revm::database::states::BundleState,
    database: &mut RpcDatabase,
) -> io::Result<AccountState> {
    let code = if let Some(c) = &info.code {
        c.original_bytes()
    } else if info.code_hash == keccak256([]) {
        Bytes::new()
    } else if let Some(code) = state.bytecode(&info.code_hash) {
        code.original_bytes()
    } else {
        database.code_by_hash(info.code_hash).map_err(invalid_data)?.original_bytes()
    };
    Ok(AccountState {
        nonce: info.nonce.to_string(),
        balance: info.balance.to_string(),
        code: format!("0x{}", hex::encode(code)),
    })
}
/// Converts changed EVM accounts into protocol deltas.
pub fn convert_accounts(
    state: &revm::database::states::BundleState,
    database: &mut RpcDatabase,
) -> io::Result<Vec<AccountDelta>> {
    let mut out = Vec::new();
    for (address, a) in &state.state {
        if a.status.is_not_modified() {
            continue;
        }
        let mut storage = Vec::new();
        for (key, v) in &a.storage {
            if v.is_changed() || (a.status.was_destroyed() && !v.present_value.is_zero()) {
                storage.push(StorageDelta {
                    key: format!("{key:#066x}"),
                    before: format!(
                        "{:#066x}",
                        database.storage(*address, *key).map_err(invalid_data)?
                    ),
                    after: format!("{:#066x}", v.present_value),
                })
            }
        }
        out.push(AccountDelta {
            address: format!("{address:#x}"),
            before: a.original_info.as_ref().map(|v| account(v, state, database)).transpose()?,
            after: a.info.as_ref().map(|v| account(v, state, database)).transpose()?,
            storage_wiped: a.status.was_destroyed(),
            storage,
        })
    }
    Ok(out)
}
/// Converts an execution receipt into its protocol representation.
pub fn convert_receipt(i: usize, r: &BaseReceipt) -> Receipt {
    let inner = r.as_receipt();
    let (nonce, version) = match r {
        BaseReceipt::Deposit(d) => (
            d.deposit_nonce.map(|v| v.to_string()),
            d.deposit_receipt_version.map(|v| v.to_string()),
        ),
        _ => (None, None),
    };
    Receipt {
        transaction_index: i as u32,
        success: inner.status.coerce_status(),
        cumulative_gas_used: inner.cumulative_gas_used.to_string(),
        logs_bloom: format!("{:#x}", r.bloom()),
        canonical_rlp: format!("0x{}", hex::encode(alloy_rlp::encode(r))),
        deposit_nonce: nonce,
        deposit_receipt_version: version,
    }
}

#[derive(Debug)]
/// Remote state database backed by framed parent-process reads.
pub struct RpcDatabase {
    request_id: String,
    sequence: u32,
    provider_error: Arc<Mutex<Option<String>>>,
}
impl RpcDatabase {
    /// Creates a database bound to one invocation.
    pub fn new(request_id: String, provider_error: Arc<Mutex<Option<String>>>) -> Self {
        Self { request_id, sequence: 0, provider_error }
    }
    /// Sends one state read and validates its response binding.
    pub fn ask(&mut self, request: ReadRequest) -> io::Result<Option<serde_json::Value>> {
        write_json(&request)?;
        let body = read_frame()?;
        let response: ReadResponse = serde_json::from_slice(&body).map_err(invalid_data)?;
        if response.request_id != self.request_id || response.sequence != self.sequence {
            return Err(invalid_data("read response binding/sequence mismatch"));
        }
        if response.error.is_some() && response.value.is_some() {
            return Err(invalid_data("read response cannot contain both value and error"));
        }
        if let Some(error) = response.error {
            *self.provider_error.lock().expect("provider error mutex poisoned") =
                Some(error.clone());
            return Err(io::Error::other(error));
        }
        Ok(response.value)
    }
    /// Advances and returns the request sequence.
    pub fn next(&mut self) -> u32 {
        self.sequence += 1;
        self.sequence
    }
}
impl Database for RpcDatabase {
    type Error = DbError;
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let sequence = self.next();
        let req = ReadRequest::Account {
            request_id: self.request_id.clone(),
            sequence,
            address: format!("{address:#x}"),
        };
        let Some(v) = self.ask(req)? else { return Ok(None) };
        let a: AccountState = serde_json::from_value(v).map_err(invalid_data)?;
        let code = Bytecode::new_raw(Bytes::from(decode_hex(&a.code)?));
        Ok(Some(AccountInfo {
            balance: a.balance.parse().map_err(invalid_data)?,
            nonce: a.nonce.parse().map_err(invalid_data)?,
            code_hash: code.hash_slow(),
            code: Some(code),
            ..Default::default()
        }))
    }
    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        let sequence = self.next();
        let id = self.request_id.clone();
        let v = self
            .ask(ReadRequest::Code {
                request_id: id,
                sequence,
                code_hash: format!("{code_hash:#x}"),
            })?
            .ok_or_else(|| invalid_data("missing code"))?;
        let s = v.as_str().ok_or_else(|| invalid_data("code response is not hex"))?;
        let code = Bytecode::new_raw(Bytes::from(decode_hex(s)?));
        if code.hash_slow() != code_hash {
            return Err(invalid_data("code response does not match requested hash").into());
        }
        Ok(code)
    }
    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let sequence = self.next();
        let id = self.request_id.clone();
        let v = self
            .ask(ReadRequest::Storage {
                request_id: id,
                sequence,
                address: format!("{address:#x}"),
                key: format!("{index:#066x}"),
            })?
            .ok_or_else(|| invalid_data("missing storage"))?;
        Ok(v.as_str().ok_or_else(|| invalid_data("storage response is not hex")).and_then(|s| {
            U256::from_str_radix(s.trim_start_matches("0x"), 16).map_err(invalid_data)
        })?)
    }
    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        let sequence = self.next();
        let id = self.request_id.clone();
        let v = self
            .ask(ReadRequest::BlockHash { request_id: id, sequence, number: number.to_string() })?
            .ok_or_else(|| invalid_data("missing block hash"))?;
        Ok(v.as_str()
            .ok_or_else(|| invalid_data("block hash response is not hex"))
            .and_then(|s| s.parse().map_err(invalid_data))?)
    }
}
