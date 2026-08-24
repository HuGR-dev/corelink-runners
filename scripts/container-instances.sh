#!/usr/bin/env bash
# scripts/container-instances.sh — the ONE trustworthy "how many boxes are
# running" answer for the Cloudflare account.
#
# ── Why this script exists ──────────────────────────────────────────────────
#
# An incident was closed out on a wrong number. The Cloudflare Containers API
# offers three different ways to ask "how many containers are running", and
# two of them lie:
#
#   * GET /accounts/{acct}/containers/applications
#     Each application object has an `instances` FIELD. DO NOT USE IT AS A
#     RUNNING COUNT. It is the sum of that application's health block
#     (active + healthy + stopped + failed + scheduling + starting) — pure
#     scheduler/rollout bookkeeping, not "instances currently running". It
#     read 22 once while 3 were actually running. `healthy` in particular
#     never corresponds to a real per-instance state.
#
#   * GET /accounts/{acct}/containers/instances (account-level, no app id)
#     Returns `{"instances": []}` UNCONDITIONALLY, even with a token that
#     works fine against the app-scoped endpoint seconds later. It is dead.
#     Do not build anything on it.
#
#   * GET /accounts/{acct}/containers/applications/{app}/instances
#     This is the truth — but only after two traps are handled, both
#     verified live against prod:
#
#       1. TOMBSTONES. Terminated instances are retained forever with
#          status.state == "inactive" and no started_at/location. One
#          application held 3 running against 350+ inactive tombstones.
#          They look like empty padding in the list; they are not — you
#          must filter on status.state == "running" explicitly.
#
#       2. PAGINATION FLIPS SHAPE. Omit `per_page` and you get every record
#          in one response with `result_info: {}` (no page metadata at
#          all). Pass `per_page` and the response switches to cursor
#          pagination (`next_page_token`) — and there is NO `total_count`
#          field in either mode. A natural-looking `per_page=100` +
#          "read page 1" can silently undercount, and nothing in the
#          response tells you it happened. Cursor pages can also OVERLAP
#          under concurrent churn (an instance starting/stopping between
#          two page fetches can shift the window and reappear on a later
#          page), so the walk below dedupes by instance id before it
#          counts or prints anything — an un-deduped merge would inflate
#          the count and make the self-check below fire on a false
#          positive instead of the real undercount it exists to catch.
#
# This script walks `next_page_token` to completion, and self-checks by also
# fetching the unpaginated form and comparing running counts — see
# `self_check_app` below. If they disagree, it fails loudly instead of
# printing a number.
#
# ── Usage ────────────────────────────────────────────────────────────────────
#
#   scripts/container-instances.sh [--older-than <hours>] [--app <app-id>]
#
#   --older-than <hours>   Only list/count running instances whose age (now -
#                           started_at) exceeds <hours>. Exits non-zero if any
#                           match, so this doubles as a CI/cron check for
#                           "boxes that outlived their idle window".
#   --app <app-id>         Restrict to one application instead of every
#                           application in the account.
#   --json                 Emit the deduped RUNNING instance list as a JSON
#                           array on stdout instead of the human table, and
#                           print nothing else. Every consumer that needs a
#                           machine-readable fleet view must go through this
#                           flag rather than re-implementing the pagination +
#                           tombstone + self-check handling documented above —
#                           a second counter is a second set of these traps.
#                           Each element: {app, id, name, location, started_at,
#                           created_at, image}. `--older-than` is ignored in
#                           this mode (the consumer applies its own policy);
#                           exit is 0 whenever the fetch itself succeeded.
#
# ── Required env ─────────────────────────────────────────────────────────────
#
#   CLOUDFLARE_ACCOUNT_ID
#   CLOUDFLARE_CONTAINERS_API_TOKEN  — preferred, or
#   CLOUDFLARE_API_TOKEN             — accepted fallback. Measured 2026-08-24:
#                                      the plain account token reads the
#                                      containers endpoints fine (200,
#                                      success: true). Whichever resolves is
#                                      reported by NAME on stderr; the value is
#                                      never printed.
#
# Get both from corelink-server/.env.local:
#   cd corelink-server && set -a && . ./.env.local && set +a
#
# Read-only. Only ever issues GET requests.
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

CF_API_BASE="https://api.cloudflare.com/client/v4"

# Single-page size for --json. Must stay comfortably above the account's total
# instance record count (running + retained tombstones) — see the completeness
# proof in fetch_app_instances. Overridable so an operator can raise it without
# an edit when tombstones grow.
PER_PAGE="${CONTAINER_INSTANCES_PER_PAGE:-2000}"

