#!/usr/bin/env bash
# orphan-box-check.selftest.sh
# =============================================================================
# Proves scripts/orphan-box-check.sh actually FIRES on the states it claims to
# catch, and actually stays quiet on the states it claims are healthy. A leak
# detector that has never been shown to fire is not evidence of anything — and
# the failure this one exists for (a box running with no record of it) has
# already gone unnoticed for 10.2 h once.
#
#   bash scripts/orphan-box-check.selftest.sh
#
# It touches NOTHING live: every case runs the real script against a generated
# instance-list fixture via `--instances-json`. No Cloudflare credential is
# needed and no API call is made.
#
# Fixtures are generated at RUN time rather than committed, because every case
# turns on an AGE relative to now — a committed timestamp would silently flip a
# "young box" fixture into an "over-age box" fixture the next day and the test
# would start passing for the wrong reason.
#
# Exits 0 only if every case behaves as specified.
# =============================================================================
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET="$HERE/orphan-box-check.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

command -v jq >/dev/null 2>&1 || { echo "jq not found." >&2; exit 2; }

pass_count=0
fail_count=0

APP="corelink-spawn-worker-runnercontainer"
FABRICD_APP="corelink-spawn-worker-fabricd"

# ts <minutes-ago> → an RFC3339 UTC timestamp that many minutes in the past.
ts() {
  local mins="$1"
  date -u -v "-${mins}M" '+%Y-%m-%dT%H:%M:%S.000000Z' 2>/dev/null || \
  date -u -d "${mins} minutes ago" '+%Y-%m-%dT%H:%M:%S.000000Z'
}

# inst <name> <minutes-ago> [app] [location] → one instance record, in exactly
# the shape `container-instances.sh --json` emits.
inst() {
  local name="$1" mins="$2" app="${3:-$APP}" loc="${4:-ewr}" t
  t="$(ts "$mins")"
  jq -nc --arg app "$app" --arg name "$name" --arg loc "$loc" --arg t "$t" \
    '{app:$app, id:("i-"+$name[0:8]), name:$name, location:$loc,
      started_at:$t, created_at:$t,
      image:"registry.cloudflare.com/corelink/runner:28d1af39-r1"}'
}

# The Cloudflare platform pool: `_system` instances, long-lived by design and not
# ours. Deliberately placed under the RUNNER application so this exercises the
# `name == "_system"` filter itself rather than being swept up by the app-class
# filter — otherwise the case would pass for the wrong reason.
system_pool() {
  local loc
  for loc in ewr yyz ord lhr nrt sjc fra; do
    inst "_system" 43200 "$APP" "$loc"
  done
}

# run <case-name> <expected-exit> <fixture-file> [extra args…]
run_case() {
  local name="$1" expected="$2" fixture="$3"; shift 3
  local out rc
  set +e
  out="$("$TARGET" --instances-json "$fixture" "$@" 2>&1)"
  rc=$?
  set -e
  if [ "$rc" -eq "$expected" ]; then
    echo "PASS  ${name}  (exit ${rc})"
    pass_count=$((pass_count + 1))
  else
    echo "FAIL  ${name}  (expected exit ${expected}, got ${rc})"
    echo "$out" | awk '{ print "        " $0 }'
    fail_count=$((fail_count + 1))
  fi
}

# ── Case 1: healthy fleet — two young boxes of ours + the platform pool ──────
{ inst "fab330dc-6cf9-4a48-9f10-000000000001" 7
  inst "fab330dc-6cf9-4a48-9f10-000000000002" 41
  system_pool
} | jq -s '.' > "$WORK/clean.json"
run_case "clean fleet ⇒ no page" 0 "$WORK/clean.json"

# ── Case 2: the measured incident — one box up 10.2 h, no bookkeeping ────────
{ cat "$WORK/clean.json" | jq -c '.[]'
  inst "fab330dc-6cf9-4a48-9f10-00000000dead" 612
} | jq -s '.' > "$WORK/leak-overage.json"
run_case "over-age box ⇒ page" 1 "$WORK/leak-overage.json"

# ── Case 3: the platform pool ALONE must not page ────────────────────────────
# Every `_system` instance is 30 days old, i.e. wildly over any lease. If the
# filter regresses this case goes red and stays red — which is precisely how the
# "alarms forever on normal state" failure would show up.
system_pool | jq -s '.' > "$WORK/system-only.json"
run_case "_system pool only ⇒ no page" 0 "$WORK/system-only.json"

# ── Case 4: an empty account is clean, not an error ──────────────────────────
echo '[]' > "$WORK/empty.json"
run_case "no instances at all ⇒ no page" 0 "$WORK/empty.json"

# ── Case 5: more boxes running than the fabric accounts for ─────────────────
# Both boxes in `clean.json` are young, so signal 1 stays silent; only the
# count delta can fire here. This is the case the sbox:-driven reaper is blind
# to by construction.
run_case "2 running vs 1 accounted ⇒ page" 1 "$WORK/clean.json" --accounted 1

