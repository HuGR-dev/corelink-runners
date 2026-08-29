#!/usr/bin/env bash
# deploy/cloudflare/entrypoint.sh
# CoreLink Runner DevEnv Container Entrypoint
set -euo pipefail

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

CLW_BIN="/usr/local/bin/clw"
CLW_REF_DOMAIN="${CLW_REF_DOMAIN:-runner}"
PROFILE_DIR="/data/chrome"
WORKSPACE_DIR="/data/workspace"

# ── 2. Export Auth Token ──────────────────────────────────────────────
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
    rm -f /dev/shm/.clw-auth || true
    log "Final snapshot complete. Exiting."
    exit 0
}

trap snapshot_on_shutdown SIGTERM SIGINT

# ── 5. Main Execution Flow ────────────────────────────────────────────
main() {
    log "CoreLink DevEnv starting up..."
    mkdir -p "${PROFILE_DIR}" "${WORKSPACE_DIR}"

    hydrate_profile
    hydrate_workspace

    log "Starting supervisord process manager..."
    exec /usr/bin/supervisord -n -c /etc/supervisor/conf.d/supervisord.conf
}

main "$@"
