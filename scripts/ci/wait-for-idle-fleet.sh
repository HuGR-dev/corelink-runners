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
# ── AUTHORITY — ASK THE WORKER, WHICH ASKS GITHUB ────────────────────────────
# "Is a box busy" is still answered by GITHUB, never by our own bookkeeping — our
# bookkeeping is exactly what has been wrong before. But this script does not ask
# GitHub DIRECTLY, because it cannot: reading a repo's self-hosted runners needs
# `administration: read` on every repo in RECONCILER_REPOS, the Actions
# `GITHUB_TOKEN` does not carry that and cannot be granted it cross-repo, and
# GitHub exposes no API to mint a PAT (`POST /user/tokens` and
# `POST /user/personal-access-tokens` both 404). So the gate could never
# authenticate and every deploy refused.
#
# The spawn-Worker already holds the answer. It owns the GitHub App credential and
# already asks GitHub, per runner, whether that runner is `busy` — that is what
# its keep-alive sweep does every minute. So the gate asks the Worker:
#
#   GET {FLEET_BUSY_URL}   header `x-corelink-internal-auth: $FLEET_BUSY_READ_KEY`
#   → 200 {"busy":N,"runners":[{"name":..,"repo":..}],"checked":N,"unverifiable":N}
#
# That needs no GitHub permission at all. See `fleetBusySnapshot` in
# deploy/cloudflare/src/index.ts.
#
# ── ⚠️ BOOTSTRAP: THE FIRST DEPLOY CARRYING THE ENDPOINT NEEDS force=true ─────
# This gate now depends on the Worker it gates. The endpoint does not exist in
# production until the deploy that ships it lands, so THAT deploy must be
# dispatched with `force: true` (the gate would otherwise 404 and, correctly,
# refuse). Under force the read is ATTEMPTED but not required: it prints what it
# is about to roll over when it can, and a loud "FLEET STATE UNKNOWN" banner when
# it cannot — never a clean idle verdict. Every deploy after it gates normally.
#
# ── THE FAIL-SAFE, AND ITS DIRECTION ─────────────────────────────────────────
# The fleet is provably idle ONLY when `busy == 0 AND unverifiable == 0`.
# `unverifiable` counts boxes whose state the Worker could not establish (no
# runner id, a cold spawn, a GitHub error/rate-limit, an undocumented body, a KV
# read that failed, a truncated key list). Those are treated EXACTLY like busy
# boxes: we cannot prove idle, so we do not roll. Reading ignorance as idle would
# roll the fleet over live work, which is the one outcome this exists to prevent.
# Likewise any non-200, an unreachable Worker, or an unparseable body is a HARD
# FAILURE, never "idle".
#
# ── SCOPE ─────────────────────────────────────────────────────────────────────
# The Worker enumerates the fabric's own `rhandle:` bindings, i.e. exactly the
# boxes a spawn-Worker deploy rolls. The persistent macOS builder boxes registered
# on the same repos are not in that list at all, so — unlike the previous
# repo-wide GitHub enumeration — no label filter is needed to keep them from
# blocking the deploy forever.
#
# PER-CALL CAP. The Worker reuses the keep-alive sweep's ceiling of 40 GitHub
# reads per call rather than adding a second, contradicting one. Bindings past it
# are reported as `unverifiable`, so the cap makes this gate MORE conservative,
# never less; `checked` is what surfaces that it bound.
#
# ── INPUTS (env) ──────────────────────────────────────────────────────────────
#   FLEET_BUSY_READ_KEY required unless FORCE=true — the Worker's ops-READ key,
#                       presented as `x-corelink-internal-auth`. Its OWN
#                       credential, not the
#                       spawn-control token: see the Env comment on
#                       FLEET_BUSY_READ_KEY in deploy/cloudflare/src/index.ts.
#                       Unset in the Worker ⇒ the route 404s ⇒ this gate fails.
#   FLEET_BUSY_URL      the endpoint. Default:
#                       https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy
#                       (origin from SPAWN_WORKER_PUBLIC_URL in
#                       deploy/cloudflare/wrangler.jsonc).
#   EXCLUDE_RUNNERS     space-separated runner NAMES never counted as busy. The
#                       calling workflow passes its OWN `RUNNER_NAME` here: the
#                       gate runs on `runs-on: corelink`, i.e. ON a fabric box
#                       that GitHub correctly reports as busy, so without this the
#                       gate would wait forever on itself.
#   POLL_INTERVAL_SECS  seconds between polls (default 30).
#   DEADLINE_SECS       give up after this long (default 2700 = 45 min).
#   FORCE               "true" to skip the wait and roll over live work, loudly.
#                       Also tolerates an unreadable fleet — see BOOTSTRAP above.
#   GITHUB_STEP_SUMMARY optional — outcome is appended when set.
#
# Exit 0 = fleet idle (or FORCE). Exit 1 = still busy/unverifiable at the
# deadline, or the fleet could not be read. Failing is the correct default:
# rolling over live customer work is the thing this exists to prevent.
set -euo pipefail

