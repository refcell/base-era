use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use alloy_consensus::Header;
use base_common_chains::Upgrades;
use base_common_evm::BaseSpecId;
use base_common_genesis::BaseUpgrade;
use base_execution_chainspec::BaseChainSpec;
use base_execution_history::{HistoryWorker, HistoryWorkerError, OutcomeAdapter};
use reth_chainspec::EthChainSpec;
use reth_execution_errors::{BlockExecutionError, BlockValidationError};
use reth_execution_types::BlockExecutionOutput;
use reth_primitives_traits::{Block, BlockBody, Receipt, RecoveredBlock};
use revm::Database;

/// Opt-in historical execution policy for the Isthmus isolation experiment.
#[derive(Debug)]
pub struct HistoricalExecution;

impl HistoricalExecution {
    /// Validates the caller's effective schedule against cached immutable manifest configuration.
    /// Only expensive genesis derivation is cached; routing and schedule checks run each time.
    pub fn validate_configuration<S>(
        spec: &S,
        worker: &HistoryWorker,
    ) -> Result<(), HistoryWorkerError>
    where
        S: EthChainSpec<Header = Header> + Upgrades,
    {
        static FROZEN: OnceLock<Mutex<Option<(String, Arc<BaseChainSpec>)>>> = OnceLock::new();
        let frozen = {
            let mut cache = FROZEN.get_or_init(|| Mutex::new(None)).lock().map_err(|_| {
                HistoryWorkerError::Infrastructure("configuration cache poisoned".into())
            })?;
            if !cache.as_ref().is_some_and(|(identity, _)| identity == &worker.manifest_identity) {
                let genesis = serde_json::from_value(worker.manifest.genesis.clone())
                    .map_err(base_execution_history::infra)?;
                let frozen = BaseChainSpec::try_from_genesis(genesis)
                    .map_err(base_execution_history::infra)?;
                *cache = Some((worker.manifest_identity.clone(), Arc::new(frozen)));
            }
            Arc::clone(&cache.as_ref().expect("configuration initialized").1)
        };
        if frozen.genesis_hash() != spec.genesis_hash()
            || frozen.chain().id() != spec.chain().id()
            || BaseUpgrade::EXECUTION_VARIANTS
                .iter()
                .any(|fork| frozen.fork_condition(*fork) != spec.fork_condition(*fork))
        {
            return Err(HistoryWorkerError::Infrastructure(
                "historical artifact effective chain configuration mismatch".into(),
            ));
        }
        Ok(())
    }

    /// Routes using the effective chain schedule, never the wall clock.
    pub fn selected(spec: &impl Upgrades, header: &Header) -> bool {
        std::env::var_os("BASE_HISTORY_MANIFEST").is_some()
            && !spec.is_isthmus_active_at_timestamp(header.timestamp)
    }

    /// Executes a complete historical block against its caller-owned immutable parent view.
    pub fn execute<S, DB, B, R>(
        spec: &S,
        db: &mut DB,
        block: &RecoveredBlock<B>,
        parent: Option<&Header>,
    ) -> Result<Option<BlockExecutionOutput<R>>, BlockExecutionError>
    where
        S: EthChainSpec<Header = Header> + Upgrades,
        DB: Database,
        B: Block<Header = Header>,
        R: Receipt,
    {
        if !Self::selected(spec, block.header()) {
            return Ok(None);
        }
        let parent = parent.ok_or_else(|| {
            BlockExecutionError::msg("historical execution requires a bound parent header")
        })?;
        let manifest_path = std::env::var_os("BASE_HISTORY_MANIFEST").ok_or_else(|| {
            BlockExecutionError::msg("historical artifact configuration disappeared")
        })?;
        let worker = HistoryWorker::from_manifest(manifest_path).map_err(Self::error)?;
        Self::validate_configuration(spec, &worker).map_err(Self::error)?;
        static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
        let request_id = format!(
            "execute-{}-{}-{}",
            std::process::id(),
            NEXT_REQUEST.fetch_add(1, Ordering::Relaxed),
            block.hash()
        );
        let era = BaseSpecId::from_header(spec, block.header()).to_string().to_ascii_lowercase();
        let outcome = worker
            .execute(
                request_id,
                era,
                format!("0x{}", hex::encode(alloy_rlp::encode(parent))),
                format!("0x{}", hex::encode(alloy_rlp::encode(block.clone().into_block()))),
                db,
            )
            .map_err(Self::error)?;
        let output = OutcomeAdapter::adapt(outcome, db).map_err(Self::error)?;
        if output.result.receipts.len() != block.body().transactions().len()
            || output.result.blob_gas_used != 0
            || !output.result.requests.is_empty()
        {
            return Err(BlockExecutionError::msg(
                "historical result has inconsistent transaction count or pre-Isthmus outputs",
            ));
        }
        Ok(Some(output))
    }

    /// Preserves reth's consensus-invalid versus internal-error distinction.
    pub fn error(error: HistoryWorkerError) -> BlockExecutionError {
        match error {
            HistoryWorkerError::Invalid(_) => BlockValidationError::other(error).into(),
            _ => BlockExecutionError::other(error),
        }
    }
}
