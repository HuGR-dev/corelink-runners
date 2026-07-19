#!/usr/bin/env bash
# E2E validation suite — single entrypoint.
#
#   scripts/e2e/run.sh critic       # G1 completeness-critic only (fast, zero-infra, CI gate)
#   scripts/e2e/run.sh ts1          # correctness (E0/E1)          [authored next]
#   scripts/e2e/run.sh ts2          # live-journey (E2/E3)          [authored next]
#   scripts/e2e/run.sh ts3          # stress (E4)                   [authored next]
#   scripts/e2e/run.sh ts5          # security-adversarial          [authored next]
#   scripts/e2e/run.sh ts4          # chaos (E5) — RUN is owner-gated, refuses without CHAOS_OK=1
#   scripts/e2e/run.sh all          # every non-gated suite, in order
#
# Every suite writes evidence to docs/validation/evidence/<run-id>/ (G2: behavior + artifact).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
what="${1:-critic}"

run_critic() {
  echo "── G1 · completeness-critic ────────────────────────────────"
  node --test "$here/completeness.test.mjs"
}

# Live suites need the real tenant PATs. Source the OOB e2e env if present (never printed).
load_live_env() {
  local envf="$HOME/.hugit/secrets/e2e-prod-env.sh"
  if [ -f "$envf" ]; then set +u; . "$envf" >/dev/null 2>&1; set -u; fi
  export E2E_LIVE=1
}

case "$what" in
  critic) run_critic ;;
  ts4)
    if [ "${CHAOS_OK:-0}" != "1" ]; then
      echo "TS-4 chaos is DISRUPTIVE on live infra — RUN is owner-gated." >&2
      echo "Re-run with CHAOS_OK=1 only after the owner has reviewed the coverage." >&2
      exit 2
    fi
    echo "TS-4 chaos cells not yet authored." >&2; exit 3 ;;
  ts2)
    echo "── TS-2 · live-journey (no-spawn behavioral probes) ────────"
    load_live_env; node --test "$here/journey/live-probes.test.mjs" ;;
  ts5)
    echo "── TS-5 · security-adversarial (gate probes) ───────────────"
    load_live_env; node --test "$here/security/gates.test.mjs" ;;
  ts6)
    echo "── TS-6 · multi-tenant + entitlement (2 real tenants) ──────"
    load_live_env; node --test "$here/tenants/multitenant.test.mjs" ;;
  door-a)
    echo "── TS-2 · Door-A box-spawn journey (E3, SPAWNS A REAL BOX) ─"
    bash "$here/journey/door-a-spawn.sh" "${2:-}" ;;
  journeys)
    echo "── STORY JOURNEYS · real user narratives (create + close REAL leases) ─"
    load_live_env; export E2E_RUN_ID=journeys
    # Warm the per-tenant plan cache first so cap-asserting journeys are deterministic on a
    # freshly-rolled container (the cold-cache pre-condition — see the findings doc).
    node "$here/warm-tenants.mjs"
    node --test "$here"/journeys/*.test.mjs ;;
  ts1|ts3)
    echo "Suite '$what' cells not yet authored (scaffold in place; build wave next)." >&2; exit 3 ;;
  all)
    run_critic
    echo "(TS-1..TS-5 cells authored in the build wave; TS-4 chaos stays owner-gated.)" ;;
  *)
    echo "unknown target: $what (see header for usage)" >&2; exit 64 ;;
esac
