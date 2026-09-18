#!/usr/bin/env bash
set -euo pipefail

revision=1eda0f7f4cebb823522e62f34fc3e513b1c450b1
root=$(cd "$(dirname "$0")/../.." && pwd)
source="$root/historical/reference"
build_target="$root/target/reference"
executable="$root/target/history-reference-node"
artifacts="$root/target/history-reference-artifacts/sha256"
metadata="$root/target/history-reference-build.json"

python3 "$root/etc/history-devnet/artifacts.py" --source reference_base
export CARGO_TARGET_DIR="$build_target"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-3}
cargo build --manifest-path "$source/Cargo.toml" --locked --profile profiling \
    -p base-reth-node --bin base-reth-node --no-default-features

built="$build_target/profiling/base-reth-node"
digest=$(sha256sum "$built" | cut -d' ' -f1)
destination="$artifacts/$digest/base-reth-node"
mkdir -p "$(dirname "$destination")"
if [[ ! -e "$destination" ]]; then
    staging="$destination.tmp.$$"
    cp "$built" "$staging"
    chmod 555 "$staging"
    mv "$staging" "$destination"
fi
test "$(sha256sum "$destination" | cut -d' ' -f1)" = "$digest"
staging="$executable.tmp.$$"
cp "$destination" "$staging"
chmod 555 "$staging"
mv "$staging" "$executable"

size=$(stat -c %s "$executable")
cat >"$metadata.tmp" <<EOF
{
  "executable": "$executable",
  "sha256": "$digest",
  "size_bytes": $size,
  "source_commit": "$revision",
  "source": "$source",
  "source_manifest_sha256": "$(sha256sum "$root/sources/frozen-sources.json" | cut -d' ' -f1)",
  "artifact": "$destination",
  "toolchain": {
    "rustc": "$(rustc --version)",
    "cargo": "$(cargo --version)"
  },
  "build": {
    "command": "cargo build --locked --profile profiling -p base-reth-node --bin base-reth-node --no-default-features",
    "cargo_target_dir": "$build_target",
    "package": "base-reth-node",
    "binary": "base-reth-node",
    "profile": "profiling",
    "locked": true,
    "no_default_features": true
  }
}
EOF
mv "$metadata.tmp" "$metadata"
printf 'reference: sha256:%s\nartifact: %s\n' "$digest" "$destination"
