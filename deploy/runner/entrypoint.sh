#!/usr/bin/env bash
# entrypoint.sh — CoreLink ephemeral runner container entrypoint (ADR-0007 Stage A)
#
# CONTRACT:
#   - $CORELINK_RUNNER_JITCONFIG must be set (JIT config token from the fabric).
#   - Exits non-zero immediately if the env var is absent or empty.
#   - Launches the runner in one-shot ephemeral JIT mode; self-deregisters on exit.
#   - NEVER prints the value of $CORELINK_RUNNER_JITCONFIG to stdout/stderr.
#
# Usage (by the CoreLink fabric — not by humans):
#   docker run --rm \
#     -e CORELINK_RUNNER_JITCONFIG="<jit-config-token>" \
#     <image>
set -euo pipefail

# ── Guard: JIT config must be present ─────────────────────────────────────────
if [[ -z "${CORELINK_RUNNER_JITCONFIG:-}" ]]; then
  echo "ERROR: CORELINK_RUNNER_JITCONFIG is not set or is empty." >&2
  echo "       The CoreLink fabric must inject this env var at provision time." >&2
  echo "       This image will NOT idle — exiting with code 1." >&2
  exit 1
fi

# ── Safety: ensure we're in the runner directory ──────────────────────────────
cd "$(dirname "$0")"

# ── Launch: ephemeral one-shot JIT mode ───────────────────────────────────────
# --jitconfig   : modern JIT path (runner ≥ v2.294.0); the token encodes
#                 registration, org, repo, labels, and a one-time use secret.
# Ephemeral + self-deregistering: the runner exits cleanly after one job and
# removes itself from the runner pool.  The fabric tears down the box on exit.
#
# We exec (replace shell) so signals pass cleanly to the runner process.
# The value of CORELINK_RUNNER_JITCONFIG is passed as an argument — it is
# NEVER echoed, logged, or expanded into a visible string here.
exec ./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG"
