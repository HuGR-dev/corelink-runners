#!/usr/bin/env bash
set +e

# clw 0.1.5 has no machine-readable, authenticated HIT status for `run`.
# required-hit therefore refuses before invoking clw; see ADR-0011. Keep the
# optional path equivalent in behavior to the pre-T6-W2 action.
required_miss=78
policy="${CL_CACHE_POLICY:-optional}"
case "$policy" in
  optional) ;;
  required-hit)
    echo "::error title=corelink-memoize::required-hit unavailable: installed clw has no authenticated no-exec HIT API; upgrade clw before enabling this policy" >&2
    exit "$required_miss"
    ;;
  *)
    echo "::error title=corelink-memoize::invalid cache-policy '$policy' (expected optional or required-hit)" >&2
    exit "$required_miss"
    ;;
esac

# Build repeated --input / --env argument arrays (simple word-split; paths
# with spaces are out of scope for v0).
input_args=(); for p in $CL_INPUTS; do input_args+=(--input "$p"); done
env_args=();   for e in $CL_ENVNAMES; do env_args+=(--env "$e"); done

# Auto-fold toolchain versions into the key (safety-by-default): capture each
# tool's version and add it via a folded env var, so a toolchain upgrade busts
# the cache. A tool that's absent contributes empty (still safe).
if [ -n "${CL_TOOLS:-}" ]; then
  tv=""
  for t in $CL_TOOLS; do
    case "$t" in
      rust)   tv="${tv}|rust=$(rustc --version 2>/dev/null)";;
      node)   tv="${tv}|node=$(node --version 2>/dev/null)";;
      python) tv="${tv}|python=$(python3 --version 2>/dev/null)";;
      go)     tv="${tv}|go=$(go version 2>/dev/null)";;
      *)      echo "::warning title=corelink-memoize::unknown tool '$t' (ignored)";;
    esac
  done
  export __CL_TOOLVERS="$tv"
  env_args+=(--env __CL_TOOLVERS)
  echo "corelink-memoize: toolchain folded into key →$tv"
fi

run_cold() { bash -c "$CL_RUN"; }

# Moat present? (CLW_* injected by the CoreLink autoscaler + clw on PATH).
# Two credential shapes are accepted:
#   • CLW_TOKEN — the raw CAS PAT (legacy direct injection), OR
#   • CLW_CRED_TICKET — env-0 ticket redeemed by clw at run time.
if [ -n "${CLW_ENDPOINT:-}" ] && { [ -n "${CLW_TOKEN:-}" ] || [ -n "${CLW_CRED_TICKET:-}" ]; } && command -v clw >/dev/null 2>&1; then
  export CLW_REF_DOMAIN="${CLW_REF_DOMAIN:-runner}"
  echo "corelink-memoize: moat present — memoizing via clw run"
  echo "corelink-memoize[env-0-check]: CLW_REF_DOMAIN=[${CLW_REF_DOMAIN:-}] CLW_CRED_TICKET_len=[${#CLW_CRED_TICKET}] CLW_LEASE_ID=[${CLW_LEASE_ID:-}] CLW_FABRIC_ENDPOINT_set=[$([ -n "${CLW_FABRIC_ENDPOINT:-}" ] && echo yes || echo no)] CLW_TOKEN_set=[$([ -n "${CLW_TOKEN:-}" ] && echo yes || echo no)]"
  clw run "${input_args[@]}" "${env_args[@]}" -- bash -c "$CL_RUN"
  rc=$?
  # clw exit contract: 125 = clw-INTERNAL error (NOT the command's verdict).
  # Any other code is the wrapped command's real exit (cached or fresh).
  if [ "$rc" -eq 125 ]; then
    echo "::warning title=corelink-memoize::clw internal error (125) — falling back to a COLD run (north star)"
    run_cold; rc=$?
  fi
  exit "$rc"
else
  echo "corelink-memoize: moat absent (no CLW_*/clw) — COLD run"
  run_cold; exit "$?"
fi