die() { echo "ERROR: $*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || die "curl not found."
command -v jq   >/dev/null 2>&1 || die "jq not found (required to parse the CF API response)."

: "${CLOUDFLARE_ACCOUNT_ID:?CLOUDFLARE_ACCOUNT_ID must be set}"

# ── Token resolution — EITHER name, explicit precedence ──────────────────────
# Measured 2026-08-24: the plain CLOUDFLARE_API_TOKEN reads
# GET /accounts/<acc>/containers/applications perfectly well (HTTP 200,
# success: true). A containers-scoped token is preferred when one exists, but
# requiring it would mean copying a second credential into every repo that wants
# to ask this question — a wider blast radius for no gain when a token already
# present does the job.
#
# The resolved variable NAME is reported (to stderr, so --json stdout stays pure
# JSON). The VALUE is never printed, here or anywhere else in this script.
CF_TOKEN=""
CF_TOKEN_VAR=""
if [ -n "${CLOUDFLARE_CONTAINERS_API_TOKEN:-}" ]; then
  CF_TOKEN="${CLOUDFLARE_CONTAINERS_API_TOKEN}"
  CF_TOKEN_VAR="CLOUDFLARE_CONTAINERS_API_TOKEN"
elif [ -n "${CLOUDFLARE_API_TOKEN:-}" ]; then
  CF_TOKEN="${CLOUDFLARE_API_TOKEN}"
  CF_TOKEN_VAR="CLOUDFLARE_API_TOKEN"
else
  die "no Cloudflare API token in the environment. Set CLOUDFLARE_CONTAINERS_API_TOKEN (preferred) or CLOUDFLARE_API_TOKEN. Both are accepted; the first one set wins."
fi
echo "auth: using \$${CF_TOKEN_VAR} (value never printed)" >&2

OLDER_THAN_HOURS=""
ONLY_APP=""
EMIT_JSON=0

while [ $# -gt 0 ]; do
  case "$1" in
    --older-than)
      OLDER_THAN_HOURS="${2:?--older-than requires a number of hours}"
      shift 2
      ;;
    --app)
      ONLY_APP="${2:?--app requires an application id}"
      shift 2
      ;;
    --json)
      EMIT_JSON=1
      shift
      ;;
    -h|--help)
      sed -n '1,80p' "$0" | grep '^#' | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

cf_get() {
  # cf_get <path-with-query> <out-file>
  # Fails loudly on a non-success API response — never silently return a
  # partial/failed body to a caller that will then count records out of it.
  local path="$1" out="$2" http_code
  http_code="$(curl -s --max-time 30 \
    -H "Authorization: Bearer ${CF_TOKEN}" \
    -o "$out" -w '%{http_code}' \
    "${CF_API_BASE}${path}")"
  # A token that cannot authenticate must NEVER read as "nothing to report".
  # Call it out by the variable NAME so the fix is obvious, and still die.
  # Measured 2026-08-24: a bad token on this endpoint comes back **400** with
  # `code: 9106, "Authentication failed"`, not 401/403. Matching only on the
  # obvious statuses would have let the actionable message go unprinted, so the
  # body's error code is checked too. Either way it dies — the status shape is
  # about the QUALITY of the message, never about whether we fail.
  if [ "$http_code" = "401" ] || [ "$http_code" = "403" ] \
     || grep -q '"code":9106' "$out" 2>/dev/null \
     || grep -qi 'authentication failed' "$out" 2>/dev/null; then
    die "GET ${path} → HTTP ${http_code}: \$${CF_TOKEN_VAR} was rejected by the Cloudflare API (wrong token, or it lacks containers read). This is NOT an empty fleet — refusing to report anything."
  fi
  if [ "$http_code" != "200" ]; then
    die "GET ${path} → HTTP ${http_code} (body: $(cat "$out" 2>/dev/null | head -c 500))"
  fi
  if [ "$(jq -r '.success' "$out" 2>/dev/null)" != "true" ]; then
    die "GET ${path} → success=false: $(jq -c '.errors' "$out" 2>/dev/null)"
  fi
}

# ── Enumerate applications ────────────────────────────────────────────────────
# NOTE: applications[].instances is the health-block sum described above —
# read only .id/.name here, never .instances.
apps_file="${WORKDIR}/apps.json"
cf_get "/accounts/${CLOUDFLARE_ACCOUNT_ID}/containers/applications" "$apps_file"

if [ -n "$ONLY_APP" ]; then
  mapfile -t APP_IDS <<<"$ONLY_APP"
  mapfile -t APP_NAMES < <(jq -r --arg id "$ONLY_APP" '.result[] | select(.id==$id) | .name' "$apps_file")
  [ "${#APP_NAMES[@]}" -gt 0 ] || die "app id ${ONLY_APP} not found in this account"
else
  mapfile -t APP_IDS < <(jq -r '.result[].id' "$apps_file")
  mapfile -t APP_NAMES < <(jq -r '.result[].name' "$apps_file")
