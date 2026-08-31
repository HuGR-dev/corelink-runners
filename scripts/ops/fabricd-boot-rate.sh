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
#   scripts/ops/fabricd-boot-rate.sh [attempts] [interval_s] [url]
#   scripts/ops/fabricd-boot-rate.sh 30 20
#
# Output: one line per observation (also appended to the log file named at the
# end), then a summary. Exit 0 if at least one attempt SERVED, 1 if none did —
# so it can gate a "is it back?" check without asserting a rate it did not see.

set -euo pipefail

ATTEMPTS="${1:-20}"
INTERVAL="${2:-20}"
URL="${3:-https://corelink-fabricd.gmhelmold.workers.dev}"
TIMEOUT="${FABRICD_PROBE_TIMEOUT:-45}"

LOG="${FABRICD_BOOT_RATE_LOG:-/tmp/fabricd-boot-rate.$(date -u +%Y%m%dT%H%M%SZ).tsv}"

served=0 startfail=0 other=0

printf 'ts\tattempt\thttp\tverdict\tdetail\n' >"$LOG"
echo "fabricd boot-rate — ${ATTEMPTS} attempts, ${INTERVAL}s apart, target ${URL}"
echo "(observation only: nothing is deployed, restarted or reconfigured)"
echo

for i in $(seq 1 "$ATTEMPTS"); do
  body_file="$(mktemp)"
  code="$(curl -s -o "$body_file" -w '%{http_code}' -m "$TIMEOUT" "${URL}/health" || echo 000)"
  body="$(head -c 200 "$body_file" | tr '\n' ' ')"
  rm -f "$body_file"
  ts="$(date -u +%H:%M:%S)"

  # Classify. "Failed to start container" is the container never coming up; it is
  # NOT the same as an application-level error, and conflating them is how an
  # intermittent boot gets mistaken for an intermittent bug.
  if [ "$code" = "200" ]; then
    verdict=SERVED; served=$((served + 1))
  elif printf '%s' "$body" | grep -q 'Failed to start container'; then
    verdict=BOOT_FAILED; startfail=$((startfail + 1))
  else
    verdict=OTHER; other=$((other + 1))
  fi

  printf '%s\t%d\t%s\t%s\t%s\n' "$ts" "$i" "$code" "$verdict" "$body" >>"$LOG"
  printf '%s  #%-3d %s  %s\n' "$ts" "$i" "$code" "$verdict"

  [ "$i" -lt "$ATTEMPTS" ] && sleep "$INTERVAL"
done

total=$((served + startfail + other))
echo
echo "── summary ─────────────────────────────────────────"
printf '  SERVED       %3d / %d\n' "$served" "$total"
printf '  BOOT_FAILED  %3d / %d\n' "$startfail" "$total"
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

[ "$served" -gt 0 ]