DEFAULT_FLEET_BUSY_URL="https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy"

FLEET_BUSY_URL="${FLEET_BUSY_URL:-${DEFAULT_FLEET_BUSY_URL}}"
EXCLUDE_RUNNERS="${EXCLUDE_RUNNERS:-}"
POLL_INTERVAL_SECS="${POLL_INTERVAL_SECS:-30}"
DEADLINE_SECS="${DEADLINE_SECS:-2700}"
FORCE="${FORCE:-false}"

# The caller's job-level `timeout-minutes` must outlast the deadline, otherwise
# GitHub cancels the job and the operator sees "cancelled" instead of the
# explicit still-busy verdict. The deploy workflow sets 60 minutes.
MAX_SUPPORTED_DEADLINE_SECS="${MAX_SUPPORTED_DEADLINE_SECS:-3300}"

# Annotation level is a variable ONLY so the force path can downgrade the read's
# refusal to a warning: under force the refusal is genuinely overridden, and an
# `::error::` annotation on a step that then succeeds reads as a broken gate.
# It is `error` everywhere else, and `die` always exits non-zero regardless.
DIE_LEVEL="error"
die() { echo "::${DIE_LEVEL}::$*" >&2; exit 1; }

# ⚠️ Deliberately NOT required under force=true. force is the documented bootstrap
# for the FIRST deploy carrying the endpoint, and at that moment neither the repo
# secret nor the Worker secret need exist yet — so demanding the key here would
# make the bootstrap impossible and leave the deploy permanently refused, which is
# the exact dead end this whole change exists to remove.
if [ -z "${FLEET_BUSY_READ_KEY:-}" ] && [ "${FORCE}" != "true" ]; then
  die "FLEET_BUSY_READ_KEY is required to read the fleet's busy state. Bind the repo secret FLEET_BUSY_READ_KEY to the value armed on the spawn-Worker (\`wrangler secret put FLEET_BUSY_READ_KEY\`), or re-dispatch with force=true to roll over live jobs deliberately."
fi

case "${POLL_INTERVAL_SECS}" in ''|*[!0-9]*) die "POLL_INTERVAL_SECS must be an integer, got '${POLL_INTERVAL_SECS}'.";; esac
case "${DEADLINE_SECS}" in ''|*[!0-9]*) die "DEADLINE_SECS must be an integer, got '${DEADLINE_SECS}'.";; esac
[ "${POLL_INTERVAL_SECS}" -ge 5 ] || die "POLL_INTERVAL_SECS must be >= 5 (the Worker fans out one GitHub read per box per poll)."
[ "${DEADLINE_SECS}" -le "${MAX_SUPPORTED_DEADLINE_SECS}" ] || die \
  "DEADLINE_SECS=${DEADLINE_SECS} exceeds ${MAX_SUPPORTED_DEADLINE_SECS}s, which is what the calling job's timeout-minutes allows. Raise the job timeout in the workflow first, otherwise the job is cancelled before this gate can report."

