#!/usr/bin/env bash
# Refuse to roll the runner fleet while it is executing customer work.
#
# ── WHY ───────────────────────────────────────────────────────────────────────
# `wrangler deploy` of the spawn-Worker rolls the `RunnerContainer` class. On a
# rollout Cloudflare's container platform sends SIGTERM to the container's main
# process so it can stop accepting new work and finish what is in flight, allows
# UP TO 15 MINUTES, then sends SIGKILL. Since #495 the runner box HONOURS that
# signal (deploy/runner/entrypoint.sh traps TERM on PID 1, forwards it to run.sh,
# waits RUNNER_TERM_GRACE_SECS and escalates) — so the box now shuts down
# cleanly instead of being killed cold.
#
# What that does NOT fix: a customer job that runs LONGER than the platform's
# 15-minute window still dies mid-flight, and nothing stops a roll from starting
# while boxes are busy. This script is that stop. It is the whole mechanism —
# there is no cordon flag, no new persistent state, and no change to the spawn
# path.
#
# NOT A DRAIN. Nothing here (and nothing in Cloudflare's container config) drains
# a busy instance. In particular `rollout_active_grace_period` is NOT a drain: it
# protects instances that have been connected to their Durable Object for FEWER
# than N seconds — young-instance protection, not busy-instance protection. This
# script does not delay a roll; it WAITS FOR THE FLEET TO GO IDLE ON ITS OWN and
# fails the deploy if it does not.
#
# ── AUTHORITY ─────────────────────────────────────────────────────────────────
# "Is a box busy" is answered by GITHUB, never by our own bookkeeping — our
# bookkeeping (KV `rhandle:` records, placement records) is exactly what has been
# wrong before. A runner is counted busy when GitHub reports
# `status == "online"` AND `busy == true`.
#
# ── SCOPE ─────────────────────────────────────────────────────────────────────
# The fabric registers runners at REPO level, not org level: the spawn-Worker
# mints each box with `POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig`
# (deploy/cloudflare/src/index.ts, `mintJit`). So enumeration is per-repo via
# `GET /repos/{owner}/{repo}/actions/runners`, paginated. The org-level endpoint
# would not see these runners at all.
#
# Only runners advertising the fleet label (default `corelink`) are counted. The
# persistent macOS builder boxes registered on the same repos advertise
# `corelink-builder` / `self-hosted` instead and are NOT rolled by a spawn-Worker
# deploy, so counting them would block the deploy forever.
#
# ── INPUTS (env) ──────────────────────────────────────────────────────────────
#   GH_TOKEN            required — token that can read runners on every repo in
#                       REPOS (`administration: read`). A 403/404 on any repo is
#                       a HARD FAILURE: "I cannot see that repo" must never be
#                       rendered as "that repo is idle".
#   REPOS               space/comma-separated `owner/repo` list. Default: parsed
#                       from RECONCILER_REPOS in deploy/cloudflare/wrangler.jsonc
#                       (single source of truth for the first-party fleet repos).
#   FLEET_LABEL         runner label that identifies a fabric box (default corelink).
#   EXCLUDE_RUNNERS     space-separated runner NAMES never counted as busy. The
#                       calling workflow passes its OWN `RUNNER_NAME` here: the
#                       gate runs on `runs-on: corelink`, i.e. ON a fabric box
#                       that GitHub correctly reports as online+busy, so without
#                       this the gate would wait forever on itself.
#   POLL_INTERVAL_SECS  seconds between polls (default 30).
#   DEADLINE_SECS       give up after this long (default 2700 = 45 min).
#   FORCE               "true" to skip the wait and roll over live work, loudly.
#   GITHUB_STEP_SUMMARY optional — outcome is appended when set.
#
# Exit 0 = fleet idle (or FORCE). Exit 1 = still busy at the deadline, or the
# fleet could not be read. Failing is the correct default: rolling over live
# customer work is the thing this exists to prevent.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WRANGLER_JSONC="${REPO_ROOT}/deploy/cloudflare/wrangler.jsonc"

FLEET_LABEL="${FLEET_LABEL:-corelink}"
EXCLUDE_RUNNERS="${EXCLUDE_RUNNERS:-}"
POLL_INTERVAL_SECS="${POLL_INTERVAL_SECS:-30}"
DEADLINE_SECS="${DEADLINE_SECS:-2700}"
FORCE="${FORCE:-false}"

# The caller's job-level `timeout-minutes` must outlast the deadline, otherwise
# GitHub cancels the job and the operator sees "cancelled" instead of the
# explicit still-busy verdict. The deploy workflow sets 60 minutes.
MAX_SUPPORTED_DEADLINE_SECS="${MAX_SUPPORTED_DEADLINE_SECS:-3300}"

