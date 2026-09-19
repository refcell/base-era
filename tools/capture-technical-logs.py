#!/usr/bin/env python3
"""Capture hash-bound cutover RPC evidence without publishing runtime credentials."""

import datetime
import json
from pathlib import Path
import re
import sys
import time
import urllib.request


run, output = map(Path, sys.argv[1:])
runtime = json.loads((run / "runtime.json").read_text())
launcher = run / "launcher.log"


def rpc(url, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(url, body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        envelope = json.load(response)
    if "error" in envelope:
        raise RuntimeError(envelope["error"])
    return envelope["result"]


def field(line, name):
    match = re.search(r"\b" + name + r"=([^ ]+)", line)
    assert match, (name, line)
    return match.group(1)


builder = runtime["builder_rpc_url"]
verifier = runtime["verifier_rpc_url"]
blocks = []
for number, era, expected in ((19, "holocene", "0x" + "0" * 64),
                              (20, "isthmus", "0x" + "0" * 63 + "1")):
    tag = hex(number)
    builder_block = rpc(builder, "eth_getBlockByNumber", [tag, False])
    verifier_block = rpc(verifier, "eth_getBlockByNumber", [tag, False])
    assert builder_block["hash"] == verifier_block["hash"]
    assert builder_block["stateRoot"] == verifier_block["stateRoot"]
    blocks.append({
        "number": number,
        "era": era,
        "hash": builder_block["hash"],
        "parentHash": builder_block["parentHash"],
        "builderStateRoot": builder_block["stateRoot"],
        "verifierHash": verifier_block["hash"],
        "verifierStateRoot": verifier_block["stateRoot"],
        "builderVerifierHashAndStateMatch": True,
        "expected": expected,
    })

# Warm only the historical path. The two following calls are the captured evidence.
warm_params = [{"to": "0x420000000000000000000000000000000000000f", "data": "0xb54501bc"},
               {"blockHash": blocks[0]["hash"], "requireCanonical": True}]
rpc(builder, "eth_call", warm_params)
offset = launcher.stat().st_size
captured_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
for block in blocks:
    params = [{"to": "0x420000000000000000000000000000000000000f", "data": "0xb54501bc"},
              {"blockHash": block["hash"], "requireCanonical": True}]
    start = time.perf_counter_ns()
    result = rpc(builder, "eth_call", params)
    block["httpMs"] = (time.perf_counter_ns() - start) / 1e6
    block["params"] = params
    block["result"] = result
    assert result == block["expected"]

with launcher.open("rb") as handle:
    handle.seek(offset)
    fresh_lines = handle.read().decode().splitlines()
starts = [line for line in fresh_lines if "historical worker execution started request=rpc-" in line]
assert len(starts) == 1, starts
started = starts[0]
request_id = field(started, "request")
assert request_id.endswith("-" + blocks[0]["hash"])
ends = [line for line in fresh_lines if "historical worker execution completed" in line
        and field(line, "request") == request_id]
assert len(ends) == 1, ends
ended = ends[0]
assert field(started, "worker_pid") == field(ended, "worker_pid")
assert field(started, "era") == "holocene"
blocks[0]["worker"] = {
    "requestId": request_id,
    "workerPid": int(field(started, "worker_pid")),
    "era": field(started, "era"),
    "artifact": field(started, "artifact"),
    "configuration": field(started, "configuration"),
    "binding": field(started, "binding"),
    "elapsedUs": int(field(ended, "elapsed_us")),
    "stateReads": int(field(ended, "requests")),
    "readBytes": int(field(ended, "read_bytes")),
    "requestBytes": int(field(ended, "request_bytes")),
}
blocks[1]["worker"] = None

all_lines = launcher.read_text().splitlines()
cutover_lines = [line for line in all_lines if any(marker in line for marker in (
    "Enqueuing external unsafe payload block_number=19 ",
    "Block added to canonical chain number=19 ",
    "Activated upgrade block_number=20 upgrade=\"isthmus\"",
    "Enqueuing external unsafe payload block_number=20 ",
    "Block added to canonical chain number=20 ",
))]

evidence = {
    "capturedAt": captured_at,
    "sourceRun": str(run),
    "contract": "0x420000000000000000000000000000000000000f",
    "selector": "0xb54501bc",
    "function": "GasPriceOracle.isIsthmus()",
    "blocks": blocks,
    "checks": {
        "block19ReturnsFalse": blocks[0]["result"] == blocks[0]["expected"],
        "block20ReturnsTrue": blocks[1]["result"] == blocks[1]["expected"],
        "bothBuilderVerifierHashAndStateMatch": all(
            block["builderVerifierHashAndStateMatch"] for block in blocks),
        "historicalRequestBoundToBlock19Hash": request_id.endswith("-" + blocks[0]["hash"]),
    },
    "methodology": "Fresh builder eth_call requests used EIP-1898 blockHash objects with requireCanonical=true. Builder/verifier block hash and stateRoot were fetched and asserted equal first. Block 19 was warmed once; reported HTTP timings are single loopback observations, not benchmarks. Block 20 executes natively, so no historical-worker interval is expected.",
}
output.write_text(json.dumps(evidence, indent=2) + "\n")
output.with_suffix(".worker.log").write_text("\n".join((started, ended)) + "\n")
output.with_suffix(".imports.log").write_text("\n".join(cutover_lines) + "\n")
print(json.dumps(evidence, indent=2))
