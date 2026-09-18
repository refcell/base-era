//! Records and runs stateless executor fixtures against an RPC endpoint.

use std::{env, path::PathBuf};

use base_common_genesis::RollupConfig;
use base_proof_executor::test_utils::{ExecutorTestFixtureCreator, run_test_fixture};

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("record") if args.len() == 6 => {
            let rpc = &args[2];
            let rollup_config: RollupConfig = serde_json::from_slice(
                &tokio::fs::read(&args[3]).await.expect("failed to read rollup config"),
            )
            .expect("failed to decode rollup config");
            let output = PathBuf::from(&args[4]);
            let block = args[5].parse().expect("invalid block number");
            ExecutorTestFixtureCreator::new(rpc, block, output)
                .create_static_fixture_with_rollup_config(rollup_config)
                .await;
            println!("recorded block {block}");
        }
        Some("record-witness") if args.len() == 6 => {
            let rpc = &args[2];
            let rollup_config: RollupConfig = serde_json::from_slice(
                &tokio::fs::read(&args[3]).await.expect("failed to read rollup config"),
            )
            .expect("failed to decode rollup config");
            let output = PathBuf::from(&args[4]);
            let block = args[5].parse().expect("invalid block number");
            ExecutorTestFixtureCreator::new(rpc, block, output)
                .create_static_fixture_from_execution_witness(rollup_config)
                .await;
            println!("recorded block {block} from execution witness");
        }
        Some("run") if args.len() == 3 => {
            run_test_fixture(PathBuf::from(&args[2])).await;
            println!("matched {}", args[2]);
        }
        _ => {
            eprintln!(
                "usage: fixture record <rpc> <rollup.json> <output-dir> <block>\n       fixture record-witness <rpc> <rollup.json> <output-dir> <block>\n       fixture run <fixture.tar.gz>"
            );
            std::process::exit(2);
        }
    }
}
