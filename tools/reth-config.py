#!/usr/bin/env python3
"""Generate Cargo path overrides for the committed reth source, without patching it."""

import json
from pathlib import Path
import tomllib

root = Path(__file__).resolve().parents[1] / "vendor/reth"
workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
members = {path.resolve() for pattern in workspace["members"] for path in root.glob(pattern)}
print('[patch."https://github.com/base/reth"]')
for manifest in sorted((root / "crates").rglob("Cargo.toml")):
    data = tomllib.loads(manifest.read_text())
    if "package" in data and manifest.parent.resolve() in members:
        print(f'{json.dumps(data["package"]["name"])} = {{ path = {json.dumps(str(manifest.parent))} }}')
