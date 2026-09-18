//! End-to-end tests for the framed history worker protocol.

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    process::{Command, Stdio},
};

use alloy_consensus::{Block, Header, Sealable};
use alloy_genesis::{Genesis, GenesisAccount};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, address, keccak256};
use base_common_consensus::{BaseTxEnvelope, TxDeposit};
use base_execution_chainspec::BaseChainSpec;
use base_history_protocol::{
    AccountState, ExecuteRequest, Outcome, ReadRequest, ReadResponse, VERSION,
};
use sha2::{Digest, Sha256};

const SENDER: Address = address!("0x1111111111111111111111111111111111111111");
const RECIPIENT: Address = address!("0x2222222222222222222222222222222222222222");
const CONTRACT: Address = address!("0x3333333333333333333333333333333333333333");
const CREATE2_DEPLOYER: Address = address!("0x13b0D85CcB8bf860b6b79AF3029fCA081AE9beF2");
const CREATE2_DEPLOYER_CODEHASH: B256 =
    alloy_primitives::b256!("0xb0550b5b431e30d38000efb7107aaa0ade03d48a7198a140edda9d27134468b2");

#[derive(Default)]
struct ParentData {
    accounts: BTreeMap<Address, AccountState>,
    storage: BTreeMap<(Address, U256), U256>,
}

