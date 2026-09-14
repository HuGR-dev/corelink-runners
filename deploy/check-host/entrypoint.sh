#!/bin/sh
# entrypoint.sh — check-host hydrate-then-serve (C5, cf-check-host-contract.md)
# Lifecycle: clw hydrate → signal-forwarding exec-server wrapper.
# DEFAULT-OFF: this container is only spawned when the check-host path is live-flipped.
set -eu

assert_no_symlink_components() {
    path=$1
    prefix=/
    rest=${path#/}
    while [ -n "$rest" ]; do
        component=${rest%%/*}
        rest=${rest#*/}
        [ "$component" = "$rest" ] && rest=
        [ -n "$component" ] || continue
        prefix="$prefix$component"
        [ ! -L "$prefix" ] || return 1
        prefix="$prefix/"
    done
}

# The provider can only deliver the bearer through the container environment.
# Convert it to a short-lived, mode-0400 file before starting any durable
# process.  The exec-server reads EXEC_SERVER_AUTH_TOKEN_FILE; retaining the
# original variable would expose the bearer through /proc/*/environ.
bridge_exec_auth_token() {
    auth_file="${EXEC_SERVER_AUTH_TOKEN_FILE:-/run/corelink/exec-server-auth-token}"
    auth_dir=${auth_file%/*}
    if [ "${auth_file#/}" = "$auth_file" ] || [ -z "$auth_dir" ] || [ "$auth_dir" = "$auth_file" ]; then
        echo "[check-host] FATAL: EXEC_SERVER_AUTH_TOKEN_FILE must name a file" >&2
        return 1
    fi
    if [ -L "$auth_dir" ] || { [ -e "$auth_dir" ] && [ ! -d "$auth_dir" ]; }; then
        echo "[check-host] FATAL: auth directory is not a safe directory" >&2
        return 1
    fi
    assert_no_symlink_components "$auth_dir" || {
        echo "[check-host] FATAL: auth directory contains a symlink component" >&2
        return 1
    }
    if [ ! -e "$auth_dir" ]; then
        mkdir -p "$auth_dir"
    fi
    resolved_auth_dir=$(CDPATH='' cd -P "$auth_dir" 2>/dev/null && pwd -P) || resolved_auth_dir=
    if [ -z "$resolved_auth_dir" ]; then
        echo "[check-host] FATAL: auth directory resolves through a symlink" >&2
        return 1
    fi
    chmod 0700 "$auth_dir"
    if [ -L "$auth_file" ] || [ -e "$auth_file" ]; then
        echo "[check-host] FATAL: refusing pre-existing auth file" >&2
        return 1
    fi
    if [ -z "${EXEC_SERVER_AUTH_TOKEN:-}" ]; then
        echo "[check-host] FATAL: EXEC_SERVER_AUTH_TOKEN is empty" >&2
        return 1
    fi
    old_umask=$(umask)
    umask 077
    if ! (set -C; printf '%s' "$EXEC_SERVER_AUTH_TOKEN" > "$auth_file"); then
        umask "$old_umask"
        echo "[check-host] FATAL: could not create auth file safely" >&2
        return 1
    fi
    umask "$old_umask"
    if ! chmod 0400 "$auth_file"; then
        rm -f "$auth_file"
        echo "[check-host] FATAL: could not secure auth file" >&2
        return 1
    fi
    export EXEC_SERVER_AUTH_TOKEN_FILE="$auth_file"
    unset EXEC_SERVER_AUTH_TOKEN
}

validate_auth_file() {
    auth_file="${EXEC_SERVER_AUTH_TOKEN_FILE:-/run/corelink/exec-server-auth-token}"
    auth_dir=${auth_file%/*}
    assert_no_symlink_components "$auth_dir" || return 1
    resolved_auth_dir=$(CDPATH='' cd -P "$auth_dir" 2>/dev/null && pwd -P) || resolved_auth_dir=
    mode=$(stat -c '%a' "$auth_file" 2>/dev/null) || mode=$(stat -f '%Lp' "$auth_file" 2>/dev/null) || mode=
    owner_uid=$(stat -c '%u' "$auth_file" 2>/dev/null) || owner_uid=$(stat -f '%u' "$auth_file" 2>/dev/null) || owner_uid=
    current_uid=$(id -u)
    if [ "${auth_file#/}" = "$auth_file" ] || [ -z "$resolved_auth_dir" ] \
        || [ -L "$auth_file" ] || [ ! -f "$auth_file" ] || [ "$mode" != 400 ] \
        || [ -z "$owner_uid" ] || [ "$owner_uid" != "$current_uid" ]; then
        echo "[check-host] FATAL: auth file is not a validated regular 0400 file" >&2
        return 1
    fi
}

if [ "${EXEC_SERVER_AUTH_TOKEN+x}" = x ]; then
    unset CORELINK_AUTH_BRIDGED CORELINK_DUMB_INIT
    bridge_exec_auth_token
    export CORELINK_AUTH_BRIDGED=1
    # Re-exec with the provider bearer removed from the kernel environment.
    # The first shell therefore cannot become a durable process with the token
    # visible through /proc, even transiently after the bridge returns.
    exec env -u EXEC_SERVER_AUTH_TOKEN "$0" "$@"
fi
if [ "${CORELINK_AUTH_BRIDGED:-}" = 1 ]; then
    validate_auth_file
else
    echo "[check-host] FATAL: auth bridge marker is missing" >&2
    exit 1
fi
unset CORELINK_AUTH_BRIDGED

cleanup_auth_file() {
    rm -f "$EXEC_SERVER_AUTH_TOKEN_FILE" || true
}
trap cleanup_auth_file EXIT

# ---------------------------------------------------------------------------
# Guard: TOOLCHAIN_DIGEST must be present (C6).
# A check-host container MUST always carry the toolchain digest injected by the
# fabric (CLW_* + TOOLCHAIN_DIGEST env are set by CloudflareEngine.spawn).
# An empty digest is a configuration error — fail closed, never proceed.
# ---------------------------------------------------------------------------
if [ -z "${TOOLCHAIN_DIGEST:-}" ]; then
    echo "[check-host] FATAL: TOOLCHAIN_DIGEST is empty. The fabric must inject TOOLCHAIN_DIGEST at spawn. Refusing to start." >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# O7 hardening: cap the process count (ulimit -u) so a runaway CheckDef (fork
# bomb / thread storm) cannot exhaust the container's PID table and starve the
# exec-server. Inherited by clw hydrate and the exec-server + its children.
# Best-effort — if the shell can't set it (unprivileged), continue; the microVM
# boundary is the hard isolation, this is defense-in-depth.
# ---------------------------------------------------------------------------
# shellcheck disable=SC3045 # Linux container shells expose -u for the O7 process cap.
ulimit -u 4096 2>/dev/null || echo "[check-host] warn: could not set ulimit -u (continuing)" >&2

# ---------------------------------------------------------------------------
# Hydrate the toolchain (C5).
# clw reads CLW_ENDPOINT, CLW_TENANT, CLW_TOKEN from env (injected by fabric).
# Do NOT echo any CLW_* or TOOLCHAIN_DIGEST values — they are secrets/digests.
# set -e ensures any non-zero exit from clw causes the container to exit
# non-zero immediately, failing the lease closed (no exec-server started).
# ---------------------------------------------------------------------------
# shellcheck disable=SC2086
# (TOOLCHAIN_DIR and TOOLCHAIN_DIGEST are single-valued env vars, no word-split risk)
clw hydrate --manifest-digest "$TOOLCHAIN_DIGEST" "$TOOLCHAIN_DIR"

# ---------------------------------------------------------------------------
# clw hydrate succeeded — start the exec-server under a small signal-forwarding
# wrapper so the ephemeral auth file can be removed on every exit path.
# The exec-server listens on port 8080 (C4 defaultPort).
# ---------------------------------------------------------------------------
trap 'if [ -n "${EXEC_SERVER_PID:-}" ]; then kill -TERM "$EXEC_SERVER_PID" 2>/dev/null || true; wait "$EXEC_SERVER_PID" 2>/dev/null || true; fi; cleanup_auth_file; exit 130' INT
trap 'if [ -n "${EXEC_SERVER_PID:-}" ]; then kill -TERM "$EXEC_SERVER_PID" 2>/dev/null || true; wait "$EXEC_SERVER_PID" 2>/dev/null || true; fi; cleanup_auth_file; exit 143' TERM
/usr/local/bin/corelink-check-exec-server &
EXEC_SERVER_PID=$!
set +e
wait "$EXEC_SERVER_PID"
exec_status=$?
set -e
cleanup_auth_file
exit "$exec_status"
