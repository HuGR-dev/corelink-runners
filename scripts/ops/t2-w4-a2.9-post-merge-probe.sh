#!/usr/bin/env bash
# T2-W4 / A2.9 finite post-merge verifier for exactly three live contracts.
# Default is an inert plan. Live mode never deploys, merges, dispatches Actions,
# calls Hugit/Githugr, or edits the delivery ledger.
#
# Safe admission legs:
#   99d5e39... shared admission freeze on the direct spawn route
#   8571f3c... shared admission freeze on Fabricd lease acquire
#
# 70a880e (standalone Fabricd JobClose) is exercised with one real isolated
# 60-second canary lease. The canary closes through the public lease API and
# verifies the signed v1/v2 result binding and released state.

set -Eeuo pipefail
IFS=$'\n\t'
umask 077

ROOT_DEFAULT="$(cd -- "$(dirname -- "$0")/../.." && pwd -P)"
readonly WORKER_DEFAULT="corelink-spawn-worker"
readonly WORKER_URL_DEFAULT="https://corelink-spawn-worker.gmhelmold.workers.dev"
readonly FABRICD_DEFAULT="corelink-fabricd"
readonly FABRICD_URL_DEFAULT="https://corelink-fabricd.gmhelmold.workers.dev"
readonly FREEZE_FIX="99d5e39e66a69d44226dc9eed0faf0f7b384156f"
readonly FABRICD_FREEZE_FIX="8571f3cf38f11cca9b1f0752bf028e3dd982bb52"
readonly FABRIC_FIX="70a880e1ba7902546931f2d615859b3a632912b5"
readonly FABRICD_IMAGE_BUILD="543fa5f253580056eb5f526d3f6d8694839e9c7c"
readonly FABRICD_HISTORICAL_IMAGE_BUILD="01560b697c87b92bf1572ceec175d5350034aebb"
readonly MAX_SECONDS=900
readonly REQUIRED_COUNTERS=(webhook_spawn_claimed jit_minted runner_spawned)

MODE=plan ROOT="$ROOT_DEFAULT" WORKER="$WORKER_DEFAULT" BASE_URL="$WORKER_URL_DEFAULT"
FABRICD="$FABRICD_DEFAULT" FABRICD_URL="$FABRICD_URL_DEFAULT"
MERGE_SHA='' MERGE_TS='' EXPECTED_VERSION='' FABRICD_VERSION='' FABRICD_DIGEST='' EVIDENCE_FILE=''
OOB_DIR="${CORELINK_A29_OOB_DIR:-$HOME/.corelink/a2.9-post-merge}"
SPAWN_TOKEN_FILE="$OOB_DIR/cf-spawn-token" FLEET_KEY_FILE="$OOB_DIR/fleet-busy-read-key" METRICS_KEY_FILE="$OOB_DIR/metrics-observability-key-a29" FABRIC_OBSERVABILITY_KEY_FILE="$OOB_DIR/fabric-observability-key-rotated" CANARY_PAT_FILE="$OOB_DIR/corelink-canary-tenant-pat" WRANGLER_BIN=''
FABRICD_APP_ID='' CANARY_IMAGE='alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc' VERIFY_BIN="$ROOT_DEFAULT/scripts/rotation/corelink-b2/bin/verify-close-attestation.mjs"
POLL_INTERVAL=2 POLL_LIMIT=5 TAIL_SECONDS=8 FABRICD_DEPLOY_TIMEOUT=60 THAW_ACK=''
RUN_ID=''
TMP_DIR='' START_EPOCH=0 DEADLINE_EPOCH=0 SELFTEST=0 OBSERVED_VERSION='' OBSERVED_DEPLOYMENT_ID='' OBSERVED_DEPLOYED_AT='' OBSERVED_FABRICD_VERSION='' OBSERVED_FABRICD_DEPLOYMENT_ID='' OBSERVED_FABRICD_AT='' OBSERVED_FABRICD_DIGEST='' OBSERVED_FABRICD_APP_ID='' CANARY_LEASE='' CANARY_RELEASED=0 THAW_INTENT=0 FINAL_FABRICD_FROZEN=0 SPAWN_REMAINED_FROZEN=false THAW_DEPLOYMENT='null' REFREEZE_DEPLOYMENT='null' EMERGENCY_REFREEZE_DEPLOYMENT='null' SPAWN_FINAL_FREEZE_FACTS='null' FABRICD_FREEZE_PROVENANCE='null'
ROW_METRICS_BEFORE='null' ROW_METRICS_AFTER='null'

