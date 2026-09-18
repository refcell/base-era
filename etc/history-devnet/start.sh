#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd); run=${1:-"$root/target/history-devnet"}
test ! -e "$run" || { echo "run directory already exists; choose a fresh path: $run" >&2; exit 1; }
mkdir -p "$run/generated" "$run/evidence"
"$root/etc/history-devnet/build-setup-image.sh"
if [[ -z "${BASE_HISTORY_APPROVAL:-}" && -f "$root/target/history-artifacts/APPROVAL.json" ]]; then
  BASE_HISTORY_APPROVAL="$root/target/history-artifacts/APPROVAL.json"
  export BASE_HISTORY_APPROVAL
fi
if [[ -n "${BASE_HISTORY_APPROVAL:-}" ]]; then
  export BASE_HISTORY_MANIFEST="$run/manifest.json"
  test -s "$root/target/history-artifacts/HOST" || { echo "run etc/history-devnet/build.sh first" >&2; exit 1; }
  devnet=$(cat "$root/target/history-artifacts/HOST")
  test -x "$devnet" || { echo "approved host artifact is missing: $devnet" >&2; exit 1; }
else
  echo "history devnet requires an approved artifact; run etc/history-devnet/build.sh first" >&2
  exit 1
fi
"$devnet" history --isthmus-block 20 --output-dir "$run/generated" --runtime-file "$run/runtime.json" >"$run/launcher.log" 2>&1 & echo $! >"$run/pid"
printf '%s\n' "$devnet" >"$run/process"
for _ in $(seq 1 240); do test -s "$run/runtime.json" && jq -e '.status == "ready"' "$run/runtime.json" >/dev/null && exit 0; kill -0 "$(cat "$run/pid")" 2>/dev/null || { cat "$run/launcher.log"; exit 1; }; sleep 1; done
exit 1
