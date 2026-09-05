#!/usr/bin/env bash
set +e

# This wrapper is deliberately stable for both policies.  The policy is not a
# cache-key axis: optional warms the same entry that required-hit can consume.
required_miss=78
policy="${CL_CACHE_POLICY:-optional}"
case "$policy" in
  optional) allow_cold=1 ;;
  required-hit) allow_cold=0 ;;
  *)
    echo "::error title=corelink-memoize::invalid cache-policy '$policy' (expected optional or required-hit)" >&2
    exit "$required_miss"
    ;;
esac

input_args=()
for p in ${CL_INPUTS:-}; do input_args+=(--input "$p"); done
env_args=()
for e in ${CL_ENVNAMES:-}; do env_args+=(--env "$e"); done

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

# The command string is identical for both policies. On a miss required-hit
# invokes only this guard; the user's command is unreachable when allow_cold=0.
export CORELINK_MEMOIZE_ALLOW_COLD="$allow_cold"
# shellcheck disable=SC2016 # this literal is intentionally evaluated by clw's child shell
memo_command='if [ "${CORELINK_MEMOIZE_ALLOW_COLD:-0}" = "1" ]; then bash -c "$CL_RUN"; else exit 78; fi'

moat_present=false
if [ -n "${CLW_ENDPOINT:-}" ] && { [ -n "${CLW_TOKEN:-}" ] || [ -n "${CLW_CRED_TICKET:-}" ]; } && command -v clw >/dev/null 2>&1; then
  moat_present=true
fi

if [ "$moat_present" != true ]; then
  if [ "$policy" = required-hit ]; then
    echo "::error title=corelink-memoize::required-hit refused: CoreLink moat is absent" >&2
    exit "$required_miss"
  fi
  echo "corelink-memoize: moat absent (no CLW_*/clw) — COLD run"
  run_cold
  exit "$?"
fi

export CLW_REF_DOMAIN="${CLW_REF_DOMAIN:-runner}"
echo "corelink-memoize: moat present — memoizing via clw run (policy=$policy)"
echo "corelink-memoize[env-0-check]: CLW_REF_DOMAIN=[${CLW_REF_DOMAIN:-}] CLW_CRED_TICKET_len=[${#CLW_CRED_TICKET}] CLW_LEASE_ID=[${CLW_LEASE_ID:-}] CLW_FABRIC_ENDPOINT_set=[$([ -n "${CLW_FABRIC_ENDPOINT:-}" ] && echo yes || echo no)] CLW_TOKEN_set=[$([ -n "${CLW_TOKEN:-}" ] && echo yes || echo no)]"

if [ "$policy" = optional ]; then
  clw run "${input_args[@]}" "${env_args[@]}" -- bash -c "$memo_command"
  rc=$?
  if [ "$rc" -eq 125 ]; then
    echo "::warning title=corelink-memoize::clw internal error (125) — falling back to a COLD run (north star)"
    run_cold
    exit "$?"
  fi
  exit "$rc"
fi

# required-hit must never trust a human-readable line or a child exit alone.
# --json is parsed as exactly one object and only the authenticated HIT schema
# below is accepted. jq is intentionally required here: no parser means 78.
temp_dir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/corelink-memoize.XXXXXX") || {
  echo "::error title=corelink-memoize::required-hit refused: secure result directory unavailable" >&2
  exit "$required_miss"
}
json_file="$temp_dir/result.json"
err_file="$temp_dir/result.err"
trap 'rm -rf "$temp_dir"' EXIT
clw run --json "${input_args[@]}" "${env_args[@]}" -- bash -c "$memo_command" >"$json_file" 2>"$err_file"
rc=$?
if ! command -v jq >/dev/null 2>&1 || ! jq -e -s '
  length == 1 and .[0].schema_version == 1 and
  .[0].verdict == "hit" and .[0].authenticated == true
' "$json_file" >/dev/null 2>&1; then
  echo "::error title=corelink-memoize::required-hit refused: no authenticated structured HIT" >&2
  tail -20 "$err_file" >&2 2>/dev/null || true
  exit "$required_miss"
fi

# A valid HIT may carry the wrapped command's original non-zero verdict; only
# the absence/untrusted/malformed/internal paths above are normalized to 78.
if [ "$rc" -eq 125 ]; then
  echo "::error title=corelink-memoize::required-hit refused: clw internal error" >&2
  exit "$required_miss"
fi
cat "$json_file"
exit "$rc"
