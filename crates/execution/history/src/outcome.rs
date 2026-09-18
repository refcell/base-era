use std::collections::HashSet;

use alloy_eips::eip7685::Requests;
use alloy_primitives::{Address, B256, Bloom, Bytes, U256, keccak256};
use history_worker_protocol::{AccountState, Outcome};
use reth_execution_types::{BlockExecutionOutput, BlockExecutionResult};
use reth_primitives_traits::Receipt;
use revm::{
    Database,
    bytecode::Bytecode,
    database::states::{AccountStatus, BundleState},
    state::AccountInfo,
};

use crate::{HistoryWorkerError, infra, infrastructure};

/// Converts a successful wire outcome into reth execution state and validates all claimed before
/// values against the immutable parent database.
#[derive(Debug)]
pub struct OutcomeAdapter;

impl OutcomeAdapter {
    /// Adapts an outcome. Worker and database failures remain infrastructure failures.
    pub fn adapt<R: Receipt, DB: Database>(
        outcome: Outcome,
        db: &mut DB,
    ) -> Result<BlockExecutionOutput<R>, HistoryWorkerError>
    where
        DB::Error: core::fmt::Display,
    {
        let Outcome::Success {
            accounts,
            block_reversions: _,
            receipts,
            gas_used,
            blob_gas_used,
            requests,
            requests_hash,
            ..
        } = outcome
        else {
            return Err(infrastructure("cannot adapt unsuccessful outcome"));
        };

        let mut contracts = Vec::new();
        let mut statuses = Vec::new();
        let mut state = Vec::new();
        let mut reversions = Vec::new();
        let mut wipes = Vec::new();
        let mut seen_accounts = HashSet::new();
        for delta in accounts {
            let address: Address = delta.address.parse().map_err(infra)?;
            if !seen_accounts.insert(address) {
                return Err(infrastructure("duplicate worker account delta"));
            }
            let database_before = db.basic(address).map_err(infra)?;
            let before = account_info(delta.before.as_ref(), &mut contracts)?;
            if !same_account(database_before.as_ref(), before.as_ref()) {
                return Err(infrastructure("worker account before value disagrees with database"));
            }
            let after = account_info(delta.after.as_ref(), &mut contracts)?;
            if before.is_none() && after.is_none() {
                return Err(infrastructure("worker delta has no account before or after"));
            }
            if after.is_none() && !delta.storage_wiped {
                return Err(infrastructure("removed worker account does not wipe storage"));
            }
            let mut slots = revm::primitives::HashMap::default();
            for slot in delta.storage {
                let key = parse_word(&slot.key)?;
                let claimed_before = parse_word(&slot.before)?;
                let database_value = db.storage(address, key).map_err(infra)?;
                if database_value != claimed_before {
                    return Err(infrastructure(
                        "worker storage before value disagrees with database",
                    ));
                }
                if slots.insert(key, (claimed_before, parse_word(&slot.after)?)).is_some() {
                    return Err(infrastructure("duplicate worker storage delta"));
                }
            }
            let status = account_status(before.is_some(), after.is_some(), delta.storage_wiped);
            statuses.push((address, status));
            reversions.push((
                address,
                Some(before.clone()),
                slots.iter().map(|(key, (old, _))| (*key, *old)).collect::<Vec<_>>(),
            ));
            wipes.push((address, delta.storage_wiped));
            state.push((address, before, after, slots));
        }

        let mut bundle = BundleState::new(state, [reversions], contracts);
        for (address, status) in statuses {
            bundle.state.get_mut(&address).expect("inserted account").status = status;
        }
        for (address, wiped) in wipes {
            let (_, revert) = bundle.reverts[0]
                .iter_mut()
                .find(|(a, _)| *a == address)
                .expect("inserted reversion");
            revert.wipe_storage = wiped;
            revert.previous_status = if bundle.state[&address].original_info.is_some() {
                AccountStatus::Loaded
            } else {
                AccountStatus::LoadedNotExisting
            };
        }

        let mut previous_cumulative_gas = 0;
        let receipts = receipts
            .into_iter()
            .enumerate()
            .map(|(index, receipt)| {
                if receipt.transaction_index as usize != index {
                    return Err(infrastructure("worker receipt indexes are not contiguous"));
                }
                // Canonical RLP preserves the era-specific deposit fields. In particular,
                // Regolith has a nonce without Canyon's receipt version.
                let bytes = decode_hex(&receipt.canonical_rlp)?;
                let mut input = bytes.as_slice();
                let decoded = R::decode(&mut input).map_err(infra)?;
                if !input.is_empty() {
                    return Err(infrastructure("trailing receipt RLP"));
                }
                let claimed_gas = receipt.cumulative_gas_used.parse().map_err(infra)?;
                let bloom_bytes = decode_hex(&receipt.logs_bloom)?;
                if bloom_bytes.len() != 256 {
                    return Err(infrastructure("worker receipt bloom has invalid length"));
                }
                let claimed_bloom = Bloom::from_slice(&bloom_bytes);
                if decoded.status() != receipt.success
                    || decoded.cumulative_gas_used() != claimed_gas
                    || decoded.bloom() != claimed_bloom
                {
                    return Err(infrastructure("worker receipt metadata disagrees with RLP"));
                }
                if claimed_gas < previous_cumulative_gas {
                    return Err(infrastructure("worker receipt cumulative gas decreases"));
                }
                previous_cumulative_gas = claimed_gas;
                Ok(decoded)
            })
            .collect::<Result<Vec<_>, HistoryWorkerError>>()?;
        let gas_used = gas_used.parse().map_err(infra)?;
        if previous_cumulative_gas != gas_used {
            return Err(infrastructure("worker terminal gas disagrees with receipts"));
        }
        let requests = requests
            .into_iter()
            .map(|request| decode_hex(&request).map(Bytes::from))
            .collect::<Result<Vec<_>, _>>()?;
        let requests = Requests::new(requests);
        if let Some(claimed) = requests_hash
            && claimed.parse::<B256>().map_err(infra)? != requests.requests_hash()
        {
            return Err(infrastructure("worker request commitment disagrees with request bytes"));
        }
        Ok(BlockExecutionOutput {
            state: bundle,
            result: BlockExecutionResult {
                receipts,
                gas_used,
                blob_gas_used: blob_gas_used.parse().map_err(infra)?,
                requests,
            },
        })
    }
}

