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
#
# ── Required env ─────────────────────────────────────────────────────────────
#
#   CLOUDFLARE_ACCOUNT_ID
#   CLOUDFLARE_CONTAINERS_API_TOKEN
#
# Get both from corelink-server/.env.local:
#   cd corelink-server && set -a && . ./.env.local && set +a
#
# Read-only. Only ever issues GET requests.
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

CF_API_BASE="https://api.cloudflare.com/client/v4"

die() { echo "ERROR: $*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || die "curl not found."
command -v jq   >/dev/null 2>&1 || die "jq not found (required to parse the CF API response)."

: "${CLOUDFLARE_ACCOUNT_ID:?CLOUDFLARE_ACCOUNT_ID must be set}"
: "${CLOUDFLARE_CONTAINERS_API_TOKEN:?CLOUDFLARE_CONTAINERS_API_TOKEN must be set}"

OLDER_THAN_HOURS=""
ONLY_APP=""

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
    -h|--help)
      sed -n '1,60p' "$0" | grep '^#' | sed 's/^# \{0,1\}//'
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
    -H "Authorization: Bearer ${CLOUDFLARE_CONTAINERS_API_TOKEN}" \
    -o "$out" -w '%{http_code}' \
    "${CF_API_BASE}${path}")"
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

printf '%-42s %-8s %-12s %-40s\n' "APPLICATION" "LOCATION" "AGE" "INSTANCE (teardown handle)"

for i in "${!APP_IDS[@]}"; do
  app_id="${APP_IDS[$i]}"
  app_name="${APP_NAMES[$i]}"

  merged_file="$(fetch_app_instances "$app_id")"

  app_running="$(jq '[.[] | select(.status.state=="running")] | length' "$merged_file")"
  app_inactive="$(jq '[.[] | select(.status.state=="inactive")] | length' "$merged_file")"
  total_running=$((total_running + app_running))
  total_inactive=$((total_inactive + app_inactive))

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
