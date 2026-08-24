#!/usr/bin/env bash
# scripts/orphan-box-check.sh — DETECTION ONLY. Reads platform truth (the
# Cloudflare Containers API) and fails loudly when a running box is one the
# fabric's own bookkeeping cannot account for.
#
# ── Why a check that starts from the PLATFORM and not from our records ───────
#
# On 2026-08-23 three `standard-4` boxes ran for 10.5 h against a 15-minute idle
# window — ~126 vCPU-hours of nothing. PR #486 shipped a reaper for exactly that
# class of failure. But the reaper enumerates the durable `sbox:` records written
# at spawn time, and **the three boxes that leaked had no such record**. A layer
# whose entire job is to catch bookkeeping loss cannot start from the
# bookkeeping; it is structurally incapable of catching the failure it exists
# for.
#
# This check starts from the only source that cannot lose a box: the platform
# itself. Whatever is running IS running, whether or not we wrote anything down.
#
# ── What it does NOT do ──────────────────────────────────────────────────────
#
# It never stops, destroys, rolls, or otherwise mutates anything. It is a GET
# and a comparison. That is not timidity — it is forced:
#
#   ⛔ THERE IS NO PATH FROM A CF CONTAINERS INSTANCE BACK TO ITS DURABLE OBJECT.
#   An instance's `name` is a bare UUID; every handle the fabric knows is
#   `cf-runner-<8hex>` minted as `crypto.randomUUID()` per spawn attempt. Calling
#   `POST /v1/teardown` with an instance name resolves `idFromName()` to a
#   fresh, unrelated DO, destroys nothing, and returns **204** — a silent no-op
#   that reads as success. (Measured 2026-08-23; see the note in
#   scripts/container-instances.sh and docs/runbook/incident-playbook.md.)
#
# So an orphan can be SEEN but not KILLED today. Seeing it is still worth
# building: an alarm on a leak we must currently drain by hand beats a leak
# nobody notices for ten hours. The fix for the kill half — making DO names
# generative and enumerable — is specified in
# docs/adr/0010-enumerable-runner-do-names.md and is deliberately NOT
# implemented here.
#
# ── The two signals ──────────────────────────────────────────────────────────
#
# 1. OVER-AGE (always on, needs no bookkeeping at all). `JOB_PAT_TTL_S` (7200 s)
#    is the lifetime the spawn path gives a job's credential, and
#    `STALE_BOX_AGE_MS` in deploy/cloudflare/src/index.ts derives the reaper's
#    own staleness threshold from it. By the fabric's OWN assumption nothing is
#    expected to still be working past it. A running box older than that plus a
#    grace window is therefore an orphan by definition — no record needed, which
#    is precisely why this signal survives the failure mode above.
#
# 2. UNACCOUNTED (only when `--accounted` is supplied). More non-`_system`
#    instances running than the fabric has bindings for means at least that many
#    boxes exist outside our records. The count is deliberately the comparison,
#    because IDENTITY comparison is impossible: instance names and fabric
#    handles are disjoint namespaces (same measurement as above). The direction
#    is conservative — `rhandle:` bindings outlive their boxes (2 h KV TTL vs a
#    15 min idle window), so the accounted number skews HIGH and this signal
#    under-reports rather than pages falsely.
#
# ── Two things on this account are long-lived BY DESIGN and are not leaks ───
#
# 1. Instances named `_system` are Cloudflare's own platform pool. They are
#    long-lived by design and are not ours at all.
#
# 2. Whole application classes that are SERVICES, not ephemeral boxes:
#    `corelink-fabricd-fabricdcontainer` (a singleton) and every
#    `corelink-prod-*-corelinkserver-*` regional server container. Measured live
#    2026-08-24, the fabricd singleton had been up 111 h — correct, and utterly
#    unrelated to a runner lease. Only the ephemeral runner/check-host classes
#    carry a lease at all, so only they can be over-age.
#
# Both are excluded. Including either would page forever on a perfectly healthy
# account, and an alarm that is always on is an alarm nobody reads.
#
# ── Usage ────────────────────────────────────────────────────────────────────
#
#   scripts/orphan-box-check.sh [--max-lease-hours <h>] [--accounted <n>]
#                               [--instances-json <file>] [--app <app-id>]
#                               [--app-pattern <extended-regex>]
#
#   --max-lease-hours <h>   Over-age threshold. Default 3 = the fabric's 2 h
#                            maximum lease + 1 h grace (the reaper's per-minute
#                            cron has ample room inside that).
#   --accounted <n>         Number of boxes our bookkeeping accounts for. Omit
#                            to skip signal 2 (it is reported as SKIPPED, not
#                            silently passed).
#   --instances-json <file> Read the running-instance array from a file instead
#                            of the live API. This is how the logic is tested
#                            without touching prod state; the file must be what
#                            `container-instances.sh --json` emits.
#   --app <app-id>          Restrict the live FETCH to one application id.
#   --app-pattern <re>      Extended regex an application NAME must match to be
#                            considered an ephemeral box. Default:
#                            ^corelink-spawn-worker-(runnercontainer|checkhostcontainer)$
#                            Widen it only for another class that genuinely has
#                            a lease; pointing it at a service class guarantees
#                            a permanent false page.
#
# Exit 0 = no delta. Exit 1 = at least one unaccounted or over-age box (page).
# Exit 2 = the check could not be performed (bad input, API failure).
#
# ── Required env (live mode only) ────────────────────────────────────────────
#
#   CLOUDFLARE_ACCOUNT_ID
#   CLOUDFLARE_CONTAINERS_API_TOKEN   preferred, or
#   CLOUDFLARE_API_TOKEN              accepted fallback. Measured 2026-08-24:
#                                     the plain account token reads the
#                                     containers endpoints fine (200,
#                                     success: true), so a second credential
#                                     does not have to be propagated into every
#                                     repo that wants to ask. First one set
#                                     wins; the resolved NAME is printed, the
#                                     value never is. Neither set, or a token
#                                     the API rejects (401/403) ⇒ exit 2
#                                     "cannot check" — NEVER a CLEAN verdict.
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# The ONE trustworthy running-instance enumerator for this account. It already
# handles the three traps that make a hand-rolled counter wrong (the lying
# `applications[].instances` field, forever-retained `inactive` tombstones, and
# the shape-flipping pagination), and self-checks the paginated walk against the
# unpaginated fetch. Do not add a second counter here — go through it.
COUNTER="${SCRIPT_DIR}/container-instances.sh"