/// Maps explicit existence and wipe semantics to the host's bundle status.
pub const fn account_status(before: bool, after: bool, storage_wiped: bool) -> AccountStatus {
    match (before, after, storage_wiped) {
        (_, false, true) => AccountStatus::Destroyed,
        (_, true, true) => AccountStatus::DestroyedChanged,
        (false, true, _) => AccountStatus::InMemoryChange,
        _ => AccountStatus::Changed,
    }
}

/// Decodes an optional wire account and records its complete bytecode.
pub fn account_info(
    state: Option<&AccountState>,
    contracts: &mut Vec<(alloy_primitives::B256, Bytecode)>,
) -> Result<Option<AccountInfo>, HistoryWorkerError> {
    state
        .map(|state| {
            let raw = Bytes::from(decode_hex(&state.code)?);
            let code_hash = keccak256(&raw);
            let code = Bytecode::new_raw(raw);
            contracts.push((code_hash, code.clone()));
            Ok(AccountInfo {
                balance: state.balance.parse().map_err(infra)?,
                nonce: state.nonce.parse().map_err(infra)?,
                code_hash,
                code: Some(code),
                ..Default::default()
            })
        })
        .transpose()
}

/// Compares consensus account fields without relying on cached bytecode presence.
pub fn same_account(left: Option<&AccountInfo>, right: Option<&AccountInfo>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.balance == right.balance
                && left.nonce == right.nonce
                && left.code_hash == right.code_hash
        }
        _ => false,
    }
}

/// Parses a protocol storage word.
pub fn parse_word(value: &str) -> Result<U256, HistoryWorkerError> {
    U256::from_str_radix(
        value.strip_prefix("0x").ok_or_else(|| infrastructure("hex value lacks 0x"))?,
        16,
    )
    .map_err(infra)
}