die() { echo "::error::$*" >&2; exit 1; }

[ -n "${GH_TOKEN:-}" ] || die "GH_TOKEN is required to read self-hosted runners."

case "${POLL_INTERVAL_SECS}" in ''|*[!0-9]*) die "POLL_INTERVAL_SECS must be an integer, got '${POLL_INTERVAL_SECS}'.";; esac
case "${DEADLINE_SECS}" in ''|*[!0-9]*) die "DEADLINE_SECS must be an integer, got '${DEADLINE_SECS}'.";; esac
[ "${POLL_INTERVAL_SECS}" -ge 5 ] || die "POLL_INTERVAL_SECS must be >= 5 (GitHub REST budget)."
[ "${DEADLINE_SECS}" -le "${MAX_SUPPORTED_DEADLINE_SECS}" ] || die \
  "DEADLINE_SECS=${DEADLINE_SECS} exceeds ${MAX_SUPPORTED_DEADLINE_SECS}s, which is what the calling job's timeout-minutes allows. Raise the job timeout in the workflow first, otherwise the job is cancelled before this gate can report."

# Default repo list = the first-party allowlist the fabric actually spawns into.
# Parsed from wrangler.jsonc rather than duplicated, so the two cannot drift.
if [ -z "${REPOS:-}" ]; then
  if [ -f "${WRANGLER_JSONC}" ]; then
    REPOS="$(sed -n 's/.*"RECONCILER_REPOS"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "${WRANGLER_JSONC}" | head -1)"
  fi
  [ -n "${REPOS}" ] || die "Could not read RECONCILER_REPOS from ${WRANGLER_JSONC}; pass REPOS explicitly."