die() { echo "ERROR: $*" >&2; exit 2; }

command -v jq >/dev/null 2>&1 || die "jq not found."

MAX_LEASE_HOURS=3
ACCOUNTED=""
INSTANCES_JSON=""
ONLY_APP=""
# The ephemeral box classes — the only ones that carry a lease and can therefore
# be over-age. Everything else on the account (the fabricd singleton, the
# regional corelinkserver containers) is a long-lived SERVICE and is excluded;
# including them pages forever on a healthy account (measured: fabricd up 111 h).
#
# ⚠️ THIS IS AN ALLOWLIST, SO IT FAILS SILENT, NOT LOUD. A NEW ephemeral box
# class added under a different application name is INVISIBLE to this check —
# it will not page, it will simply never be looked at, and a leak in it reads as
# CLEAN. Whoever adds a spawnable container class must add it here in the same
# PR. `--app-pattern` overrides it ad hoc; `--app-pattern .` inspects everything
# (and will fire on the service classes, by design).
APP_PATTERN='^corelink-spawn-worker-(runnercontainer|checkhostcontainer)$'

while [ $# -gt 0 ]; do
  case "$1" in
    --max-lease-hours) MAX_LEASE_HOURS="${2:?--max-lease-hours requires a number}"; shift 2 ;;
    --accounted)       ACCOUNTED="${2:?--accounted requires a number}";              shift 2 ;;
    --instances-json)  INSTANCES_JSON="${2:?--instances-json requires a path}";      shift 2 ;;
    --app)             ONLY_APP="${2:?--app requires an application id}";            shift 2 ;;
    --app-pattern)     APP_PATTERN="${2:?--app-pattern requires a regex}";            shift 2 ;;
    -h|--help)         sed -n '1,110p' "$0" | grep '^#' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)                 die "unknown argument: $1" ;;
  esac
done

case "$MAX_LEASE_HOURS" in (*[!0-9]*|'') die "--max-lease-hours must be a non-negative integer" ;; esac
if [ -n "$ACCOUNTED" ]; then
  case "$ACCOUNTED" in (*[!0-9]*|'') die "--accounted must be a non-negative integer" ;; esac
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT
RAW="${WORKDIR}/running.json"

if [ -n "$INSTANCES_JSON" ]; then
  [ -f "$INSTANCES_JSON" ] || die "--instances-json: no such file: ${INSTANCES_JSON}"
  cp "$INSTANCES_JSON" "$RAW"
  echo "source: fixture ${INSTANCES_JSON}"