fn invoke(body: &[u8]) -> Outcome {
    let mut child = Command::new(env!("CARGO_BIN_EXE_base-history-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&(body.len() as u32).to_be_bytes()).unwrap();
    stdin.write_all(body).unwrap();
    drop(stdin);
    let mut stdout = child.stdout.take().unwrap();
    let mut size = [0; 4];
    stdout.read_exact(&mut size).unwrap();
    let mut result = vec![0; u32::from_be_bytes(size) as usize];
    stdout.read_exact(&mut result).unwrap();
    assert!(child.wait().unwrap().success());
    serde_json::from_slice(&result).unwrap()
}

fn write_json_frame(stdin: &mut impl Write, value: &impl serde::Serialize) {
    let body = serde_json::to_vec(value).unwrap();
    stdin.write_all(&(body.len() as u32).to_be_bytes()).unwrap();
    stdin.write_all(&body).unwrap();
    stdin.flush().unwrap();
}

fn read_json_frame(stdout: &mut impl Read) -> serde_json::Value {
    let mut size = [0; 4];
    stdout.read_exact(&mut size).unwrap();
    let mut body = vec![0; u32::from_be_bytes(size) as usize];
    stdout.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn genesis() -> Genesis {
    let mut alloc = BTreeMap::new();
    alloc.insert(
        SENDER,
        GenesisAccount::default().with_balance(U256::from(10_000)).with_nonce(Some(3)),
    );
    alloc.insert(RECIPIENT, GenesisAccount::default().with_balance(U256::from(700)));
    serde_json::from_value(serde_json::json!({
        "config": {
            "chainId": 901,
            "homesteadBlock": 0,
            "eip150Block": 0,
            "eip155Block": 0,
            "eip158Block": 0,
            "byzantiumBlock": 0,
            "constantinopleBlock": 0,
            "petersburgBlock": 0,
            "istanbulBlock": 0,
            "muirGlacierBlock": 0,
            "berlinBlock": 0,
            "londonBlock": 0,
            "arrowGlacierBlock": 0,
            "grayGlacierBlock": 0,
            "mergeNetsplitBlock": 0,
            "terminalTotalDifficulty": "0x0",
            "terminalTotalDifficultyPassed": true,
            "bedrockBlock": 0,
            "regolithTime": 0,
            "canyonTime": 10,
            "shanghaiTime": 10,
            "optimism": { "eip1559Elasticity": 6, "eip1559Denominator": 50 }
        },
        "gasLimit": "0x1c9c380",
        "difficulty": "0x0",
        "alloc": alloc
    }))
    .unwrap()
}

fn request(timestamp: u64, era: &str) -> ExecuteRequest {
    let deposit = BaseTxEnvelope::Deposit(
        TxDeposit {
            source_hash: B256::repeat_byte(0x44),
            from: SENDER,
            to: TxKind::Call(RECIPIENT),
            mint: 1_000,
            value: U256::from(200),
            gas_limit: 100_000,
            input: Bytes::new(),
            is_system_transaction: false,
        }
        .seal_slow(),
    );
    request_with_transactions(timestamp, era, [deposit])
}

fn request_with_transactions(
    timestamp: u64,
    era: &str,
    transactions: impl IntoIterator<Item = BaseTxEnvelope>,
) -> ExecuteRequest {
    let genesis = genesis();
    let genesis_value = serde_json::to_value(&genesis).unwrap();
    let spec = BaseChainSpec::try_from_genesis(genesis).unwrap();
    let parent = Header {
        number: 1,
        timestamp: timestamp - 1,
        gas_limit: 30_000_000,
        base_fee_per_gas: Some(1),
        ..Default::default()
    };
    let child = Block::from_transactions(
        Header {
            parent_hash: parent.hash_slow(),
            number: 2,
            timestamp,
            gas_limit: 30_000_000,
            base_fee_per_gas: Some(1),
            ..Default::default()
        },
        transactions,
    );
    let executable = fs::read(env!("CARGO_BIN_EXE_base-history-worker")).unwrap();
    ExecuteRequest {
        version: VERSION,
        request_id: format!("deposit-{timestamp}"),
        executable_sha256: format!("0x{:x}", Sha256::digest(executable)),
        worker_pid: 0,
        binding_hash: String::new(),
        era: era.into(),
        chain_id: "901".into(),
        genesis_identity: format!("{:#x}", keccak256(serde_json::to_vec(&genesis_value).unwrap())),
        genesis_header_hash: format!("{:#x}", spec.genesis_hash()),
        config_identity: format!(
            "{:#x}",
            keccak256(serde_json::to_vec(&genesis_value.get("config")).unwrap())
        ),
        genesis: genesis_value,
        parent_header_rlp: format!("0x{}", hex::encode(alloy_rlp::encode(parent))),
        child_block_rlp: format!("0x{}", hex::encode(alloy_rlp::encode(child))),
        operation: None,
    }
}

fn deposit(to: TxKind, value: u64, input: &str) -> BaseTxEnvelope {
    BaseTxEnvelope::Deposit(
        TxDeposit {
            source_hash: keccak256(input),
            from: SENDER,
            to,
            mint: 1_000,
            value: U256::from(value),
            gas_limit: 500_000,
            input: Bytes::from(hex::decode(input).unwrap()),
            is_system_transaction: false,
        }
        .seal_slow(),
    )
}

fn execute_value(request: ExecuteRequest, provider_error: bool) -> serde_json::Value {
    execute_value_with_failure(request, provider_error.then_some(1))
}

fn execute_value_with_failure(
    request: ExecuteRequest,
    provider_failure_at: Option<u32>,
) -> serde_json::Value {
    let mut parent = ParentData::default();
    parent.accounts.insert(
        SENDER,
        AccountState { nonce: "3".into(), balance: "10000".into(), code: "0x".into() },
    );
    parent.accounts.insert(
        RECIPIENT,
        AccountState { nonce: "0".into(), balance: "700".into(), code: "0x".into() },
    );
    execute_value_with_parent(request, provider_failure_at, &parent)
}

fn execute_value_with_parent(
    mut request: ExecuteRequest,
    provider_failure_at: Option<u32>,
    parent: &ParentData,
) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_base-history-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    request.worker_pid = child.id();
    request.binding_hash = format!("{:#x}", keccak256(request.binding_payload_bytes().unwrap()));
    let expected_request_id = request.request_id.clone();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    write_json_frame(&mut stdin, &request);
    let mut expected_sequence = 1;
    let outcome = loop {
        let frame = read_json_frame(&mut stdout);
        if frame.get("outcome").is_some()
            || frame.get("result").is_some()
            || frame.get("code").is_some()
        {
            break frame;
        }
        let read: ReadRequest = serde_json::from_value(frame).unwrap();
        let (request_id, sequence, value) = match read {
            ReadRequest::Account { request_id, sequence, address } => {
                let address: Address = address.parse().unwrap();
                let value = parent
                    .accounts
                    .get(&address)
                    .map(|account| serde_json::to_value(account).unwrap());
                (request_id, sequence, value)
            }
            ReadRequest::Storage { request_id, sequence, address, key } => {
                let address: Address = address.parse().unwrap();
                let key: U256 = key.parse().unwrap();
                let value = parent.storage.get(&(address, key)).copied().unwrap_or_default();
                (request_id, sequence, Some(serde_json::json!(format!("{value:#066x}"))))
            }
            ReadRequest::Code { request_id, sequence, .. } => {
                (request_id, sequence, Some(serde_json::json!("0x")))
            }
            ReadRequest::BlockHash { request_id, sequence, .. } => {
                (request_id, sequence, Some(serde_json::json!(format!("{:#x}", B256::ZERO))))
            }
        };
        assert_eq!(request_id, expected_request_id);
        assert_eq!(sequence, expected_sequence);
        expected_sequence += 1;
        write_json_frame(
            &mut stdin,
            &ReadResponse {
                request_id,
                sequence,
                value: (provider_failure_at.is_none_or(|at| sequence < at))
                    .then_some(value)
                    .flatten(),
                error: provider_failure_at
                    .filter(|at| sequence >= *at)
                    .map(|_| "injected provider failure".into()),
            },
        );
    };
    drop(stdin);
    assert!(child.wait().unwrap().success());
    outcome
}

fn execute_fixture(request: ExecuteRequest, provider_error: bool) -> Outcome {
    serde_json::from_value(execute_value(request, provider_error)).unwrap()
}

fn rpc_request(method: &str, params: serde_json::Value) -> ExecuteRequest {
    let mut request = request(9, "regolith");
    request.request_id = format!("rpc-{method}");
    request.operation = Some(serde_json::json!({ "method": method, "params": params }));
    // Calls use the selected post-state header without a synthetic child block.
    request.child_block_rlp.clear();
    request
}

fn assert_deposit(outcome: Outcome, canyon: bool) {
    let Outcome::Success { accounts, receipts, gas_used, .. } = outcome else {
        panic!("expected successful execution")
    };
    assert_ne!(gas_used, "0", "the fixture must execute its deposit");
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].success);
    assert_eq!(receipts[0].deposit_nonce.as_deref(), Some("3"));
    assert_eq!(receipts[0].deposit_receipt_version.as_deref(), canyon.then_some("1"));

    let sender = accounts.iter().find(|delta| delta.address == format!("{SENDER:#x}")).unwrap();
    assert_eq!(sender.before.as_ref().unwrap().balance, "10000");
    assert_eq!(sender.after.as_ref().unwrap().balance, "10800");
    assert_eq!(sender.after.as_ref().unwrap().nonce, "4");
    let recipient =
        accounts.iter().find(|delta| delta.address == format!("{RECIPIENT:#x}")).unwrap();
    assert_eq!(recipient.before.as_ref().unwrap().balance, "700");
    assert_eq!(recipient.after.as_ref().unwrap().balance, "900");

    let create2 = accounts.iter().find(|delta| delta.address == format!("{CREATE2_DEPLOYER:#x}"));
    assert_eq!(create2.is_some(), canyon, "CREATE2 deployment must occur exactly at Canyon");
    if let Some(delta) = create2 {
        assert!(delta.before.is_none());
        let code =
            hex::decode(delta.after.as_ref().unwrap().code.trim_start_matches("0x")).unwrap();
        assert_eq!(keccak256(code), CREATE2_DEPLOYER_CODEHASH);
    }
}