# ── Case 6: bookkeeping that covers the fleet ⇒ silent ───────────────────────
# `rhandle:` bindings outlive their boxes, so accounted skews HIGH — the check
# must treat accounted > running as normal, never as a negative-leak error.
run_case "2 running vs 5 accounted ⇒ no page" 0 "$WORK/clean.json" --accounted 5

# ── Case 7: _system must not be counted toward the accounted comparison ─────
# 9 records, 7 of them platform pool. Against 2 accounted this is clean; if the
# filter regressed it would read as 7 unaccounted boxes.
run_case "_system excluded from the count delta" 0 "$WORK/clean.json" --accounted 2

# ── Case 8: a threshold the operator widened still catches the real thing ───
run_case "over-age box vs a 10h threshold ⇒ still page" 1 "$WORK/leak-overage.json" --max-lease-hours 10
run_case "over-age box vs a 24h threshold ⇒ silent"     0 "$WORK/leak-overage.json" --max-lease-hours 24

# ── Case 9: unusable input must be exit 2 (cannot check), never exit 0 ──────
echo '{"not":"an array"}' > "$WORK/bad.json"
run_case "non-array input ⇒ exit 2, not a false clean" 2 "$WORK/bad.json"
run_case "missing fixture ⇒ exit 2" 2 "$WORK/does-not-exist.json"

# ── Case 10: long-lived SERVICE classes are out of scope ────────────────────
# Measured live 2026-08-24: the fabricd singleton had been up 111 h and the
# regional corelinkserver containers likewise. They carry no lease, so they can
# never be over-age. Before the app-class filter existed this exact data made
# the check page on a healthy account.
{ inst "fabricd-singleton" 6684 "$FABRICD_APP" "bog"
  inst "0e1c2f3a-prod-ewr" 20000 "corelink-prod-corelinkserver-prod" "ewr"
  inst "0e1c2f3a-prod-nrt" 20000 "corelink-prod-nrt-corelinkserver-prod-nrt" "nrt"
} | jq -s '.' > "$WORK/services.json"
run_case "long-lived service classes ⇒ no page" 0 "$WORK/services.json"

# …but pointing the pattern AT a service class must still fire, proving the
# exclusion is the pattern and not an accidental hard-coded blindness.
run_case "service class, pattern widened ⇒ page" 1 "$WORK/services.json" \
  --app-pattern '.'

# ── Case 11: LIVE-mode token resolution ─────────────────────────────────────
# These run WITHOUT --instances-json, i.e. down the real credential path, but
# they are still prod-safe: cases (a) and (b) never reach the network, and (c)
# only ever issues a GET that the API rejects. Nothing is mutated in any of them.
#
# The load-bearing assertion is that every one of these is exit 2 ("cannot
# check") and NOT exit 0. A sweep that cannot authenticate and then reports
# nothing is silent success — the defect class this whole check exists to end.
live_case() {
  local name="$1" expected="$2"; shift 2
  local out rc
  set +e
  out="$(env "$@" "$TARGET" 2>&1)"
  rc=$?
  set -e
  if [ "$rc" -eq "$expected" ]; then
    echo "PASS  ${name}  (exit ${rc})"
    pass_count=$((pass_count + 1))
  else
    echo "FAIL  ${name}  (expected exit ${expected}, got ${rc})"
    echo "$out" | awk '{ print "        " $0 }'
    fail_count=$((fail_count + 1))
  fi
}

live_case "no token at all ⇒ exit 2, not a false clean" 2 \
  -u CLOUDFLARE_CONTAINERS_API_TOKEN -u CLOUDFLARE_API_TOKEN \
  CLOUDFLARE_ACCOUNT_ID=0000

live_case "no account id ⇒ exit 2" 2 \
  -u CLOUDFLARE_ACCOUNT_ID CLOUDFLARE_API_TOKEN=irrelevant

# A token the API rejects must be exit 2. Uses a syntactically valid but bogus
# value against the real endpoint: one GET, 401/403, nothing touched.
live_case "rejected token ⇒ exit 2, not a false clean" 2 \
  -u CLOUDFLARE_CONTAINERS_API_TOKEN \
  CLOUDFLARE_ACCOUNT_ID=6a1fc1c626fc2628823e60b9db01f5cd \
  CLOUDFLARE_API_TOKEN=deliberately-invalid-token-for-the-selftest

# ── Case 12: a truncated page must be exit 2, never CLEAN ───────────────────
# Truncation is the one failure that would silently hide the orphan that matters
# — the one past the cap. Forcing per_page below the account's record count
# reproduces it exactly, and is still just a GET. Skipped when no credential is
# available (it needs the real API to produce a real next_page_token).
if [ -n "${CLOUDFLARE_CONTAINERS_API_TOKEN:-}${CLOUDFLARE_API_TOKEN:-}" ] && [ -n "${CLOUDFLARE_ACCOUNT_ID:-}" ]; then
  live_case "truncated page ⇒ exit 2, not a false clean" 2 \
    CONTAINER_INSTANCES_PER_PAGE=1
else
  echo "SKIP  truncated page ⇒ exit 2  (no Cloudflare credential in the environment)"
fi

echo
echo "selftest: ${pass_count} passed, ${fail_count} failed"
[ "$fail_count" -eq 0 ]