# The last snapshot's scalar counts. `poll_fleet` is always invoked inside a
# command substitution (its stdout is the busy list), so it CANNOT assign these
# directly — a subshell's variables die with it, and this gate would then read
# `unverifiable` as a permanent 0, i.e. exactly the fail-unsafe it exists to
# refuse. They travel through a file instead, and `read_fleet` is the only
# caller-side entry point.
UNVERIFIABLE=0
CHECKED=0
FLEET_STATE_FILE="$(mktemp)"
trap 'rm -f "${FLEET_STATE_FILE}"' EXIT

# Poll, then lift the scalar counts back out of the subshell. Sets BUSY (one
# `repo<TAB>name` line per busy runner, minus the exclusions), UNVERIFIABLE and
# CHECKED. Hard-exits on any failure inside `poll_fleet`.
read_fleet() {
  BUSY="$(poll_fleet)"
  read -r UNVERIFIABLE CHECKED <"${FLEET_STATE_FILE}"
}

# The same read, but reporting failure instead of exiting. ONLY the force path may
# use it: everywhere else an unreadable fleet must stay a hard failure.
read_fleet_tolerant() {
  local rc=0
  DIE_LEVEL="warning"
  BUSY="$(poll_fleet)" || rc=$?
  DIE_LEVEL="error"
  if [ "${rc}" -eq 0 ]; then
    read -r UNVERIFIABLE CHECKED <"${FLEET_STATE_FILE}"
    return 0
  fi
  return 1
}

# Print one `repo<TAB>name` line per BUSY fabric runner, and write
# "<unverifiable> <checked>" to FLEET_STATE_FILE. Every failure path is a hard
# exit: a gate that prints nothing on error would be read as "idle" and roll the
# fleet, which is the silent-success failure mode this workflow exists to refuse.
poll_fleet() {
  local resp code body
  resp="$(curl -sS -w '\n%{http_code}' \
    -H "x-corelink-internal-auth: ${FLEET_BUSY_READ_KEY:-}" \
    -H "accept: application/json" \
    -H "user-agent: corelink-wait-for-idle-fleet" \
    --max-time 30 \
    "${FLEET_BUSY_URL}")" || die "Could not reach ${FLEET_BUSY_URL}. Cannot prove the fleet is idle, so refusing to roll."
  code="${resp##*$'\n'}"
  body="${resp%$'\n'*}"
  if [ "${code}" = "404" ]; then
    die "GET ${FLEET_BUSY_URL} returned HTTP 404 — the route is invisible because FLEET_BUSY_READ_KEY is not armed on the spawn-Worker (\`wrangler secret put FLEET_BUSY_READ_KEY\`), or the deployed Worker predates the endpoint. If this is the FIRST deploy carrying the endpoint, re-dispatch with force=true; that is the documented bootstrap."
  fi
  if [ "${code}" != "200" ]; then
    die "GET ${FLEET_BUSY_URL} returned HTTP ${code}. Cannot prove the fleet is idle, so refusing to roll. Check that FLEET_BUSY_READ_KEY matches the value armed on the spawn-Worker, or re-dispatch with force=true to roll over live jobs deliberately."
  fi

  # A jq failure MUST be fatal for the same reason. Each field is validated as a
  # number so a shape change cannot arrive as an empty string and compare as zero.
  local snap
  snap="$(printf '%s' "${body}" | jq -er '
    if (.busy|type) == "number" and (.unverifiable|type) == "number"
       and (.checked|type) == "number" and (.runners|type) == "array"
    then "\(.unverifiable) \(.checked)"
    else error("missing or wrongly-typed busy/unverifiable/checked/runners") end')" \
    || die "Could not parse the fleet-busy response from ${FLEET_BUSY_URL}; refusing to treat an unparsed response as an idle fleet. Body: ${body}"
  printf '%s\n' "${snap}" >"${FLEET_STATE_FILE}"

  # Name the busy boxes, minus this job's own. The Worker reports every busy box
  # including the one running this gate, which GitHub correctly calls busy.
  # A jq failure here is fatal too — an empty list must mean "no busy runners",
  # never "the names could not be read".
  local names
  names="$(printf '%s' "${body}" | jq -r --arg exclude "${EXCLUDE_RUNNERS}" '
    [$exclude | splits("[ ,]+")] as $skip
    | .runners[]
    # Bind the runner BEFORE the select: inside `$skip | index(...)` the input is
    # $skip, so a bare `.name` there indexes the exclusion ARRAY, not the runner.
    | . as $r
    | select($skip | index($r.name) | not)
    | "\($r.repo)\t\($r.name)"')" \
    || die "Could not read the busy-runner names from ${FLEET_BUSY_URL}; refusing to treat an unparsed response as an idle fleet. Body: ${body}"
  printf '%s' "${names}"
}

