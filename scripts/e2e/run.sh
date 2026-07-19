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

case "$what" in
  critic) run_critic ;;
  ts4)
    if [ "${CHAOS_OK:-0}" != "1" ]; then
      echo "TS-4 chaos is DISRUPTIVE on live infra — RUN is owner-gated." >&2
      echo "Re-run with CHAOS_OK=1 only after the owner has reviewed the coverage." >&2
      exit 2
    fi
    echo "TS-4 chaos cells not yet authored." >&2; exit 3 ;;
  ts1|ts2|ts3|ts5)
    echo "Suite '$what' cells not yet authored (scaffold in place; build wave next)." >&2; exit 3 ;;
  all)
    run_critic
    echo "(TS-1..TS-5 cells authored in the build wave; TS-4 chaos stays owner-gated.)" ;;
  *)
    echo "unknown target: $what (see header for usage)" >&2; exit 64 ;;
esac