#[test]
fn spawned_worker_executes_deposit_across_canyon() {
    assert_deposit(execute_fixture(request(9, "regolith"), false), false);
    assert_deposit(execute_fixture(request(10, "canyon"), false), true);
}

#[test]
fn spawned_worker_executes_pre_canyon_selfdestruct_lifecycle() {
    // SSTORE(0, 1), then SELFDESTRUCT to an account absent from the parent snapshot.
    let code = "0x6001600055732222222222222222222222222222222222222222ff";
    let mut parent = ParentData::default();
    parent.accounts.insert(
        SENDER,
        AccountState { nonce: "3".into(), balance: "10000".into(), code: "0x".into() },
    );
    parent.accounts.insert(
        CONTRACT,
        AccountState { nonce: "1".into(), balance: "500".into(), code: code.into() },
    );
    parent.storage.insert((CONTRACT, U256::ZERO), U256::from(9));
    let request =
        request_with_transactions(9, "regolith", [deposit(TxKind::Call(CONTRACT), 200, "")]);
    let Outcome::Success { accounts, receipts, .. } =
        serde_json::from_value(execute_value_with_parent(request, None, &parent)).unwrap()
    else {
        panic!("expected successful execution")
    };
    assert!(receipts[0].success);

    let destroyed = accounts.iter().find(|delta| delta.address == CONTRACT.to_string()).unwrap();
    let before = destroyed.before.as_ref().unwrap();
    assert_eq!(
        (before.nonce.as_str(), before.balance.as_str(), before.code.as_str()),
        ("1", "500", code)
    );
    assert!(destroyed.after.is_none());
    assert!(destroyed.storage_wiped);

    let beneficiary = accounts.iter().find(|delta| delta.address == RECIPIENT.to_string()).unwrap();
    assert!(beneficiary.before.is_none());
    let after = beneficiary.after.as_ref().unwrap();
    assert_eq!(
        (after.nonce.as_str(), after.balance.as_str(), after.code.as_str()),
        ("0", "700", "0x")
    );
}

