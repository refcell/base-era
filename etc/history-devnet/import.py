#!/usr/bin/env python3
"""Exercise full-state raw-RLP import with the history and reference nodes."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import urllib.request

from artifacts import capture_provenance, complete_provenance, verify_reference

ROOT = Path(__file__).resolve().parents[2]


def rpc(endpoint, method, params, timeout=10):
    request = urllib.request.Request(
        endpoint,
        json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        {"content-type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        reply = json.load(response)
    if "error" in reply:
        raise RuntimeError(f"{method}: {reply['error']}")
    return reply["result"]


def runtime_source(run_dir):
    return json.loads((run_dir / "runtime.json").read_text())["builder_rpc_url"]


def runtime_genesis(run_dir):
    preferred = run_dir / "generated/l2/genesis.json"
    candidates = [preferred] if preferred.is_file() else sorted(run_dir.glob("**/genesis.json"))
    if not candidates:
        raise RuntimeError(f"no genesis.json below run directory {run_dir}")
    return candidates[0]


def rlp_decode(raw):
    data = bytes.fromhex(raw.removeprefix("0x"))

    def one(pos):
        lead = data[pos]
        if lead <= 0x7F:
            return data[pos : pos + 1], pos + 1
        if lead <= 0xB7:
            size = lead - 0x80
            return data[pos + 1 : pos + 1 + size], pos + 1 + size
        if lead <= 0xBF:
            width = lead - 0xB7
            size = int.from_bytes(data[pos + 1 : pos + 1 + width])
            start = pos + 1 + width
            return data[start : start + size], start + size
        width = lead - 0xF7 if lead > 0xF7 else 0
        size = lead - 0xC0 if lead <= 0xF7 else int.from_bytes(data[pos + 1 : pos + 1 + width])
        start = pos + 1 if lead <= 0xF7 else pos + 1 + width
        end, out = start + size, []
        while start < end:
            value, start = one(start)
            out.append(value)
        return out, end

    return one(0)[0]


def rlp_encode(value):
    if isinstance(value, list):
        body, offset = b"".join(rlp_encode(item) for item in value), 0xC0
    else:
        if len(value) == 1 and value[0] < 0x80:
            return value
        body, offset = value, 0x80
    if len(body) <= 55:
        return bytes([offset + len(body)]) + body
    length = len(body).to_bytes((len(body).bit_length() + 7) // 8, "big")
    return bytes([offset + 55 + len(length)]) + length + body


def mutate(raw, header_index):
    block = rlp_decode(raw)
    old = block[0][header_index]
    block[0][header_index] = old[:-1] + bytes([old[-1] ^ 1])
    return rlp_encode(block)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def environment(binary, manifest):
    env = os.environ.copy()
    env.pop("BASE_HISTORY_MANIFEST", None)
    if manifest is not None:
        env["BASE_HISTORY_MANIFEST"] = str(manifest)
    return env


def run_logged(command, log, env):
    started = time.monotonic()
    with log.open("w") as output:
        result = subprocess.run([str(value) for value in command], stdout=output, stderr=subprocess.STDOUT, env=env)
    return result.returncode, round(time.monotonic() - started, 3)


def verify_node(binary, datadir, genesis, manifest, expected, log):
    port = free_port()
    command = [binary, "node", "--datadir", datadir, "--chain", genesis, "--http",
               "--http.port", str(port), "--http.api", "eth,debug", "--disable-discovery",
               "--port", "0", "--rpc.eth-proof-window", "128"]
    with log.open("w") as output:
        process = subprocess.Popen([str(value) for value in command], stdout=output, stderr=subprocess.STDOUT,
                                   env=environment(binary, manifest))
        endpoint = f"http://127.0.0.1:{port}"
        try:
            for _ in range(120):
                if process.poll() is not None:
                    raise RuntimeError(f"{binary.name} node exited {process.returncode}; see {log}")
                try:
                    rpc(endpoint, "eth_chainId", [])
                    break
                except Exception:
                    time.sleep(0.25)
            else:
                raise RuntimeError(f"{binary.name} HTTP did not start; see {log}")
            block = rpc(endpoint, "eth_getBlockByNumber", [hex(expected["number"]), False])
            latest = int(rpc(endpoint, "eth_blockNumber", []), 16)
            balance = rpc(endpoint, "eth_getBalance", ["0x4200000000000000000000000000000000000015", hex(expected["number"])])
            actual = {key: block[key] for key in ("hash", "stateRoot", "receiptsRoot")}
            if latest != expected["number"] or actual != expected["commitments"]:
                raise RuntimeError(f"{binary.name} verification mismatch: head={latest}, commitments={actual}")
            return {"head": latest, **actual, "state_probe_balance": balance}
        finally:
            process.terminate()
            try:
                process.wait(10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True,
                        help="live history-devnet run directory")
    parser.add_argument("--worker-bin", type=Path, default=ROOT / "target/history-host-node")
    parser.add_argument("--reference-bin", type=Path, default=ROOT / "target/history-reference-node")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output", type=Path, default=None)
    return parser.parse_args()


def main():
    args = parse_args()
    args.reference_bin = verify_reference(args.reference_bin)
    args.manifest = args.manifest or args.run_dir / "manifest.json"
    out = args.output or ROOT / f"target/history-import-{time.strftime('%Y%m%d-%H%M%S')}"
    out.mkdir(parents=True, exist_ok=False)
    approved = json.loads(args.manifest.read_text())
    provenance, launches = capture_provenance(out, "import", {
        "host": args.worker_bin, "reference": args.reference_bin,
        "worker": approved["executable"],
    }, (("manifest", args.manifest), ("genesis", runtime_genesis(args.run_dir))))
    args.worker_bin, args.reference_bin = launches["host"], launches["reference"]
    owned_manifest = out / "worker-manifest.json"
    approved.update(executable=str(launches["worker"]),
                    executable_sha256="0x" + provenance["binaries"]["worker"]["sha256"])
    owned_manifest.write_text(json.dumps(approved, indent=2) + "\n")
    args.manifest = owned_manifest
    summary = {"output": str(out), "tests": []}
    try:
        source, genesis = runtime_source(args.run_dir), runtime_genesis(args.run_dir)
        summary.update(source=source, genesis=str(genesis), manifest=str(args.manifest))
        head = int(rpc(source, "eth_blockNumber", []), 16)
        if head < 22:
            raise RuntimeError(f"source head {head} is below required block 22")
        raws = {number: rpc(source, "debug_getRawBlock", [hex(number)]) for number in range(1, 23)}
        encoded = {number: bytes.fromhex(raws[number][2:]) for number in raws}
        canonical = b"".join(encoded.values())
        pre19 = b"".join(encoded[n] for n in range(1, 19)) + mutate(raws[19], 3)
        post20 = b"".join(encoded[n] for n in range(1, 20)) + mutate(raws[20], 5)
        fixtures = {"blocks-1-22.rlp": canonical, "malformed-pre19-state-root.rlp": pre19,
                    "malformed-post20-receipts-root.rlp": post20}
        summary["rlp"] = {}
        for name, data in fixtures.items():
            (out / name).write_bytes(data)
            summary["rlp"][name] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        source_block = rpc(source, "eth_getBlockByNumber", ["0x16", False])
        expected = {"number": 22, "commitments": {key: source_block[key] for key in ("hash", "stateRoot", "receiptsRoot")}}

        binaries = (args.worker_bin, args.reference_bin)
        for binary in binaries:
            manifest = args.manifest if binary == args.worker_bin else None
            code, seconds = run_logged([binary, "import", "--help"], out / f"{binary.name}-import-help.log", os.environ.copy())
            if code:
                raise RuntimeError(f"{binary} import --help exited {code}")
            datadir = out / f"{binary.name}-canonical"
            env = environment(binary, manifest)
            init_code, init_seconds = run_logged([binary, "init", "--datadir", datadir, "--chain", genesis],
                                                 out / f"{binary.name}-canonical-init.log", env)
            import_code, import_seconds = run_logged([binary, "import", "--datadir", datadir, "--chain", genesis,
                                                      out / "blocks-1-22.rlp"],
                                                     out / f"{binary.name}-canonical-import.log", env)
            if init_code or import_code:
                raise RuntimeError(f"{binary.name} canonical init/import exited {init_code}/{import_code}")
            verified = verify_node(binary, datadir, genesis, manifest, expected,
                                   out / f"{binary.name}-canonical-node.log")
            summary["tests"].append({"test": f"{binary.name} canonical full-state import", "result": "PASS",
                                     "init_exit": init_code, "import_exit": import_code,
                                     "init_seconds": init_seconds, "import_seconds": import_seconds,
                                     "blocks_per_second": round(22 / import_seconds, 3), "verified": verified})

            # fail-on-invalid-block makes the import transactional: no input block is committed.
            for label, fixture, invalid in (("pre19-state-root", "malformed-pre19-state-root.rlp", 19),
                                             ("post20-receipts-root", "malformed-post20-receipts-root.rlp", 20)):
                bad_dir = out / f"{binary.name}-malformed-{label}"
                init_code, _ = run_logged([binary, "init", "--datadir", bad_dir, "--chain", genesis],
                                          out / f"{binary.name}-{label}-init.log", env)
                code, seconds = run_logged([binary, "import", "--fail-on-invalid-block", "--datadir", bad_dir,
                                            "--chain", genesis, out / fixture],
                                           out / f"{binary.name}-{label}-import.log", env)
                if init_code or code == 0:
                    raise RuntimeError(f"{binary.name} malformed {label} init/import exited {init_code}/{code}")
                expected_error = "mismatched block state root" if invalid == 19 else "receipt root mismatch"
                error_log = (out / f"{binary.name}-{label}-import.log").read_text()
                if expected_error not in error_log:
                    raise RuntimeError(f"{binary.name} failed for a different reason than {expected_error}")
                block = rpc(source, "eth_getBlockByNumber", ["0x0", False])
                malformed_expected = {"number": 0,
                                      "commitments": {key: block[key] for key in ("hash", "stateRoot", "receiptsRoot")}}
                verified = verify_node(binary, bad_dir, genesis, manifest, malformed_expected,
                                       out / f"{binary.name}-{label}-node.log")
                summary["tests"].append({"test": f"{binary.name} rejects malformed {label}", "result": "PASS",
                                         "import_exit": code, "seconds": seconds, "verified": verified,
                                         "invalid_block": invalid, "invalid_block_committed": False})
        summary["result"] = "PASS"
    except Exception as error:
        summary.update(result="FAIL", failure=str(error))
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    complete_provenance(out, provenance)
    print(json.dumps(summary, indent=2), file=sys.stdout if summary["result"] == "PASS" else sys.stderr)
    return 0 if summary["result"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
