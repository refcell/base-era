#!/usr/bin/env python3
"""Bounded, read-only JSON-RPC latency, traffic, CPU, and memory measurement."""

import argparse
import hashlib
import json
import os
import platform
import statistics
import time
import urllib.request
from pathlib import Path


def process_sample(pid):
    if pid is None:
        return None
    stat = Path(f"/proc/{pid}/stat").read_text().split()
    status = Path(f"/proc/{pid}/status").read_text().splitlines()
    values = {line.split(":", 1)[0]: line.split(":", 1)[1].strip() for line in status}
    return {
        "cpu_ticks": int(stat[13]) + int(stat[14]),
        "rss_bytes": int(values["VmRSS"].split()[0]) * 1024,
        "high_water_rss_bytes": int(values["VmHWM"].split()[0]) * 1024,
    }


def rpc(url, payload, timeout):
    body = json.dumps(payload, separators=(",", ":")).encode()
    request = urllib.request.Request(url, body, {"Content-Type": "application/json"})
    started = time.perf_counter_ns()
    with urllib.request.urlopen(request, timeout=timeout) as response:
        result = response.read()
    elapsed = (time.perf_counter_ns() - started) / 1_000_000
    parsed = json.loads(result)
    if "error" in parsed:
        raise RuntimeError(f"RPC error: {parsed['error']}")
    return elapsed, len(body), len(result), parsed["result"]


def percentile(values, fraction):
    return sorted(values)[round((len(values) - 1) * fraction)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rpc", action="append", required=True, metavar="LABEL=URL")
    parser.add_argument("--requests", type=int, default=30)
    parser.add_argument("--timeout", type=float, default=5)
    parser.add_argument("--pid", type=int, help="optional shared server PID for process samples")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 2 <= args.requests <= 1000:
        parser.error("--requests must be between 2 and 1000")

    payload = {
        "jsonrpc": "2.0", "id": 1, "method": "eth_getBlockByNumber",
        "params": ["0x13", False],
    }
    before = process_sample(args.pid)
    wall_started = time.perf_counter()
    measurements = []
    for specification in args.rpc:
        label, separator, url = specification.partition("=")
        if not separator:
            parser.error("--rpc requires LABEL=URL")
        samples = [rpc(url, payload, args.timeout) for _ in range(args.requests)]
        latencies = [sample[0] for sample in samples]
        results = [json.dumps(sample[3], sort_keys=True, separators=(",", ":")) for sample in samples]
        if len(set(results)) != 1:
            raise RuntimeError(f"{label}: inconsistent RPC results")
        warm = latencies[1:]
        measurements.append({
            "label": label, "url": url, "requests": len(samples),
            "first_request_ms": latencies[0],
            "warm_median_ms": statistics.median(warm),
            "warm_p95_ms": percentile(warm, 0.95),
            "warm_min_ms": min(warm), "warm_max_ms": max(warm),
            "request_bytes": sum(sample[1] for sample in samples),
            "response_bytes": sum(sample[2] for sample in samples),
            "result_sha256": hashlib.sha256(results[0].encode()).hexdigest(),
        })
    wall_seconds = time.perf_counter() - wall_started
    after = process_sample(args.pid)
    process = None
    if before is not None:
        process = {
            "pid": args.pid,
            "cpu_seconds_during_measurement":
                (after["cpu_ticks"] - before["cpu_ticks"]) / os.sysconf("SC_CLK_TCK"),
            "wall_seconds": wall_seconds,
            "rss_bytes_before": before["rss_bytes"], "rss_bytes_after": after["rss_bytes"],
            "high_water_rss_bytes_after": after["high_water_rss_bytes"],
            "scope": "entire process; may include work unrelated to measured RPC endpoints",
        }
    report = {
        "schema": "base-history-rpc-measurement-v1",
        "captured_at_unix_seconds": time.time(),
        "environment": {"platform": platform.platform(), "python": platform.python_version(),
                        "logical_cpus": os.cpu_count()},
        "workload": payload, "measurements": measurements, "process": process,
        "limitations": [
            "Read-only RPC measurement; it does not mutate or restart either node.",
            "First-request latency is cache-state-observed, not an OS page-cache cold start.",
            "Startup time is not measured because this mode does not own the node lifecycle.",
        ],
    }
    encoded = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded)
    print(encoded, end="")


if __name__ == "__main__":
    main()
