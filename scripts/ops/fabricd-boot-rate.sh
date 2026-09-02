#!/usr/bin/env bash
# fabricd-boot-rate — measure the boot SUCCESS RATE of the fabricd container.
#
# WHY THIS EXISTS (2026-08-31): the fabricd singleton was found to start
# INTERMITTENTLY. Two deploys of a provably identical configuration — verified
# with `wrangler versions view` — produced a serving control plane and a dead
# one. Under an intermittent fault a single deploy-and-probe cannot distinguish
# a fix from a lucky boot, and during the outage triage it repeatedly did not:
# three consecutive successes were read as a root cause that five subsequent
# failures on the same configuration then refuted.
#
# So: never conclude from ONE boot. Measure a RATE, on a FIXED configuration,
# and record the raw observations so someone else can recompute the verdict.
#
# This script deploys nothing and changes nothing. It only observes.
#
# Usage:
#   scripts/ops/fabricd-boot-rate.sh --selftest
#   scripts/ops/fabricd-boot-rate.sh [attempts] [interval_s] [url]
#   scripts/ops/fabricd-boot-rate.sh --require-all [attempts] [interval_s] [url]
#   scripts/ops/fabricd-boot-rate.sh 30 20
#
# Output: one line per observation (also appended to the log file named at the
# end), then a summary. By default exit 0 if at least one attempt SERVED, 1 if
# none did. `--require-all` is the fail-closed gate for a 100% cold-start
# requirement: one non-serving attempt returns 1.

set -euo pipefail

print_help() {
  sed -n '2,25p' "$0" | sed 's/^# //;s/^#//'
}

run_selftest() {
  local normal_help required_help

  if ! normal_help="$("$0" --help)"; then
    printf 'SELFTEST=FAIL path=--help\n' >&2
    return 1
  fi
  if ! required_help="$("$0" --require-all --help)"; then
    printf 'SELFTEST=FAIL path=--require-all--help\n' >&2
    return 1
  fi
  if [[ "$normal_help" != *'Usage:'* || "$required_help" != *'Usage:'* ]]; then
    printf 'SELFTEST=FAIL path=help-output\n' >&2
    return 1
  fi
  printf 'SELFTEST=PASS path=--help\n'
  printf 'SELFTEST=PASS path=--require-all--help\n'
  printf 'SELFTEST=PASS\n'
}

if [[ "${1:-}" == "--selftest" ]]; then
  [[ "$#" -eq 1 ]] || { printf 'usage: %s --selftest\n' "${0##*/}" >&2; exit 2; }
  run_selftest
  exit $?
fi

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  print_help
  exit 0
fi

REQUIRE_ALL=0
if [[ "${1:-}" == "--require-all" ]]; then
  REQUIRE_ALL=1
  shift
fi

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  print_help
  exit 0
fi

if [[ "$#" -gt 3 ]]; then
  printf 'usage: %s [--require-all] [attempts] [interval_s] [url]\n' "${0##*/}" >&2
  exit 2
fi

ATTEMPTS="${1:-20}"
INTERVAL="${2:-20}"
URL="${3:-https://corelink-fabricd.gmhelmold.workers.dev}"
TIMEOUT="${FABRICD_PROBE_TIMEOUT:-45}"
CURL_BIN="${FABRICD_CURL:-curl}"

if [[ ! "$ATTEMPTS" =~ ^[1-9][0-9]*$ ]] || (( ATTEMPTS > 10000 )); then
  printf 'fabricd boot-rate: attempts must be an integer from 1 to 10000\n' >&2
  exit 2
fi
if [[ ! "$INTERVAL" =~ ^(0|[1-9][0-9]*)$ ]] || (( INTERVAL > 86400 )); then
  printf 'fabricd boot-rate: interval_s must be an integer from 0 to 86400\n' >&2
  exit 2
fi
if [[ ! "$TIMEOUT" =~ ^[1-9][0-9]*$ ]] || (( TIMEOUT > 300 )); then
  printf 'fabricd boot-rate: FABRICD_PROBE_TIMEOUT must be an integer from 1 to 300\n' >&2
  exit 2
