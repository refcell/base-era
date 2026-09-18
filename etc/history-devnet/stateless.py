#!/usr/bin/env python3
"""Record and replay the history-devnet cutover stateless fixtures."""

import argparse
import hashlib
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import time
import urllib.request
from pathlib import Path

from artifacts import capture_provenance, complete_provenance


BLOCKS = (19, 20, 21)


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def rpc(url, method, params):
    request = urllib.request.Request(
        url,
        json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        {"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        value = json.load(response)
    if "error" in value:
        raise RuntimeError(f"RPC {method} failed: {value['error']}")
    return value["result"]


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir", type=Path)
    parser.add_argument("source_datadir", type=Path, help="closed replay datadir to copy")
    parser.add_argument("node_binary", type=Path)
    parser.add_argument("--fixture-binary", type=Path, default=Path("target/debug/examples/fixture"))
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    run = args.run_dir.resolve()
    source = args.source_datadir.resolve()
    node_binary = args.node_binary.resolve()
    fixture_binary = args.fixture_binary.resolve()
    output = (args.output or run / "evidence/stateless-final").resolve()
    genesis = run / "generated/l2/genesis.json"
    rollup = run / "generated/l2/rollup.json"
    manifest = run / "manifest.json"
    jwt = run / "generated/jwt.hex"
    for required in (source, node_binary, fixture_binary, genesis, rollup, manifest, jwt):
        if not required.exists():
            parser.error(f"required path does not exist: {required}")
    if output.exists():
        parser.error(f"output already exists: {output}")

    (output / "logs").mkdir(parents=True)
    fixtures = output / "fixtures"
    fixtures.mkdir()
    approved = json.loads(manifest.read_text())
    provenance, launches = capture_provenance(output, "stateless", {
        "host": node_binary, "worker": approved["executable"], "fixture": fixture_binary,
    }, (("manifest", manifest), ("genesis", genesis), ("rollup", rollup)))
    node_binary, fixture_binary = launches["host"], launches["fixture"]
    owned_manifest = output / "worker-manifest.json"
    approved.update(executable=str(launches["worker"]),
                    executable_sha256="0x" + provenance["binaries"]["worker"]["sha256"])
    owned_manifest.write_text(json.dumps(approved, indent=2) + "\n")
    manifest = owned_manifest
    datadir = output / "datadir"
    shutil.copytree(source, datadir)
    http_port, auth_port, p2p_port = port(), port(), port()
    rpc_url = f"http://127.0.0.1:{http_port}"
    command = [str(node_binary), "node", "--datadir", str(datadir), "--chain", str(genesis),
               "--http", "--http.addr", "127.0.0.1", "--http.port", str(http_port),
               "--http.api", "eth,debug", "--rpc.eth-proof-window", "128",
               "--authrpc.addr", "127.0.0.1", "--authrpc.port", str(auth_port),
               "--authrpc.jwtsecret", str(jwt), "--addr", "127.0.0.1", "--port",
               str(p2p_port), "--disable-discovery"]
    (output / "node-command.json").write_text(json.dumps(command, indent=2) + "\n")
    result = {"blocks": list(BLOCKS), "record_witness": [], "offline_fixture": [],
              "full_header_parity": [], "node": "not_started", "error": None}
    environment = os.environ.copy()
    environment["BASE_HISTORY_MANIFEST"] = str(manifest)
    process = None
    failure = None
    try:
        node_log = (output / "logs/node.log").open("wb")
        process = subprocess.Popen(command, stdout=node_log, stderr=subprocess.STDOUT, env=environment)
        result["node"] = "running"
        for _ in range(120):
            if process.poll() is not None:
                raise RuntimeError(f"node exited before readiness with {process.returncode}")
            try:
                if rpc(rpc_url, "eth_blockNumber", []) is not None:
                    break
            except Exception:
                time.sleep(0.5)
        else:
            raise RuntimeError("node did not become ready in 60 seconds")

        for block in BLOCKS:
            header = rpc(rpc_url, "eth_getBlockByNumber", [hex(block), False])
            (output / f"block-{block}-full-header.json").write_text(json.dumps(header, indent=2) + "\n")
            cmd = [str(fixture_binary), "record-witness", rpc_url, str(rollup), str(fixtures), str(block)]
            log = output / f"logs/record-witness-{block}.log"
            with log.open("wb") as stream:
                completed = subprocess.run(cmd, stdout=stream, stderr=subprocess.STDOUT, env=environment)
            result["record_witness"].append({"block": block, "exit": completed.returncode})
            if completed.returncode:
                raise RuntimeError(f"record-witness failed for block {block}")

        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=10)
        result["node"] = "stopped"

        for block in BLOCKS:
            archive = fixtures / f"block-{block}.tar.gz"
            with tarfile.open(archive, "r:gz") as tar:
                fixture = json.load(tar.extractfile(f"block-{block}/fixture.json"))
            rpc_header = json.loads((output / f"block-{block}-full-header.json").read_text())
            expected = fixture["expected_block_hash"]
            comparison = {"block": block, "rpc_hash": rpc_header["hash"],
                          "fixture_expected_hash": expected, "full_header_parity": rpc_header["hash"] == expected}
            result["full_header_parity"].append(comparison)
            cmd = [str(fixture_binary), "run", str(archive)]
            with (output / f"logs/run-block-{block}.log").open("wb") as stream:
                completed = subprocess.run(cmd, stdout=stream, stderr=subprocess.STDOUT, env=environment)
            result["offline_fixture"].append({"block": block, "exit": completed.returncode})
            if completed.returncode or not comparison["full_header_parity"]:
                raise RuntimeError(f"offline replay/parity failed for block {block}")
        result["all_match"] = True
        result["fixtures"] = [{"block": block, "path": f"fixtures/block-{block}.tar.gz",
                               "sha256": sha256(fixtures / f"block-{block}.tar.gz")} for block in BLOCKS]
    except Exception as error:
        failure = error
        result["error"] = str(error)
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            result["node"] = "stopped_after_error"
        result["node_exit"] = process.returncode if process is not None else None
        (output / "results.json").write_text(json.dumps(result, indent=2) + "\n")
        complete_provenance(output, provenance)
    if failure:
        print(f"stateless parity failed: {failure}", file=sys.stderr)
        return 1
    print(output / "results.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
