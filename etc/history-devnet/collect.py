#!/usr/bin/env python3
"""Capture portable acceptance evidence, excluding Engine credentials and mutable databases."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

from replay import rpc


ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "etc/history-devnet/evidence/final")
    args = parser.parse_args()
    run = args.run.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    runtime = json.loads((run / "runtime.json").read_text())
    manifest = json.loads((run / "manifest.json").read_text())
    for source, name in [
        ("evidence/verification.json", "live.json"),
        ("replay-evidence/summary.json", "replay.json"),
        ("import-evidence/summary.json", "import.json"),
        ("failure-evidence/evidence.json", "failures.json"),
        ("evidence/stateless-final/results.json", "stateless.json"),
        ("evidence/benchmark.json", "benchmark.json"),
    ]:
        shutil.copy2(run / source, args.output / name)
    workload = args.output / "workload"
    workload.mkdir()
    shutil.copy2(run / "evidence/workload.json", workload / "workload.json")
    for phase in ("pre", "at", "post", "deploy", "storage"):
        for side in ("builder", "verifier"):
            name = f"{phase}-{side}-receipt.json"
            shutil.copy2(run / "evidence" / name, workload / name)
    shutil.copy2(run / "import-evidence/blocks-1-22.rlp", args.output / "blocks-1-22.rlp")
    corpus = args.output / "corpus"
    corpus.mkdir()
    for number in (19, 20, 21):
        fixture = run / f"evidence/stateless-final/fixtures/block-{number}.tar.gz"
        shutil.copy2(fixture, corpus / fixture.name)
    log = (run / "launcher.log").read_text()
    route_lines = [line for line in log.splitlines() if "historical worker execution started" in line]
    routing = []
    for number in range(1, 23):
        block = rpc(runtime["builder_rpc_url"], "eth_getBlockByNumber", [hex(number), False])
        matching = [line for line in route_lines
                    if f'-{block["hash"]} ' in line and "operation=None " in line]
        pids = sorted({int(re.search(r"worker_pid=(\d+)", line)[1]) for line in matching})
        historical = number < runtime["isthmus_block"]
        if bool(pids) != historical:
            raise RuntimeError(f"unexpected canonical worker routing for block {number}: {pids}")
        routing.append({"operation": "execute-block", "number": number, "hash": block["hash"],
                        "parent_hash": block["parentHash"], "state_root": block["stateRoot"],
                        "timestamp": block["timestamp"], "route": "worker" if historical else "local",
                        "era": "holocene" if historical else "isthmus",
                        "configuration_identity": manifest["config_identity"],
                        "request_bindings": [re.search(r"binding=(\S+)", line)[1] for line in matching],
                        "worker_pids": pids,
                        "worker_sha256": manifest["executable_sha256"] if historical else None})
    (args.output / "routing.json").write_text(json.dumps(routing, indent=2) + "\n")
    (args.output / "routing.log").write_text("\n".join(route_lines) + "\n")
    inputs = ["Cargo.lock", "rust-toolchain.toml", "etc/history-worker/Cargo.lock",
              "etc/history-worker/rust-toolchain.toml", "sources/frozen-sources.json",
              "sources/reth-history.patch", "historical/reference/Cargo.lock",
              "etc/history-devnet/reference-cli.patch", "etc/history-devnet/optimism-isthmus.patch"]
    changed = subprocess.check_output(["git", "diff", "--name-only", "HEAD", "-z"], cwd=ROOT).split(b"\0")
    untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard", "-z"],
                                       cwd=ROOT).split(b"\0")
    source_paths = sorted({path.decode() for path in changed + untracked if path
                           and not path.startswith(b"etc/history-devnet/evidence/")})
    record = {
        "base_source": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "base_branch": subprocess.check_output(["git", "branch", "--show-current"], cwd=ROOT, text=True).strip(),
        "reth_source": "5877708bbf9219c44758cd2ce28a365f738661f7",
        "rustc": subprocess.check_output(["rustc", "-Vv"], cwd=ROOT, text=True).strip(),
        "inputs_sha256": {path: digest(ROOT / path) for path in inputs},
        "working_source_sha256": {path: digest(ROOT / path) for path in source_paths if (ROOT / path).is_file()},
        "host_sha256": digest(ROOT / "target/history-host-node"),
        "reference_sha256": digest(ROOT / "target/history-reference-node"),
        "devnet_sha256": digest((run / "process").read_text().strip()),
        "worker_sha256": digest(manifest["executable"]),
        "genesis_identity": manifest["genesis_identity"], "configuration_identity": manifest["config_identity"],
        "chain_id": manifest["chain_id"], "genesis_header_hash": manifest["genesis_header_hash"],
        "fixture_sha256": {path.name: digest(path) for path in sorted(corpus.glob("*.tar.gz"))},
        "full_evidence_location": str(run),
    }
    (args.output / "provenance.json").write_text(json.dumps(record, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
