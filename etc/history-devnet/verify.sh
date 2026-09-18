#!/usr/bin/env bash
set -euo pipefail
run=${1:?RUN_DIR required}; r="$run/runtime.json"; a=$(jq -r .builder_rpc_url "$r"); b=$(jq -r .verifier_rpc_url "$r"); fork=$(jq -r .isthmus_block "$r")
last=$(cast to-dec "$(jq -r .blockNumber "$run/evidence/storage-receipt.json")")
through=$((last > fork+2 ? last : fork+2))
deadline=$((SECONDS+600)); while (( $(cast block-number --rpc-url "$b") < through )); do ((SECONDS<deadline)) || { echo "verifier failed to reach workload block $through within 600s" >&2; exit 1; }; sleep 1; done
ok=true
for n in $(seq $((fork-2)) $((fork+2))); do
  for side in builder verifier; do rpc=$a; test "$side" = verifier && rpc=$b; cast rpc --rpc-url "$rpc" eth_getBlockByNumber "$(printf '0x%x' "$n")" true | jq '{number,hash,parentHash,stateRoot,receiptsRoot,transactionsRoot,withdrawalsRoot,requestsHash,transactions}' >"$run/evidence/block-$n-$side.json"; done
  cmp -s "$run/evidence/block-$n-builder.json" "$run/evidence/block-$n-verifier.json" || ok=false
  cast rpc --rpc-url "$a" debug_getRawBlock "$(printf '0x%x' "$n")" >"$run/evidence/raw-block-$n.json"
done
contract=$(jq -r .contract "$run/evidence/workload.json"); sa=$(cast storage --rpc-url "$a" --block "$last" "$contract" 0); sb=$(cast storage --rpc-url "$b" --block "$last" "$contract" 0)
recipient=0x000000000000000000000000000000000000bEEF
pre_block=$(cast to-dec "$(jq -r .blockNumber "$run/evidence/pre-receipt.json")")
post_block=$(cast to-dec "$(jq -r .blockNumber "$run/evidence/post-receipt.json")")
for rpc in "$a" "$b"; do
  test "$(cast balance --rpc-url "$rpc" --block 0 "$recipient")" = 0
  test "$(cast balance --rpc-url "$rpc" --block "$pre_block" "$recipient")" = 11
  test "$(cast balance --rpc-url "$rpc" --block "$post_block" "$recipient")" = 44
done
for phase in pre at post deploy storage; do
  transaction=$(jq -r .transactionHash "$run/evidence/$phase-receipt.json")
  for side in builder verifier; do
    rpc=$a; test "$side" = verifier && rpc=$b
    cast receipt --json --rpc-url "$rpc" "$transaction" >"$run/evidence/$phase-$side-receipt.json"
    jq -e '.status == "0x1"' "$run/evidence/$phase-$side-receipt.json" >/dev/null
  done
  diff -u <(jq -S . "$run/evidence/$phase-builder-receipt.json") <(jq -S . "$run/evidence/$phase-verifier-receipt.json")
done
pre_file="$run/evidence/block-$((fork-1))-builder.json"; fork_file="$run/evidence/block-$fork-builder.json"
gas_oracle=0x420000000000000000000000000000000000000F
pre_isthmus=$(cast call --rpc-url "$a" --block $((fork-1)) "$gas_oracle" 'isIsthmus()(bool)')
fork_isthmus=$(cast call --rpc-url "$a" --block "$fork" "$gas_oracle" 'isIsthmus()(bool)')
jq -n --argjson fork "$fork" --arg a "$sa" --arg b "$sb" --argjson parity "$ok" \
  --arg preHash "$(jq -r .hash "$pre_file")" --arg forkHash "$(jq -r .hash "$fork_file")" \
  --arg preRoot "$(jq -r .stateRoot "$pre_file")" --arg forkRoot "$(jq -r .stateRoot "$fork_file")" \
  --arg requestsHash "$(jq -r .requestsHash "$fork_file")" --argjson preIsthmus "$pre_isthmus" --argjson forkIsthmus "$fork_isthmus" \
  '{fork:"Isthmus",forkBlock:$fork,builderVerifierParity:$parity,builderStorage:$a,verifierStorage:$b,preBlockHash:$preHash,cutoverBlockHash:$forkHash,preStateRoot:$preRoot,cutoverStateRoot:$forkRoot,cutoverRequestsHash:$requestsHash,gasPriceOracleIsthmus:{pre:$preIsthmus,cutover:$forkIsthmus},recipientBalancesWei:{genesis:0,pre:11,post:44},workloadReceiptsMatch:true}' | tee "$run/evidence/verification.json"
test "$ok" = true; test "$sa" = "$sb"
test "$sa" = 0x0000000000000000000000000000000000000000000000000000000000001234
test "$(jq -r .requestsHash "$pre_file")" = null
test "$(jq -r .requestsHash "$fork_file")" = 0xe3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
test "$pre_isthmus" = false; test "$fork_isthmus" = true