else
  [ -x "$COUNTER" ] || die "counter not executable: ${COUNTER}"
  # NOT the `${VAR:?msg}` idiom: under `set -e` that exits 1, which this script
  # reserves for "a leak was found". A missing account id is exit 2, cannot check.
  [ -n "${CLOUDFLARE_ACCOUNT_ID:-}" ] \
    || die "CLOUDFLARE_ACCOUNT_ID is not set. Cannot check — refusing to report a clean fleet."
  # Same precedence as the counter, so behaviour outside CI is identical:
  # CLOUDFLARE_CONTAINERS_API_TOKEN wins when set, CLOUDFLARE_API_TOKEN is the
  # accepted fallback (measured 2026-08-24: the plain account token reads the
  # containers endpoints fine). Reported by NAME; the value is never printed.
  if [ -n "${CLOUDFLARE_CONTAINERS_API_TOKEN:-}" ]; then
    token_var="CLOUDFLARE_CONTAINERS_API_TOKEN"
  elif [ -n "${CLOUDFLARE_API_TOKEN:-}" ]; then
    token_var="CLOUDFLARE_API_TOKEN"
  else
    die "no Cloudflare API token in the environment. Set CLOUDFLARE_CONTAINERS_API_TOKEN (preferred) or CLOUDFLARE_API_TOKEN. Both are accepted; the first one set wins. Cannot check — refusing to report a clean fleet."
  fi
  echo "source: Cloudflare Containers API (account ${CLOUDFLARE_ACCOUNT_ID}, auth \$${token_var} — value never printed)"
  # `die` here is exit 2 = CANNOT CHECK, never exit 0 = clean. The counter fails
  # loudly on a rejected token (401/403) and on a page the API truncated — both
  # are states where we have NOT seen the fleet, and reporting CLEAN from either
  # is the silent-success defect this whole check exists to end.
  if [ -n "$ONLY_APP" ]; then
    "$COUNTER" --json --app "$ONLY_APP" > "$RAW" \
      || die "container-instances.sh --json failed (rejected token, truncated page, or API error — see its message above). CANNOT CHECK; not reporting a clean fleet."
  else
    "$COUNTER" --json > "$RAW" \
      || die "container-instances.sh --json failed (rejected token, truncated page, or API error — see its message above). CANNOT CHECK; not reporting a clean fleet."
  fi
fi

jq -e 'type == "array"' "$RAW" >/dev/null 2>&1 \
  || die "instance list is not a JSON array (got: $(head -c 200 "$RAW"))"

# ── Drop the platform pool and every non-ephemeral application class ─────────
OURS="${WORKDIR}/ours.json"
jq --arg re "$APP_PATTERN" \
  '[.[] | select(.name != "_system") | select((.app // "") | test($re))]' "$RAW" > "$OURS"
total_count="$(jq 'length' "$RAW")"
system_count="$(jq '[.[] | select(.name == "_system")] | length' "$RAW")"
ours_count="$(jq 'length' "$OURS")"
service_count=$((total_count - system_count - ours_count))

echo "running instances: ${total_count}"
echo "  ephemeral boxes in scope (${APP_PATTERN}): ${ours_count}"
echo "  platform _system pool:                     ${system_count}  (not ours)"
echo "  long-lived service classes:                ${service_count}  (no lease — cannot be over-age)"

now_epoch="$(date -u +%s)"
max_age_sec=$((MAX_LEASE_HOURS * 3600))
overage=0

# started_at is authoritative for "how long has this box been up"; created_at is
# the documented fallback for a record that has not reported a start.
while IFS=$'\t' read -r app name loc started; do
  [ -n "$name" ] || continue
  [ -n "$started" ] && [ "$started" != "null" ] || continue
  started_epoch="$(date -u -j -f '%Y-%m-%dT%H:%M:%S' "${started%%.*}" +%s 2>/dev/null || \
                    date -u -d "$started" +%s 2>/dev/null || echo "")"
  [ -n "$started_epoch" ] || continue
  age_sec=$((now_epoch - started_epoch))
  if [ "$age_sec" -gt "$max_age_sec" ]; then
    overage=$((overage + 1))
    echo "::error::OVER-AGE BOX: app=${app} instance=${name} location=${loc} up=$((age_sec / 3600))h$(( (age_sec % 3600) / 60 ))m > ${MAX_LEASE_HOURS}h max lease — nothing is expected to still be working past the fabric's own JOB_PAT_TTL_S."
  fi
done < <(jq -r '.[] | [(.app // "?"), .name, (.location // "?"), (.started_at // .created_at // "")] | @tsv' "$OURS")

unaccounted=0
if [ -n "$ACCOUNTED" ]; then
  if [ "$ours_count" -gt "$ACCOUNTED" ]; then
    unaccounted=$((ours_count - ACCOUNTED))
    echo "::error::UNACCOUNTED BOXES: ${ours_count} running but the fabric accounts for only ${ACCOUNTED} — ${unaccounted} box(es) exist outside our bookkeeping. This is the failure class the sbox:-driven reaper cannot see."
  else
    echo "accounted check: ${ours_count} running <= ${ACCOUNTED} accounted — OK"
  fi
else
  echo "accounted check: SKIPPED (no --accounted supplied) — over-age signal only"
fi

echo
if [ "$overage" -gt 0 ] || [ "$unaccounted" -gt 0 ]; then
  echo "VERDICT: LEAK — over-age=${overage} unaccounted=${unaccounted}"
  echo "⛔ Do NOT try to teardown by instance name: it is a silent 204 no-op." >&2
  echo "   The only lever that removes a running box today is an image roll," >&2
  echo "   which kills in-flight jobs on the OTHER boxes. See" >&2
  echo "   docs/adr/0010-enumerable-runner-do-names.md for the real fix." >&2
  exit 1
fi

echo "VERDICT: CLEAN — no over-age box, no unaccounted box."
exit 0