fi
# shellcheck disable=SC2206
REPO_LIST=(${REPOS//,/ })
[ "${#REPO_LIST[@]}" -gt 0 ] || die "REPOS resolved to an empty list."

api() {
  curl -sS -w '\n%{http_code}' \
    -H "authorization: Bearer ${GH_TOKEN}" \
    -H "accept: application/vnd.github+json" \
    -H "x-github-api-version: 2022-11-28" \
    -H "user-agent: corelink-wait-for-idle-fleet" \
    "$1"
}

# Print one `repo<TAB>name` line per BUSY fabric runner across every repo.
# PAGINATED — a single page would silently under-count a fleet above 100 boxes
# (max_instances is 250).
busy_runners() {
  local repo page body code total seen matched
  for repo in "${REPO_LIST[@]}"; do
    page=1
    seen=0
    total=-1
    while :; do
      local resp
      resp="$(api "https://api.github.com/repos/${repo}/actions/runners?per_page=100&page=${page}")"
      code="${resp##*$'\n'}"
      body="${resp%$'\n'*}"
      if [ "${code}" != "200" ]; then
        die "GET /repos/${repo}/actions/runners returned HTTP ${code}. Cannot prove the fleet is idle, so refusing to roll. Bind the repo secret FLEET_RUNNERS_READ_TOKEN to a token carrying 'administration: read' on ${repo}, or re-dispatch with force=true to roll over live jobs deliberately."
      fi
      if [ "${total}" -lt 0 ]; then total="$(printf '%s' "${body}" | jq -r '.total_count')"; fi
      local n
      n="$(printf '%s' "${body}" | jq -r '.runners | length')"
      # A jq failure here MUST be fatal. Swallowed, it prints nothing — which
      # this gate would read as "idle" and roll the fleet on. Silent success is
      # the failure mode that this whole workflow exists to refuse.
      local matched
      matched="$(printf '%s' "${body}" | jq -r \
        --arg repo "${repo}" --arg label "${FLEET_LABEL}" --arg exclude "${EXCLUDE_RUNNERS}" '
        [$exclude | splits("[ ,]+")] as $skip
        | .runners[]
        | select(.status == "online" and .busy == true)
        | select([.labels[].name] | index($label))
        | . as $r
        | select($skip | index($r.name) | not)
        | "\($repo)\t\(.name)"')" \
        || die "jq failed while parsing the runner list for ${repo}; refusing to treat an unparsed response as an idle fleet."
      if [ -n "${matched}" ]; then printf '%s\n' "${matched}"; fi
      seen=$((seen + n))
      if [ "${n}" -eq 0 ] || [ "${seen}" -ge "${total}" ]; then break; fi
      page=$((page + 1))
      [ "${page}" -le 20 ] || die "Runner pagination for ${repo} exceeded 20 pages; refusing to guess."
    done
  done
}

# Count lines, treating the empty string as zero (a bare `wc -l` on "" says 1).
count_of() {
  if [ -z "$1" ]; then echo 0; else printf '%s\n' "$1" | wc -l | tr -d ' '; fi
}

summary() {
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then printf '%s\n' "$*" >>"${GITHUB_STEP_SUMMARY}"; fi
  return 0
}

if [ -n "${EXCLUDE_RUNNERS}" ]; then echo "Excluding (this job's own box): ${EXCLUDE_RUNNERS}"; fi
echo "Fleet-idle gate — repos: ${REPO_LIST[*]} | label: ${FLEET_LABEL} | interval: ${POLL_INTERVAL_SECS}s | deadline: ${DEADLINE_SECS}s | force: ${FORCE}"

BUSY="$(busy_runners)"
INITIAL_COUNT="$(count_of "${BUSY}")"
POLLS=1
echo "poll ${POLLS}: ${INITIAL_COUNT} busy fabric runner(s)"
if [ -n "${BUSY}" ]; then printf '%s\n' "${BUSY}" | sed 's/^/  busy: /'; fi

summary "### Fleet-idle gate"
summary ""
summary "- repos: \`${REPO_LIST[*]}\`"
summary "- fleet label: \`${FLEET_LABEL}\`"
summary "- busy at start: **${INITIAL_COUNT}**"

if [ "${FORCE}" = "true" ]; then
  if [ "${INITIAL_COUNT}" -gt 0 ]; then
    # An override that is silent is the same defect wearing a different hat.
    echo "::warning title=FORCED ROLL OVER LIVE WORK::force=true — rolling the fleet while ${INITIAL_COUNT} runner(s) are EXECUTING CUSTOMER JOBS. Those jobs get SIGTERM and are SIGKILLed within 15 minutes."
    echo "################################################################"
    echo "##  FORCED ROLL — ${INITIAL_COUNT} BUSY RUNNER(S) WILL BE KILLED"
    printf '%s\n' "${BUSY}" | sed 's/^/##    /'
    echo "################################################################"
    summary "- polls: 1"
    summary "- verdict: ⚠️ **FORCED** — rolled over ${INITIAL_COUNT} BUSY runner(s); their jobs are killed within the platform's 15-minute SIGTERM window."
    summary ""
    summary "Busy runners rolled over:"
    summary ""
    printf '%s\n' "${BUSY}" | while IFS=$'\t' read -r r n; do summary "- \`${n}\` (${r})"; done
  else
    echo "force=true, and the fleet is idle anyway — nothing was rolled over."
    summary "- polls: 1"
    summary "- verdict: ✅ force=true, fleet already idle — nothing rolled over."
  fi
  exit 0
fi

START="$(date +%s)"
while [ -n "${BUSY}" ]; do
  NOW="$(date +%s)"
  ELAPSED=$((NOW - START))
  if [ "${ELAPSED}" -ge "${DEADLINE_SECS}" ]; then
    COUNT="$(count_of "${BUSY}")"
    NAMES="$(printf '%s\n' "${BUSY}" | awk -F'\t' '{printf "%s(%s) ", $2, $1}')"
    summary "- polls: ${POLLS}"
    summary "- verdict: ❌ **REFUSED** — still ${COUNT} busy runner(s) after ${ELAPSED}s."
    summary ""
    summary "Still busy:"
    summary ""
    printf '%s\n' "${BUSY}" | while IFS=$'\t' read -r r n; do summary "- \`${n}\` (${r})"; done
    die "Deadline of ${DEADLINE_SECS}s expired with ${COUNT} runner(s) still executing customer jobs: ${NAMES}— refusing to roll the fleet. Wait for them to finish and re-dispatch, or re-dispatch with force=true to kill them deliberately."
  fi
  REMAIN=$((DEADLINE_SECS - ELAPSED))
  SLEEP="${POLL_INTERVAL_SECS}"
  if [ "${SLEEP}" -gt "${REMAIN}" ]; then SLEEP="${REMAIN}"; fi
  sleep "${SLEEP}"
  BUSY="$(busy_runners)"
  POLLS=$((POLLS + 1))
  COUNT="$(count_of "${BUSY}")"
  echo "poll ${POLLS} (t+$(( $(date +%s) - START ))s): ${COUNT} busy fabric runner(s)"
  if [ -n "${BUSY}" ]; then printf '%s\n' "${BUSY}" | sed 's/^/  busy: /'; fi
done

echo "Fleet is idle after ${POLLS} poll(s) — safe to roll."
summary "- polls: ${POLLS}"
summary "- verdict: ✅ fleet idle — safe to roll."
