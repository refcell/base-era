#!/usr/bin/env bash
set -euo pipefail

image=base-era-setup:local-v1
docker image inspect "$image" >/dev/null 2>&1 && exit 0

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
docker build --quiet -t "$image" -f "$repo_root/etc/docker/Dockerfile.devnet" "$repo_root"