die() { printf 'A2.9 REFUSED: %s\n' "$*" >&2; exit 2; }
usage() {
  cat <<'USAGE'
Finite T2-W4/A2.9 post-merge verifier (default: inert plan)

  t2-w4-a2.9-post-merge-probe.sh [--mode plan|mock|live]
    --merge-sha SHA --merge-timestamp RFC3339 --expected-worker-version ID
    --fabricd-version ID --fabricd-digest sha256:<64hex> --fabricd-app-id ID
    [--evidence-file PATH] [--repo-root PATH] [--worker-url URL]
    [--spawn-token-file FILE] [--fleet-key-file FILE]
    [--metrics-key-file FILE] [--fabric-observability-key-file FILE]
    [--canary-pat-file FILE] [--wrangler-bin FILE]
    --fabricd-thaw-ack a29-controlled-fabricd-thaw

plan: no file reads, network, provider calls, deploy, merge, or Actions.
mock: deterministic local evidence; no network or provider calls.
live: direct frozen Spawn /v1/spawn proof using the independent spawn token;
      direct frozen Fabricd /v1/leases proof using the tenant PAT and separate
      Fabricd observability key; one isolated 60-second
      Fabricd canary lease, exact signed CloseResponse and v1/v2 verification,
      bounded metrics/fleet polls, bounded Wrangler tail, final fleet 0/0/refreeze.
      The live canary requires the explicit thaw acknowledgement; it thaws only
      Fabricd, then refreezes it in the EXIT path on every result.

The live probe is bounded to 900 seconds. Evidence has statuses, booleans,
counters, fixed SHAs, timestamps, deployment versions/digest, and latency only.
Secret values and response bodies are never emitted.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) [[ $# -ge 2 ]] || die '--mode requires plan, mock, or live'; MODE=$2; shift 2 ;;
    --merge-sha) [[ $# -ge 2 ]] || die '--merge-sha requires a full SHA'; MERGE_SHA=$2; shift 2 ;;
    --merge-timestamp) [[ $# -ge 2 ]] || die '--merge-timestamp requires RFC3339'; MERGE_TS=$2; shift 2 ;;
    --expected-worker-version) [[ $# -ge 2 ]] || die '--expected-worker-version requires a version id'; EXPECTED_VERSION=$2; shift 2 ;;
    --evidence-file) [[ $# -ge 2 ]] || die '--evidence-file requires a path'; EVIDENCE_FILE=$2; shift 2 ;;
    --repo-root) [[ $# -ge 2 ]] || die '--repo-root requires a path'; ROOT=$2; shift 2 ;;
    --worker-url) [[ $# -ge 2 ]] || die '--worker-url requires a URL'; BASE_URL=$2; shift 2 ;;
    --worker-name) [[ $# -ge 2 ]] || die '--worker-name requires a name'; WORKER=$2; shift 2 ;;
    --fabricd-url) [[ $# -ge 2 ]] || die '--fabricd-url requires a URL'; FABRICD_URL=$2; shift 2 ;;
    --fabricd-name) [[ $# -ge 2 ]] || die '--fabricd-name requires a name'; FABRICD=$2; shift 2 ;;
    --fabricd-version) [[ $# -ge 2 ]] || die '--fabricd-version requires a version id'; FABRICD_VERSION=$2; shift 2 ;;
    --fabricd-digest) [[ $# -ge 2 ]] || die '--fabricd-digest requires sha256:<64hex>'; FABRICD_DIGEST=$2; shift 2 ;;
    --fabricd-app-id) [[ $# -ge 2 ]] || die '--fabricd-app-id requires an application id'; FABRICD_APP_ID=$2; shift 2 ;;
    --canary-image) [[ $# -ge 2 ]] || die '--canary-image requires an immutable image'; CANARY_IMAGE=$2; shift 2 ;;
    --canary-pat-file) [[ $# -ge 2 ]] || die '--canary-pat-file requires a path'; CANARY_PAT_FILE=$2; shift 2 ;;
    --verify-bin) [[ $# -ge 2 ]] || die '--verify-bin requires a verifier path'; VERIFY_BIN=$2; shift 2 ;;
    --spawn-token-file) [[ $# -ge 2 ]] || die '--spawn-token-file requires a path'; SPAWN_TOKEN_FILE=$2; shift 2 ;;
    --fleet-key-file) [[ $# -ge 2 ]] || die '--fleet-key-file requires a path'; FLEET_KEY_FILE=$2; shift 2 ;;
    --metrics-key-file) [[ $# -ge 2 ]] || die '--metrics-key-file requires a path'; METRICS_KEY_FILE=$2; shift 2 ;;
    --fabric-observability-key-file) [[ $# -ge 2 ]] || die '--fabric-observability-key-file requires a path'; FABRIC_OBSERVABILITY_KEY_FILE=$2; shift 2 ;;
    --fabricd-thaw-ack) [[ $# -ge 2 ]] || die '--fabricd-thaw-ack requires its exact acknowledgement'; THAW_ACK=$2; shift 2 ;;
    --wrangler-bin) [[ $# -ge 2 ]] || die '--wrangler-bin requires a path'; WRANGLER_BIN=$2; shift 2 ;;
    --selftest) SELFTEST=1; shift ;;
    --poll-interval-seconds) [[ $# -ge 2 ]] || die '--poll-interval-seconds requires an integer'; POLL_INTERVAL=$2; shift 2 ;;
    --poll-limit) [[ $# -ge 2 ]] || die '--poll-limit requires an integer'; POLL_LIMIT=$2; shift 2 ;;
    --tail-seconds) [[ $# -ge 2 ]] || die '--tail-seconds requires an integer'; TAIL_SECONDS=$2; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

if [[ "$SELFTEST" == 0 ]]; then
  [[ "$MODE" == plan || "$MODE" == mock || "$MODE" == live ]] || die 'mode must be plan, mock, or live'
  [[ "$MERGE_SHA" =~ ^[0-9a-f]{40}$ ]] || die '--merge-sha must be a 40-character lowercase SHA-1'
  [[ -n "$MERGE_TS" ]] || die '--merge-timestamp is required'
  [[ -n "$EXPECTED_VERSION" ]] || die '--expected-worker-version is required'
  [[ -n "$FABRICD_VERSION" ]] || die '--fabricd-version is required'
  [[ "$FABRICD_DIGEST" =~ ^sha256:[0-9a-f]{64}$ ]] || die '--fabricd-digest must be sha256:<64hex>'
  [[ "$MODE" != live || "$FABRICD_APP_ID" =~ ^[0-9a-fA-F-]{8,}$ ]] || die '--fabricd-app-id is malformed'
  [[ "$POLL_INTERVAL" =~ ^[0-9]+$ && "$POLL_LIMIT" =~ ^[1-9][0-9]*$ && "$TAIL_SECONDS" =~ ^[1-9][0-9]*$ ]] || die 'poll/tail bounds must be positive integers'
  [[ "$POLL_INTERVAL" -le 30 && "$POLL_LIMIT" -le 30 && "$TAIL_SECONDS" -le 60 ]] || die 'poll/tail bounds exceed finite limits'
fi

parse_epoch() {
  python3 - "$1" <<'PY'
import datetime as dt, sys
value = sys.argv[1]
try:
    parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    if parsed.tzinfo is None:
        raise ValueError('timestamp must include timezone')
    print(int(parsed.timestamp()))
except Exception as exc:
    raise SystemExit(f'invalid RFC3339 timestamp: {exc}')
PY
}
MERGE_EPOCH=0
if [[ "$SELFTEST" == 0 ]]; then
  MERGE_EPOCH="$(parse_epoch "$MERGE_TS")" || die 'merge timestamp is not RFC3339 with timezone'
fi

now_s() { date +%s; }
require_python3() {
  command -v python3 >/dev/null 2>&1 || return 1
  python3 -c 'import os; raise SystemExit(0 if hasattr(os, "setsid") else 1)' >/dev/null 2>&1
}
check_deadline() {
  local now; now="$(now_s)"
  if [[ "$DEADLINE_EPOCH" -gt 0 ]]; then
    (( now <= DEADLINE_EPOCH )) || die 'absolute merge-relative 900-second deadline exceeded'
  else
    (( now <= START_EPOCH + MAX_SECONDS )) || die 'pre-merge-authority 900-second deadline exceeded'
  fi
}
remaining_seconds() {
  local end=$DEADLINE_EPOCH now; now="$(now_s)"
  [[ "$end" -gt 0 ]] || end=$((START_EPOCH + MAX_SECONDS))
  printf '%s' "$((end - now))"
}

run_bounded_capture() {
  local output=$1 error=$2; shift 2
  check_deadline
  local remaining pid watchdog rc
  remaining="$(remaining_seconds)"; (( remaining > 0 )) || return 124
  : > "$output"; : > "$error"; chmod 600 "$output" "$error"
  launch_group "$output" "$error" "$@" & pid=$!
  ( sleep "$remaining"; kill -TERM -- "-$pid" 2>/dev/null || true; sleep 1; kill -KILL -- "-$pid" 2>/dev/null || true ) & watchdog=$!
  set +e; wait "$pid"; rc=$?; set -e
  kill "$watchdog" 2>/dev/null || true; wait "$watchdog" 2>/dev/null || true
  scrub_file "$error"
  return "$rc"
}

launch_group() {
  local output=$1 error=$2; shift 2
  python3 -c '
import os
import sys

out_path, err_path, *argv = sys.argv[1:]
if not argv:
    raise SystemExit("missing child command")
os.setsid()
out_fd = os.open(out_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
err_fd = os.open(err_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
os.dup2(out_fd, 1)
os.dup2(err_fd, 2)
os.close(out_fd)
os.close(err_fd)
os.execvp(argv[0], argv)
' "$output" "$error" "$@"
}

file_mode() { stat -f '%OLp' "$1" 2>/dev/null || stat -c '%a' "$1"; }
file_uid() { stat -f '%u' "$1" 2>/dev/null || stat -c '%u' "$1"; }
safe_secret_file() {
  local file=$1
  [[ -f "$file" && ! -L "$file" ]] || return 1
  [[ "$(file_mode "$file")" == 600 && "$(file_uid "$file")" == "$(id -u)" ]] || return 1
  [[ -s "$file" ]] || return 1
  [[ "$(tr -cd '\r' < "$file" | wc -c | tr -d ' ')" == 0 ]] || return 1
  [[ "$(tr -cd '\n' < "$file" | wc -c | tr -d ' ')" -le 1 ]] || return 1
}

distinct_secret_surfaces() {
  local left=$1 right=$2
  [[ "$left" != "$right" ]] || return 1
  ! cmp -s -- "$left" "$right"
}

scrub_file() {
  local source=$1 target="${1}.scrubbed"
  sed -E \
    -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1<REDACTED>/Ig' \
    -e 's/(X-Corelink-Internal-Auth:[[:space:]]*)[^[:space:]]+/\1<REDACTED>/Ig' \
    -e 's/(sha256=)[0-9a-f]{64}/\1<REDACTED>/Ig' \
    -e 's/(corelink_|pat_|cas[-_]?pat[-_]|sk-)[A-Za-z0-9._~+\/-]{12,}/\1<REDACTED>/Ig' \
    -e 's/-----BEGIN[^-]*-----/<REDACTED>/g' "$source" > "$target"
  chmod 600 "$target"; mv -f -- "$target" "$source"
}

json_rows='[]'
record_row() {
  local fix=$1 cell=$2 pass=$3 status=$4 detail=$5 elapsed=$6
  local deployed_version="$OBSERVED_VERSION" deployed_id="$OBSERVED_DEPLOYMENT_ID" deployed_at="$OBSERVED_DEPLOYED_AT" deployed_digest=''
  if [[ "$fix" == "$FABRIC_FIX" || "$fix" == "$FABRICD_FREEZE_FIX" ]]; then
    deployed_version="$OBSERVED_FABRICD_VERSION"; deployed_id="$OBSERVED_FABRICD_DEPLOYMENT_ID"; deployed_at="$OBSERVED_FABRICD_AT"; deployed_digest="$OBSERVED_FABRICD_DIGEST"
  fi
  json_rows="$(jq --arg fix "$fix" --arg cell "$cell" --argjson pass "$pass" \
    --arg status "$status" --arg detail "$detail" --arg version "$deployed_version" --arg deployment_id "$deployed_id" --arg deployed_at "$deployed_at" --arg digest "$deployed_digest" --argjson elapsed "$elapsed" \
    --argjson metrics_before "$ROW_METRICS_BEFORE" --argjson metrics_after "$ROW_METRICS_AFTER" \
    '. + [{fix:$fix,cell:$cell,pass:$pass,status:$status,detail:$detail,deployed_version:$version,deployed_id:$deployment_id,deployed_at:$deployed_at,deployed_digest:(if $digest == "" then null else $digest end),metrics_before:$metrics_before,metrics_after:$metrics_after,elapsed_s:$elapsed}]' <<<"$json_rows")"
  ROW_METRICS_BEFORE='null'; ROW_METRICS_AFTER='null'
}

all_rows_pass() {
  jq -e --arg a "$FREEZE_FIX" --arg b "$FABRICD_FREEZE_FIX" --arg c "$FABRIC_FIX" \
    'length == 3 and all(.[]; .pass == true) and ([.[].fix] | sort) == ([$a,$b,$c] | sort)' <<<"$json_rows" >/dev/null
}

metric_values() {
  local file=$1 names
  names="$(printf '%s\n' "${REQUIRED_COUNTERS[@]}" | jq -Rsc 'split("\n") | map(select(length > 0))')"
  jq -ce --argjson names "$names" '
    . as $root |
    ($root.counters | type == "object") and
    ([$names[] as $n | select(($root.counters | has($n)) | not)] | length == 0) and
    ([$names[] as $n | $root.counters[$n] | type == "number" and floor == . and . >= 0] | length == ($names|length))
    | if . then {webhook_spawn_claimed:$root.counters.webhook_spawn_claimed, jit_minted:$root.counters.jit_minted, runner_spawned:$root.counters.runner_spawned} else error("required metric counter missing or non-integer") end
  ' "$file"
}

metric_facts() {
  local before=$1 after=$2
  jq -n --argjson b "$(metric_values "$before")" --argjson a "$(metric_values "$after")" '{before:$b,after:$a}'
}

validate_metrics_file() { metric_values "$1" >/dev/null; }

emit_evidence() {
  local requested_status=$1 finish_epoch=$2 note=${3:-}
  local elapsed=$((finish_epoch - START_EPOCH)) effective_status promotion live_qualification
  (( elapsed <= MAX_SECONDS )) || die "live evidence exceeded ${MAX_SECONDS}s bound"
  all_rows_pass || die 'evidence aggregate gate failed: all three selected rows must pass'
  [[ -n "$EVIDENCE_FILE" ]] || return 0
  local destination=$EVIDENCE_FILE
  [[ "$destination" = /* ]] || destination="$ROOT/$destination"
  mkdir -p -- "$(dirname -- "$destination")"
  if [[ "$MODE" == live && "$requested_status" == PASS ]]; then
    effective_status=PASS; promotion=eligible; live_qualification=true
  else
    effective_status=NON_PROMOTABLE; promotion=never; live_qualification=false
  fi
  jq -n --arg schema 'a2.9/post-merge/v1' --arg artifact 't2-w4-a2.9-post-merge' \
    --arg mode "$MODE" --arg status "$effective_status" --arg promotion "$promotion" --arg sha "$MERGE_SHA" \
    --arg merge_ts "$MERGE_TS" --arg worker "$WORKER" --arg version "$EXPECTED_VERSION" --arg observed "$OBSERVED_VERSION" --arg deployment_id "$OBSERVED_DEPLOYMENT_ID" --arg observed_at "$OBSERVED_DEPLOYED_AT" \
    --arg fabricd "$FABRICD" --arg fabricd_version "$FABRICD_VERSION" --arg fabricd_observed "$OBSERVED_FABRICD_VERSION" --arg fabricd_deployment_id "$OBSERVED_FABRICD_DEPLOYMENT_ID" --arg fabricd_app_id "$OBSERVED_FABRICD_APP_ID" --arg fabricd_expected_digest "$FABRICD_DIGEST" --arg fabricd_observed_digest "$OBSERVED_FABRICD_DIGEST" --arg fabricd_at "$OBSERVED_FABRICD_AT" \
    --arg note "$note" --argjson live_qualification "$live_qualification" --argjson started "$START_EPOCH" --argjson elapsed "$elapsed" \
    --argjson rows "$json_rows" --arg fabric_sha "$FABRIC_FIX" --argjson thaw "$THAW_DEPLOYMENT" --argjson refreeze "$REFREEZE_DEPLOYMENT" --argjson emergency_refreeze "$EMERGENCY_REFREEZE_DEPLOYMENT" --argjson spawn_final "$SPAWN_FINAL_FREEZE_FACTS" --argjson provenance "$FABRICD_FREEZE_PROVENANCE" --argjson spawn_frozen "$SPAWN_REMAINED_FROZEN" \
    '{schema_version:$schema,artifact_id:$artifact,mode:$mode,status:$status,promotion:$promotion,live_qualification:$live_qualification,
      source:{repository:"corelink-runners",pull_request:563,merge_sha:$sha,merge_timestamp:$merge_ts},
      worker:{name:$worker,expected_deployed_version:$version,observed_deployed_version:$observed,observed_deployment_id:$deployment_id,observed_deployed_at:$observed_at},
      fabricd:{name:$fabricd,expected_deployed_version:$fabricd_version,observed_deployed_version:$fabricd_observed,observed_deployment_id:$fabricd_deployment_id,observed_app_id:$fabricd_app_id,expected_digest:$fabricd_expected_digest,observed_digest:$fabricd_observed_digest,observed_deployed_at:$fabricd_at},
      bounds:{max_elapsed_s:900,elapsed_s:$elapsed,started_epoch:$started},rows:$rows,
      fabric_fix:([$rows[] | select(.fix == $fabric_sha)] | if length == 1 then {sha:$fabric_sha,status:(if .[0].pass then "PASS" else "FAIL" end),close_response:(if .[0].pass then "signed_v1_v2_verified" else "not_verified" end)} else {sha:$fabric_sha,status:"FAIL",close_response:"not_verified"} end),
      operational_transition:{fabricd_thaw:$thaw,fabricd_refreeze:$refreeze,emergency_fabricd_refreeze:$emergency_refreeze,spawn_final_freeze_facts:$spawn_final,spawn_remained_frozen:$spawn_frozen,final_fabricd_frozen:(($refreeze != null) or ($emergency_refreeze != null))},
      fabricd_freeze_provenance:$provenance,
      secrets:"excluded",response_bodies:"excluded",notes:$note,actions:"not invoked",deploy:"controlled_fabricd_thaw_refreeze_only",merge:"not invoked"}' \
    > "$destination.tmp"
  chmod 644 "$destination.tmp"; mv -f -- "$destination.tmp" "$destination"
  if grep -Eqi '(Bearer[[:space:]]+[A-Za-z0-9._~-]{12,}|corelink_[A-Za-z0-9._~-]{12,}|pat_[A-Za-z0-9._~-]{12,}|-----BEGIN|sha256=[0-9a-f]{64})' "$destination"; then
    rm -f -- "$destination"; die 'evidence scrub check failed'
  fi
}

plan() {
  printf '%s\n' 'PLAN ONLY: no network, secret-file reads, provider calls, deploy, merge, or GitHub Actions.'
  printf 'merge_sha=%s\nmerge_timestamp=%s\nexpected_worker_version=%s\nfabricd_version=%s\nfabricd_digest=%s\n' "$MERGE_SHA" "$MERGE_TS" "$EXPECTED_VERSION" "$FABRICD_VERSION" "$FABRICD_DIGEST"
  printf 'selected=spawn-admission-freeze:%s fabricd-admission-freeze:%s standalone-jobclose:%s\n' "$FREEZE_FIX" "$FABRICD_FREEZE_FIX" "$FABRIC_FIX"
  printf '%s\n' 'sequence=verify authoritative PR/deployments -> fleet 0/0 -> authenticated frozen Spawn direct spawn -> authenticated frozen Fabricd lease acquire -> isolated 60-second Fabricd acquire/close canary only after the proven frozen checks -> bounded tail/metrics -> final fleet 0/0/refreeze'
}

require_live_inputs() {
  [[ -d "$ROOT" ]] || die 'repository root is not a directory'
  git -C "$ROOT" rev-parse --verify HEAD >/dev/null 2>&1 || die 'repository root is not a Git worktree'
  [[ "$(git -C "$ROOT" rev-parse HEAD)" == "$MERGE_SHA" ]] || die 'HEAD does not exactly equal --merge-sha'
  [[ -z "$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)" ]] || die 'repository worktree is not clean'
  [[ -n "$SPAWN_TOKEN_FILE" && -n "$FLEET_KEY_FILE" && -n "$METRICS_KEY_FILE" && -n "$FABRIC_OBSERVABILITY_KEY_FILE" ]] || die 'live mode requires spawn token, fleet key, Spawn metrics key, and Fabricd observability key paths'
  safe_secret_file "$SPAWN_TOKEN_FILE" || die 'spawn token must be an owner-only regular 0600 single-line file'
  safe_secret_file "$FLEET_KEY_FILE" || die 'fleet key must be an owner-only regular 0600 single-line file'
  safe_secret_file "$METRICS_KEY_FILE" || die 'metrics key must be an owner-only regular 0600 single-line file'
  safe_secret_file "$FABRIC_OBSERVABILITY_KEY_FILE" || die 'Fabricd observability key must be an owner-only regular 0600 single-line file'
  distinct_secret_surfaces "$METRICS_KEY_FILE" "$FABRIC_OBSERVABILITY_KEY_FILE" || die 'Spawn metrics and Fabricd observability keys must be distinct secret surfaces'
  canary_pat_valid "$CANARY_PAT_FILE" || die 'canary PAT must be an owner-only 0600 canonical tenant PAT file'
  command -v curl >/dev/null 2>&1 || die 'curl is unavailable'
  command -v jq >/dev/null 2>&1 || die 'jq is unavailable'
  require_python3 || die 'live mode requires python3 with os.setsid for process-group isolation'
  [[ -x "$WRANGLER_BIN" ]] || die 'live mode requires an executable local Wrangler binary'
  [[ -x "$VERIFY_BIN" ]] || die 'v1/v2 close verifier is not executable'
  [[ -n "$FABRICD_APP_ID" ]] || die 'live mode requires --fabricd-app-id for provider digest proof'
  [[ "$THAW_ACK" == 'a29-controlled-fabricd-thaw' ]] || die 'live canary requires --fabricd-thaw-ack a29-controlled-fabricd-thaw'
  [[ "$BASE_URL" =~ ^https://[^[:space:]/]+$ ]] || die 'worker URL must be an HTTPS origin'
}

PROVIDER_SEQ=0
run_provider() {
  local label=$1; shift
  PROVIDER_SEQ=$((PROVIDER_SEQ + 1))
  local output="$TMP_DIR/provider-${PROVIDER_SEQ}-${label}.out" error="$TMP_DIR/provider-${PROVIDER_SEQ}-${label}.err"
  run_bounded_capture "$output" "$error" "$@" || return 1
  cat "$output"
}
run_wrangler() { run_provider worker-deployments "$WRANGLER_BIN" deployments list --name "$WORKER" --json; }
run_wrangler_cmd() { run_provider wrangler-command "$WRANGLER_BIN" "$@"; }

validate_pr_record() {
  local file=$1
  jq -e --arg sha "$MERGE_SHA" --arg timestamp "$MERGE_TS" \
    '.merged == true and .merge_commit_sha == $sha and .merged_at == $timestamp' "$file" >/dev/null
}

assert_authoritative_merge() {
  command -v gh >/dev/null 2>&1 || die 'live mode requires gh for authoritative PR 563 verification'
  run_bounded_capture "$TMP_DIR/pr-563.json" "$TMP_DIR/pr-563.err" gh api "repos/HuGR-Labs/corelink-runners/pulls/563" --header 'Accept: application/vnd.github+json' || die 'authoritative PR 563 fetch failed'
  validate_pr_record "$TMP_DIR/pr-563.json" || die 'PR 563 is not merged at the exact requested merge SHA/timestamp'
  MERGE_EPOCH="$(parse_epoch "$MERGE_TS")" || die 'authoritative merged_at is not RFC3339 with timezone'
  DEADLINE_EPOCH=$((MERGE_EPOCH + MAX_SECONDS))
  check_deadline
  [[ "$(git -C "$ROOT" rev-parse HEAD)" == "$MERGE_SHA" ]] || die 'HEAD does not exactly equal authoritative PR 563 merge SHA'
  local fix
  for fix in "$FREEZE_FIX" "$FABRICD_FREEZE_FIX" "$FABRIC_FIX"; do
    git -C "$ROOT" merge-base --is-ancestor "$fix" "$MERGE_SHA" || die "selected fix $fix is not an ancestor of authoritative merge SHA"
  done
}

assert_deployed_version() {
  local capture="$TMP_DIR/deployments.json" deployment version deployment_id created
  run_wrangler > "$capture" || return 1
  deployment="$(jq -c 'def rows: if type=="array" then . elif .items? then .items elif .result? then .result elif .deployments? then .deployments else [] end; [rows[] | {deployment_id:(.id // .deployment_id // ""),version:(.version_id // .version // .versions[0].version_id // ""),created:(.created_on // .created_at // .deployment_triggered_at // "")} ] | map(select(.version != "" and .deployment_id != "")) | sort_by(.created) | last // {}' "$capture")" || return 1
  version="$(jq -r '.version' <<<"$deployment")"; deployment_id="$(jq -r '.deployment_id' <<<"$deployment")"; created="$(jq -r '.created' <<<"$deployment")"
  [[ "$version" == "$EXPECTED_VERSION" && -n "$deployment_id" && -n "$created" ]] || return 1
  local deployed_epoch; deployed_epoch="$(parse_epoch "$created")" || return 1
  (( deployed_epoch >= MERGE_EPOCH && deployed_epoch <= MERGE_EPOCH + MAX_SECONDS )) || return 1
  check_deadline
  OBSERVED_VERSION="$version"; OBSERVED_DEPLOYMENT_ID="$deployment_id"; OBSERVED_DEPLOYED_AT="$created"
  printf '%s\t%s\t%s\n' "$deployment_id" "$version" "$created" > "$TMP_DIR/deployed-version"
}

assert_fabricd_deployment() {
  local capture="$TMP_DIR/fabricd-deployments.json" deployment version deployment_id created deployed_epoch info config
  run_wrangler_cmd deployments list --name "$FABRICD" --json > "$capture" || return 1
  deployment="$(jq -c 'def rows: if type=="array" then . elif .items? then .items elif .result? then .result elif .deployments? then .deployments else [] end; [rows[] | {deployment_id:(.id // .deployment_id // ""),version:(.version_id // .version // .versions[0].version_id // ""),created:(.created_on // .created_at // .deployment_triggered_at // "")} ] | map(select(.version != "" and .deployment_id != "")) | sort_by(.created) | last // {}' "$capture")" || return 1
  version="$(jq -r '.version' <<<"$deployment")"; deployment_id="$(jq -r '.deployment_id' <<<"$deployment")"; created="$(jq -r '.created' <<<"$deployment")"
  [[ "$version" == "$FABRICD_VERSION" && -n "$deployment_id" && -n "$created" ]] || return 1
  deployed_epoch="$(parse_epoch "$created")" || return 1
  (( deployed_epoch >= MERGE_EPOCH && deployed_epoch <= MERGE_EPOCH + MAX_SECONDS )) || return 1
  check_deadline
  config="$ROOT/deploy/cloudflare-fabricd/wrangler.jsonc"
  [[ -f "$config" && ! -L "$config" ]] || return 1
  grep -Fq "$FABRICD_DIGEST" "$config" || return 1
  info="$(run_wrangler_cmd containers info "$FABRICD_APP_ID")" || return 1
  jq -e --arg digest "$FABRICD_DIGEST" '.name == "corelink-fabricd-fabricdcontainer" and ([.. | strings | scan("sha256:[0-9a-f]{64}")] | unique) == [$digest]' <<<"$info" >/dev/null || return 1
  check_deadline
  OBSERVED_FABRICD_VERSION="$version"; OBSERVED_FABRICD_DEPLOYMENT_ID="$deployment_id"; OBSERVED_FABRICD_AT="$created"; OBSERVED_FABRICD_DIGEST="$FABRICD_DIGEST"; OBSERVED_FABRICD_APP_ID="$FABRICD_APP_ID"
}

run_curl() {
  local output=$1 error=$2; shift 2
  run_bounded_capture "$output" "$error" curl "$@"
}

fleet_zero() {
  local header="$TMP_DIR/fleet.header" body="$TMP_DIR/fleet.json" status
  printf 'x-corelink-internal-auth: ' > "$header"; tr -d '\r\n' < "$FLEET_KEY_FILE" >> "$header"; printf '\n' >> "$header"; chmod 600 "$header"
  : > "$body"; chmod 600 "$body"
  run_curl "$TMP_DIR/fleet.status" "$TMP_DIR/fleet.stderr" --silent --show-error --fail-with-body --connect-timeout 10 --max-time 20 \
    -H "@${header}" -o "$body" -w '%{http_code}' "$BASE_URL/internal/v1/fleet/busy" || true
  scrub_file "$TMP_DIR/fleet.stderr"; status="$(cat "$TMP_DIR/fleet.status")"
  [[ "$status" == 200 ]] || return 1
  jq -e '((.busy // 0) | tonumber) == 0 and ((.unverifiable // 0) | tonumber) == 0' "$body" >/dev/null
}

poll_fleet_zero() {
  local i
  for ((i=1; i<=POLL_LIMIT; i++)); do
    check_deadline || return 1
    fleet_zero && return 0
    (( i == POLL_LIMIT )) || { check_deadline || return 1; sleep "$POLL_INTERVAL"; }
  done
  return 1
}

final_fleet_refreeze_guard() {
  # This verifier never unfreezes admission or deploys. The final guard proves
  # cleanup left the fleet at 0/0 and preserves the existing admission state.
  poll_fleet_zero
}

metrics_snapshot() {
  local header="$TMP_DIR/metrics.header" body="$TMP_DIR/metrics.json" status
  printf 'x-corelink-internal-auth: ' > "$header"; tr -d '\r\n' < "$METRICS_KEY_FILE" >> "$header"; printf '\n' >> "$header"; chmod 600 "$header"
  run_curl "$TMP_DIR/metrics.status" "$TMP_DIR/metrics.stderr" --silent --show-error --fail-with-body --connect-timeout 10 --max-time 20 \
    -H "@${header}" -o "$body" -w '%{http_code}' "$BASE_URL/internal/v1/metrics" || true
  scrub_file "$TMP_DIR/metrics.stderr"; status="$(cat "$TMP_DIR/metrics.status")"
  [[ "$status" == 200 ]] || return 1
  validate_metrics_file "$body" || return 1
  cat "$body"
}

fabricd_status_snapshot() {
  local header="$TMP_DIR/fabricd-metrics.header" body="$TMP_DIR/fabricd-metrics.json" status
  printf 'x-corelink-internal-auth: ' > "$header"; tr -d '\r\n' < "$FABRIC_OBSERVABILITY_KEY_FILE" >> "$header"; printf '\n' >> "$header"; chmod 600 "$header"
  run_curl "$TMP_DIR/fabricd-metrics.status" "$TMP_DIR/fabricd-metrics.stderr" --silent --show-error --fail-with-body --connect-timeout 10 --max-time 20 \
    -H "@$header" -o "$body" -w '%{http_code}' "$FABRICD_URL/internal/v1/status" || true
  scrub_file "$TMP_DIR/fabricd-metrics.stderr"; status="$(cat "$TMP_DIR/fabricd-metrics.status")"
  [[ "$status" == 200 ]] || return 1
  jq -ce '
    .counters as $c | ($c | type == "object") and
    (["leases_acquired", "mint_attempts", "revoke_attempts", "leases_closed"] | all(.[]; ($c[.] | type == "number" and floor == . and . >= 0)))
    | if . then {leases_acquired:$c.leases_acquired,mint_attempts:$c.mint_attempts,revoke_attempts:$c.revoke_attempts,leases_closed:$c.leases_closed} else error("required Fabricd status counters missing or non-integer") end
  ' "$body"
}

fabricd_no_allocation() {
  local before=$1 after=$2
  jq -ne --argjson before "$before" --argjson after "$after" '
    $after.leases_acquired == $before.leases_acquired and
    $after.mint_attempts == $before.mint_attempts and
    $after.revoke_attempts == $before.revoke_attempts and
    $after.leases_closed == $before.leases_closed
  ' >/dev/null
}

metrics_equal() {
  local before=$1 after=$2
  jq -e --argjson before "$(metric_values "$before")" --argjson after "$(metric_values "$after")" '$before == $after' >/dev/null
}

fabricd_snapshot_epoch_valid() {
  local epoch=$1 emergency=${2:-0}
  (( epoch >= MERGE_EPOCH )) || return 1
  [[ "$emergency" == 1 ]] || (( epoch <= MERGE_EPOCH + MAX_SECONDS ))
}

assert_fabricd_freeze_provenance() {
  local config="$ROOT/deploy/cloudflare-fabricd/wrangler.jsonc" config_sha
  git -C "$ROOT" merge-base --is-ancestor "$FABRICD_FREEZE_FIX" "$FABRICD_HISTORICAL_IMAGE_BUILD" || die 'Fabricd freeze fix is not an ancestor of historical 01560b image build source'
  git -C "$ROOT" cat-file -e "$FABRICD_IMAGE_BUILD^{commit}" || die 'published Fabricd image source commit is unavailable for provenance proof'
  git -C "$ROOT" merge-base --is-ancestor "$FABRICD_FREEZE_FIX" "$MERGE_SHA" || die 'Fabricd freeze fix is not an ancestor of authoritative merge SHA'
  [[ -f "$config" && ! -L "$config" ]] || die 'Fabricd Wrangler configuration is missing for provenance proof'
  grep -Fq "$FABRICD_DIGEST" "$config" || die 'Fabricd configuration does not bind expected immutable digest'
  config_sha="$(shasum -a 256 "$config" | awk '{print $1}')" || die 'unable to hash Fabricd configuration provenance'
  [[ "$config_sha" =~ ^[0-9a-f]{64}$ ]] || die 'Fabricd configuration hash malformed'
  FABRICD_FREEZE_PROVENANCE="$(jq -nc --arg fix "$FABRICD_FREEZE_FIX" --arg image_build "$FABRICD_IMAGE_BUILD" --arg merge "$MERGE_SHA" --arg digest "$FABRICD_DIGEST" --arg config_sha256 "$config_sha" '{freeze_fix:$fix,image_build_commit:$image_build,merge_sha:$merge,digest:$digest,config_sha256:$config_sha256}')"
}

fabricd_provider_snapshot() {
  local phase=$1 emergency=${2:-0} deployments latest version deployment_id created epoch info
  deployments="$(run_wrangler_cmd deployments list --name "$FABRICD" --json)" || return 1
  latest="$(jq -ce 'def rows: if type == "array" then . elif .items? then .items elif .result? then .result elif .deployments? then .deployments else [] end; [rows[] | {deployment_id:(.id // .deployment_id // ""),version:(.version_id // .version // .versions[0].version_id // ""),created:(.created_on // .created_at // .deployment_triggered_at // "")} | select(.deployment_id != "" and .version != "" and .created != "")] | sort_by(.created) | last' <<<"$deployments")" || return 1
  version="$(jq -r '.version' <<<"$latest")"; deployment_id="$(jq -r '.deployment_id' <<<"$latest")"; created="$(jq -r '.created' <<<"$latest")"
  epoch="$(parse_epoch "$created")" || return 1
  fabricd_snapshot_epoch_valid "$epoch" "$emergency" || return 1
  info="$(run_wrangler_cmd containers info "$FABRICD_APP_ID")" || return 1
  jq -e --arg digest "$FABRICD_DIGEST" '.name == "corelink-fabricd-fabricdcontainer" and ([.. | strings | scan("sha256:[0-9a-f]{64}")] | unique) == [$digest]' <<<"$info" >/dev/null || return 1
  if [[ "$emergency" == 1 ]]; then
    jq -e 'any(.. | objects; ((.FABRIC_ADMISSION_PAUSED? // .fabric_admission_paused? // "") == "1") or ((.name? // "") == "FABRIC_ADMISSION_PAUSED" and (.value? // .current? // "") == "1"))' <<<"$info" >/dev/null || return 1
  fi
  jq -nc --arg phase "$phase" --arg version "$version" --arg deployment_id "$deployment_id" --arg deployed_at "$created" --arg digest "$FABRICD_DIGEST" --arg emergency "$emergency" '{phase:$phase,version:$version,deployment_id:$deployment_id,deployed_at:$deployed_at,digest:$digest} + (if $emergency == "1" then {emergency:true,admission_paused:"1"} else {} end)'
}

fabricd_deploy_phase() {
  local phase=$1 paused=$2 emergency=${3:-0} output error pid watchdog rc remaining limit snapshot
  output="$TMP_DIR/fabricd-${phase}.out"; error="$TMP_DIR/fabricd-${phase}.err"
  if [[ "$emergency" == 1 ]]; then limit=$FABRICD_DEPLOY_TIMEOUT; else remaining="$(remaining_seconds)"; (( remaining > 0 )) || return 124; limit=$FABRICD_DEPLOY_TIMEOUT; (( limit < remaining )) || limit=$remaining; fi
  : > "$output"; : > "$error"; chmod 600 "$output" "$error"
  launch_group "$output" "$error" "$WRANGLER_BIN" deploy --config "$ROOT/deploy/cloudflare-fabricd/wrangler.jsonc" --keep-vars --strict --var "FABRIC_ADMISSION_PAUSED:$paused" --containers-rollout=immediate & pid=$!
  ( sleep "$limit"; kill -TERM -- "-$pid" 2>/dev/null || true; sleep 1; kill -KILL -- "-$pid" 2>/dev/null || true ) & watchdog=$!
  set +e; wait "$pid"; rc=$?; set -e
  kill "$watchdog" 2>/dev/null || true; wait "$watchdog" 2>/dev/null || true
  scrub_file "$error"
  [[ "$rc" == 0 ]] || return "$rc"
  snapshot="$(fabricd_provider_snapshot "$phase" "$emergency")" || return 1
  if [[ "$emergency" == 1 ]]; then EMERGENCY_REFREEZE_DEPLOYMENT="$snapshot"; FINAL_FABRICD_FROZEN=1
  elif [[ "$phase" == thaw ]]; then THAW_DEPLOYMENT="$snapshot"; else REFREEZE_DEPLOYMENT="$snapshot"; FINAL_FABRICD_FROZEN=1; fi
}

controlled_thaw() { THAW_INTENT=1; fabricd_deploy_phase thaw 0; }
controlled_refreeze() { fabricd_deploy_phase refreeze 1 && THAW_INTENT=0; }
refreeze_required() { [[ "$THAW_INTENT" == 1 && "$FINAL_FABRICD_FROZEN" != 1 ]]; }

canary_pat_valid() {
  local file=$1
  safe_secret_file "$file" || return 1
  LC_ALL=C grep -Eq '^corelink_pat_[0-9A-HJKMNP-TV-Z]{16}\.[A-Za-z0-9_-]{43}\.[A-Za-z0-9_-]{22}$' "$file"
}

fabric_request() {
  local method=$1 path=$2 body_file=${3:-} out_file=$4
  local header="$TMP_DIR/fabric.header" status_file="$out_file.status"
  printf 'authorization: Bearer ' > "$header"; tr -d '\r\n' < "$CANARY_PAT_FILE" >> "$header"; printf '\n' >> "$header"; chmod 600 "$header"
  : > "$out_file"; chmod 600 "$out_file"
  if [[ -n "$body_file" ]]; then
    run_curl "$status_file" "$out_file.stderr" --silent --show-error --connect-timeout 10 --max-time 20 -X "$method" -H "@${header}" -H 'content-type: application/json' --data-binary "@$body_file" -o "$out_file" -w '%{http_code}' "$FABRICD_URL$path" || true
  else
    run_curl "$status_file" "$out_file.stderr" --silent --show-error --connect-timeout 10 --max-time 30 -X "$method" -H "@${header}" -o "$out_file" -w '%{http_code}' "$FABRICD_URL$path" || true
  fi
  scrub_file "$out_file.stderr"; cat "$status_file"
}

now_ms() { python3 -c 'import time; print(time.monotonic_ns() // 1_000_000)'; }

fabric_canary() {
  canary_pat_valid "$CANARY_PAT_FILE" || die 'canary PAT must be owner-only 0600 and canonical corelink_pat format'
  [[ -x "$VERIFY_BIN" ]] || die 'v1/v2 close verifier is not executable'
  local keys="$TMP_DIR/attestation-keys.json" key_status acquire="$TMP_DIR/acquire.json" acquire_status
  key_status="$(fabric_request GET /v1/attestation/key '' "$keys")"
  [[ "$key_status" == 200 ]] || die 'attestation key endpoint did not return HTTP 200'
  jq -e '.keys | type == "array" and length == 1 and .[0].expires_ms == null and (.[0].key_id | type == "string" and length > 0) and (.[0].pubkey_b64 | type == "string" and length > 0)' "$keys" >/dev/null || die 'attestation key response shape invalid'
  jq -nc --arg image "$CANARY_IMAGE" --arg tmp "/tmp/$RUN_ID" '{image_digest:$image,net_policy:"isolated",tmp_root:$tmp,expiry_ms:60000}' > "$TMP_DIR/acquire-request.json"
  acquire_status="$(fabric_request POST /v1/leases "$TMP_DIR/acquire-request.json" "$acquire")"
  [[ "$acquire_status" == 200 ]] || die 'isolated canary acquire did not return HTTP 200'
  CANARY_LEASE="$(jq -er '.lease.lease_id | strings | select(length > 0)' "$acquire")" || die 'canary acquire response has no lease id'
  jq -e --arg lease "$CANARY_LEASE" '.lease.lease_id == $lease and .lease.state == "held"' "$acquire" >/dev/null || die 'canary acquire did not return a held lease'

  local state="$TMP_DIR/lease-state-held.json" state_status
  state_status="$(fabric_request GET "/v1/leases/$CANARY_LEASE" '' "$state")"
  [[ "$state_status" == 200 ]] || die 'held canary GET did not return HTTP 200'
  jq -e --arg lease "$CANARY_LEASE" '.lease_id == $lease and .state == "held"' "$state" >/dev/null || die 'held canary GET shape/state invalid'

  jq -nc '{status:"succeeded"}' > "$TMP_DIR/close-request.json"
  local close="$TMP_DIR/close-response.json" close_status close_started close_finished close_ms
  close_started="$(now_ms)"
  close_status="$(fabric_request POST "/v1/leases/$CANARY_LEASE/close" "$TMP_DIR/close-request.json" "$close")"
  close_finished="$(now_ms)"; close_ms=$((close_finished - close_started))
  [[ "$close_status" == 200 ]] || die 'canary close did not return HTTP 200'
  (( close_ms < 10000 )) || die "standalone close latency ${close_ms}ms is not below 10s bound"
  close_response_valid "$close" "$CANARY_LEASE" || die 'full signed CloseResponse shape invalid'
  run_bounded_capture "$TMP_DIR/verifier.out" "$TMP_DIR/verifier.err" node "$VERIFY_BIN" "$(cat "$keys")" < "$close" || { scrub_file "$TMP_DIR/verifier.err"; die 'v1/v2 CloseResponse verifier rejected response'; }
  scrub_file "$TMP_DIR/verifier.err"
  grep -Fxq 'corelink_v1_v2=accepted' "$TMP_DIR/verifier.out" || die 'v1/v2 CloseResponse verifier did not confirm acceptance'
  state="$TMP_DIR/lease-state-released.json"
  state_status="$(fabric_request GET "/v1/leases/$CANARY_LEASE" '' "$state")"
  [[ "$state_status" == 200 ]] || die 'released canary GET did not return HTTP 200'
  lease_state_released "$state" "$CANARY_LEASE" || die 'released canary GET did not prove exact released state'
  CANARY_RELEASED=1
  record_row "$FABRIC_FIX" standalone-jobclose true 200 "HTTP 200 signed CloseResponse released=true capture_incomplete=false; v1/v2 verified; GET released; close latency ${close_ms}ms" "$(( $(date +%s) - START_EPOCH ))"
}

close_response_valid() {
  local body=$1 lease=$2
  jq -e --arg lease "$lease" '
    type == "object" and
    ((keys - ["attestation", "capture_incomplete", "check_result", "fabric_key_id", "intent_metrics_sig", "lease_id", "metrics", "released", "result_binding_sig", "result_binding_sig_v2"]) | length == 0) and
    .lease_id == $lease and (.released | type == "boolean" and . == true) and
    (.capture_incomplete | type == "boolean" and . == false) and
    (.metrics | type == "object" and
      (keys | sort) == ["active_ms", "cost_usd_micros", "model_turns", "tokens", "tool_breakdown", "tool_calls", "wall_ms"] and
      (.tokens | type == "object" and (keys | sort) == ["cache_read", "cache_write", "input", "output", "total"] and all(.[]; type == "number" and floor == . and . >= 0)) and
      (.tool_breakdown | type == "array" and all(.[]; type == "object" and (keys | sort) == ["count", "tool"] and (.tool | type == "string") and (.count | type == "number" and floor == . and . >= 0))) and
      all([.active_ms, .cost_usd_micros, .model_turns, .tool_calls, .wall_ms][]; type == "number" and floor == . and . >= 0)) and
    (has("check_result") and ((.check_result == null) or
      (.check_result | type == "object" and
        (keys | sort) == ["artifacts", "def_digest", "duration_ms", "exit", "memo_key", "produced_at", "runner_ref", "stderr_ref", "stdout_ref", "toolchain_digest", "tree_hash"] and
        all([.memo_key, .tree_hash, .def_digest, .toolchain_digest, .runner_ref, .stdout_ref, .stderr_ref][]; type == "string") and
        (.exit | type == "number" and floor == .) and
        all([.duration_ms, .produced_at][]; type == "number" and floor == . and . >= 0) and
        (.artifacts | type == "array" and all(.[]; type == "object" and (keys | sort) == ["digest", "path"] and (.digest | type == "string") and (.path | type == "string")))))) and
    (.attestation | type == "object" and (keys | sort) == ["def", "model", "principal", "runner", "sig", "tree"] and all([.tree, .def, .runner, .model, .sig][]; type == "string") and (.principal | type == "array" and all(.[]; type == "string"))) and
    (.result_binding_sig | type == "string" and length > 0) and
    (.result_binding_sig_v2 | type == "string" and length > 0) and
    (.fabric_key_id | type == "string" and length > 0)
  ' "$body" >/dev/null
}

lease_state_released() {
  local body=$1 lease=$2
  jq -e --arg lease "$lease" 'type == "object" and (keys | sort) == ["lease_id", "state"] and .lease_id == $lease and .state == "released"' "$body" >/dev/null
}

direct_frozen_spawn() {
  local body="$TMP_DIR/frozen-spawn.json" response="$TMP_DIR/frozen-spawn-response.json"
  local header="$TMP_DIR/frozen-spawn.header" headers_out="$TMP_DIR/frozen-spawn.headers" status
  # The freeze runs before body validation/container access.  Keep the body
  # syntactically valid and intentionally inert, then require the documented
  # freeze response and retry contract from an authenticated caller.
  jq -nc '{}' > "$body"
  printf 'authorization: Bearer ' > "$header"; tr -d '\r\n' < "$SPAWN_TOKEN_FILE" >> "$header"; printf '\ncontent-type: application/json\n' >> "$header"; chmod 600 "$header"
  run_curl "$TMP_DIR/frozen-spawn.status" "$TMP_DIR/frozen-spawn.stderr" --silent --show-error --connect-timeout 10 --max-time 20 \
    -X POST -H "@$header" -D "$headers_out" --data-binary "@$body" -o "$response" -w '%{http_code}' "$BASE_URL/v1/spawn" || true
  scrub_file "$TMP_DIR/frozen-spawn.stderr"
  status="$(cat "$TMP_DIR/frozen-spawn.status")"
  frozen_spawn_response_valid "$status" "$response" "$headers_out"
}

frozen_spawn_response_valid() {
  local status=$1 response=$2 headers=$3
  [[ "$status" == 503 ]] || return 1
  jq -e 'type == "object" and (keys | sort) == ["error"] and .error == "fabric admission paused"' "$response" >/dev/null || return 1
  grep -Eqi '^retry-after:[[:space:]]*60[[:space:]]*$' "$headers" || return 1
}

frozen_fabricd_acquire() {
  local body="$TMP_DIR/frozen-fabricd-acquire.json" response="$TMP_DIR/frozen-fabricd-acquire-response.json"
  local header="$TMP_DIR/frozen-fabricd-acquire.header" headers_out="$TMP_DIR/frozen-fabricd-acquire.headers" status
  jq -nc --arg image "$CANARY_IMAGE" --arg tmp "/tmp/$RUN_ID-frozen" '{image_digest:$image,net_policy:"isolated",tmp_root:$tmp,expiry_ms:60000}' > "$body"
  printf 'authorization: Bearer ' > "$header"; tr -d '\r\n' < "$CANARY_PAT_FILE" >> "$header"; printf '\ncontent-type: application/json\n' >> "$header"; chmod 600 "$header"
  run_curl "$TMP_DIR/frozen-fabricd-acquire.status" "$TMP_DIR/frozen-fabricd-acquire.stderr" --silent --show-error --connect-timeout 10 --max-time 20 \
    -X POST -H "@$header" -D "$headers_out" --data-binary "@$body" -o "$response" -w '%{http_code}' "$FABRICD_URL/v1/leases" || true
  scrub_file "$TMP_DIR/frozen-fabricd-acquire.stderr"
  status="$(cat "$TMP_DIR/frozen-fabricd-acquire.status")"
  frozen_fabricd_response_valid "$status" "$response" "$headers_out"
}

frozen_fabricd_response_valid() {
  local status=$1 response=$2 headers=$3
  [[ "$status" == 503 ]] || return 1
  jq -e 'type == "object" and (keys | sort) == ["error"] and .error == "fabric admission paused"' "$response" >/dev/null || return 1
  grep -Eqi '^retry-after:[[:space:]]*60[[:space:]]*$' "$headers" || return 1
}

counter() {
  jq -er --arg n "$1" '.counters | select(type == "object") | select(has($n)) | .[$n] | select(type == "number" and floor == . and . >= 0)' "$2" >/dev/null
  jq -er --arg n "$1" '.counters[$n]' "$2"
}
nonincrease() { (( $(counter "$1" "$3") <= $(counter "$1" "$2") )); }

collect_tail() {
  local output="$TMP_DIR/tail.log" error="$TMP_DIR/tail.stderr" pid killer rc remaining limit
  check_deadline
  remaining="$(remaining_seconds)"; (( remaining > 0 )) || return 124
  limit="$TAIL_SECONDS"; (( limit < remaining )) || limit=$remaining
  : > "$output"; : > "$error"; chmod 600 "$output" "$error"
  launch_group "$output" "$error" "$WRANGLER_BIN" tail "$WORKER" --format json & pid=$!
  ( sleep "$limit"; kill -TERM -- "-$pid" 2>/dev/null || true; sleep 1; kill -KILL -- "-$pid" 2>/dev/null || true ) & killer=$!
  set +e; wait "$pid"; rc=$?; set -e
  kill "$killer" 2>/dev/null || true; wait "$killer" 2>/dev/null || true
  scrub_file "$error"; scrub_file "$output"
  check_deadline || [[ "$rc" == 143 || "$rc" == 124 ]] || return 1
  [[ "$rc" == 0 || "$rc" == 143 || "$rc" == 124 ]] || return 1
}

cleanup_live() {
  local rc=$?
  if [[ -n "$CANARY_LEASE" && "$CANARY_RELEASED" != 1 && -n "$TMP_DIR" ]] && check_deadline >/dev/null 2>&1; then
    jq -nc '{status:"succeeded"}' > "$TMP_DIR/cleanup-close.json" 2>/dev/null || true
    fabric_request POST "/v1/leases/$CANARY_LEASE/close" "$TMP_DIR/cleanup-close.json" "$TMP_DIR/cleanup-close-response.json" >/dev/null 2>/dev/null || true
  fi
  if refreeze_required && [[ -n "$TMP_DIR" ]]; then
    fabricd_deploy_phase exit-refreeze 1 1 >/dev/null 2>&1 || true
  fi
  if [[ -n "$TMP_DIR" ]]; then poll_fleet_zero >/dev/null 2>&1 || true; rm -rf -- "$TMP_DIR"; fi
  exit "$rc"
}

live() {
  START_EPOCH="$(date +%s)"
  require_live_inputs
  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/corelink-a29.XXXXXXXX")"; chmod 700 "$TMP_DIR"
  trap cleanup_live EXIT
  assert_authoritative_merge
  assert_fabricd_freeze_provenance
  RUN_ID="a29-$(date -u +%Y%m%dT%H%M%SZ)-$$-$(openssl rand -hex 5)"
  assert_deployed_version || die 'expected current Worker version is not deployed after merge timestamp'
  assert_fabricd_deployment || die 'expected current Fabricd version/digest is not deployed after merge timestamp'
  poll_fleet_zero || die 'preflight fleet is not provably 0 busy / 0 unverifiable'
  metrics_snapshot > "$TMP_DIR/metrics-before.json" || die 'metrics preflight failed'

  local before after
  direct_frozen_spawn || die 'authenticated /v1/spawn did not return the exact frozen-admission response'
  metrics_snapshot > "$TMP_DIR/metrics-freeze.json" || die 'metrics read after frozen direct spawn failed'
  before="$TMP_DIR/metrics-before.json"; after="$TMP_DIR/metrics-freeze.json"
  nonincrease webhook_spawn_claimed "$before" "$after" || die 'frozen direct spawn changed webhook claim counter'
  nonincrease jit_minted "$before" "$after" || die 'frozen direct spawn changed mint counter'
  nonincrease runner_spawned "$before" "$after" || die 'frozen direct spawn changed runner counter'
  ROW_METRICS_BEFORE="$(metric_values "$before")"; ROW_METRICS_AFTER="$(metric_values "$after")"
  record_row "$FREEZE_FIX" admission-freeze true 503 'authenticated POST /v1/spawn returned exact fabric admission paused response with Retry-After: 60; required counters did not increase' "$(( $(date +%s) - START_EPOCH ))"

  before="$(fabricd_status_snapshot)" || die 'Fabricd status before frozen-acquire proof failed'
  frozen_fabricd_acquire || die 'authenticated Fabricd /v1/leases did not return the exact frozen-admission response'
  after="$(fabricd_status_snapshot)" || die 'Fabricd status after frozen-acquire proof failed'
  fabricd_no_allocation "$before" "$after" || die 'frozen Fabricd acquire changed admission/mint counters'
  ROW_METRICS_BEFORE="$before"; ROW_METRICS_AFTER="$after"
  record_row "$FABRICD_FREEZE_FIX" fabricd-admission-freeze true 503 'authenticated POST /v1/leases returned exact fabric admission paused response with Retry-After: 60; Fabricd lease and mint counters did not increase' "$(( $(date +%s) - START_EPOCH ))"

  collect_tail || die 'bounded Worker tail failed'
  poll_fleet_zero || die 'fleet was not 0/0 before controlled Fabricd thaw'
  controlled_thaw || die 'controlled Fabricd thaw failed or exceeded its independent deploy timeout'
  poll_fleet_zero || die 'fleet was not 0/0 immediately after controlled Fabricd thaw'
  fabric_canary
  poll_fleet_zero || die 'fleet was not 0/0 after canary close before final Fabricd refreeze'
  controlled_refreeze || die 'controlled Fabricd refreeze failed or exceeded its independent deploy timeout'
  frozen_fabricd_acquire || die 'Fabricd did not prove frozen again after controlled refreeze'
  metrics_snapshot > "$TMP_DIR/metrics-spawn-final-before.json" || die 'final Spawn metrics preflight failed'
  direct_frozen_spawn || die 'Spawn did not prove frozen again after Fabricd thaw/refreeze'
  metrics_snapshot > "$TMP_DIR/metrics-spawn-final-after.json" || die 'final Spawn metrics postflight failed'
  metrics_equal "$TMP_DIR/metrics-spawn-final-before.json" "$TMP_DIR/metrics-spawn-final-after.json" || die 'final authenticated Spawn freeze probe changed required Spawn counters'
  SPAWN_FINAL_FREEZE_FACTS="$(metric_facts "$TMP_DIR/metrics-spawn-final-before.json" "$TMP_DIR/metrics-spawn-final-after.json")"
  SPAWN_REMAINED_FROZEN=true
  final_fleet_refreeze_guard || die 'final fleet is not provably 0 busy / 0 unverifiable for refreeze'
  [[ "$FINAL_FABRICD_FROZEN" == 1 && "$THAW_INTENT" == 0 && "$SPAWN_REMAINED_FROZEN" == true ]] || die 'final frozen outcomes were not recorded'
  emit_evidence PASS "$(date +%s)" 'final_fleet_0_0_refreeze_guard=verified; Spawn remained frozen; Fabricd thawed only for isolated canary and was refrozen'
  printf '%s\n' 'A2.9 PASS: Spawn admission freeze, Fabricd admission freeze, and standalone Fabricd JobClose verified.'
  return 0
}

mock() {
  START_EPOCH="$(date +%s)"
  OBSERVED_VERSION="mock-spawn-version"; OBSERVED_DEPLOYMENT_ID="mock-worker-deployment"; OBSERVED_DEPLOYED_AT="$MERGE_TS"
  OBSERVED_FABRICD_VERSION="mock-fabricd-version"; OBSERVED_FABRICD_DEPLOYMENT_ID="mock-fabricd-deployment"; OBSERVED_FABRICD_AT="$MERGE_TS"; OBSERVED_FABRICD_DIGEST="$FABRICD_DIGEST"; OBSERVED_FABRICD_APP_ID="mock-fabricd-app"
  THAW_DEPLOYMENT="$(jq -nc --arg timestamp "$MERGE_TS" --arg digest "$FABRICD_DIGEST" '{phase:"thaw",version:"mock-fabricd-thaw",deployment_id:"mock-thaw",deployed_at:$timestamp,digest:$digest}')"
  REFREEZE_DEPLOYMENT="$(jq -nc --arg timestamp "$MERGE_TS" --arg digest "$FABRICD_DIGEST" '{phase:"refreeze",version:"mock-fabricd-refreeze",deployment_id:"mock-refreeze",deployed_at:$timestamp,digest:$digest}')"
  SPAWN_FINAL_FREEZE_FACTS='{"before":{"webhook_spawn_claimed":0,"jit_minted":0,"runner_spawned":0},"after":{"webhook_spawn_claimed":0,"jit_minted":0,"runner_spawned":0}}'; SPAWN_REMAINED_FROZEN=true
  FABRICD_FREEZE_PROVENANCE="$(jq -nc --arg fix "$FABRICD_FREEZE_FIX" --arg image_build "$FABRICD_IMAGE_BUILD" --arg digest "$FABRICD_DIGEST" '{freeze_fix:$fix,image_build_commit:$image_build,digest:$digest,config_sha256:"mock"}')"
  record_row "$FREEZE_FIX" admission-freeze true 503 'mock authenticated spawn request -> exact frozen-admission response with no allocation' 0
  record_row "$FABRICD_FREEZE_FIX" fabricd-admission-freeze true 503 'mock authenticated Fabricd lease acquire -> exact frozen-admission response with no allocation' 0
  record_row "$FABRIC_FIX" standalone-jobclose true 200 'mock HTTP 200 signed CloseResponse released=true capture_incomplete=false; v1/v2 verified; GET released; latency 12ms' 0
  emit_evidence PASS "$(date +%s)" 'mock bounded thaw/canary/refreeze sequence represented; no network performed'
  printf '%s\n' 'MOCK NON-PROMOTABLE: three selected fix contracts represented; no network performed.'
  return 0
}

selftest() {
  local d out rc child_pid launcher_path
  require_python3 || die 'selftest requires python3 with os.setsid for process-group isolation'
  d="$(mktemp -d "${TMPDIR:-/tmp}/a29-selftest.XXXXXXXX")"; chmod 700 "$d"
  trap 'rm -rf -- "$d"' RETURN
  out="$d/evidence.json"
  set +e
  "$0" --mode mock --merge-sha 0123456789012345678901234567890123456789 --merge-timestamp 2026-09-09T00:00:00Z --expected-worker-version mock-version --fabricd-version mock-fabricd-version --fabricd-digest sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb --evidence-file "$out" >/dev/null 2>"$d/stderr"
  rc=$?
  set -e
  [[ "$rc" == 0 ]] || die "mock selftest expected exit 0, got $rc"
  jq -e '.schema_version == "a2.9/post-merge/v1" and .mode == "mock" and .status == "NON_PROMOTABLE" and .promotion == "never" and .live_qualification == false and (.rows|length) == 3 and all(.rows[]; .pass == true) and ([.rows[].fix]|sort) == (["99d5e39e66a69d44226dc9eed0faf0f7b384156f","8571f3cf38f11cca9b1f0752bf028e3dd982bb52","70a880e1ba7902546931f2d615859b3a632912b5"]|sort) and .fabric_fix.status == "PASS" and .operational_transition.spawn_remained_frozen == true and .operational_transition.spawn_final_freeze_facts.before == .operational_transition.spawn_final_freeze_facts.after and .operational_transition.fabricd_thaw.phase == "thaw" and .operational_transition.fabricd_refreeze.phase == "refreeze" and .fabricd_freeze_provenance.freeze_fix == "8571f3cf38f11cca9b1f0752bf028e3dd982bb52" and .fabricd_freeze_provenance.image_build_commit == "543fa5f253580056eb5f526d3f6d8694839e9c7c" and .secrets == "excluded" and .bounds.elapsed_s <= .bounds.max_elapsed_s' "$out" >/dev/null || die 'mock evidence schema/non-promotable assertion failed'
  ! grep -Eqi '(Bearer[[:space:]]+[A-Za-z0-9._~-]{12,}|corelink_[A-Za-z0-9._~-]{12,}|pat_[A-Za-z0-9._~-]{12,}|-----BEGIN|sha256=[0-9a-f]{64})' "$out" || die 'mock evidence contains secret-shaped material'

  MERGE_SHA=0123456789012345678901234567890123456789
  MERGE_TS=2026-09-09T00:00:00Z
  printf '%s\n' '{"merged":true,"merge_commit_sha":"ffffffffffffffffffffffffffffffffffffffff","merged_at":"2026-09-09T00:00:00Z"}' > "$d/tampered-pr.json"
  ! validate_pr_record "$d/tampered-pr.json" || die 'tampered PR merge SHA was accepted'
  printf '%s\n' '{"merged":true,"merge_commit_sha":"0123456789012345678901234567890123456789","merged_at":"2026-09-09T00:00:01Z"}' > "$d/tampered-timestamp.json"
  ! validate_pr_record "$d/tampered-timestamp.json" || die 'tampered PR timestamp was accepted'

  START_EPOCH=$(( $(now_s) - MAX_SECONDS - 1 )); DEADLINE_EPOCH=$((START_EPOCH + MAX_SECONDS))
  ! ( check_deadline ) 2>/dev/null || die 'deadline overrun was accepted'
  START_EPOCH="$(now_s)"; DEADLINE_EPOCH=$((START_EPOCH + MAX_SECONDS))

  json_rows="$(jq -nc --arg a "$FREEZE_FIX" --arg b "$FABRICD_FREEZE_FIX" --arg c "$FABRIC_FIX" '[{fix:$a,pass:true},{fix:$b,pass:false},{fix:$c,pass:true}]')"
  ! all_rows_pass 2>/dev/null || die 'failed row passed aggregate gate'
  json_rows='[]'

  printf '%s\n' '{"counters":{"webhook_spawn_claimed":1,"jit_minted":2}}' > "$d/missing-counters.json"
  ! validate_metrics_file "$d/missing-counters.json" 2>/dev/null || die 'missing required counter was accepted'
  printf '%s\n' '{"counters":{"webhook_spawn_claimed":1,"jit_minted":2,"runner_spawned":3}}' > "$d/valid-counters.json"
  validate_metrics_file "$d/valid-counters.json" || die 'valid integer counters were rejected'
  facts="$(metric_facts "$d/valid-counters.json" "$d/valid-counters.json")"
  jq -e '.before.webhook_spawn_claimed == 1 and .after.runner_spawned == 3' <<<"$facts" >/dev/null || die 'before/after metric facts were not recorded'

  # Contract-only probes: an incorrect secret surface is refused by the same
  # owner/mode validator used by live mode; both freeze response contracts must
  # retain their exact terminal shapes.  No token value enters evidence.
  printf '%s\n' 'mock-secret-value' > "$d/secret"; chmod 600 "$d/secret"
  safe_secret_file "$d/secret" || die 'valid owner-only secret surface was rejected'
  chmod 644 "$d/secret"
  ! safe_secret_file "$d/secret" || die 'world-readable secret surface was accepted'
  chmod 600 "$d/secret"
  cp "$d/secret" "$d/same-surface"; chmod 600 "$d/same-surface"
  ! distinct_secret_surfaces "$d/secret" "$d/same-surface" || die 'duplicated Spawn/Fabricd key surfaces were accepted'
  printf '%s\n' 'independent-fabric-observability-key' > "$d/fabric-secret"; chmod 600 "$d/fabric-secret"
  distinct_secret_surfaces "$d/secret" "$d/fabric-secret" || die 'independent Spawn/Fabricd key surfaces were rejected'
  printf '%s\n' '{"error":"fabric admission paused"}' > "$d/frozen.json"
  printf '%s\r\n' 'retry-after: 60' > "$d/frozen.headers"
  frozen_spawn_response_valid 503 "$d/frozen.json" "$d/frozen.headers" || die 'exact frozen spawn response was rejected'
  printf '%s\n' '{"error":"different"}' > "$d/wrong-frozen.json"
  ! frozen_spawn_response_valid 503 "$d/wrong-frozen.json" "$d/frozen.headers" || die 'wrong frozen spawn response was accepted'
  printf '%s\n' '{"error":"fabric admission paused"}' > "$d/fabricd-frozen.json"
  frozen_fabricd_response_valid 503 "$d/fabricd-frozen.json" "$d/frozen.headers" || die 'exact frozen Fabricd acquire response was rejected'
  ! frozen_fabricd_response_valid 503 "$d/wrong-frozen.json" "$d/frozen.headers" || die 'wrong frozen Fabricd response was accepted'
  printf '%s\n' '{"ok":true,"handle":"allocation"}' > "$d/allocation.json"
  ! frozen_fabricd_response_valid 201 "$d/allocation.json" "$d/frozen.headers" || die 'allocation response was accepted as a frozen Fabricd response'
  fabricd_no_allocation '{"leases_acquired":3,"mint_attempts":4,"revoke_attempts":0,"leases_closed":0}' '{"leases_acquired":3,"mint_attempts":4,"revoke_attempts":0,"leases_closed":0}' || die 'unchanged Fabricd counter snapshot was rejected'
  ! fabricd_no_allocation '{"leases_acquired":3,"mint_attempts":4,"revoke_attempts":0,"leases_closed":0}' '{"leases_acquired":3,"mint_attempts":4,"revoke_attempts":1,"leases_closed":0}' || die 'Fabricd revoke counter increase was accepted during frozen acquire'
  ! fabricd_no_allocation '{"leases_acquired":3,"mint_attempts":4,"revoke_attempts":0,"leases_closed":0}' '{"leases_acquired":4,"mint_attempts":4,"revoke_attempts":0,"leases_closed":0}' || die 'Fabricd admission increase was accepted'

  # The intent is set before dispatch, so a partial deploy, timeout, or canary
  # failure all retain the EXIT-refreeze obligation. Only a verified final
  # refreeze clears it.
  THAW_INTENT=1; FINAL_FABRICD_FROZEN=0
  refreeze_required || die 'partial/thaw-timeout state did not require EXIT refreeze'
  FINAL_FABRICD_FROZEN=1
  ! refreeze_required || die 'verified refreeze still required EXIT refreeze'
  FINAL_FABRICD_FROZEN=0; THAW_INTENT=0
  ! refreeze_required || die 'pre-thaw state incorrectly required refreeze'
  MERGE_EPOCH=100
  fabricd_snapshot_epoch_valid 1001 1 || die 'late emergency refreeze proof was rejected'
  ! fabricd_snapshot_epoch_valid 1001 0 || die 'late normal deployment was accepted as promotable'
  MERGE_EPOCH=0

  printf '%s\n' '#!/bin/sh' 'exit 99' > "$d/setsid"
  chmod 700 "$d/setsid"
  launcher_path="$d:$(dirname "$(command -v python3)"):/bin:/usr/bin"
  DEADLINE_EPOCH=$(( $(now_s) + 1 ))
  # shellcheck disable=SC2016
  ! ( PATH="$launcher_path" run_bounded_capture "$d/tree.out" "$d/tree.err" sh -c 'sleep 30 & child=$!; printf "%s\n" "$child" > "$1"; wait' sh "$d/child.pid" ) 2>/dev/null || die 'child/grandchild timeout unexpectedly passed'
  [[ -s "$d/child.pid" ]] || die 'child/grandchild selftest did not record child PID'
  child_pid="$(cat "$d/child.pid")"
  [[ "$child_pid" =~ ^[0-9]+$ ]] || die 'child/grandchild selftest recorded malformed PID'
  sleep 0.1
  ! kill -0 "$child_pid" 2>/dev/null || die 'grandchild survived process-group reap'

  DEADLINE_EPOCH=0
  printf '%s\n' 'a2.9 post-merge probe selftest: PASS (authoritative PR/timestamp, 900s deadline, aggregate gate, independent frozen Spawn/Fabricd contracts, distinct observability surfaces, bounded controlled transition evidence, hanging child, mock NON_PROMOTABLE, exact three SHAs)'
}

if [[ "$SELFTEST" == 1 ]]; then
  selftest
elif [[ "$MODE" == plan ]]; then
  plan
elif [[ "$MODE" == mock ]]; then
  mock
else
  live
fi