# Count lines, treating the empty string as zero (a bare `wc -l` on "" says 1).
count_of() {
  if [ -z "$1" ]; then echo 0; else printf '%s\n' "$1" | wc -l | tr -d ' '; fi
}

summary() {
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then printf '%s\n' "$*" >>"${GITHUB_STEP_SUMMARY}"; fi
  return 0
}

# "Can we prove the fleet is idle?" — busy AND unverifiable must both be zero.
blocked() { [ -n "${BUSY}" ] || [ "${UNVERIFIABLE}" -gt 0 ]; }

if [ -n "${EXCLUDE_RUNNERS}" ]; then echo "Excluding (this job's own box): ${EXCLUDE_RUNNERS}"; fi
echo "Fleet-idle gate — endpoint: ${FLEET_BUSY_URL} | interval: ${POLL_INTERVAL_SECS}s | deadline: ${DEADLINE_SECS}s | force: ${FORCE}"

summary "### Fleet-idle gate"
summary ""
summary "- source: \`${FLEET_BUSY_URL}\` (the spawn-Worker's own GitHub reads)"

# ── force: read if we can, roll either way, and SAY which of the two happened ──
if [ "${FORCE}" = "true" ]; then
  if ! read_fleet_tolerant; then
    # Bootstrap, or a broken endpoint. Rolling blind is the operator's explicit
    # instruction, but it must never look like a clean idle verdict.
    echo "::warning title=FORCED ROLL, FLEET STATE UNKNOWN::force=true and the fleet-busy endpoint could not be read (see the error above). Rolling ANYWAY, on the operator's instruction. Any box currently executing a customer job gets SIGTERM and is SIGKILLed within 15 minutes."
    echo "################################################################"
    echo "##  FORCED ROLL — FLEET STATE COULD NOT BE READ AT ALL"
    echo "################################################################"
    summary "- busy at start: **unknown** (endpoint unreadable)"
    summary "- polls: 1"
    summary "- verdict: ⚠️ **FORCED, BLIND** — the fleet-busy endpoint could not be read; rolled anyway on the operator's instruction. Expected exactly once, on the first deploy carrying \`/internal/v1/fleet/busy\`."
    exit 0
  fi
  INITIAL_COUNT="$(count_of "${BUSY}")"
  echo "poll 1: ${INITIAL_COUNT} busy fabric runner(s), ${UNVERIFIABLE} unverifiable, ${CHECKED} checked"
  summary "- busy at start: **${INITIAL_COUNT}**"
  summary "- unverifiable at start: **${UNVERIFIABLE}** (counts as \"cannot prove idle\")"
  summary "- bindings checked: ${CHECKED}"
  if blocked; then
    # An override that is silent is the same defect wearing a different hat.
    echo "::warning title=FORCED ROLL OVER LIVE WORK::force=true — rolling the fleet while ${INITIAL_COUNT} runner(s) are EXECUTING CUSTOMER JOBS and ${UNVERIFIABLE} could not be verified. Those jobs get SIGTERM and are SIGKILLed within 15 minutes."
    echo "################################################################"
    echo "##  FORCED ROLL — ${INITIAL_COUNT} BUSY + ${UNVERIFIABLE} UNVERIFIABLE RUNNER(S) WILL BE KILLED"
    if [ -n "${BUSY}" ]; then printf '%s\n' "${BUSY}" | sed 's/^/##    /'; fi
    echo "################################################################"
    summary "- polls: 1"
    summary "- verdict: ⚠️ **FORCED** — rolled over ${INITIAL_COUNT} busy + ${UNVERIFIABLE} unverifiable runner(s); their jobs are killed within the platform's 15-minute SIGTERM window."
    if [ -n "${BUSY}" ]; then
      summary ""
      summary "Busy runners rolled over:"
      summary ""
      printf '%s\n' "${BUSY}" | while IFS=$'\t' read -r r n; do summary "- \`${n}\` (${r})"; done
    fi
  else
    echo "force=true, and the fleet is idle anyway — nothing was rolled over."
    summary "- polls: 1"
    summary "- verdict: ✅ force=true, fleet already idle — nothing rolled over."
  fi
  exit 0
