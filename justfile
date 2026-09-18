demo:
    #!/usr/bin/env bash
    set -Eeuo pipefail

    root=$PWD
    if [[ -n "${BASE_ERA_RUN_DIR:-}" ]]; then
        run=$(realpath -m "$BASE_ERA_RUN_DIR")
    else
        run="$root/target/demo-one-command-$(date -u +%Y%m%dT%H%M%SZ)-$$"
    fi
    export BASE_ERA_RUN_DIR="$run"
    printf -v stop_command 'BASE_ERA_RUN_DIR=%q ./demo stop' "$run"

    failed() {
        status=$?
        printf '\nDemo failed (exit %d). Data was left at: %s\nRecovery/stop command: %s\n' \
            "$status" "$run" "$stop_command" >&2
        exit "$status"
    }
    trap failed ERR

    for stage in setup build test start exercise verify evidence; do
        printf '\n==> demo %s\n' "$stage"
        ./demo "$stage"
    done

    trap - ERR
    printf '\nDemo completed successfully and remains running.\nRun directory: %s\n' "$run"
    jq '{status, builder_rpc_url, verifier_rpc_url, l1_rpc_url, isthmus_block}' "$run/runtime.json"
    jq '{builderVerifierParity, workloadReceiptsMatch}' "$run/evidence/verification.json"
    printf 'Portable evidence: %s/portable-evidence\nStop command: %s\n' "$run" "$stop_command"