/// Decodes protocol bytes, requiring the hexadecimal prefix.
pub fn decode_hex(value: &str) -> Result<Vec<u8>, HistoryWorkerError> {
    hex::decode(value.strip_prefix("0x").ok_or_else(|| infrastructure("hex value lacks 0x"))?)
        .map_err(infra)
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{Eip658Value, Receipt as AlloyReceipt, TxReceipt};
    use alloy_primitives::{Log, address};
    use base_common_consensus::{BaseReceipt, DepositReceipt};
    use history_worker_protocol::{AccountDelta, Receipt as WireReceipt, StorageDelta};
    use revm::database::{CacheDB, EmptyDB, states::OriginalValuesKnown};

    use super::*;

    fn account(nonce: u64, balance: u64) -> AccountState {
        AccountState { nonce: nonce.to_string(), balance: balance.to_string(), code: "0x".into() }
    }

    fn delta(
        address: Address,
        before: Option<AccountState>,
        after: Option<AccountState>,
        storage_wiped: bool,
        storage: Vec<StorageDelta>,
    ) -> AccountDelta {
        AccountDelta { address: address.to_string(), before, after, storage_wiped, storage }
    }

    fn success(accounts: Vec<AccountDelta>, receipts: Vec<WireReceipt>, gas: u64) -> Outcome {
        Outcome::Success {
            request_id: "test".into(),
            accounts,
            block_reversions: Vec::new(),
            receipts,
            gas_used: gas.to_string(),
            blob_gas_used: "0".into(),
            requests: Vec::new(),
            requests_hash: None,
        }
    }

    fn receipt(index: u32, success: bool, gas: u64) -> WireReceipt {
        let decoded = BaseReceipt::Legacy(AlloyReceipt {
            status: Eip658Value::Eip658(success),
            cumulative_gas_used: gas,
            logs: Vec::<Log>::new(),
        });
        WireReceipt {
            transaction_index: index,
            success,
            cumulative_gas_used: gas.to_string(),
            logs_bloom: format!("0x{}", hex::encode([0; 256])),
            canonical_rlp: format!("0x{}", hex::encode(alloy_rlp::encode(decoded))),
            deposit_nonce: None,
            deposit_receipt_version: None,
        }
    }

    fn info(nonce: u64, balance: u64) -> AccountInfo {
        account_info(Some(&account(nonce, balance)), &mut Vec::new()).unwrap().unwrap()
    }

    #[test]
    fn adapts_creation_wipe_recreation_and_deletion_with_observable_reverts() {
        let created = address!("1000000000000000000000000000000000000000");
        let recreated = address!("2000000000000000000000000000000000000000");
        let deleted = address!("3000000000000000000000000000000000000000");
        let mut db = CacheDB::<EmptyDB>::default();
        db.insert_account_info(recreated, info(1, 10));
        db.insert_account_storage(recreated, U256::from(7), U256::from(9)).unwrap();
        db.insert_account_info(deleted, info(2, 20));
        db.insert_account_storage(deleted, U256::from(8), U256::from(11)).unwrap();

        let output = OutcomeAdapter::adapt::<BaseReceipt, _>(
            success(
                vec![
                    delta(created, None, Some(account(1, 1)), false, vec![]),
                    delta(
                        recreated,
                        Some(account(1, 10)),
                        Some(account(3, 30)),
                        true,
                        vec![StorageDelta {
                            key: "0x7".into(),
                            before: "0x9".into(),
                            after: "0x9".into(),
                        }],
                    ),
                    delta(deleted, Some(account(2, 20)), None, true, vec![]),
                ],
                vec![],
                0,
            ),
            &mut db,
        )
        .unwrap();

        let plain = output.state.to_plain_state(OriginalValuesKnown::Yes);
        assert!(
            plain.accounts.iter().any(|(address, value)| *address == created && value.is_some())
        );
        assert!(
            plain.accounts.iter().any(|(address, value)| *address == deleted && value.is_none())
        );
        let recreated_storage =
            plain.storage.iter().find(|storage| storage.address == recreated).unwrap();
        assert!(recreated_storage.wipe_storage);
        assert_eq!(recreated_storage.storage, vec![(U256::from(7), U256::from(9))]);

        let mut reverted = output.state;
        assert!(reverted.revert_latest());
        assert!(reverted.account(&created).is_none());
        assert_eq!(reverted.account(&recreated).unwrap().info.as_ref().unwrap().nonce, 1);
        assert_eq!(reverted.account(&deleted).unwrap().info.as_ref().unwrap().nonce, 2);
        assert_eq!(reverted.storage(&recreated, U256::from(7)), Some(U256::from(9)));
    }

    #[test]
    fn merges_two_outputs_and_reverts_each_block_to_its_actual_parent() {
        let address = address!("4000000000000000000000000000000000000000");
        let mut parent = CacheDB::<EmptyDB>::default();
        parent.insert_account_info(address, info(1, 10));
        let first = OutcomeAdapter::adapt::<BaseReceipt, _>(
            success(
                vec![delta(address, Some(account(1, 10)), Some(account(2, 20)), false, vec![])],
                vec![],
                0,
            ),
            &mut parent,
        )
        .unwrap();
        let mut second_parent = CacheDB::<EmptyDB>::default();
        second_parent.insert_account_info(address, info(2, 20));
        let second = OutcomeAdapter::adapt::<BaseReceipt, _>(
            success(
                vec![delta(address, Some(account(2, 20)), Some(account(3, 30)), false, vec![])],
                vec![],
                0,
            ),
            &mut second_parent,
        )
        .unwrap();
        let mut merged = first.state;
        merged.extend(second.state);
        assert_eq!(merged.account(&address).unwrap().info.as_ref().unwrap().nonce, 3);
        assert!(merged.revert_latest());
        assert_eq!(merged.account(&address).unwrap().info.as_ref().unwrap().nonce, 2);
        assert!(merged.revert_latest());
        assert_eq!(merged.account(&address).unwrap().info.as_ref().unwrap().nonce, 1);
    }

    #[test]
    fn validates_receipt_metadata_indexes_and_terminal_gas() {
        let mut db = CacheDB::<EmptyDB>::default();
        let output = OutcomeAdapter::adapt::<BaseReceipt, _>(
            success(vec![], vec![receipt(0, true, 21), receipt(1, false, 34)], 34),
            &mut db,
        )
        .unwrap();
        assert!(output.result.receipts[0].status());
        assert!(!output.result.receipts[1].status());
        assert_eq!(output.result.receipts[1].cumulative_gas_used(), 34);
        assert_eq!(output.result.gas_used, 34);

        for invalid in [
            success(vec![], vec![receipt(1, true, 1)], 1),
            success(vec![], vec![receipt(0, true, 2), receipt(1, true, 1)], 1),
            success(vec![], vec![receipt(0, true, 1)], 2),
            success(vec![], vec![], 1),
        ] {
            assert!(OutcomeAdapter::adapt::<BaseReceipt, _>(invalid, &mut db).is_err());
        }
        let mut wrong_status = receipt(0, true, 1);
        wrong_status.success = false;
        assert!(
            OutcomeAdapter::adapt::<BaseReceipt, _>(
                success(vec![], vec![wrong_status], 1),
                &mut db,
            )
            .is_err()
        );
        let mut wrong_bloom = receipt(0, true, 1);
        wrong_bloom.logs_bloom = format!("0x{}", hex::encode([1; 256]));
        assert!(matches!(
            OutcomeAdapter::adapt::<BaseReceipt, _>(success(vec![], vec![wrong_bloom], 1), &mut db,),
            Err(HistoryWorkerError::Infrastructure(_))
        ));
    }

    #[test]
    fn rejects_inconsistent_request_commitment() {
        let mut outcome = success(vec![], vec![], 0);
        if let Outcome::Success { requests_hash, .. } = &mut outcome {
            *requests_hash =
                Some("0xe3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into());
        }
        assert!(
            OutcomeAdapter::adapt::<BaseReceipt, _>(
                outcome.clone(),
                &mut CacheDB::<EmptyDB>::default(),
            )
            .is_ok()
        );
        if let Outcome::Success { requests_hash, .. } = &mut outcome {
            *requests_hash = Some(B256::ZERO.to_string());
        }
        assert!(
            OutcomeAdapter::adapt::<BaseReceipt, _>(outcome, &mut CacheDB::<EmptyDB>::default(),)
                .is_err()
        );
    }

    #[test]
    fn preserves_regolith_deposit_nonce_without_canyon_version() {
        let decoded = BaseReceipt::Deposit(DepositReceipt {
            inner: AlloyReceipt {
                status: Eip658Value::Eip658(true),
                cumulative_gas_used: 21_000,
                logs: Vec::<Log>::new(),
            },
            deposit_nonce: Some(7),
            deposit_receipt_version: None,
        });
        let mut wire = receipt(0, true, 21_000);
        wire.canonical_rlp = format!("0x{}", hex::encode(alloy_rlp::encode(&decoded)));
        wire.deposit_nonce = Some("7".into());
        let output = OutcomeAdapter::adapt::<BaseReceipt, _>(
            success(vec![], vec![wire], 21_000),
            &mut CacheDB::<EmptyDB>::default(),
        )
        .unwrap();
        assert_eq!(output.result.receipts, vec![decoded]);
    }

    #[test]
    fn rejects_database_disagreement_absent_empty_and_duplicate_deltas() {
        let address = address!("5000000000000000000000000000000000000000");
        let mut db = CacheDB::<EmptyDB>::default();
        db.insert_account_info(address, info(0, 0));
        assert!(
            OutcomeAdapter::adapt::<BaseReceipt, _>(
                success(vec![delta(address, None, Some(account(1, 1)), false, vec![])], vec![], 0),
                &mut db,
            )
            .is_err()
        );
        let duplicate = delta(address, Some(account(0, 0)), Some(account(1, 1)), false, vec![]);
        assert!(
            OutcomeAdapter::adapt::<BaseReceipt, _>(
                success(vec![duplicate.clone(), duplicate], vec![], 0),
                &mut db,
            )
            .is_err()
        );
        let duplicate_slot =
            StorageDelta { key: "0x1".into(), before: "0x0".into(), after: "0x2".into() };
        assert!(
            OutcomeAdapter::adapt::<BaseReceipt, _>(
                success(
                    vec![delta(
                        address,
                        Some(account(0, 0)),
                        Some(account(1, 1)),
                        false,
                        vec![duplicate_slot.clone(), duplicate_slot]
                    )],
                    vec![],
                    0
                ),
                &mut db,
            )
            .is_err()
        );
    }
}