fi

# ── the normal path: read (hard-failing on any unreadable fleet), then wait ────
read_fleet
INITIAL_COUNT="$(count_of "${BUSY}")"
POLLS=1
echo "poll ${POLLS}: ${INITIAL_COUNT} busy fabric runner(s), ${UNVERIFIABLE} unverifiable, ${CHECKED} checked"
if [ -n "${BUSY}" ]; then printf '%s\n' "${BUSY}" | sed 's/^/  busy: /'; fi
summary "- busy at start: **${INITIAL_COUNT}**"
summary "- unverifiable at start: **${UNVERIFIABLE}** (counts as \"cannot prove idle\")"
summary "- bindings checked: ${CHECKED}"

START="$(date +%s)"
while blocked; do
  NOW="$(date +%s)"
  ELAPSED=$((NOW - START))
  if [ "${ELAPSED}" -ge "${DEADLINE_SECS}" ]; then
    COUNT="$(count_of "${BUSY}")"
    NAMES="$(printf '%s\n' "${BUSY}" | awk -F'\t' 'NF{printf "%s(%s) ", $2, $1}')"
    summary "- polls: ${POLLS}"
    summary "- verdict: ❌ **REFUSED** — still ${COUNT} busy + ${UNVERIFIABLE} unverifiable runner(s) after ${ELAPSED}s."
    if [ -n "${BUSY}" ]; then
      summary ""
      summary "Still busy:"
      summary ""
      printf '%s\n' "${BUSY}" | while IFS=$'\t' read -r r n; do summary "- \`${n}\` (${r})"; done
    fi
    die "Deadline of ${DEADLINE_SECS}s expired with ${COUNT} runner(s) still executing customer jobs (${NAMES}) and ${UNVERIFIABLE} whose state could not be established — refusing to roll the fleet. An unverifiable box is NOT an idle box. Wait for them to finish and re-dispatch, or re-dispatch with force=true to kill them deliberately."
  fi
  REMAIN=$((DEADLINE_SECS - ELAPSED))
  SLEEP="${POLL_INTERVAL_SECS}"
  if [ "${SLEEP}" -gt "${REMAIN}" ]; then SLEEP="${REMAIN}"; fi
  sleep "${SLEEP}"
  read_fleet
  POLLS=$((POLLS + 1))
  COUNT="$(count_of "${BUSY}")"
  echo "poll ${POLLS} (t+$(( $(date +%s) - START ))s): ${COUNT} busy fabric runner(s), ${UNVERIFIABLE} unverifiable"
  if [ -n "${BUSY}" ]; then printf '%s\n' "${BUSY}" | sed 's/^/  busy: /'; fi
done

echo "Fleet is idle after ${POLLS} poll(s) — 0 busy, 0 unverifiable, ${CHECKED} checked — safe to roll."
summary "- polls: ${POLLS}"
summary "- verdict: ✅ fleet idle (0 busy, 0 unverifiable) — safe to roll."
