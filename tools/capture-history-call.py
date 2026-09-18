#!/usr/bin/env python3
"""Capture a real historical RPC envelope and its correlated worker log interval.

Usage: python3 tools/capture-history-call.py RUN_DIR OUTPUT.json
Publishes only selected RPC/log fields, never runtime credentials.
"""

import datetime
import json
from pathlib import Path
import re
import sys
import time
import urllib.request


run, output = map(Path, sys.argv[1:])
runtime = json.loads((run / "runtime.json").read_text())
log = run / "launcher.log"


def rpc(url, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(url, body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)
    if "error" in result:
        raise RuntimeError(result["error"])
    return result["result"]


builder = runtime["builder_rpc_url"]
block = rpc(builder, "eth_getBlockByNumber", ["0x13", False])
verifier = rpc(runtime["verifier_rpc_url"], "eth_getBlockByNumber", ["0x13", False])
assert block["hash"] == verifier["hash"]
assert block["stateRoot"] == verifier["stateRoot"]
params = [{"to": "0x420000000000000000000000000000000000000f", "data": "0xb54501bc"},
          {"blockHash": block["hash"], "requireCanonical": True}]
# Warm-up is explicit; the recorded sample is not a startup benchmark.
rpc(builder, "eth_call", params)
offset = log.stat().st_size
wall_start = time.time_ns()
start = time.perf_counter_ns()
result = rpc(builder, "eth_call", params)
elapsed_ms = (time.perf_counter_ns() - start) / 1e6
assert result == "0x" + "0" * 64
with log.open("rb") as handle:
    handle.seek(offset)
    lines = handle.read().decode().splitlines()
starts = [line for line in lines if "historical worker execution started request=rpc-" in line]
assert len(starts) == 1, starts
started = starts[0]


def field(line, name):
    match = re.search(r"\b" + name + r"=([^ ]+)", line)
    assert match, name
    return match.group(1)


request_id = field(started, "request")
assert request_id.endswith("-" + block["hash"])
ends = [line for line in lines if "historical worker execution completed" in line
        and field(line, "request") == request_id]
assert len(ends) == 1, ends
ended = ends[0]
assert field(started, "worker_pid") == field(ended, "worker_pid")
worker_start = datetime.datetime.fromisoformat(started.split()[0]).timestamp()
worker_offset_ms = (worker_start - wall_start / 1e9) * 1000
worker_ms = int(field(ended, "elapsed_us")) / 1000
assert 0 <= worker_offset_ms < worker_offset_ms + worker_ms <= elapsed_ms
data = {
    "capturedAt": datetime.datetime.fromtimestamp(wall_start / 1e9, datetime.timezone.utc).isoformat(),
    "method": "eth_call", "params": params, "result": result,
    "blockNumber": 19, "blockHash": block["hash"], "parentHash": block["parentHash"],
    "stateRoot": block["stateRoot"], "builderVerifierMatch": True,
    "requestId": request_id, "workerPid": int(field(started, "worker_pid")),
    "artifact": field(started, "artifact"), "configuration": field(started, "configuration"),
    "binding": field(started, "binding"), "era": field(started, "era"),
    "httpMs": elapsed_ms, "workerOffsetMs": worker_offset_ms, "workerMs": worker_ms,
    "stateReads": int(field(ended, "requests")), "readBytes": int(field(ended, "read_bytes")),
    "requestBytes": int(field(ended, "request_bytes")),
    "logLines": [started, ended],
    "methodology": "One warm loopback sample. HTTP envelope uses a monotonic timer; worker offset uses same-machine wall-clock log timestamps. Worker exchange includes state reads, IPC and execution, not EVM-only time. Remaining time is unattributed; no phase breakdown inferred.",
}
output.write_text(json.dumps(data, indent=2) + "\n")
print(f"Saved {output}: HTTP {elapsed_ms:.3f} ms, worker exchange {worker_ms:.3f} ms")
