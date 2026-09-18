#!/usr/bin/env python3
"""Benchmark historical RPC execution on fresh, owned copies of replay databases."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import socket
import statistics
import struct
import subprocess
import time
import urllib.request

from artifacts import capture_provenance, complete_provenance, verify_reference

ROOT = Path(__file__).resolve().parents[2]
ORACLE = {"to": "0x420000000000000000000000000000000000000F", "data": "0xb54501bc"}


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return "0x" + digest.hexdigest()


def free_port():
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


def rpc(url, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(url, body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=40) as response:
        reply = json.load(response)
    if "error" in reply:
        raise RuntimeError(f"{method}: {reply['error']}")
    return reply["result"]


def timed_rpc(url, method, params):
    started = time.perf_counter_ns()
    result = rpc(url, method, params)
    return result, (time.perf_counter_ns() - started) / 1_000_000


def usage_record(usage, elapsed_ms):
    return {
        "user_seconds": usage.ru_utime, "system_seconds": usage.ru_stime,
        "elapsed_ms": elapsed_ms, "peak_rss_kib": usage.ru_maxrss,
    }


def child_resources(parent):
    """Snapshot owned persistent workers before host exit (wait4 may not include them)."""
    records = []
    ticks = os.sysconf("SC_CLK_TCK")
    for status_path in Path("/proc").glob("[0-9]*/status"):
        try:
            fields = dict(line.split(":", 1) for line in status_path.read_text().splitlines())
            if int(fields["PPid"]) != parent:
                continue
            executable = os.readlink(status_path.parent / "exe")
            if "memfd:base-history-worker" not in executable:
                continue
            stat = (status_path.parent / "stat").read_text().rsplit(") ", 1)[1].split()
            records.append({"pid": int(status_path.parent.name), "executable": executable,
                            "user_seconds": int(stat[11]) / ticks,
                            "system_seconds": int(stat[12]) / ticks,
                            "rss_kib": int(fields["VmRSS"].split()[0]),
                            "peak_rss_kib": int(fields["VmHWM"].split()[0])})
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
    return records


def distribution(values):
    ordered = sorted(values)
    percentile = lambda p: ordered[min(len(ordered) - 1, int((len(ordered) - 1) * p))]
    return {"count": len(values), "min_ms": ordered[0], "median_ms": statistics.median(ordered),
            "p95_ms": percentile(.95), "max_ms": ordered[-1], "mean_ms": statistics.mean(values),
            "samples_ms": values}


def launch(name, binary, datadir, genesis, manifest, run_dir):
    http, auth, p2p = free_port(), free_port(), free_port()
    log_path = run_dir / f"{name}.log"
    command = [str(binary), "node", "--datadir", str(datadir), "--chain", str(genesis), "--http",
               "--http.api", "eth,net,web3,debug", "--disable-discovery", "--port", str(p2p),
               "--http.addr", "127.0.0.1", "--http.port", str(http), "--authrpc.addr", "127.0.0.1",
               "--authrpc.port", str(auth), "--authrpc.jwtsecret", str(run_dir / "jwt.hex"),
               "--ipcdisable"]
    env = os.environ.copy()
    if manifest:
        env["BASE_HISTORY_MANIFEST"] = str(manifest)
    else:
        env.pop("BASE_HISTORY_MANIFEST", None)
    log = open(log_path, "wb")
    started = time.perf_counter_ns()
    process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, env=env,
                               start_new_session=True)
    process.benchmark_started_ns = started
    url = f"http://127.0.0.1:{http}"
    for _ in range(400):
        if process.poll() is not None:
            raise RuntimeError(f"{name} exited before readiness; inspect {log_path}")
        try:
            rpc(url, "eth_blockNumber", [])
            return process, log, url, (time.perf_counter_ns() - started) / 1_000_000
        except Exception:
            time.sleep(.025)
    raise RuntimeError(f"{name} readiness timeout")


def worker_probe(binary, run_dir):
    request = {"version": 999, "request_id": "benchmark-unsupported", "executable_sha256": "unused",
               "worker_pid": 0, "binding_hash": "unused", "era": "bedrock", "chain_id": "1",
               "genesis_identity": "unused", "genesis_header_hash": "unused",
               "config_identity": "unused", "genesis": {}, "parent_header_rlp": "0x",
               "child_block_rlp": "0x", "operation": None}
    body = json.dumps(request, separators=(",", ":")).encode()
    started = time.perf_counter_ns()
    process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE)
    process.stdin.write(struct.pack(">I", len(body)) + body)
    process.stdin.close()
    stdout, stderr = process.stdout.read(), process.stderr.read()
    _, status, usage = os.wait4(process.pid, 0)
    process.returncode = os.waitstatus_to_exitcode(status)
    elapsed = (time.perf_counter_ns() - started) / 1_000_000
    if process.returncode != 0:
        raise RuntimeError(f"worker probe failed: {stderr.decode(errors='replace')}")
    size = struct.unpack(">I", stdout[:4])[0]
    reply = json.loads(stdout[4:4 + size])
    return {"label": "process startup + framed request parsing (not cold page cache)",
            "wall_ms": elapsed, "response": reply, "resource_usage": usage_record(usage, elapsed)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--genesis", type=Path, required=True)
    parser.add_argument("--worker-datadir", type=Path, required=True)
    parser.add_argument("--reference-datadir", type=Path, required=True)
    parser.add_argument("--source-manifest", type=Path, required=True)
    parser.add_argument("--approval", type=Path, default=ROOT / "target/history-artifacts/APPROVAL.json")
    parser.add_argument("--host-bin", type=Path, default=ROOT / "target/history-host-node")
    parser.add_argument("--reference-bin", type=Path, default=ROOT / "target/history-reference-node")
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "etc/history-devnet/evidence/benchmark.json")
    parser.add_argument("--repetitions", type=int, default=20)
    args = parser.parse_args()
    args.reference_bin = verify_reference(args.reference_bin)
    if args.run_dir.exists():
        raise SystemExit(f"owned run directory already exists: {args.run_dir}")
    args.run_dir.mkdir(parents=True)
    shutil.copytree(args.worker_datadir, args.run_dir / "worker-datadir")
    shutil.copytree(args.reference_datadir, args.run_dir / "reference-datadir")
    (args.run_dir / "jwt.hex").write_text("11" * 32 + "\n")
    os.chmod(args.run_dir / "jwt.hex", 0o600)
    approval = json.loads(args.approval.read_text())
    provenance, launches = capture_provenance(args.run_dir, "benchmark", {
        "host": args.host_bin, "reference": args.reference_bin,
        "worker": approval["executable"],
    }, (("approval", args.approval), ("manifest", args.source_manifest),
        ("genesis", args.genesis)))
    args.host_bin, args.reference_bin = launches["host"], launches["reference"]
    approval["executable"] = str(launches["worker"])
    approval["executable_sha256"] = "0x" + provenance["binaries"]["worker"]["sha256"]
    manifest = json.loads(args.source_manifest.read_text())
    manifest.update({"executable": approval["executable"],
                     "executable_sha256": approval["executable_sha256"]})
    manifest_path = args.run_dir / "worker-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    processes, logs, usages, persistent_resources = [], [], {}, {}
    try:
        worker, worker_log, worker_url, worker_ready = launch("worker-host", args.host_bin,
            args.run_dir / "worker-datadir", args.genesis, manifest_path, args.run_dir)
        processes.append(worker); logs.append(worker_log)
        reference, reference_log, reference_url, reference_ready = launch("reference", args.reference_bin,
            args.run_dir / "reference-datadir", args.genesis, None, args.run_dir)
        processes.append(reference); logs.append(reference_log)
        results = {}
        workloads = [("historical_eth_call", "eth_call", [ORACLE, "0x13"]),
                     ("historical_estimate_gas", "eth_estimateGas", [ORACLE, "0x13"]),
                     ("historical_block_19", "eth_getBlockByNumber", ["0x13", False]),
                     ("current_eth_call", "eth_call", [ORACLE, "0x14"])]
        for name, method, params in workloads:
            entries = {}
            for node, url in (("worker", worker_url), ("reference", reference_url)):
                first, first_ms = timed_rpc(url, method, params)
                samples, repeated = [], []
                for _ in range(args.repetitions):
                    value, elapsed = timed_rpc(url, method, params)
                    repeated.append(value); samples.append(elapsed)
                if any(value != first for value in repeated):
                    raise RuntimeError(f"non-deterministic result for {node} {name}")
                entries[node] = {"first_result": first, "observed_first_ms": first_ms,
                                 "repeated": distribution(samples)}
            entries["parity"] = entries["worker"]["first_result"] == entries["reference"]["first_result"]
            if not entries["parity"]:
                raise RuntimeError(f"reference mismatch for {name}")
            if method == "eth_call":
                expected = "0x" + "00" * 31 + ("01" if name == "current_eth_call" else "00")
                if entries["worker"]["first_result"] != expected:
                    raise RuntimeError(f"incorrect activation flag for {name}")
            results[name] = entries
    finally:
        for name, process in zip(("worker", "reference"), processes):
            persistent_resources[name] = child_resources(process.pid)
        for process in processes:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGINT)
        for name, process in zip(("worker", "reference"), processes):
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
            usages[name] = usage_record(usage, (time.perf_counter_ns() - process.benchmark_started_ns) / 1_000_000)
        for log in logs:
            log.close()
    host_log = (args.run_dir / "worker-host.log").read_text(errors="replace")
    plain_log = re.sub(r"\x1b\[[0-9;]*m", "", host_log)
    accesses = []
    for line in plain_log.splitlines():
        fields = {key: int(value) for key, value in
                  re.findall(r"(worker_pid|elapsed_us|requests|read_bytes|request_bytes)=(\d+)", line)}
        if {"elapsed_us", "requests", "read_bytes"} <= fields.keys():
            fields["state_read_request_count"] = fields.pop("requests")
            fields["state_read_frame_bytes"] = fields.pop("read_bytes")
            accesses.append(fields)
    worker_pids = [entry["worker_pid"] for entry in accesses if "worker_pid" in entry]
    reused_warm_requests = sum(left == right for left, right in zip(worker_pids, worker_pids[1:]))
    if reused_warm_requests < 2 * args.repetitions:
        raise RuntimeError("historical benchmark did not demonstrate warm worker reuse")
    evidence = {
        "schema": 1, "measurement_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "cold_definition": "Fresh node processes and owned datadir copies; OS page cache was not dropped.",
        "inputs": {"genesis": str(args.genesis), "source_datadirs": [str(args.worker_datadir), str(args.reference_datadir)],
                   "run_dir": str(args.run_dir), "repetitions": args.repetitions},
        "binaries": {"host": {"path": str(args.host_bin), "sha256": sha256(args.host_bin)},
                     "reference": {"path": str(args.reference_bin), "sha256": sha256(args.reference_bin)},
                     "release_worker": {"path": approval["executable"], "sha256": sha256(approval["executable"])},
                     "manifest_approved_sha256": approval["executable_sha256"]},
        "environment": {"kernel": platform.release(), "architecture": platform.machine(),
                        "python": platform.python_version(), "rust_toolchain": "1.96.0",
                        "rustc_verbose": subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
                        "cpu": next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name"))},
        "startup_readiness_ms": {"worker": worker_ready, "reference": reference_ready},
        "workloads": results, "worker_state_access_from_host_log": accesses,
        "persistent_worker_reuse": {"logged_worker_pids": worker_pids,
                                    "consecutive_same_pid_count": reused_warm_requests,
                                    "available": bool(worker_pids)},
        "persistent_worker_resources_before_shutdown": persistent_resources,
        "node_resource_usage_including_waited_descendants": {
            "method": "Linux wait4 rusage after graceful shutdown; includes only descendants each node waited for",
            "limitation": "Persistent worker CPU and peak RSS may be excluded when the host does not wait for that child; these values are not claimed as complete worker cost.",
            "worker": usages["worker"], "reference": usages["reference"]},
        "release_worker_probe": worker_probe(Path(approval["executable"]), args.run_dir),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2) + "\n")
    complete_provenance(args.run_dir, provenance)
    print(args.output)


if __name__ == "__main__":
    main()
