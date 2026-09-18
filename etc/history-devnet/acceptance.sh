#!/usr/bin/env bash
# Run after start.sh and exercise.sh, against a live, disposable history devnet.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
run=${1:?RUN_DIR required}
run=$(realpath "$run")
test -f "$run/runtime.json"
test -f "$run/manifest.json"
test -x "$root/target/history-reference-node" || {
  echo "run etc/history-devnet/build-reference.sh first" >&2
  exit 1
}
"$root/etc/history-devnet/verify.sh" "$run"
python3 "$root/etc/history-devnet/replay.py" --run-dir "$run" --output "$run/replay-evidence"
python3 "$root/etc/history-devnet/import.py" --run-dir "$run" --output "$run/import-evidence"
python3 "$root/etc/history-devnet/failures.py" --run-dir "$run" --output "$run/failure-evidence"
cargo --config "$root/target/history-reth-overrides.toml" build --locked -j "${CARGO_BUILD_JOBS:-6}" \
  -p base-proof-executor --features test-utils --example fixture
python3 "$root/etc/history-devnet/stateless.py" "$run" "$run/replay-evidence/worker-datadir" \
  "$root/target/history-host-node" --fixture-binary "$root/target/debug/examples/fixture"
python3 "$root/etc/history-devnet/benchmark.py" \
  --genesis "$run/generated/l2/genesis.json" \
  --worker-datadir "$run/replay-evidence/worker-datadir" \
  --reference-datadir "$run/replay-evidence/reference-datadir" \
  --source-manifest "$run/manifest.json" --run-dir "$run/benchmark" \
  --output "$run/evidence/benchmark.json"
printf 'Acceptance completed; evidence: %s\n' "$run"
