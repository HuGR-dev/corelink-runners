#!/bin/sh
# entrypoint.sh — check-host hydrate-then-serve (C5, cf-check-host-contract.md)
# Lifecycle: clw hydrate → exec-server (PID 1).
# DEFAULT-OFF: this container is only spawned when the check-host path is live-flipped.
set -eu

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
# clw hydrate succeeded — start the exec-server as PID 1 (exec replaces this
# shell so signals propagate correctly for clean teardown on container stop).
# The exec-server listens on port 8080 (C4 defaultPort).
# ---------------------------------------------------------------------------
exec /usr/local/bin/corelink-check-exec-server
