#!/usr/bin/env python3
"""Verify and count the manually audited historical-line inventory."""

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "docs/history-deletion.json"


def fail(message: str) -> None:
    raise SystemExit(f"history deletion inventory: {message}")


data = json.loads(INVENTORY.read_text())
if data.get("actual_removed") != {"physical": 0}:
    fail("inventory must explicitly report zero actual removed lines")
source_root = ROOT / data["source_root"]
revision = (source_root / ".base-revision").read_text().strip()
if revision != data["base_revision"]:
    fail(f"revision drift: expected {data['base_revision']}, found {revision}")

totals = {}
for file_entry in data["files"]:
    path = source_root / file_entry["path"]
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != file_entry["sha256"]:
        fail(f"hash drift in {file_entry['path']}: expected {file_entry['sha256']}, found {digest}")
    lines = raw.decode().splitlines()
    occupied = set()
    for item in file_entry["ranges"]:
        start, end = item["start"], item["end"]
        if start < 1 or end < start or end > len(lines):
            fail(f"invalid range {file_entry['path']}:{start}-{end}")
        selected = set(range(start, end + 1))
        if occupied & selected:
            fail(f"overlapping range {file_entry['path']}:{start}-{end}")
        occupied |= selected
        segment = lines[start - 1 : end]
        measured = {
            "physical": len(segment),
            "blank": sum(not line.strip() for line in segment),
            "comment_only": sum(line.lstrip().startswith("//") for line in segment),
        }
        if measured != item["counts"]:
            fail(
                f"count drift in {file_entry['path']}:{start}-{end}: "
                f"expected {item['counts']}, found {measured}"
            )
        bucket = totals.setdefault(item["kind"], {"physical": 0, "blank": 0, "comment_only": 0})
        for key, value in measured.items():
            bucket[key] += value

if totals != data["totals"]:
    fail(f"total drift: expected {data['totals']}, found {totals}")
print(json.dumps(totals, indent=2, sort_keys=True))