#[test]
fn spawned_worker_executes_pre_canyon_create_lifecycle() {
    const RUNTIME: &str = "0x602a60005260206000f3";
    // Copy the ten runtime bytes following this twelve-byte init program and return them.
    let init = "600a600c600039600a6000f3602a60005260206000f3";
    let mut parent = ParentData::default();
    parent.accounts.insert(
        SENDER,
        AccountState { nonce: "3".into(), balance: "10000".into(), code: "0x".into() },
    );
    let created = SENDER.create(3);
    let request = request_with_transactions(9, "regolith", [deposit(TxKind::Create, 123, init)]);
    let Outcome::Success { accounts, receipts, .. } =
        serde_json::from_value(execute_value_with_parent(request, None, &parent)).unwrap()
    else {
        panic!("expected successful execution")
    };
    assert!(receipts[0].success);
    let deployed = accounts
        .iter()
        .find(|delta| delta.address == format!("{created:#x}"))
        .unwrap_or_else(|| panic!("missing {created}; accounts: {accounts:#?}"));
    assert!(deployed.before.is_none());
    let after = deployed.after.as_ref().unwrap();
    assert_eq!(
        (after.nonce.as_str(), after.balance.as_str(), after.code.as_str()),
        ("1", "123", RUNTIME)
    );
    let sender = accounts.iter().find(|delta| delta.address == SENDER.to_string()).unwrap();
    assert_eq!(sender.after.as_ref().unwrap().balance, "10877");
}

