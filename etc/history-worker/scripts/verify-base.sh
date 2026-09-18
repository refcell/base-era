#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
source_root="$root/historical/base"
metadata="$root/../../sources/frozen-sources.json"

expected=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["historical_base"]["content_sha256"])' "$metadata")
actual=$(
  cd "$source_root"
  find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1
)

test "$actual" = "$expected" || {
  echo "historical Base source integrity check failed" >&2
  exit 1
}
echo "$actual  historical/base"
