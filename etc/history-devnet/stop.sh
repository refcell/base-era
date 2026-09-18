#!/usr/bin/env bash
set -euo pipefail
run=${1:?RUN_DIR required}; test -s "$run/pid" || exit 0; pid=$(cat "$run/pid")
expected=$(cat "$run/process" 2>/dev/null || true); actual=$(readlink -f "/proc/$pid/exe" 2>/dev/null || true)
actual=${actual%" (deleted)"}
test -n "$expected" && test "$actual" = "$expected" || { echo "refusing to signal unowned PID $pid" >&2; exit 1; }
tr '\0' '\n' <"/proc/$pid/cmdline" | rg -Fx -- "$run/runtime.json" >/dev/null || { echo "PID does not belong to this run" >&2; exit 1; }
kill -INT "$pid" 2>/dev/null || true
for _ in $(seq 1 60); do kill -0 "$pid" 2>/dev/null || { rm -f "$run/pid"; exit 0; }; sleep 1; done
exit 1
