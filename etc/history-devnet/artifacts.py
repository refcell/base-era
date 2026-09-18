#!/usr/bin/env python3
"""Verify frozen source inputs and the approved independent reference executable."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil


ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def verify_source(name):
    record = json.loads((ROOT / "sources/frozen-sources.json").read_text())[name]
    source = ROOT / record["path"]
    # Match find + LC_ALL=C sort + sha256sum, including the './' prefix.
    paths = sorted((p for p in source.rglob("*") if p.is_file() and not p.is_symlink()),
                   key=lambda p: str(p.relative_to(source)).encode())
    hashes = "".join(f"{digest(p)}  ./{p.relative_to(source)}\n" for p in paths)
    actual = hashlib.sha256(hashes.encode()).hexdigest()
    if actual != record["content_sha256"]:
        raise RuntimeError(f"{name} source integrity mismatch: {actual}")
    if digest(source / "Cargo.lock") != record["lockfile_sha256"]:
        raise RuntimeError(f"{name} lockfile integrity mismatch")


def verify_reference(binary):
    verify_source("reference_base")
    record = json.loads((ROOT / "target/history-reference-build.json").read_text())
    if digest(ROOT / "sources/frozen-sources.json") != record["source_manifest_sha256"]:
        raise RuntimeError("reference was built from a different source approval")
    artifact = Path(record["artifact"])
    expected = record["sha256"]
    if artifact.parent.name != expected or digest(artifact) != expected or digest(binary) != expected:
        raise RuntimeError("reference executable integrity mismatch; rebuild the approved reference")
    return artifact


def capture_provenance(output, stage, binaries, files=()):
    """Freeze launch binaries and record the approved inputs used by a harness."""
    output = Path(output)
    store = output / "provenance-artifacts"
    store.mkdir(parents=True, exist_ok=True)
    recorded = {}
    launches = {}
    for name, value in binaries.items():
        # Retain the alias as well as a frozen launch copy so retargeting a symlink
        # after testing cannot silently change the artifact associated with evidence.
        source = Path(value).absolute()
        checksum = digest(source)
        launch = store / checksum / f"{name}-{source.name}"
        launch.parent.mkdir(parents=True, exist_ok=True)
        if not launch.exists():
            shutil.copy2(source, launch)
        if digest(launch) != checksum:
            raise RuntimeError(f"failed to freeze {name} executable")
        recorded[name] = {"source": str(source), "launch": str(launch.resolve()), "sha256": checksum}
        launches[name] = launch.resolve()
    for name, value in files:
        if name in ("manifest", "approval") and "worker" in recorded:
            approved = json.loads(Path(value).read_text())
            if approved["executable_sha256"] != "0x" + recorded["worker"]["sha256"]:
                raise RuntimeError("worker executable does not match approved digest")
    inputs = {name: {"path": str(Path(value).resolve()), "sha256": digest(value)}
              for name, value in files}
    approval = ROOT / "sources/frozen-sources.json"
    record = {"schema": 1, "stage": stage, "status": "running",
              "source_manifest_sha256": digest(approval), "binaries": recorded, "inputs": inputs}
    (output / "provenance.json").write_text(json.dumps(record, indent=2) + "\n")
    return record, launches


def complete_provenance(output, record):
    """Check that every source input still has the identity captured before launch."""
    approval = ROOT / "sources/frozen-sources.json"
    if digest(approval) != record["source_manifest_sha256"]:
        raise RuntimeError("source approval changed while acceptance was running")
    for kind in ("binaries", "inputs"):
        for name, item in record[kind].items():
            paths = [item["path"]] if kind == "inputs" else [item["source"], item["launch"]]
            if any(not Path(path).is_file() or digest(path) != item["sha256"] for path in paths):
                raise RuntimeError(f"{name} provenance input changed while acceptance was running")
    record["status"] = "complete"
    (Path(output) / "provenance.json").write_text(json.dumps(record, indent=2) + "\n")


def validate_provenance(path, expected_stage=None):
    """Validate a completed stage record and all paths to which it is bound."""
    path = Path(path)
    if not path.is_file():
        raise RuntimeError(f"missing provenance record: {path}")
    record = json.loads(path.read_text())
    if record.get("schema") != 1 or record.get("status") != "complete":
        raise RuntimeError(f"incomplete provenance record: {path}")
    if expected_stage and record.get("stage") != expected_stage:
        raise RuntimeError(f"provenance stage mismatch: expected {expected_stage}")
    complete_provenance(path.parent, record)
    return record


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", choices=["historical_base", "reference_base", "host_reth"])
    parser.add_argument("--reference", type=Path)
    args = parser.parse_args()
    if args.source:
        verify_source(args.source)
    elif args.reference:
        print(verify_reference(args.reference))
    else:
        parser.error("choose --source or --reference")
