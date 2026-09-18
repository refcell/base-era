#!/usr/bin/env bash
set -euo pipefail
run=${1:?RUN_DIR required}; r="$run/runtime.json"; rpc=$(jq -r .builder_rpc_url "$r"); fork=$(jq -r .isthmus_block "$r")
key=0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d; to=0x000000000000000000000000000000000000bEEF
send() { cast send --json --rpc-url "$rpc" --private-key "$key" "$to" --value "$2" | jq --arg phase "$1" '. + {phase:$phase}'; }
send pre 11 >"$run/evidence/pre-receipt.json"
while (( $(cast block-number --rpc-url "$rpc") < fork )); do sleep .1; done
fork_hex=$(printf '0x%x' "$fork")
upgrade_hash=$(cast rpc --rpc-url "$rpc" eth_getBlockByNumber "$fork_hex" true | jq -er '.transactions[] | select(.type == "0x7e" and .from != "0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001") | .hash' | head -1)
cast receipt --json --rpc-url "$rpc" "$upgrade_hash" | jq '. + {phase:"at",kind:"isthmus-upgrade-deposit"}' >"$run/evidence/at-receipt.json"
send post 33 >"$run/evidence/post-receipt.json"
pre=$(cast to-dec "$(jq -r .blockNumber "$run/evidence/pre-receipt.json")"); at=$(cast to-dec "$(jq -r .blockNumber "$run/evidence/at-receipt.json")"); post=$(cast to-dec "$(jq -r .blockNumber "$run/evidence/post-receipt.json")")
(( pre < fork && at == fork && post > fork )) || { echo "transactions missed pre/at/post boundary: $pre/$at/$post" >&2; exit 1; }
cast send --json --rpc-url "$rpc" --private-key "$key" --create 0x600a600c600039600a6000f360003560005560006000f3 >"$run/evidence/deploy-receipt.json"
contract=$(jq -r .contractAddress "$run/evidence/deploy-receipt.json"); cast send --json --rpc-url "$rpc" --private-key "$key" "$contract" 0x$(printf '%064x' 4660) >"$run/evidence/storage-receipt.json"
jq -n --arg contract "$contract" --argjson fork "$fork" '{contract:$contract,forkBlock:$fork}' >"$run/evidence/workload.json"
