#!/usr/bin/env bash
# deploy/cloudflare/entrypoint.sh
# CoreLink Runner DevEnv Container Entrypoint
set -euo pipefail

# Cloudflare Containers 0.3.7 has no secret mount. Convert the provider's
# ingress-only env secret to a regular 0400 file before hydration or supervisor
# starts; durable children receive only the path and never the bearer itself.
bridge_exec_auth_token() {
    local auth_file="${EXEC_SERVER_AUTH_TOKEN_FILE:-/run/corelink/exec-server-auth-token}"
    local auth_dir="${auth_file%/*}"
    if [[ -z "$auth_dir" || "$auth_dir" == "$auth_file" ]]; then
        error "EXEC_SERVER_AUTH_TOKEN_FILE must name a file"
        return 1
    fi
    if [[ -L "$auth_dir" || ( -e "$auth_dir" && ! -d "$auth_dir" ) ]]; then
        error "auth directory is not a safe directory"
        return 1
    fi
    [[ -e "$auth_dir" ]] || mkdir -p "$auth_dir"
    chmod 0700 "$auth_dir"
    if [[ -L "$auth_file" || -e "$auth_file" ]]; then
        error "refusing pre-existing auth file"
        return 1
    fi
    if [[ -z "${EXEC_SERVER_AUTH_TOKEN:-}" ]]; then
        error "EXEC_SERVER_AUTH_TOKEN is empty"
        return 1
    fi
    local old_umask
    old_umask=$(umask)
    umask 077
    if ! (set -o noclobber; printf '%s' "$EXEC_SERVER_AUTH_TOKEN" > "$auth_file"); then
        umask "$old_umask"
        error "could not create auth file safely"
        return 1
    fi
    umask "$old_umask"
    if ! chmod 0400 "$auth_file"; then
        rm -f "$auth_file"
        error "could not secure auth file"
        return 1
    fi
    export EXEC_SERVER_AUTH_TOKEN_FILE="$auth_file"
    unset EXEC_SERVER_AUTH_TOKEN
}

log() {
    echo "[$(date -u +'%Y-%m-%dT%H:%M:%SZ')] [entrypoint] $*"
}

error() {
    echo "[$(date -u +'%Y-%m-%dT%H:%M:%SZ')] [entrypoint] ERROR: $*" >&2
}

# ── 1. Validate Environment Variables ─────────────────────────────────
: "${CLW_TENANT:?CLW_TENANT must be set}"
: "${WORKSPACE_NAME:?WORKSPACE_NAME must be set}"
: "${PROFILE_NAME:?PROFILE_NAME must be set}"

if [[ "${CORELINK_AUTH_BRIDGED:-}" != 1 ]]; then
    bridge_exec_auth_token
    export CORELINK_AUTH_BRIDGED=1
    # Force a fresh process environment so the provider bearer is absent from
    # this shell's /proc entry before hydration and supervisor startup.
    exec env -u EXEC_SERVER_AUTH_TOKEN "$0" "$@"
fi
unset CORELINK_AUTH_BRIDGED

cleanup_auth_file() {
    rm -f "${EXEC_SERVER_AUTH_TOKEN_FILE}" || true
}
trap cleanup_auth_file EXIT

CLW_BIN="/usr/local/bin/clw"
CLW_REF_DOMAIN="${CLW_REF_DOMAIN:-runner}"
PROFILE_DIR="/data/chrome"
WORKSPACE_DIR="/data/workspace"

# ── 2. CLW auth remains available only to the hydration command ────────
if [[ -n "${CLW_TOKEN:-}" ]]; then
    export CLW_TOKEN="${CLW_TOKEN}"
fi

# ── 3. Hydration Functions ───────────────────────────────────────────
clw_ref_exists() {
    local ref_name="$1"
    local ls_out
    if ! ls_out=$("${CLW_BIN}" --ref-domain "${CLW_REF_DOMAIN}" ls --name "${ref_name}" --json 2>&1); then
        error "clw ls failed for ref '${ref_name}': ${ls_out}"
        return 2
    fi
    if [[ "${ls_out}" == "[]" || -z "${ls_out// /}" ]]; then
        return 1
    fi
    return 0
}

hydrate_profile() {
    log "Hydrating browser profile: ${PROFILE_NAME}"
    local ref_rc=0
    clw_ref_exists "${PROFILE_NAME}" || ref_rc=$?
    if [[ ${ref_rc} -eq 1 ]]; then
        log "No existing browser profile (first run), continuing fresh"
        return 0
    fi
    if [[ ${ref_rc} -ne 0 ]]; then
        error "clw ls failed for profile (rc=${ref_rc})"
        return "${ref_rc}"
    fi

    "${CLW_BIN}" --ref-domain "${CLW_REF_DOMAIN}" --concurrency 8 \
        hydrate "${PROFILE_DIR}" --name "${PROFILE_NAME}"
    log "Browser profile hydrated successfully"
}

hydrate_workspace() {
    log "Hydrating workspace: ${WORKSPACE_NAME}"
    local ref_rc=0
    clw_ref_exists "${WORKSPACE_NAME}" || ref_rc=$?
    if [[ ${ref_rc} -eq 1 ]]; then
        log "No existing workspace (first run), starting fresh"
        return 0
    fi
    if [[ ${ref_rc} -ne 0 ]]; then
        error "clw ls failed for workspace (rc=${ref_rc})"
        return "${ref_rc}"
    fi

    "${CLW_BIN}" --ref-domain "${CLW_REF_DOMAIN}" --concurrency 8 \
        hydrate "${WORKSPACE_DIR}" --name "${WORKSPACE_NAME}"
    log "Workspace hydrated successfully"
}

# ── 4. Snapshot & Shutdown Trap ───────────────────────────────────────
snapshot_on_shutdown() {
    log "Shutdown signal received, performing final sync and snapshot..."
    sync || true

    "${CLW_BIN}" --ref-domain "${CLW_REF_DOMAIN}" --concurrency 8 \
        snapshot "${PROFILE_DIR}" --name "${PROFILE_NAME}" --force || true

    "${CLW_BIN}" --ref-domain "${CLW_REF_DOMAIN}" --concurrency 8 \
        snapshot "${WORKSPACE_DIR}" --name "${WORKSPACE_NAME}" --force || true

    # Clean up auth file from memory
    rm -f /dev/shm/.clw-auth "${EXEC_SERVER_AUTH_TOKEN_FILE}" || true
    log "Final snapshot complete. Exiting."
    exit 0
}

SUPERVISOR_PID=""
forward_shutdown() {
    if [[ -n "$SUPERVISOR_PID" ]]; then
        kill -TERM "$SUPERVISOR_PID" 2>/dev/null || true
        wait "$SUPERVISOR_PID" 2>/dev/null || true
    fi
    snapshot_on_shutdown
}

trap forward_shutdown SIGTERM SIGINT

# ── 5. Main Execution Flow ────────────────────────────────────────────
main() {
    log "CoreLink DevEnv starting up..."
    mkdir -p "${PROFILE_DIR}" "${WORKSPACE_DIR}"

    hydrate_profile
    hydrate_workspace

    log "Starting supervisord process manager..."
    /usr/bin/supervisord -n -c /etc/supervisor/conf.d/supervisord.conf &
    SUPERVISOR_PID=$!
    set +e
    wait "$SUPERVISOR_PID"
    supervisor_status=$?
    set -e
    SUPERVISOR_PID=""
    cleanup_auth_file
    return "$supervisor_status"
}

main "$@"