fi

# ── Fetch + paginate one application's instances, with a self-check ──────────
# Returns (via files under $WORKDIR) the full instance list for one app, and
# aborts if the paginated walk and the unpaginated fetch disagree on the
# running count — that disagreement IS the undercount this script guards
# against, so it must never be swallowed.
fetch_app_instances() {
  local app_id="$1"
  local merged="${WORKDIR}/${app_id}.merged.json"
  local page_dir="${WORKDIR}/${app_id}.pages"
  mkdir -p "$page_dir"

  # ── --json takes ONE page big enough to prove it is the whole list ────────
  #
  # Two things had to be true at once here, and the unpaginated form gives only
  # one of them.
  #
  # (a) ONE INSTANT. Measured live 2026-08-24, the paginated walk below needs 17
  #     requests over ~a minute and returned 1640 records for 483 unique ids —
  #     the cursor window slides under churn. Its RUNNING count read 6 on one
  #     attempt and 20 on the next, which trips the self-check and aborts the
  #     script. Right for a human reading a number off a table; fatal for a
  #     detector that must still report on a live fleet.
  #
  # (b) PROVABLY COMPLETE. This is the half the unpaginated form CANNOT give.
  #     It answers with `result_info: {}` — no page metadata of any kind — so
  #     "it returned everything" is an inference, never a fact carried in the
  #     payload. Truncation is precisely the failure this consumer exists to
  #     catch: if the response were silently capped, the orphan that matters is
  #     the one past the cap, and the verdict would read CLEAN.
  #
  # So: request a single page LARGER than the whole record set and require the
  # cursor to be ABSENT. `next_page_token` missing is the API stating there is
  # nothing after this page — completeness proven BY the payload, in one request,
  # at one instant. Measured on this account (runner app, 532 records):
  #
  #     per_page=100   → 100 records, next_page_token PRESENT
  #     per_page=500   → 500 records, next_page_token PRESENT
  #     per_page=1000  → 532 records, next_page_token ABSENT   ← complete
  #     per_page=2000  → 532 records, next_page_token ABSENT
  #     per_page=5000  → 532 records, next_page_token ABSENT
  #
  # per_page is honoured, not clamped to 100 or 500. 2000 is the default here:
  # ~4x today's record count, and the fleet cap is 250 live boxes.
  #
  # ⛔ If the cursor IS present the page was capped and we have NOT seen the
  # whole fleet. That must fail — never a count, never a CLEAN verdict. Raise
  # CONTAINER_INSTANCES_PER_PAGE when tombstones eventually outgrow the default.
  if [ "$EMIT_JSON" -eq 1 ]; then
    local snap="${page_dir}/single-page.json"
    cf_get "/accounts/${CLOUDFLARE_ACCOUNT_ID}/containers/applications/${app_id}/instances?per_page=${PER_PAGE}" "$snap"
    if [ -n "$(jq -r '.result_info.next_page_token // empty' "$snap")" ]; then
      die "TRUNCATED PAGE for app ${app_id}: asked for per_page=${PER_PAGE} and the API still returned a next_page_token, so this is NOT the whole instance list and an orphan past the cap would read as CLEAN. Refusing to emit a partial fleet. Raise CONTAINER_INSTANCES_PER_PAGE above ${PER_PAGE}."
    fi
    jq '.result.instances' "$snap" > "$merged"
    echo "$merged"
    return 0
  fi

  # Paginated walk: follow next_page_token until absent.
  local token="" page=0 page_file
  echo "[]" > "$merged"
  while :; do
    page=$((page + 1))
    page_file="${page_dir}/${page}.json"
    if [ -z "$token" ]; then
      cf_get "/accounts/${CLOUDFLARE_ACCOUNT_ID}/containers/applications/${app_id}/instances?per_page=100" "$page_file"
    else
      cf_get "/accounts/${CLOUDFLARE_ACCOUNT_ID}/containers/applications/${app_id}/instances?per_page=100&page_token=${token}" "$page_file"
    fi
    jq -s '.[0] + .[1].result.instances' "$merged" "$page_file" > "${merged}.tmp" && mv "${merged}.tmp" "$merged"
    token="$(jq -r '.result_info.next_page_token // empty' "$page_file")"
    [ -n "$token" ] || break
  done
  # Cursor pages can overlap under concurrent churn (an instance that starts
  # or stops between two page fetches can shift the window and reappear on
  # more than one page). Dedupe by instance id before counting/printing —
  # otherwise a duplicate record inflates the paginated count above the
  # true number and the self-check below fires on a false positive instead
  # of the real thing it exists to catch.
  jq 'unique_by(.id)' "$merged" > "${merged}.tmp" && mv "${merged}.tmp" "$merged"
  local paginated_running
  paginated_running="$(jq '[.[] | select(.status.state=="running")] | length' "$merged")"

  # Unpaginated fetch (no per_page at all): returns everything in one shot
  # with result_info: {}. Used purely as a self-check against the walk above.
  local unpaginated="${page_dir}/unpaginated.json"
  cf_get "/accounts/${CLOUDFLARE_ACCOUNT_ID}/containers/applications/${app_id}/instances" "$unpaginated"
  local unpaginated_running
  unpaginated_running="$(jq '[.result.instances[] | select(.status.state=="running")] | length' "$unpaginated")"

  if [ "$paginated_running" != "$unpaginated_running" ]; then
    die "PAGINATION SELF-CHECK FAILED for app ${app_id}: paginated walk found ${paginated_running} running, unpaginated fetch found ${unpaginated_running} running. This is exactly the silent-undercount failure this script exists to catch — refusing to print a count."
  fi

  echo "$merged"
}

