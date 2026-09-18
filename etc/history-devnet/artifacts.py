#!/usr/bin/env python3
"""Verify frozen source inputs and the approved independent reference executable."""

import argparse
import hashlib
import json
from pathlib import Path


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
