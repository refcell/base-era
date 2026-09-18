#!/usr/bin/env bash
set -euo pipefail

image=base-era-setup:local-v1
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
# Let Docker validate the committed build context, even when the tag exists.
# Cached layers are fine; an unrelated image carrying the same tag is not.
docker build --quiet -t "$image" -f "$repo_root/etc/docker/Dockerfile.devnet" "$repo_root"
