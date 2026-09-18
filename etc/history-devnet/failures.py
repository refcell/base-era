#!/usr/bin/env python3
"""Fault-inject the history worker boundary against a persisted replay node."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import threading
import time
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
ORACLE = {"to": "0x420000000000000000000000000000000000000F", "data": "0xb54501bc"}
ZERO = "0x" + "00" * 32
ONE = "0x" + "00" * 31 + "01"


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def rpc(url, method, params, timeout=30):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(url, body, {"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.load(response)
    except Exception as error:
        return {"transportError": str(error)}


def children(parent):
    found = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            status = (entry / "status").read_text()
            ppid = int(next(line.split()[1] for line in status.splitlines() if line.startswith("PPid:")))
            if ppid == parent:
                found.append((int(entry.name), os.readlink(entry / "exe"), status))
        except (FileNotFoundError, PermissionError, StopIteration):
            pass
    return found


def assert_error(reply, label):
    error = reply.get("error")
    if not error or error.get("code") != -32603:
        raise AssertionError(f"{label}: expected -32603, got {reply}")
    return error


def assert_error_message(reply, label, expected):
    error = assert_error(reply, label)
    message = error.get("message", "")
    if "worker pipe failed" in message or expected not in message:
        raise AssertionError(f"{label}: expected {expected!r} (not a pipe failure), got {reply}")
    return error


def write_manifest(path, original, executable=None, digest=None):
    value = dict(original)
    if executable is not None:
        value["executable"] = str(executable)
    if digest is not None:
        value["executable_sha256"] = "0x" + digest
    path.write_text(json.dumps(value, indent=2) + "\n")


def build_fixtures(directory):
    source = directory / "protocol-fixture.c"
    source.write_text(r'''#include <arpa/inet.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifndef MODE
#define MODE 0
#endif
static int read_exact(int fd, void *buffer, size_t length) {
  unsigned char *cursor=buffer;
  while (length) {
    ssize_t count=read(fd,cursor,length);
    if (count>0) { cursor+=count; length-=(size_t)count; continue; }
    if (count<0 && errno==EINTR) continue;
    return -1;
  }
  return 0;
}
static int write_all(int fd, const void *buffer, size_t length) {
  const unsigned char *cursor=buffer;
  while (length) {
    ssize_t count=write(fd,cursor,length);
    if (count>0) { cursor+=count; length-=(size_t)count; continue; }
    if (count<0 && errno==EINTR) continue;
    return -1;
  }
  return 0;
}
int main(void) {
  unsigned char h[4]; if (read_exact(0,h,4)) return 2;
  unsigned n=(h[0]<<24)|(h[1]<<16)|(h[2]<<8)|h[3];
  char *in=calloc(n+1,1); if (!in || read_exact(0,in,n)) return 3;
  if (MODE==4) {
    unsigned net=htonl(8); unsigned char partial='{';
    if (write_all(1,&net,4) || write_all(1,&partial,1)) return 4;
    sleep(35); free(in); return 0;
  }
  const char *out;
  if (MODE==1) out="{";
  else if (MODE==2) out="{\"outcome\":\"unsupported\",\"request_id\":\"schema-fixture\",\"error\":\"terminal outcome where RPC reply is required\"}";
  else out="{\"request_id\":\"stale-request-id\",\"result\":\"0x00\"}";
  unsigned m=strlen(out); unsigned net=htonl(m);
  if (write_all(1,&net,4) || write_all(1,out,m)) return 4;
  free(in); return 0;
}''')
    fixtures = {}
    for mode, number in (("malformed", 1), ("incompatible-terminal-schema", 2),
                         ("stale-request", 3), ("timeout", 4)):
        binary = directory / f"fixture-{mode}"
        subprocess.run(["gcc", "-O2", "-Wall", "-Werror", f"-DMODE={number}",
                        "-o", binary, source], check=True)
        fixtures[mode] = (binary, hashlib.sha256(binary.read_bytes()).hexdigest())
    return fixtures


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True,
                        help="history-devnet run directory used to derive genesis and replay data")
    parser.add_argument("--datadir", type=Path,
                        help="populated worker datadir (default: RUN_DIR/replay-evidence/worker-datadir)")
    parser.add_argument("--genesis", type=Path,
                        help="L2 genesis (default: RUN_DIR/generated/l2/genesis.json)")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--host", type=Path, default=ROOT / "target/history-host-node")
    parser.add_argument("--output", type=Path, default=ROOT / "target/failure-test")
    parser.add_argument("--use-existing-datadir", action="store_true",
                        help="open the supplied owned datadir directly instead of testing a copy")
    args = parser.parse_args()
    run_dir = args.run_dir.resolve()
    args.datadir = args.datadir or run_dir / "replay-evidence/worker-datadir"
    args.genesis = args.genesis or run_dir / "generated/l2/genesis.json"
    args.manifest = args.manifest or run_dir / "manifest.json"
    source_datadir = Path(args.datadir).resolve(); output = Path(args.output).resolve()
    if output.exists():
        output = output.with_name(output.name + "-" + str(int(time.time())))
    output.mkdir(parents=True)
    evidence = []

    for pid, _, _ in children(os.getpid()):
        raise RuntimeError(f"unexpected pre-existing child {pid}")
    for entry in Path("/proc").glob("[0-9]*/cmdline"):
        try:
            command = entry.read_bytes().replace(b"\0", b" ").decode(errors="replace")
            if str(source_datadir) in command:
                raise RuntimeError(f"datadir is open by PID {entry.parent.name}: {command}")
        except (FileNotFoundError, PermissionError):
            pass

    # Copy the executable only after checking that no build process currently has it open.
    host_source = Path(args.host).resolve(); host = output / "history-host-node"
    shutil.copy2(host_source, host)
    datadir = source_datadir
    if not args.use_existing_datadir:
        datadir = output / "worker-datadir"
        shutil.copytree(source_datadir, datadir)
    original = json.loads(Path(args.manifest).read_text())
    owned_manifest = output / "manifest.json"
    write_manifest(owned_manifest, original)
    fixtures = build_fixtures(output)
    http, engine, p2p = free_port(), free_port(), free_port()
    env = os.environ.copy(); env["BASE_HISTORY_MANIFEST"] = str(owned_manifest)
    log = (output / "host.log").open("wb")
    command = [str(host), "node", "--datadir", str(datadir), "--chain", str(Path(args.genesis).resolve()),
               "--http", "--http.api", "eth,net,web3,debug", "--disable-discovery", "--port", str(p2p),
               "--http.addr", "127.0.0.1", "--http.port", str(http), "--authrpc.addr", "127.0.0.1",
               "--authrpc.port", str(engine)]
    process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
    url = f"http://127.0.0.1:{http}"
    try:
        for _ in range(120):
            if "result" in rpc(url, "eth_blockNumber", [], 1): break
            if process.poll() is not None: raise RuntimeError("owned host exited during startup")
            time.sleep(.25)
        else: raise RuntimeError("owned host did not start")

        latest = rpc(url, "eth_getBlockByNumber", ["latest", False])["result"]
        latest_hash = latest["hash"]
        historical = rpc(url, "eth_call", [ORACLE, "0x13"])
        current = rpc(url, "eth_call", [ORACLE, "0x14"])
        assert historical.get("result") == ZERO and current.get("result") == ONE
        evidence.append({"claim": "real historical/current execution", "historical19": historical,
                         "current20": current, "latest": latest_hash})

        write_manifest(owned_manifest, original, digest="00" * 32)
        wrong_digest = rpc(url, "eth_call", [ORACLE, "0x13"])
        assert_error(wrong_digest, "wrong digest")
        current_bad_manifest = rpc(url, "eth_call", [ORACLE, "0x14"])
        assert current_bad_manifest.get("result") == ONE
        write_manifest(owned_manifest, original, executable=output / "missing-worker")
        missing = rpc(url, "eth_call", [ORACLE, "0x13"])
        assert_error(missing, "missing executable")
        assert rpc(url, "eth_getBlockByNumber", ["latest", False])["result"]["hash"] == latest_hash
        write_manifest(owned_manifest, original)
        recovered = rpc(url, "eth_call", [ORACLE, "0x13"])
        assert recovered.get("result") == ZERO
        evidence.append({"claim": "manifest failure causality and recovery", "wrongDigest": wrong_digest,
                         "missingExecutable": missing, "currentWhileBroken": current_bad_manifest,
                         "recovered": recovered, "latestUnchanged": latest_hash})

        result = {}
        thread = threading.Thread(target=lambda: result.update(rpc(url, "eth_call", [ORACLE, "0x13"])))
        thread.start(); victim = None
        for _ in range(200):
            candidates = children(process.pid)
            verified = [(pid, exe, status) for pid, exe, status in candidates
                        if "memfd:" in exe and "history" in exe.lower()]
            if len(verified) == 1:
                victim = verified[0]; break
            time.sleep(.01)
        if victim is None: raise AssertionError("could not uniquely identify owned memfd worker child")
        os.kill(victim[0], signal.SIGKILL)
        thread.join(30)
        crash_error = assert_error(result, "worker SIGKILL")
        after_crash = rpc(url, "eth_getBlockByNumber", ["latest", False])["result"]["hash"]
        retry = rpc(url, "eth_call", [ORACLE, "0x13"])
        assert after_crash == latest_hash and retry.get("result") == ZERO
        evidence.append({"claim": "in-flight worker crash fails closed", "hostPid": process.pid,
                         "workerPid": victim[0], "workerExe": victim[1], "workerStatus": victim[2],
                         "error": crash_error, "latestUnchanged": after_crash, "retry": retry})

        expected_errors = {
            "malformed": "EOF while parsing",
            "incompatible-terminal-schema": "terminal RPC response is not bound to request",
            "stale-request": "terminal RPC response is not bound to request",
            "timeout": "worker transport timeout",
        }
        for mode, expected_error in expected_errors.items():
            mode_binary, digest = fixtures[mode]
            write_manifest(owned_manifest, original, executable=mode_binary, digest=digest)
            started = time.monotonic()
            reply = rpc(url, "eth_call", [ORACLE, "0x13"], 40)
            fixture_error = assert_error_message(reply, mode, expected_error)
            if mode == "timeout":
                elapsed = time.monotonic() - started
                if elapsed < 29 or elapsed > 40:
                    raise AssertionError(f"timeout: unexpected elapsed time {elapsed:.2f}s")
                if children(process.pid):
                    raise AssertionError("timeout: fixture worker was not reaped")
            assert rpc(url, "eth_getBlockByNumber", ["latest", False])["result"]["hash"] == latest_hash
            evidence.append({"claim": f"fixture {mode} response fails closed", "response": reply,
                             "error": fixture_error, "latestUnchanged": latest_hash,
                             "artifactOnly": True})
        write_manifest(owned_manifest, original)
    finally:
        write_manifest(owned_manifest, original)
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            try: process.wait(15)
            except subprocess.TimeoutExpired: process.terminate(); process.wait(5)
        log.close()
        (output / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
    summary = {"result": "PASS", "output": str(output), "claims": len(evidence)}
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
