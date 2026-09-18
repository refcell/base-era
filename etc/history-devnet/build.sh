#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
config="$root/target/history-reth-overrides.toml"
worker="$root/etc/history-worker/target/release/base-history-worker"
host="$root/target/profiling/base-devnet"
artifacts="$root/target/history-artifacts"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-3}
mkdir -p "$artifacts"
python3 "$root/tools/reth-config.py" > "$config"
"$root/etc/history-worker/scripts/verify-base.sh"
python3 "$root/etc/history-devnet/artifacts.py" --source host_reth
CARGO_TARGET_DIR="$root/etc/history-worker/target" cargo build \
    --manifest-path "$root/etc/history-worker/Cargo.toml" --locked --release --bin base-history-worker
CARGO_TARGET_DIR="$root/target" cargo --config "$config" build --locked --profile profiling \
    --manifest-path "$root/Cargo.toml" \
    -p base-reth-node -p base-system-tests -p base-execution-evm -p base-execution-rpc \
    --bin base-reth-node --bin base-devnet --no-default-features \
    --features base-execution-evm/history,base-execution-rpc/history

publish() {
    local source=$1 name=$2 digest destination temporary
    digest=$(sha256sum "$source" | cut -d' ' -f1)
    destination="$artifacts/sha256/$digest/$name"
    if [[ ! -e "$destination" ]]; then
        mkdir -p "$(dirname "$destination")"
        temporary="$destination.tmp.$$"
        cp "$source" "$temporary"
        chmod 555 "$temporary"
        mv "$temporary" "$destination"
    fi
    test "$(sha256sum "$destination" | cut -d' ' -f1)" = "$digest"
    printf '%s\n' "$digest"
}

worker_digest=$(publish "$worker" base-history-worker)
host_digest=$(publish "$host" base-devnet)
node_digest=$(publish "$root/target/profiling/base-reth-node" base-reth-node)
ln -sfn "$artifacts/sha256/$node_digest/base-reth-node" "$root/target/history-host-node"
worker_artifact="$artifacts/sha256/$worker_digest/base-history-worker"
host_artifact="$artifacts/sha256/$host_digest/base-devnet"
cat >"$artifacts/APPROVAL.json.tmp" <<EOF
{"executable":"$worker_artifact","executable_sha256":"0x$worker_digest"}
EOF
mv "$artifacts/APPROVAL.json.tmp" "$artifacts/APPROVAL.json"
printf '%s\n' "$host_artifact" >"$artifacts/HOST.tmp"
mv "$artifacts/HOST.tmp" "$artifacts/HOST"
printf '%s\n' "$worker_digest" >"$artifacts/DIGEST.tmp"
mv "$artifacts/DIGEST.tmp" "$artifacts/DIGEST"
printf 'worker: sha256:%s\nhost: sha256:%s\napproval: %s\n' \
    "$worker_digest" "$host_digest" "$artifacts/APPROVAL.json"