#[test]
fn spawned_worker_keeps_parent_storage_bound_to_each_execution() {
    // SLOAD(0), SSTORE(1, loaded value), STOP.
    let code = "0x60005460015500";
    let execute = |request_id: &str, value: u64| {
        let mut parent = ParentData::default();
        parent.accounts.insert(
            SENDER,
            AccountState { nonce: "3".into(), balance: "10000".into(), code: "0x".into() },
        );
        parent.accounts.insert(
            CONTRACT,
            AccountState { nonce: "1".into(), balance: "0".into(), code: code.into() },
        );
        parent.storage.insert((CONTRACT, U256::ZERO), U256::from(value));
        let mut request =
            request_with_transactions(9, "regolith", [deposit(TxKind::Call(CONTRACT), 0, "")]);
        request.request_id = request_id.into();
        let Outcome::Success { accounts, .. } =
            serde_json::from_value(execute_value_with_parent(request, None, &parent)).unwrap()
        else {
            panic!("expected successful execution")
        };
        accounts
            .into_iter()
            .find(|delta| delta.address == CONTRACT.to_string())
            .unwrap()
            .storage
            .into_iter()
            .find(|delta| delta.key == format!("{:#066x}", U256::from(1)))
            .unwrap()
            .after
    };
    assert_eq!(execute("parent-seven", 7), format!("{:#066x}", U256::from(7)));
    assert_eq!(execute("parent-eleven", 11), format!("{:#066x}", U256::from(11)));
}

#[test]
fn spawned_worker_classifies_provider_failure_as_infrastructure() {
    let outcome = execute_fixture(request(9, "regolith"), true);
    assert!(matches!(
        outcome,
        Outcome::Infrastructure { error, .. } if error == "injected provider failure"
    ));
}

#[test]
fn spawned_worker_classifies_configuration_drift_as_infrastructure() {
    for field in ["genesis_identity", "chain_id", "genesis_header_hash", "config_identity", "era"] {
        let mut input = serde_json::to_value(request(9, "regolith")).unwrap();
        input[field] = serde_json::json!("wrong-deployment");
        let outcome = execute_fixture(serde_json::from_value(input).unwrap(), false);
        assert!(matches!(outcome, Outcome::Infrastructure { .. }), "{field}: {outcome:?}");
    }
}

#[test]
fn spawned_worker_rejects_unknown_protocol_before_execution() {
    let request = ExecuteRequest {
        version: 999,
        request_id: "spawn-protocol".into(),
        executable_sha256: "unused".into(),
        worker_pid: 0,
        binding_hash: "unused".into(),
        era: "bedrock".into(),
        chain_id: "1".into(),
        genesis_identity: "unused".into(),
        genesis_header_hash: "unused".into(),
        config_identity: "unused".into(),
        genesis: serde_json::json!({}),
        parent_header_rlp: "0x".into(),
        child_block_rlp: "0x".into(),
        operation: None,
    };
    assert!(matches!(invoke(&serde_json::to_vec(&request).unwrap()), Outcome::Unsupported { .. }));
}