# ── Walk every application, collect running + inactive ────────────────────────
total_running=0
total_inactive=0
older_than_matches=0
now_epoch="$(date -u +%s)"

# The instance name is NOT a teardown handle. Measured 2026-08-23: live instance
# names are bare UUIDs (e.g. fab330dc-6cf9-…), while every handle the fabric knows
# is of the form cf-runner-<8hex> — none of the 28 `sbox:` or 2 `rhandle:` keys in
# RUNNER_JOB_PATS matches any running instance name. Calling POST /v1/teardown with
# one of these resolves `idFromName()` to a fresh, unrelated Durable Object stub,
# destroys nothing, and returns 204 — a silent no-op that looks like success. The
# earlier label here said "teardown handle" and would have walked an operator
# straight into that during an incident.
if [ "$EMIT_JSON" -eq 0 ]; then
  printf '%-42s %-8s %-12s %-40s\n' "APPLICATION" "LOCATION" "AGE" "INSTANCE ID (not a teardown handle)"
fi

# --json accumulator: one JSON array of every RUNNING instance across every app.
json_all="${WORKDIR}/running.json"
echo "[]" > "$json_all"

for i in "${!APP_IDS[@]}"; do
  app_id="${APP_IDS[$i]}"
  app_name="${APP_NAMES[$i]}"

  merged_file="$(fetch_app_instances "$app_id")"

  app_running="$(jq '[.[] | select(.status.state=="running")] | length' "$merged_file")"
  app_inactive="$(jq '[.[] | select(.status.state=="inactive")] | length' "$merged_file")"
  total_running=$((total_running + app_running))
  total_inactive=$((total_inactive + app_inactive))

  if [ "$EMIT_JSON" -eq 1 ]; then
    jq -s --arg app "$app_name" \
      '.[0] + [.[1][]
        | select(.status.state=="running")
        | {app: $app,
           id: .id,
           name: .name,
           location: (.location.name // null),
           started_at: (.started_at // null),
           created_at: (.created_at // null),
           image: (.image // .configuration.image // null)}]' \
      "$json_all" "$merged_file" > "${json_all}.tmp" && mv "${json_all}.tmp" "$json_all"
    continue
  fi

  while IFS=$'\t' read -r loc started name; do
    [ -n "$name" ] || continue
    age_str="unknown"
    if [ -n "$started" ] && [ "$started" != "null" ]; then
      started_epoch="$(date -u -j -f '%Y-%m-%dT%H:%M:%S' "${started%%.*}" +%s 2>/dev/null || \
                        date -u -d "$started" +%s 2>/dev/null || echo "")"
      if [ -n "$started_epoch" ]; then
        age_sec=$((now_epoch - started_epoch))
        age_hours=$((age_sec / 3600))
        age_str="${age_hours}h$(( (age_sec % 3600) / 60 ))m"
        if [ -n "$OLDER_THAN_HOURS" ] && [ "$age_hours" -gt "$OLDER_THAN_HOURS" ]; then
          older_than_matches=$((older_than_matches + 1))
          age_str="${age_str} *"
        fi
      fi
    fi
    printf '%-42s %-8s %-12s %-40s\n' "$app_name" "${loc:-?}" "$age_str" "$name"
  done < <(jq -r '.[] | select(.status.state=="running") | [(.location.name // "?"), (.started_at // ""), .name] | @tsv' "$merged_file")
done

if [ "$EMIT_JSON" -eq 1 ]; then
  cat "$json_all"
  exit 0
fi

echo
echo "TOTAL running:  ${total_running}"
echo "TOTAL inactive (tombstones, NOT running): ${total_inactive}"

if [ -n "$OLDER_THAN_HOURS" ]; then
  echo "Older than ${OLDER_THAN_HOURS}h: ${older_than_matches} (marked with * above)"
  if [ "$older_than_matches" -gt 0 ]; then
    exit 1
  fi
fi

exit 0
