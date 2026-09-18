#!/usr/bin/env python3
"""Replay a live history devnet into worker and reference Engine APIs."""

import argparse
import base64
import hashlib
import hmac
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.request

from artifacts import capture_provenance, complete_provenance, verify_reference

ROOT = Path(__file__).resolve().parents[2]


class RpcError(RuntimeError):
    """A JSON-RPC or transport failure."""

    def __init__(self, method, detail, transport=False):
        super().__init__(f"{method}: {detail}")
        self.detail = detail
        self.transport = transport


def rpc(url, method, params, token=None, transcript=None):
    body = {"jsonrpc": "2.0", "id": rpc.counter, "method": method, "params": params}
    rpc.counter += 1
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token()
    try:
        request = urllib.request.Request(url, json.dumps(body).encode(), headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            reply = json.load(response)
    except Exception as error:
        reply = {"transportError": str(error)}
    if transcript is not None:
        transcript.append({"endpoint": url, "request": body, "response": reply})
    if "transportError" in reply:
        raise RpcError(method, reply["transportError"], transport=True)
    if "error" in reply:
        raise RpcError(method, reply["error"])
    return reply["result"]


rpc.counter = 1


def jwt(secret):
    key = bytes.fromhex(secret.removeprefix("0x"))

    def make():
        enc = lambda value: base64.urlsafe_b64encode(value).rstrip(b"=")
        head = enc(b'{"alg":"HS256","typ":"JWT"}')
        payload = enc(json.dumps({"iat": int(time.time())}, separators=(",", ":")).encode())
        signed = head + b"." + payload
        return (signed + b"." + enc(hmac.new(key, signed, hashlib.sha256).digest())).decode()

    return make


def quantity(value):
    return hex(value) if isinstance(value, int) else value


def source_payload(url, number, transcript):
    block = rpc(url, "eth_getBlockByNumber", [hex(number), True], transcript=transcript)
    if not block:
        raise RuntimeError(f"source block {number} is unavailable")
    transactions = []
    for tx in block["transactions"]:
        raw = rpc(url, "eth_getRawTransactionByHash", [tx["hash"]], transcript=transcript)
        if not raw:
            raise RuntimeError(f"raw transaction unavailable: {tx['hash']}")
        transactions.append(raw)
    names = {
        "parentHash": "parentHash", "miner": "feeRecipient", "stateRoot": "stateRoot",
        "receiptsRoot": "receiptsRoot", "logsBloom": "logsBloom", "mixHash": "prevRandao",
        "number": "blockNumber", "gasLimit": "gasLimit", "gasUsed": "gasUsed",
        "timestamp": "timestamp", "extraData": "extraData", "baseFeePerGas": "baseFeePerGas",
        "hash": "blockHash", "withdrawalsRoot": "withdrawalsRoot", "blobGasUsed": "blobGasUsed",
        "excessBlobGas": "excessBlobGas",
    }
    payload = {target: block[source] for source, target in names.items() if block.get(source) is not None}
    payload["transactions"] = transactions
    payload["withdrawals"] = block.get("withdrawals", [])
    return block, payload


def engine_params(block, payload, isthmus):
    root = block.get("parentBeaconBlockRoot") or "0x" + "00" * 32
    if isthmus:
        return "engine_newPayloadV4", [payload, [], root, []]
    return "engine_newPayloadV3", [payload, [], root]


def fcu(endpoint, token, head, safe, finalized, transcript):
    state = {"headBlockHash": head, "safeBlockHash": safe, "finalizedBlockHash": finalized}
    return rpc(endpoint, "engine_forkchoiceUpdatedV3", [state, None], token, transcript)


def payload_attributes(block, payload):
    extra = bytes.fromhex(block["extraData"].removeprefix("0x"))
    if len(extra) != 9 or extra[0] != 0:
        raise RuntimeError(f"block {block['number']} has invalid Holocene extraData")
    return {
        "timestamp": block["timestamp"],
        "prevRandao": block["mixHash"],
        "suggestedFeeRecipient": "0x1000000000000000000000000000000000000001",
        "withdrawals": block.get("withdrawals", []),
        "parentBeaconBlockRoot": block.get("parentBeaconBlockRoot") or "0x" + "00" * 32,
        "transactions": payload["transactions"],
        "noTxPool": True,
        "gasLimit": block["gasLimit"],
        "eip1559Params": "0x" + extra[1:].hex(),
    }


def alternate_branch(summary, transcript, blocks, hashes, through, isthmus_block,
                     engines, public_endpoints, token):
    parent = hashes[18]
    alternate = {}
    commitment_keys = ("blockHash", "stateRoot", "receiptsRoot", "gasUsed")
    for number in range(19, through + 1):
        block, source = blocks[number]
        attributes = payload_attributes(block, source)
        state = {"headBlockHash": parent, "safeBlockHash": hashes[18],
                 "finalizedBlockHash": hashes[0]}
        built = []
        build_replies = []
        for endpoint in engines:
            reply = rpc(endpoint, "engine_forkchoiceUpdatedV3", [state, attributes], token, transcript)
            build_replies.append(reply)
            payload_id = reply.get("payloadId")
            if reply.get("payloadStatus", {}).get("status") != "VALID" or not payload_id:
                raise RuntimeError(f"builder rejected alternate block {number}: {reply}")
            version = 4 if number >= isthmus_block else 3
            envelope = rpc(endpoint, f"engine_getPayloadV{version}", [payload_id], token, transcript)
            built.append(envelope["executionPayload"])
        commitments = [{key: payload[key] for key in commitment_keys} for payload in built]
        same = commitments[0] == commitments[1]
        different = all(payload["blockHash"] != hashes[number] for payload in built)
        summary.append({"block": number, "test": "alternate builder commitment parity",
                        "result": "PASS" if same and different else "FAIL",
                        "buildResponses": build_replies, "commitments": commitments})
        if not same:
            raise RuntimeError(f"alternate builders disagreed at block {number}: {commitments}")
        for endpoint, payload in zip(engines, built):
            method, params = engine_params(block, payload, number >= isthmus_block)
            accepted = rpc(endpoint, method, params, token, transcript)
            selected = fcu(endpoint, token, payload["blockHash"], hashes[18], hashes[0], transcript)
            if accepted.get("status") != "VALID" or selected.get("payloadStatus", {}).get("status") != "VALID":
                raise RuntimeError(f"alternate block {number} was not accepted: {accepted}, {selected}")
        parent = built[0]["blockHash"]
        alternate[number] = parent

    latest = [wait_latest(endpoint, alternate[through], transcript) for endpoint in public_endpoints]
    summary.append({"test": "alternate branch selected", "result": "PASS" if all(
        block["hash"] == alternate[through] for block in latest) else "FAIL",
                    "alternateHead": alternate[through], "latest": [block["hash"] for block in latest]})
    return alternate


def rlp_decode(raw):
    data = bytes.fromhex(raw.removeprefix("0x"))
    def one(pos):
        lead = data[pos]
        if lead <= 0x7f: return data[pos:pos + 1], pos + 1
        if lead <= 0xb7:
            size = lead - 0x80; return data[pos + 1:pos + 1 + size], pos + 1 + size
        if lead <= 0xbf:
            width = lead - 0xb7; size = int.from_bytes(data[pos + 1:pos + 1 + width]); start = pos + 1 + width
            return data[start:start + size], start + size
        width = lead - 0xf7 if lead > 0xf7 else 0
        size = lead - 0xc0 if lead <= 0xf7 else int.from_bytes(data[pos + 1:pos + 1 + width])
        start = pos + 1 if lead <= 0xf7 else pos + 1 + width; end = start + size; out = []
        while start < end:
            value, start = one(start); out.append(value)
        return out, end
    return one(0)[0]


def rlp_encode(value):
    if isinstance(value, list):
        body = b"".join(rlp_encode(item) for item in value); offset = 0xc0
    else:
        if len(value) == 1 and value[0] < 0x80: return value
        body = value; offset = 0x80
    if len(body) <= 55: return bytes([offset + len(body)]) + body
    length = len(body).to_bytes((len(body).bit_length() + 7) // 8, "big")
    return bytes([offset + 55 + len(length)]) + length + body


def keccak(raw):
    cast = shutil.which("cast")
    if not cast:
        raise RuntimeError("cast is required for malformed-header block hash recomputation")
    return subprocess.check_output([cast, "keccak", "0x" + raw.hex()], text=True).strip()


def malformed(source, block, payload, endpoints, token, isthmus, transcript):
    raw = rpc(source, "debug_getRawHeader", [block["number"]], transcript=transcript)
    header = rlp_decode(raw)
    cases = (("stateRoot", 3), ("receiptsRoot", 5), ("gasUsed", 10))
    results = []
    for field, index in cases:
        changed = list(header); old = changed[index]
        changed[index] = bytes([old[-1] ^ 1]) if len(old) == 1 else old[:-1] + bytes([old[-1] ^ 1])
        candidate = dict(payload)
        candidate[field] = "0x" + changed[index].hex() if field != "gasUsed" else hex(int.from_bytes(changed[index], "big"))
        candidate["blockHash"] = keccak(rlp_encode(changed))
        method, params = engine_params(block, candidate, isthmus)
        observed = []
        infrastructure = False
        for endpoint in endpoints:
            try:
                reply = rpc(endpoint, method, params, token, transcript)
                observed.append({"status": reply.get("status"), "validationError": reply.get("validationError")})
            except RpcError as error:
                infrastructure |= error.transport
                observed.append({"transportError" if error.transport else "rpcError": error.detail})
        statuses = [item.get("status") for item in observed]
        invalid = all(status in ("INVALID", "INVALID_BLOCK_HASH") for status in statuses)
        same = observed[0] == observed[1]
        results.append({"case": field, "test": "malformed payload parity",
                        "result": "PASS" if invalid and same and not infrastructure else "FAIL",
                        "responses": observed})
    return results


def free_port():
    sock = socket.socket(); sock.bind(("127.0.0.1", 0)); port = sock.getsockname()[1]; sock.close(); return port


def launch(binary, genesis, datadir, jwt_file, extra, env):
    http, auth, p2p = free_port(), free_port(), free_port()
    command = [binary, "node", "--datadir", str(datadir), "--chain", str(genesis), "--http",
               "--http.api", "eth,net,web3,debug", "--disable-discovery", "--port", str(p2p),
               "--http.addr", "127.0.0.1", "--http.port", str(http), "--authrpc.addr", "127.0.0.1",
               "--authrpc.port", str(auth), "--authrpc.jwtsecret", str(jwt_file), *extra]
    log = open(datadir.parent / (datadir.name + ".log"), "wb")
    process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
    return process, f"http://127.0.0.1:{auth}", f"http://127.0.0.1:{http}"


def wait_engine(endpoint, token, transcript):
    for _ in range(120):
        try:
            rpc(endpoint, "engine_exchangeCapabilities", [[]], token, transcript)
            return
        except RpcError:
            time.sleep(.5)
    raise RuntimeError(f"Engine endpoint did not become ready: {endpoint}")


def wait_latest(endpoint, expected, transcript):
    latest = None
    for _ in range(100):
        latest = rpc(endpoint, "eth_getBlockByNumber", ["latest", False], transcript=transcript)
        if latest["hash"] == expected:
            return latest
        time.sleep(.05)
    return latest


def parity(summary, transcript, worker, reference, method, params, name, check=None):
    values = []
    for endpoint in (worker, reference):
        try:
            values.append({"result": rpc(endpoint, method, params, transcript=transcript)})
        except RpcError as error:
            values.append({"transportError" if error.transport else "rpcError": error.detail})
    equal = values[0] == values[1]
    sufficient = check(values[0].get("result")) if check and "result" in values[0] else True
    summary.append({"test": name, "result": "PASS" if equal and sufficient else "FAIL", "responses": values})
    return values


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True,
                        help="history-devnet run directory containing runtime.json and generated output")
    parser.add_argument("--worker-engine"); parser.add_argument("--reference-engine")
    parser.add_argument("--worker-rpc"); parser.add_argument("--reference-rpc")
    parser.add_argument("--worker-bin", default=ROOT / "target/history-host-node")
    parser.add_argument("--reference-bin", default=ROOT / "target/history-reference-node")
    parser.add_argument("--genesis", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--node-arg", action="append", default=[])
    parser.add_argument("--through", type=int, default=22)
    parser.add_argument("--output")
    args = parser.parse_args()
    run = args.run_dir.resolve()
    args.genesis = args.genesis or run / "generated/l2/genesis.json"
    args.manifest = args.manifest or run / "manifest.json"
    runtime = json.loads((run / "runtime.json").read_text())
    source = runtime["builder_rpc_url"]; secret = runtime["engine_jwt"]; token = jwt(secret)
    out = Path(args.output or run / "replay-evidence"); out.mkdir(parents=True, exist_ok=True)
    processes = []
    launch_settings = None
    endpoint_mode = args.worker_engine and args.reference_engine
    if endpoint_mode and not (args.worker_rpc and args.reference_rpc):
        parser.error("endpoint mode also requires --worker-rpc and --reference-rpc for commitment checks")
    if not endpoint_mode:
        if not all((args.worker_bin, args.reference_bin, args.genesis, args.manifest)):
            parser.error("provide both endpoint options, or both binaries plus --genesis and --manifest")
        args.reference_bin = verify_reference(args.reference_bin)
        approved = json.loads(Path(args.manifest).read_text())
        provenance, launches = capture_provenance(out, "replay", {
            "host": args.worker_bin, "reference": args.reference_bin,
            "worker": approved["executable"],
        }, (("manifest", args.manifest), ("genesis", args.genesis)))
        args.worker_bin = launches["host"]
        args.reference_bin = launches["reference"]
        for name in ("worker", "reference"):
            datadir = out / (name + "-datadir")
            if datadir.exists() and any(datadir.iterdir()): parser.error(f"owned datadir is not empty: {datadir}")
            datadir.mkdir(exist_ok=True)
        jwt_file = out / "jwt.hex"; jwt_file.write_text(secret + "\n"); os.chmod(jwt_file, 0o600)
        owned_manifest = out / "worker-manifest.json"
        shutil.copy2(args.manifest, owned_manifest)
        approved["executable"] = str(launches["worker"])
        approved["executable_sha256"] = "0x" + provenance["binaries"]["worker"]["sha256"]
        owned_manifest.write_text(json.dumps(approved, indent=2) + "\n")
        worker_env = os.environ.copy(); worker_env["BASE_HISTORY_MANIFEST"] = str(owned_manifest.resolve())
        reference_env = os.environ.copy(); reference_env.pop("BASE_HISTORY_MANIFEST", None)
        wp, args.worker_engine, args.worker_rpc = launch(args.worker_bin, args.genesis, out / "worker-datadir", jwt_file, args.node_arg, worker_env)
        rp, args.reference_engine, args.reference_rpc = launch(args.reference_bin, args.genesis, out / "reference-datadir", jwt_file, args.node_arg, reference_env)
        processes = [wp, rp]
        launch_settings = (jwt_file, worker_env, reference_env, owned_manifest)
    transcript, summary, blocks = [], [], {}
    try:
        for endpoint in (args.worker_engine, args.reference_engine):
            wait_engine(endpoint, token, transcript)
        latest = int(rpc(source, "eth_blockNumber", [], transcript=transcript), 16)
        if latest < args.through: raise RuntimeError(f"source only at {latest}, need at least {args.through}")
        genesis = rpc(source, "eth_getBlockByNumber", ["0x0", False], transcript=transcript)
        hashes = {0: genesis["hash"]}
        for number in range(1, args.through + 1):
            block, payload = source_payload(source, number, transcript); blocks[number] = (block, payload); hashes[number] = block["hash"]
            method, params = engine_params(block, payload, number >= runtime["isthmus_block"])
            if number == 1 and launch_settings:
                owned_manifest = launch_settings[3]
                approved_bytes = owned_manifest.read_bytes()
                corrupt = json.loads(approved_bytes)
                corrupt["executable_sha256"] = "0x" + "00" * 32
                owned_manifest.write_text(json.dumps(corrupt))
                try:
                    try:
                        unexpected = rpc(args.worker_engine, method, params, token, transcript)
                        raise RuntimeError(f"wrong artifact did not fail Engine execution: {unexpected}")
                    except RpcError as error:
                        if error.transport or error.detail.get("code") != -32603:
                            raise RuntimeError(f"artifact failure was not an internal error: {error}")
                    head = rpc(args.worker_rpc, "eth_getBlockByNumber", ["latest", False], transcript=transcript)
                    summary.append({"test": "Engine artifact failure is internal and commits nothing",
                                    "result": "PASS" if head["hash"] == hashes[0] else "FAIL"})
                finally:
                    owned_manifest.write_bytes(approved_bytes)
            replies = [rpc(ep, method, params, token, transcript) for ep in (args.worker_engine, args.reference_engine)]
            valid = all(reply.get("status") == "VALID" for reply in replies)
            same = replies[0].get("status") == replies[1].get("status")
            summary.append({"block": number, "test": "newPayload parity", "result": "PASS" if valid and same else "FAIL", "responses": replies})
            fcu_replies = [fcu(ep, token, hashes[number], hashes[0], hashes[0], transcript)
                           for ep in (args.worker_engine, args.reference_engine)]
            fcu_valid = all(reply.get("payloadStatus", {}).get("status") == "VALID" for reply in fcu_replies)
            summary.append({"block": number, "test": "forkchoice VALID", "result": "PASS" if fcu_valid else "FAIL",
                            "responses": fcu_replies})
            keys = ("hash", "stateRoot", "receiptsRoot", "transactionsRoot")
            canonical = [rpc(ep, "eth_getBlockByNumber", [hex(number), False], transcript=transcript)
                         for ep in (args.worker_rpc, args.reference_rpc)]
            expected = {key: block[key] for key in keys}
            observed = [{key: got[key] for key in keys} for got in canonical]
            summary.append({"block": number, "test": "canonical commitments", "result": "PASS" if observed == [expected, expected] else "FAIL", "source": expected, "nodes": observed})
        for test_number in (runtime["isthmus_block"] - 1, runtime["isthmus_block"]):
            summary.extend(malformed(source, *blocks[test_number], (args.worker_engine, args.reference_engine),
                                     token, test_number >= runtime["isthmus_block"], transcript))

        oracle_call = {"to": "0x420000000000000000000000000000000000000F", "data": "0xb54501bc"}
        for number in (19, 20):
            parity(summary, transcript, args.worker_rpc, args.reference_rpc, "eth_call",
                   [oracle_call, hex(number)], f"GasPriceOracle isIsthmus block {number}")
        for number in (19, 20):
            tx_hash = blocks[number][0]["transactions"][0]["hash"]
            trace_call = [oracle_call, hex(number)]
            parity(summary, transcript, args.worker_rpc, args.reference_rpc, "debug_traceCall",
                   trace_call + [{"tracer": "callTracer"}], f"debug_traceCall callTracer block {number}")
            parity(summary, transcript, args.worker_rpc, args.reference_rpc, "debug_traceCall",
                   trace_call + [{}], f"debug_traceCall default block {number}")
            parity(summary, transcript, args.worker_rpc, args.reference_rpc, "debug_traceTransaction",
                   [tx_hash, {"tracer": "callTracer"}], f"traceTransaction block {number}")
            parity(summary, transcript, args.worker_rpc, args.reference_rpc, "debug_traceBlockByNumber",
                   [hex(number), {"tracer": "callTracer"}], f"traceBlock block {number}")
        override_address = "0x1000000000000000000000000000000000000001"
        override_call = {"to": override_address, "gas": "0x186a0"}
        overrides = {override_address: {"code": "0x60005460005260206000f3", "state": {
            "0x" + "00" * 32: "0x" + "00" * 31 + "2a"}}}
        for number in (19, 20):
            parity(summary, transcript, args.worker_rpc, args.reference_rpc, "eth_call",
                   [override_call, hex(number), overrides], f"eth_call code and state override block {number}",
                   lambda value: int(value, 16) == 42)
            parity(summary, transcript, args.worker_rpc, args.reference_rpc, "eth_estimateGas",
                   [override_call, hex(number), overrides], f"estimateGas bounded block {number}",
                   lambda value: 21_000 <= int(value, 16) <= int(override_call["gas"], 16))

        if args.through < 20:
            raise RuntimeError("alternate cutover test requires --through of at least 20")
        canonical_flags = {}
        for number in (19, 20):
            canonical_flags[number] = [rpc(endpoint, "eth_call", [oracle_call, hex(number)], transcript=transcript)
                                       for endpoint in (args.worker_rpc, args.reference_rpc)]
        alternate = alternate_branch(summary, transcript, blocks, hashes, args.through,
                                     runtime["isthmus_block"],
                                     (args.worker_engine, args.reference_engine),
                                     (args.worker_rpc, args.reference_rpc), token)
        alternate_flags = {}
        for number in (19, 20):
            block_id = {"blockHash": alternate[number]}
            alternate_flags[number] = [rpc(endpoint, "eth_call", [oracle_call, block_id], transcript=transcript)
                                       for endpoint in (args.worker_rpc, args.reference_rpc)]
        for endpoint in (args.worker_engine, args.reference_engine):
            restored = fcu(endpoint, token, hashes[args.through], hashes[18], hashes[0], transcript)
            if restored.get("payloadStatus", {}).get("status") != "VALID":
                raise RuntimeError(f"failed to restore canonical head: {restored}")
        restored_heads = [wait_latest(endpoint, hashes[args.through], transcript)
                          for endpoint in (args.worker_rpc, args.reference_rpc)]
        restored_flags = {number: [rpc(endpoint, "eth_call", [oracle_call, hex(number)], transcript=transcript)
                                   for endpoint in (args.worker_rpc, args.reference_rpc)]
                          for number in (19, 20)}
        false_flag = "0x" + "00" * 32
        true_flag = "0x" + "00" * 31 + "01"
        expected_flags = {19: [false_flag, false_flag], 20: [true_flag, true_flag]}
        isolated = (canonical_flags == expected_flags and alternate_flags == expected_flags and
                    restored_flags == expected_flags and
                    all(block["hash"] == hashes[args.through] for block in restored_heads))
        summary.append({"test": "alternate branch Isthmus storage and canonical restore",
                        "result": "PASS" if isolated else "FAIL", "canonical": canonical_flags,
                        "alternate": alternate_flags, "restored": restored_flags,
                        "restoredHeads": [block["hash"] for block in restored_heads]})

        if launch_settings:
            for process in processes:
                process.send_signal(signal.SIGINT)
            for process in processes:
                process.wait(timeout=15)
            jwt_file, worker_env, reference_env, owned_manifest = launch_settings
            wp, args.worker_engine, args.worker_rpc = launch(args.worker_bin, args.genesis, out / "worker-datadir", jwt_file, args.node_arg, worker_env)
            rp, args.reference_engine, args.reference_rpc = launch(args.reference_bin, args.genesis, out / "reference-datadir", jwt_file, args.node_arg, reference_env)
            processes = [wp, rp]
            for endpoint in (args.worker_engine, args.reference_engine):
                wait_engine(endpoint, token, transcript)
            restarted = parity(summary, transcript, args.worker_rpc, args.reference_rpc, "eth_call",
                               [oracle_call, "0x13"], "historical RPC after graceful restart")
            baseline = restarted[0].get("result")

            before = rpc(args.worker_rpc, "eth_getBlockByNumber", ["latest", False], transcript=transcript)["hash"]
            original_manifest = owned_manifest.read_text()
            wrong = json.loads(original_manifest)
            wrong["executable_sha256"] = "0x" + "00" * 32
            owned_manifest.write_text(json.dumps(wrong, indent=2) + "\n")
            try:
                rpc(args.worker_rpc, "eth_call", [oracle_call, "0x13"], transcript=transcript)
                failed_as_infrastructure = False
                failure = None
            except RpcError as error:
                failed_as_infrastructure = True
                failure = error.detail
            after = rpc(args.worker_rpc, "eth_getBlockByNumber", ["latest", False], transcript=transcript)["hash"]
            owned_manifest.write_text(original_manifest)
            restored = rpc(args.worker_rpc, "eth_call", [oracle_call, "0x13"], transcript=transcript)
            causal = failed_as_infrastructure and before == after and restored == baseline
            summary.append({"test": "worker manifest digest causality", "result": "PASS" if causal else "FAIL",
                            "failure": failure, "latestBefore": before, "latestAfter": after, "restoredResult": restored})
    finally:
        (out / "transcript.json").write_text(json.dumps(transcript, indent=2) + "\n")
        (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        for process in processes:
            process.send_signal(signal.SIGINT)
        for process in processes:
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired: process.terminate()
        if not endpoint_mode:
            complete_provenance(out, provenance)
    failed = [item for item in summary if item.get("result") == "FAIL"]
    passed = [item for item in summary if item.get("result") == "PASS"]
    skipped = [item for item in summary if item.get("result") == "SKIP"]
    print(json.dumps({"evidence": str(out), "passed": len(passed), "failed": len(failed), "skipped": len(skipped)}, indent=2))
    return bool(failed)


if __name__ == "__main__":
    sys.exit(main())
