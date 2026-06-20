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

# ── Cache-warm preflight (the moat) — fail-OPEN to a COLD run ─────────────────
# When the fabric injects the CLW_* moat env (in-network CoreLink CAS endpoint +
# the per-job CAS PAT), warm the build cache from the CAS via `clw` BEFORE the
# job. This is the differentiator: cache-warm by construction.
#
# NORTH STAR (hard invariant): the cache is an OPTIMIZATION over a correct cold
# run. Cache absent / unreachable / clw-error ⇒ a SLOW (cold) run, NEVER a broken
# one. EVERY failure here is logged and SWALLOWED; the job always proceeds. The
# moat is off entirely when CLW_* is not injected (today's cold dogfood path, and
# until the D-9 per-job mint + the in-network CAS endpoint are wired).
#
# CLW_TOKEN is a sensitive per-job PAT (A6) — `clw` reads it from the env; it is
# NEVER echoed or expanded into a visible string here.
if [[ -n "${CLW_ENDPOINT:-}" && -n "${CLW_TOKEN:-}" ]]; then
  echo "cache-warm: CLW_* injected — hydrating build cache from the CoreLink CAS (in-network)…"
  if command -v clw >/dev/null 2>&1; then
    if clw hydrate "${CLW_CACHE_DEST:-$HOME/.cache/corelink}" \
         --name "${CLW_CACHE_KEY:-runner-cache}"; then
      echo "cache-warm: hydrate OK — warm run."
    else
      echo "cache-warm: hydrate failed — proceeding COLD (north-star: slow, never broken)." >&2
    fi
  else
    echo "cache-warm: clw not found in image — proceeding COLD." >&2
  fi
else
  echo "cache-warm: CLW_* not injected — cold run (moat off)."
fi

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