fi
if [[ ! "$URL" =~ ^https?://[^[:space:]]+$ ]]; then
  printf 'fabricd boot-rate: URL must use http:// or https:// and contain no whitespace\n' >&2
  exit 2
fi
if ! command -v "$CURL_BIN" >/dev/null 2>&1; then
  printf 'fabricd boot-rate: curl command not found: %s\n' "$CURL_BIN" >&2
  exit 2
fi

LOG="${FABRICD_BOOT_RATE_LOG:-/tmp/fabricd-boot-rate.$(date -u +%Y%m%dT%H%M%SZ).tsv}"
[[ -n "$LOG" ]] || { printf 'fabricd boot-rate: log path is empty\n' >&2; exit 2; }
umask 077

served=0 startfail=0 transportfail=0 other=0
body_file=''
cleanup() {
  if [[ -n "$body_file" ]]; then
    rm -f -- "$body_file"
    body_file=''
  fi
}
trap cleanup EXIT INT TERM

printf 'ts\tattempt\thttp\tverdict\tdetail\n' >"$LOG"
echo "fabricd boot-rate — ${ATTEMPTS} attempts, ${INTERVAL}s apart, target ${URL}"
echo "(observation only: nothing is deployed, restarted or reconfigured)"
echo

for i in $(seq 1 "$ATTEMPTS"); do
  body_file="$(mktemp "${TMPDIR:-/tmp}/fabricd-boot-rate.XXXXXX")"
  curl_rc=0
  code="$("$CURL_BIN" -sS -o "$body_file" -w '%{http_code}' -m "$TIMEOUT" "${URL}/health" 2>/dev/null)" || curl_rc=$?
  if (( curl_rc != 0 )); then
    code=000
  fi
  body="$(head -c 200 "$body_file" 2>/dev/null | tr '\r\n\t' '   ' || true)"
  cleanup
  ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  # Classify. "Failed to start container" is the container never coming up; it is
  # NOT the same as an application-level error, and conflating them is how an
  # intermittent boot gets mistaken for an intermittent bug.
  if [[ "$code" == "200" ]]; then
    verdict=SERVED; served=$((served + 1))
  elif (( curl_rc != 0 )); then
    verdict=TRANSPORT_FAILED; transportfail=$((transportfail + 1))
  elif printf '%s' "$body" | grep -Fqi -- 'Failed to start container'; then
    verdict=BOOT_FAILED; startfail=$((startfail + 1))
  else
    verdict=OTHER; other=$((other + 1))
  fi

  printf '%s\t%d\t%s\t%s\t%s\n' "$ts" "$i" "$code" "$verdict" "$body" >>"$LOG"
  printf '%s  #%-3d %s  %s\n' "$ts" "$i" "$code" "$verdict"

  if (( i < ATTEMPTS && INTERVAL > 0 )); then
    sleep "$INTERVAL"
  fi
done

total=$((served + startfail + transportfail + other))
echo
echo "── summary ─────────────────────────────────────────"
printf '  SERVED       %3d / %d\n' "$served" "$total"
printf '  BOOT_FAILED  %3d / %d\n' "$startfail" "$total"
printf '  TRANSPORT_FAILED %3d / %d\n' "$transportfail" "$total"
printf '  OTHER        %3d / %d\n' "$other" "$total"
if [ "$total" -gt 0 ]; then
  printf '  boot success rate: %d%%\n' $((served * 100 / total))
fi
echo "  raw observations: $LOG"
echo
echo "  Reading this honestly: a rate is only comparable against another rate"
echo "  measured the same way on a FIXED config. Do not compare one run of this"
echo "  against a single ad-hoc probe, and do not call a config fixed unless"
echo "  \`wrangler versions view\` says the bindings actually match."

if (( REQUIRE_ALL )); then
  (( served == total ))
else
  (( served > 0 ))
fi