#[test]
fn spawned_worker_classifies_malformed_request_as_infrastructure() {
    assert!(matches!(invoke(br#"{"version":1}"#), Outcome::Infrastructure { .. }));
}

#[test]
fn spawned_worker_calls_with_state_and_block_overrides() {
    let value = execute_value(
        rpc_request(
            "eth_call",
            serde_json::json!([
                { "from": SENDER, "to": RECIPIENT, "maxFeePerGas": "0x0",
                  "maxPriorityFeePerGas": "0x0", "accessList": [] },
                { RECIPIENT.to_string(): { "code": "0x4360005260206000f3" } },
                { "number": "0x2a", "baseFeePerGas": "0xffff" }
            ]),
        ),
        false,
    );
    assert_eq!(value["result"], format!("0x{:064x}", 42), "{value}");
}

#[test]
fn spawned_worker_estimates_gas() {
    let value = execute_value(
        rpc_request(
            "eth_estimateGas",
            serde_json::json!([{ "from": SENDER, "to": RECIPIENT, "gasPrice": "0x0" }]),
        ),
        false,
    );
    assert_eq!(value["result"], "0x5208", "{value}");
}

#[test]
fn spawned_worker_estimates_refund_and_eip150_sensitive_call() {
    let value = execute_value(
        rpc_request(
            "eth_estimateGas",
            serde_json::json!([
                { "from": SENDER, "to": RECIPIENT, "gasPrice": "0x0" },
                { RECIPIENT.to_string(): {
                    "code": "0x600060005560006000600060006000600161fffff15000",
                    "state": { format!("{:#066x}", U256::ZERO): format!("{:#066x}", U256::from(1)) }
                } }
            ]),
        ),
        false,
    );
    let estimate =
        u64::from_str_radix(value["result"].as_str().unwrap().trim_start_matches("0x"), 16)
            .unwrap();
    assert!((21_000..100_000).contains(&estimate), "{value}");
}

#[test]
fn spawned_worker_rejects_low_estimation_limit() {
    let value = execute_value(
        rpc_request(
            "eth_estimateGas",
            serde_json::json!([{ "from": SENDER, "to": RECIPIENT, "gas": "0x5207" }]),
        ),
        false,
    );
    assert_eq!(value["code"], -32000, "{value}");
    assert!(value["message"].as_str().unwrap().contains("gas"), "{value}");
}

#[test]
fn spawned_worker_propagates_provider_failure_during_estimation() {
    let request = rpc_request(
        "eth_estimateGas",
        serde_json::json!([
            { "from": SENDER, "to": RECIPIENT, "gasPrice": "0x0" },
            { RECIPIENT.to_string(): { "code": "0x5a5400" } }
        ]),
    );
    let value = execute_value_with_failure(request, Some(4));
    assert_eq!(value["code"], -32000, "{value}");
    assert_eq!(value["message"], "injected provider failure", "{value}");
}

#[test]
fn spawned_worker_traces_call_with_struct_logger_and_call_tracer() {
    let call = serde_json::json!({
        "from": SENDER, "to": RECIPIENT, "gas": "0x186a0", "gasPrice": "0x1"
    });
    let overrides = serde_json::json!({
        "stateOverrides": { RECIPIENT.to_string(): { "code": "0x600160020100" } }
    });
    let value =
        execute_value(rpc_request("debug_traceCall", serde_json::json!([call, overrides])), false);
    assert_eq!(value["result"]["structLogs"][0]["op"], "PUSH1", "{value}");
    assert_eq!(value["result"]["structLogs"][2]["op"], "ADD", "{value}");
    assert_eq!(value["result"]["structLogs"][2]["stack"][0], "0x1", "{value}");

    let value = execute_value(
        rpc_request(
            "debug_traceCall",
            serde_json::json!([call, {
                "tracer": "callTracer",
                "stateOverrides": { RECIPIENT.to_string(): { "code": "0x600160020100" } }
            }]),
        ),
        false,
    );
    assert_eq!(value["result"]["type"], "CALL", "{value}");
    assert_eq!(value["result"]["from"], SENDER.to_string(), "{value}");
    assert_eq!(value["result"]["to"], RECIPIENT.to_string(), "{value}");
}

#[test]
fn spawned_worker_traces_deposit_block_and_transaction() {
    let mut block_request = request(9, "regolith");
    block_request.operation = Some(serde_json::json!({
        "method": "debug_traceBlock", "params": [{}]
    }));
    let value = execute_value(block_request, false);
    assert_eq!(value["result"].as_array().unwrap().len(), 1, "{value}");
    assert!(value["result"][0]["result"]["structLogs"].is_array(), "{value}");
    assert!(value["result"][0]["txHash"].is_string(), "{value}");

    let mut tx_request = request(9, "regolith");
    tx_request.operation = Some(serde_json::json!({
        "method": "debug_traceTransaction", "params": [0, { "tracer": "callTracer" }]
    }));
    let value = execute_value(tx_request, false);
    assert_eq!(value["result"]["type"], "CALL", "{value}");
    assert_eq!(value["result"]["from"], SENDER.to_string(), "{value}");
    assert_eq!(value["result"]["to"], RECIPIENT.to_string(), "{value}");
}
