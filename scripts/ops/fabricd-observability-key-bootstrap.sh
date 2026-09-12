#!/usr/bin/env bash
# CoreLink-only bootstrap for a missing local FABRIC_OBSERVABILITY_KEY.
# Default is inert. Execute requires explicit pins and the exact acknowledgement.
# Resume requires --recovery-source-sha, --recovery-expected-version, and
# --recovery-fabricd-app-id for the historical RED artifact pins.
set -Eeuo pipefail
set +x
umask 077

readonly ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908'
readonly RECOVERY_ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-RECOVER-INTROSPECT-LIVE-20260909'
readonly RESUME_ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-RESUME-INTROSPECT-LIVE-20260909'
readonly REPAIR_ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-REPAIR-INTROSPECT-AUTH-LIVE-20260909'
readonly REPAIR_RESUME_ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-REPAIR-INTROSPECT-AUTH-RESUME-LIVE-20260909'
readonly HISTORICAL_RECOVERY_SOURCE_SHA='37565619dae31a61f67a095daa0cd15b06386237'
readonly HISTORICAL_RECOVERY_VERSION='c38233a3-4ede-4803-8e1c-1a3b5ad4d667'
readonly HISTORICAL_RECOVERY_APP_ID='a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7'
readonly HISTORICAL_RECOVERY_DIGEST='sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5'
readonly WORKER_NAME='corelink-fabricd' APP_NAME='corelink-fabricd-fabricdcontainer'
readonly CONFIG_REL='deploy/cloudflare-fabricd/wrangler.jsonc'
readonly WRANGLER_EXPECTED_VERSION='4.105.0'
readonly CANONICAL_INTROSPECT_URL='https://corelink-api.humangr.com/internal/v1/auth/introspect'
readonly LEGACY_REPAIR_PROGRESS_SOURCE_SHA='74a534cd7fc5d2578ffb80f2bcf03f0d147f3d1b'
MODE=plan ACK_ARG='' ROOT='' COMMIT='' RECOVERY_SOURCE_SHA='' RECOVERY_VERSION='' RECOVERY_APP_ID='' VERSION='' APP_ID='' DIGEST='' RECOVER_INTROSPECT=0 RESUME_RECOVERY=0 REPAIR_INTROSPECT_AUTH=0 RESUME_INTROSPECT_AUTH_REPAIR=0 KEY_FILE_PROVIDED=0
OOB_DIR="${CORELINK_OOB_DIR:-$HOME/.corelink/rotation-b2-20260908}" KEY_FILE=''
FLEET_KEY_FILE='' INTROSPECT_KEY_FILE='' NEW_INTROSPECT_KEY_FILE='' PAT_FILE='' TENANT_ID='' EVIDENCE_FILE=''
ACCESS_CLIENT_ID_FILE='' ACCESS_CLIENT_SECRET_FILE=''
STATUS_URL='https://corelink-fabricd.gmhelmold.workers.dev/internal/v1/status'
HEALTH_URL='https://corelink-fabricd.gmhelmold.workers.dev/health'
FLEET_URL='https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy'
INTROSPECT_URL='https://corelink-api.humangr.com/internal/v1/auth/introspect'
STABILITY_SECS=120 MOCK_WRANGLER='' CURL_BIN=curl TMP_DIR='' LOCK_DIR=''
WRANGLER_DIR='' WRANGLER_BIN=''
MUTATION_STARTED=0 FINAL_FROZEN=0 RECOVERY_GUARD=''
PROBE_STATUS=''
RECOVERY_PHASE='preflight'
RECOVERY_INTROSPECT_SECRET_PUT=false RECOVERY_OBSERVABILITY_SECRET_PUT=false
REFREEZE_RESULT='not-attempted'
FINAL_APP_ID=''
LINEAGE_OLD_APP_ID=''
RESUMED_RECOVERY=false
REPAIR_PHASE='preflight' REPAIR_SECRET_PUT=false REPAIR_RECREATE=false REPAIR_RESUMED=false REPAIR_SECRET_PUT_INTENT=false REPAIR_SECRET_PUT_ATTEMPTS=0 REPAIR_DELETE_INTENT=false REPAIR_DELETE_COMPLETED=false REPAIR_DELETE_ATTEMPTS=0 REPAIR_RECREATE_INTENT=false REPAIR_DEPLOY_ATTEMPTS=0
REPAIR_PROGRESS='' REPAIR_PRE_VERSION='' REPAIR_FINAL_VERSION=''

die() { printf 'REFUSED: %s\n' "$*" >&2; exit 2; }
usage() { sed -n '1,12p' "$0"; printf '\nPlan is inert. Execute requires --execute, exact --ack, and all provider pins.\n' >&2; }
while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:?}"; shift 2;; --execute) MODE=execute; shift;; --ack) ACK_ARG="${2:?}"; shift 2;;
    --repo-root) ROOT="${2:?}"; shift 2;; --expected-commit) COMMIT="${2:?}"; shift 2;; --recovery-source-sha) RECOVERY_SOURCE_SHA="${2:?}"; shift 2;;
    --recovery-expected-version) RECOVERY_VERSION="${2:?}"; shift 2;; --recovery-fabricd-app-id) RECOVERY_APP_ID="${2:?}"; shift 2;;
    --expected-version) VERSION="${2:?}"; shift 2;; --fabricd-app-id) APP_ID="${2:?}"; shift 2;;
    --expected-image-digest) DIGEST="${2:?}"; shift 2;; --oob-dir) OOB_DIR="${2:?}"; shift 2;;
    --key-file) KEY_FILE="${2:?}"; KEY_FILE_PROVIDED=1; shift 2;; --fleet-key-file) FLEET_KEY_FILE="${2:?}"; shift 2;;
    --introspect-key-file) INTROSPECT_KEY_FILE="${2:?}"; shift 2;; --new-introspect-key-file) NEW_INTROSPECT_KEY_FILE="${2:?}"; shift 2;;
    --recover-introspect) RECOVER_INTROSPECT=1; shift;; --resume-recovery) RESUME_RECOVERY=1; shift;;
    --repair-introspect-auth) REPAIR_INTROSPECT_AUTH=1; shift;; --resume-introspect-auth-repair) RESUME_INTROSPECT_AUTH_REPAIR=1; shift;; --introspect-pat-file) PAT_FILE="${2:?}"; shift 2;;
    --tenant-id) TENANT_ID="${2:?}"; shift 2;; --evidence-file) EVIDENCE_FILE="${2:?}"; shift 2;;
    --access-client-id-file|--cf-access-client-id-file) ACCESS_CLIENT_ID_FILE="${2:?}"; shift 2;;
    --access-client-secret-file|--cf-access-client-secret-file) ACCESS_CLIENT_SECRET_FILE="${2:?}"; shift 2;;
    --status-url) STATUS_URL="${2:?}"; shift 2;; --fleet-url) FLEET_URL="${2:?}"; shift 2;;
    --introspect-url) INTROSPECT_URL="${2:?}"; shift 2;; --health-url) HEALTH_URL="${2:?}"; shift 2;; --stability-seconds) STABILITY_SECS="${2:?}"; shift 2;;
    --mock-wrangler) MOCK_WRANGLER="${2:?}"; shift 2;; --curl-bin) CURL_BIN="${2:?}"; shift 2;;
    --help|-h) usage; exit 0;; *) die "unknown argument $1";;
  esac
done
case "$MODE" in
  plan) printf '%s\n' 'PLAN ONLY: no network, file read, key generation, or provider mutation.'; exit 0;;
  execute|mock);; *) die 'mode must be plan, execute, or mock';;
esac
if [ $((RECOVER_INTROSPECT + RESUME_RECOVERY + REPAIR_INTROSPECT_AUTH + RESUME_INTROSPECT_AUTH_REPAIR)) -gt 1 ]; then
  die 'recovery, repair, and resume modes are mutually exclusive'
fi
if [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  [ "$ACK_ARG" = "$REPAIR_RESUME_ACK" ] || die 'exact introspect-auth repair resume acknowledgement required'
elif [ "$REPAIR_INTROSPECT_AUTH" = 1 ]; then
  [ "$ACK_ARG" = "$REPAIR_ACK" ] || die 'exact introspect-auth repair acknowledgement required'
elif [ "$RESUME_RECOVERY" = 1 ]; then
  [ "$ACK_ARG" = "$RESUME_ACK" ] || die 'exact resume acknowledgement required'
elif [ "$RECOVER_INTROSPECT" = 1 ]; then
  [ "$ACK_ARG" = "$RECOVERY_ACK" ] || die 'exact recovery acknowledgement required'
elif [ "$MODE" = execute ]; then
  [ "$ACK_ARG" = "$ACK" ] || die 'exact live acknowledgement required'
fi
[ "$MODE" != mock ] || [ -x "$MOCK_WRANGLER" ] || die 'mock mode requires --mock-wrangler'
[[ "$STABILITY_SECS" =~ ^[0-9]+$ ]] || die 'stability seconds must be a nonnegative integer'
if [ "$MODE" = mock ]; then :; elif [ "$STABILITY_SECS" = 120 ]; then :; else die 'live stability window must be exactly 120 seconds'; fi
[ -n "$ROOT" ] && [ -n "$COMMIT" ] && [ -n "$VERSION" ] && [ -n "$APP_ID" ] && [ -n "$DIGEST" ] || die 'missing repository/provider pins'
[[ "$COMMIT" =~ ^[a-f0-9]{40}$ ]] || die 'expected commit must be a full SHA-1'
[[ "$DIGEST" =~ ^sha256:[a-f0-9]{64}$ ]] || die 'expected image digest must be sha256:<64 hex>'
[ -d "$ROOT" ] || die 'repository root is not a directory'
ROOT="$(cd "$ROOT" && pwd -P)"
FINAL_APP_ID="$APP_ID"
LINEAGE_OLD_APP_ID="$APP_ID"
command -v "$CURL_BIN" >/dev/null 2>&1 || die 'curl is unavailable'
EVIDENCE_FILE="${EVIDENCE_FILE:-$ROOT/docs/plan/evidence/fabricd-observability-key-bootstrap.json}"
FLEET_KEY_FILE="${FLEET_KEY_FILE:-$OOB_DIR/fleet-busy-read-key}"
INTROSPECT_KEY_FILE="${INTROSPECT_KEY_FILE:-$OOB_DIR/fabric-introspect-key}"
PAT_FILE="${PAT_FILE:-$OOB_DIR/corelink-canary-tenant-pat}"
[ -n "$TENANT_ID" ] || die 'tenant id is required for introspection proof'
[ "$INTROSPECT_URL" = "$CANONICAL_INTROSPECT_URL" ] || die 'direct introspection URL must be the canonical Cloudflare Access URL'
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ] || [ "$REPAIR_INTROSPECT_AUTH" = 1 ] || [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  [ -n "$NEW_INTROSPECT_KEY_FILE" ] || die 'recovery requires --new-introspect-key-file'
fi

file_mode() { case "$(uname -s)" in Darwin) stat -f '%OLp' "$1";; Linux) stat -c '%a' "$1";; *) return 1;; esac; }
file_uid() { case "$(uname -s)" in Darwin) stat -f '%u' "$1";; Linux) stat -c '%u' "$1";; *) return 1;; esac; }
safe_dir() { [ -d "$1" ] && [ ! -L "$1" ] && [ "$(file_mode "$1")" = 700 ] && [ "$(file_uid "$1")" = "$(id -u)" ]; }
safe_file() { [ -f "$1" ] && [ ! -L "$1" ] && [ "$(file_mode "$1")" = 600 ] && [ "$(file_uid "$1")" = "$(id -u)" ] && [ -s "$1" ]; }
single_line_file() { safe_file "$1" || return 1; [ "$(tr -cd '\r' < "$1" | wc -c | tr -d ' ')" = 0 ] || return 1; [ "$(tr -cd '\n' < "$1" | wc -c | tr -d ' ')" -le 1 ]; }
[ -z "$ACCESS_CLIENT_ID_FILE" ] && [ -z "$ACCESS_CLIENT_SECRET_FILE" ] || {
  [ -n "$ACCESS_CLIENT_ID_FILE" ] && [ -n "$ACCESS_CLIENT_SECRET_FILE" ] || die 'Cloudflare Access requires both client id and client secret files'
  single_line_file "$ACCESS_CLIENT_ID_FILE" || die 'Cloudflare Access client id must be owner-only regular 0600 single-line file'
  single_line_file "$ACCESS_CLIENT_SECRET_FILE" || die 'Cloudflare Access client secret must be owner-only regular 0600 single-line file'
}
mkdir_private() { local d="$1"; if [ -e "$d" ] || [ -L "$d" ]; then safe_dir "$d" || die "directory must be owner-only 0700: $d"; else mkdir -p "$d"; chmod 700 "$d"; safe_dir "$d" || die "cannot secure directory: $d"; fi; }
mkdir_private "$OOB_DIR"; mkdir_private "$(dirname -- "$EVIDENCE_FILE")"
path_canonical() { local p="$1" d b; d="$(dirname -- "$p")"; b="$(basename -- "$p")"; d="$(cd "$d" && pwd -P)" || return 1; printf '%s/%s\n' "$d" "$b"; }
RECOVERY_GUARD="$OOB_DIR/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
RECOVERY_COMPLETE="$OOB_DIR/.fabricd-observability-key-bootstrap-introspect-recovery.complete"
REPAIR_PROGRESS="$OOB_DIR/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json"
REPAIR_FAILURE="$OOB_DIR/fabricd-observability-key-bootstrap-introspect-auth-repair-failure.json"
REPAIR_COMPLETE="$OOB_DIR/.fabricd-observability-key-bootstrap-introspect-auth-repair.complete"
[ "$RECOVER_INTROSPECT" != 1 ] || { [ ! -e "$RECOVERY_GUARD" ] && [ ! -L "$RECOVERY_GUARD" ] && [ ! -e "$RECOVERY_COMPLETE" ] && [ ! -L "$RECOVERY_COMPLETE" ] || die 'introspection recovery already started; refusing rerun'; }
[ "$REPAIR_INTROSPECT_AUTH" != 1 ] || { [ ! -e "$REPAIR_PROGRESS" ] && [ ! -L "$REPAIR_PROGRESS" ] && [ ! -e "$REPAIR_FAILURE" ] && [ ! -L "$REPAIR_FAILURE" ] && [ ! -e "$REPAIR_COMPLETE" ] && [ ! -L "$REPAIR_COMPLETE" ] || die 'introspect-auth repair already started; use explicit repair resume'; }

run_wrangler() {
  if [ "$MODE" = mock ]; then "$MOCK_WRANGLER" "$@"; return; fi
  local token
  token="$(cd "$WRANGLER_DIR" && "$WRANGLER_BIN" auth token --json | jq -er '.token // .access_token // .')" || die 'wrangler OAuth unavailable'
  [[ "$token" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]] || die 'invalid wrangler OAuth token'
  (cd "$WRANGLER_DIR" && CLOUDFLARE_API_TOKEN="$token" env -u LC_ALL -u LC_CTYPE -u LANG perl -e 'alarm shift; exec @ARGV' 60 "$WRANGLER_BIN" --config "$ROOT/$CONFIG_REL" "$@")
}
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-bootstrap.XXXXXXXX")"; chmod 700 "$TMP_DIR"
EVIDENCE_DIR="$TMP_DIR/evidence"; mkdir "$EVIDENCE_DIR"; chmod 700 "$EVIDENCE_DIR"
EVENT_LOG="$EVIDENCE_DIR/events.log"; : > "$EVENT_LOG"; chmod 600 "$EVENT_LOG"
log_event() { printf '%s %s\n' "$(date -u +%FT%H:%M:%SZ)" "$*" >> "$EVENT_LOG"; }
scrub() { local f="$1" s; s="$f.scrubbed"; sed -E -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1<REDACTED>/g' -e 's/(X-Corelink-Internal-Auth:[[:space:]]*)[^[:space:]]+/\1<REDACTED>/Ig' -e 's/[A-Za-z0-9+\/_=-]{40,}/<REDACTED>/g' "$f" > "$s" || true; chmod 600 "$s"; mv -f -- "$s" "$f"; }
run_safe() { local label="$1"; shift; local err rc; err="$EVIDENCE_DIR/${label}.stderr"; : > "$err"; chmod 600 "$err"; set +e; "$@" >/dev/null 2>"$err"; rc=$?; set -e; scrub "$err"; log_event "$label rc=$rc"; return "$rc"; }
capture() { local label="$1"; shift; local out err rc; out="$EVIDENCE_DIR/${label}.json"; err="$EVIDENCE_DIR/${label}.stderr"; : > "$out"; : > "$err"; chmod 600 "$out" "$err"; set +e; "$@" >"$out" 2>"$err"; rc=$?; set -e; scrub "$err"; log_event "$label rc=$rc"; [ "$rc" = 0 ] || return "$rc"; printf '%s\n' "$out"; }

config="$ROOT/$CONFIG_REL"
[ -f "$config" ] && [ ! -L "$config" ] || die 'canonical fabricd config missing or symlinked'
git -C "$ROOT" rev-parse --verify "$COMMIT^{commit}" >/dev/null || die 'expected commit unavailable'
[ "$(git -C "$ROOT" rev-parse HEAD)" = "$COMMIT" ] || die 'repository HEAD does not match expected commit'
if [ "$RESUME_RECOVERY" = 1 ]; then
  [[ "$RECOVERY_SOURCE_SHA" =~ ^[a-f0-9]{40}$ ]] || die 'resume requires --recovery-source-sha as a full SHA-1'
  [ -n "$RECOVERY_VERSION" ] || die 'resume requires --recovery-expected-version'
  [ -n "$RECOVERY_APP_ID" ] || die 'resume requires --recovery-fabricd-app-id'
  [[ "$VERSION" =~ ^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$ ]] || die 'resume expected version must be a UUID'
  [[ "$RECOVERY_VERSION" =~ ^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$ ]] || die 'resume recovery version must be a UUID'
  [[ "$APP_ID" =~ ^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$ ]] || die 'resume fabricd app id must be a UUID'
  [[ "$RECOVERY_APP_ID" =~ ^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$ ]] || die 'resume recovery app id must be a UUID'
  [ "$RECOVERY_APP_ID" != "$APP_ID" ] || die 'resume historical and current app ids must differ'
  git -C "$ROOT" rev-parse --verify "$RECOVERY_SOURCE_SHA^{commit}" >/dev/null || die 'recovery source commit unavailable'
  git -C "$ROOT" merge-base --is-ancestor "$RECOVERY_SOURCE_SHA" "$COMMIT" || die 'recovery source commit is not an ancestor of expected commit'
fi
if [ "$REPAIR_INTROSPECT_AUTH" = 1 ] || [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  [[ "$VERSION" =~ ^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$ ]] || die 'introspect-auth repair expected version must be a UUID'
  [[ "$APP_ID" =~ ^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$ ]] || die 'introspect-auth repair app id must be a UUID'
fi
[ -z "$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)" ] || die 'repository worktree is not clean'
grep -Fq "$DIGEST" "$config" || die 'expected immutable digest absent from canonical config'
resolve_wrangler() {
  [ "$MODE" = mock ] && return 0
  WRANGLER_DIR="$ROOT/${CONFIG_REL%/*}"
  WRANGLER_BIN="$WRANGLER_DIR/node_modules/.bin/wrangler"
  [ -x "$WRANGLER_BIN" ] || die "local Wrangler binary missing: $WRANGLER_BIN"
  local version
  version="$(cd "$WRANGLER_DIR" && "$WRANGLER_BIN" --version)" || die 'local Wrangler version probe failed'
  [ "$version" = "$WRANGLER_EXPECTED_VERSION" ] || die "local Wrangler version mismatch: expected $WRANGLER_EXPECTED_VERSION, got $version"
}
resolve_wrangler
LOCK_DIR="$OOB_DIR/.fabricd-observability-key-bootstrap.lock"; mkdir "$LOCK_DIR" 2>/dev/null || die 'another bootstrap holds the local lock'; chmod 700 "$LOCK_DIR"; log_event 'lock=acquired'

cleanup() {
  local rc=$? failure_tmp=''; trap - EXIT
  if [ "$MUTATION_STARTED" = 1 ] && [ "$FINAL_FROZEN" != 1 ]; then
    REFREEZE_RESULT='red'
    if run_safe refreeze run_wrangler deploy --keep-vars --strict --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate; then REFREEZE_RESULT='green'; log_event 'refreeze=GREEN admission_paused=1'; else log_event 'refreeze=RED escalation_required'; rc=1; fi
  fi
  rmdir "$LOCK_DIR" 2>/dev/null || true
  if [ "$REPAIR_INTROSPECT_AUTH" = 1 ] || [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
    if [ "$rc" = 0 ]; then
      stable_events="$OOB_DIR/fabricd-observability-key-bootstrap-$(date -u +%Y%m%dT%H%M%SZ).events.log"
      cp "$EVENT_LOG" "$stable_events" && chmod 600 "$stable_events" && safe_file "$stable_events" || rc=1
      if [ "$rc" = 0 ]; then
        jq -n --arg commit "$COMMIT" --arg version "$VERSION" --arg digest "$DIGEST" --arg old_app "$LINEAGE_OLD_APP_ID" --arg new_app "$FINAL_APP_ID" --arg key_path "$KEY_FILE" --arg introspect_key_path "$NEW_INTROSPECT_KEY_FILE" --arg events "$stable_events" --argjson resumed "$REPAIR_RESUMED" --argjson secret_metadata "${REPAIR_SECRET_METADATA:-[]}" --argjson secret_attempts "$REPAIR_SECRET_PUT_ATTEMPTS" --argjson delete_attempts "$REPAIR_DELETE_ATTEMPTS" --argjson deploy_attempts "$REPAIR_DEPLOY_ATTEMPTS" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap",status:"PASS",operation_mode:"introspect-auth-repair",resumed:$resumed,source:{repository:"corelink-runners",commit_sha:$commit},repair:{wrong_name:"FABRIC_INTROSPECT_KEY",correct_name:"FABRIC_INTROSPECT_AUTH_KEY",historical:{source_commit:"37565619dae31a61f67a095daa0cd15b06386237",version:"c38233a3-4ede-4803-8e1c-1a3b5ad4d667",app_id:"a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7",image_digest:"sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5"},pre_repair_app_id:$old_app,final_app_id:$new_app,attempts:{secret_put:$secret_attempts,delete:$delete_attempts,deploy:$deploy_attempts},provider_secret_metadata:$secret_metadata,outcome:"PASS"},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$new_app},lineage:{historical_old_app_id:"a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7",pre_repair_app_id:$old_app,final_app_id:$new_app},secret:{name:"FABRIC_OBSERVABILITY_KEY",value:"excluded",local_path:$key_path,mode:"0600"},introspection_repair:{name:"FABRIC_INTROSPECT_AUTH_KEY",value:"excluded",local_path:$introspect_key_path,mode:"0600"},gates:{quiescence:"GREEN",admission_paused:true,introspection_pat_schema:"200_valid_json",observability_status:"200_valid_json",health:"200",refreeze:"GREEN"},logs:{events:$events,secrets:"excluded",mode:"0600"}}' > "$EVIDENCE_FILE" && chmod 644 "$EVIDENCE_FILE" || rc=1
      fi
      if [ "$rc" = 0 ]; then mv -f -- "$RECOVERY_GUARD" "$REPAIR_COMPLETE" || rc=1; fi
    else
      log_event 'outcome=FAILED'
      if [ "$MUTATION_STARTED" = 1 ]; then
        jq -n --arg commit "$COMMIT" --arg pre_version "$REPAIR_PRE_VERSION" --arg final_version "$REPAIR_FINAL_VERSION" --arg digest "$DIGEST" --arg pre_app "${LINEAGE_OLD_APP_ID:-$APP_ID}" --arg final_app "$FINAL_APP_ID" --arg phase "$REPAIR_PHASE" --arg refreeze "$REFREEZE_RESULT" --argjson secret_put "$REPAIR_SECRET_PUT" --argjson recreate "$REPAIR_RECREATE" --argjson secret_intent "$REPAIR_SECRET_PUT_INTENT" --argjson secret_attempts "$REPAIR_SECRET_PUT_ATTEMPTS" --argjson delete_intent "$REPAIR_DELETE_INTENT" --argjson delete_attempts "$REPAIR_DELETE_ATTEMPTS" --argjson deploy_intent "$REPAIR_RECREATE_INTENT" --argjson deploy_attempts "$REPAIR_DEPLOY_ATTEMPTS" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-auth-repair-failure",status:"RED",operation_mode:"introspect-auth-repair",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$pre_version,expected_image_digest:$digest,app_id:$pre_app},repair:{wrong_name:"FABRIC_INTROSPECT_KEY",correct_name:"FABRIC_INTROSPECT_AUTH_KEY",pre_repair_app_id:$pre_app,pre_repair_version:$pre_version,final_app_id:$final_app,final_version:$final_version,secret_put_completed:$secret_put,recreate_completed:$recreate,attempts:{secret_put:$secret_attempts,delete:$delete_attempts,deploy:$deploy_attempts},intents:{secret_put:$secret_intent,delete:$delete_intent,deploy:$deploy_intent}},phase:$phase,gates:{refreeze_result:$refreeze},outcome:"RED",rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$REPAIR_FAILURE.tmp" && chmod 600 "$REPAIR_FAILURE.tmp" && mv -f -- "$REPAIR_FAILURE.tmp" "$REPAIR_FAILURE" || rc=1
      fi
    fi
    find "$TMP_DIR" -type f -exec rm -f -- {} + 2>/dev/null || true; rmdir "$EVIDENCE_DIR" "$TMP_DIR" 2>/dev/null || true; exit "$rc"
  fi
  if [ "$rc" = 0 ]; then
    stable_events="$OOB_DIR/fabricd-observability-key-bootstrap-$(date -u +%Y%m%dT%H%M%SZ).events.log"
    cp "$EVENT_LOG" "$stable_events" && chmod 600 "$stable_events" && safe_file "$stable_events" || rc=1
    if [ "$rc" = 0 ] && [ "$RECOVER_INTROSPECT" = 1 ]; then
      printf '%s\n' 'complete' > "$RECOVERY_GUARD".complete.tmp && chmod 600 "$RECOVERY_GUARD".complete.tmp
      mv -f -- "$RECOVERY_GUARD".complete.tmp "$RECOVERY_COMPLETE" || rc=1
    fi
    recovery_mode=normal; [ "$RECOVER_INTROSPECT" = 1 ] && recovery_mode=introspect-recovery
    [ "$RESUME_RECOVERY" = 1 ] && recovery_mode=introspect-recovery-resumed
    jq -n --arg commit "$COMMIT" --arg version "$VERSION" --arg digest "$DIGEST" --arg old_app "$LINEAGE_OLD_APP_ID" --arg new_app "$FINAL_APP_ID" --arg key_path "$KEY_FILE" --arg introspect_key_path "$NEW_INTROSPECT_KEY_FILE" --arg events "$stable_events" --arg mode "$recovery_mode" --argjson resumed "$RESUMED_RECOVERY" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap",status:"PASS",operation_mode:$mode,resumed:$resumed,source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$new_app},lineage:{old_app_id:$old_app,new_app_id:$new_app},secret:{name:"FABRIC_OBSERVABILITY_KEY",value:"excluded",local_path:$key_path,mode:"0600"},introspection_recovery:(if $mode == "normal" then null else {name:"FABRIC_INTROSPECT_AUTH_KEY",value:"excluded",local_path:$introspect_key_path,mode:"0600"} end),gates:{quiescence:"GREEN",admission_paused:true,status_endpoint:"200_valid_json",refreeze:"GREEN"},logs:{events:$events,secrets:"excluded",mode:"0600"}}' > "$EVIDENCE_FILE" && chmod 644 "$EVIDENCE_FILE" || rc=1
    if [ "$rc" = 0 ] && [ "$RESUME_RECOVERY" = 1 ]; then
      mv -f -- "$RECOVERY_GUARD" "$RECOVERY_COMPLETE" || rc=1
    elif [ "$rc" = 0 ] && [ "$RECOVER_INTROSPECT" = 1 ]; then
      printf '%s\n' 'complete' > "$RECOVERY_GUARD".complete.tmp && chmod 600 "$RECOVERY_GUARD".complete.tmp
      mv -f -- "$RECOVERY_GUARD".complete.tmp "$RECOVERY_COMPLETE" || rc=1
    fi
  else
    log_event 'outcome=FAILED'
    if [ "$RECOVER_INTROSPECT" = 1 ] && [ "$MUTATION_STARTED" = 1 ]; then
      if failure_tmp="$(mktemp "$OOB_DIR/.fabricd-observability-key-bootstrap-introspect-recovery-failure.XXXXXXXX")"; then
        chmod 600 "$failure_tmp"
        jq -n \
          --arg commit "$COMMIT" \
          --arg version "$VERSION" \
          --arg digest "$DIGEST" \
          --arg app "$APP_ID" \
          --arg phase "$RECOVERY_PHASE" \
          --arg refreeze "$REFREEZE_RESULT" \
          --argjson introspect_put "$RECOVERY_INTROSPECT_SECRET_PUT" \
          --argjson observability_put "$RECOVERY_OBSERVABILITY_SECRET_PUT" \
          '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$app},phase:$phase,secrets:{FABRIC_INTROSPECT_AUTH_KEY_put_completed:$introspect_put,FABRIC_OBSERVABILITY_KEY_put_completed:$observability_put},gates:{refreeze_result:$refreeze},outcome:"RED",rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$failure_tmp" && chmod 600 "$failure_tmp" && mv -f -- "$failure_tmp" "$OOB_DIR/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" || rc=1
      else rc=1; fi
    fi
  fi
  find "$TMP_DIR" -type f -exec rm -f -- {} + 2>/dev/null || true; rmdir "$EVIDENCE_DIR" "$TMP_DIR" 2>/dev/null || true; exit "$rc"
}
trap cleanup EXIT

snapshot() {
  local label="$1" expected_app_id="${2:-$APP_ID}" discover="${3:-false}" deploys info container version image app_id
  deploys="$(capture "$label-deployments" run_wrangler deployments list --name "$WORKER_NAME" --json)" || return 1
  # Wrangler 4.105.0 rejects `containers info APP --json`. The supported
  # machine-readable surface is the complete list; select the exact pinned
  # application id and fail closed if the provider returns zero or multiple
  # matches. Keep this lookup independent of human-formatted CLI output.
  info="$(capture "$label-containers" run_wrangler containers list --json)" || return 1
  container="$(jq -ce --arg id "$expected_app_id" --arg n "$APP_NAME" --argjson discover "$discover" '
    (if $discover then [.. | objects | select(.name? == $n)] else [.. | objects | select(.id? == $id)] end)
    | if length != 1 then error("exact fabricd application identity is absent or ambiguous")
      elif .[0].name? != $n then error("fabricd application identity mismatch")
      else .[0]
      end
  ' "$info")" || { log_event "$label app_identity=RED"; return 1; }
  app_id="$(printf '%s\n' "$container" | jq -er '.id')" || return 1
  version="$(jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$deploys")" || return 1
  image="$(printf '%s\n' "$container" | jq -er '[.. | strings | scan("sha256:[0-9a-f]{64}")] | unique | if length == 1 then .[0] else error("ambiguous image digest") end')" || return 1
  printf '%s\t%s\t%s\n' "$version" "$image" "$app_id"
}
assert_frozen() {
  local version="$1" vars; vars="$(capture frozen-vars run_wrangler versions view "$version" --name "$WORKER_NAME" --json)" || return 1
  jq -e '[.. | objects | select(.name? == "FABRIC_ADMISSION_PAUSED") | (.text? // .value? // "")] | length == 1 and .[0] == "1"' "$vars" >/dev/null || { log_event 'admission_paused=RED'; return 1; }; log_event 'admission_paused=GREEN'
}

fleet_header="$TMP_DIR/fleet.header"; introspect_header="$TMP_DIR/introspect.header"; access_header=''
single_line_file "$FLEET_KEY_FILE" || die 'fleet key must be owner-only regular 0600 single-line file'
if [ "$RESUME_RECOVERY" != 1 ] && [ "$REPAIR_INTROSPECT_AUTH" != 1 ] && [ "$RESUME_INTROSPECT_AUTH_REPAIR" != 1 ]; then
  single_line_file "$INTROSPECT_KEY_FILE" || die 'introspect key must be owner-only regular 0600 single-line file'
fi
single_line_file "$PAT_FILE" || die 'introspection PAT must be owner-only regular 0600 single-line file'
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ] || [ "$REPAIR_INTROSPECT_AUTH" = 1 ] || [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  [ -e "$NEW_INTROSPECT_KEY_FILE" ] && [ ! -L "$NEW_INTROSPECT_KEY_FILE" ] || die 'new introspect key file is missing'
  single_line_file "$NEW_INTROSPECT_KEY_FILE" || die 'new introspect key must be owner-only regular 0600 single-line file'
fi
printf 'X-Corelink-Internal-Auth: ' > "$fleet_header"; tr -d '\r\n' < "$FLEET_KEY_FILE" >> "$fleet_header"; printf '\n' >> "$fleet_header"; chmod 600 "$fleet_header"
if [ "$RESUME_RECOVERY" != 1 ] && [ "$REPAIR_INTROSPECT_AUTH" != 1 ] && [ "$RESUME_INTROSPECT_AUTH_REPAIR" != 1 ]; then
  printf 'X-Corelink-Internal-Auth: ' > "$introspect_header"; tr -d '\r\n' < "$INTROSPECT_KEY_FILE" >> "$introspect_header"; printf '\n' >> "$introspect_header"; chmod 600 "$introspect_header"
fi
if [ -n "$ACCESS_CLIENT_ID_FILE" ]; then
  access_header="$TMP_DIR/cf-access.header"
  { printf 'CF-Access-Client-Id: '; tr -d '\r\n' < "$ACCESS_CLIENT_ID_FILE"; printf '\n';
    printf 'CF-Access-Client-Secret: '; tr -d '\r\n' < "$ACCESS_CLIENT_SECRET_FILE"; printf '\n';
  } > "$access_header"
  chmod 600 "$access_header"
fi

if [ "$RESUME_RECOVERY" = 1 ] || [ "$REPAIR_INTROSPECT_AUTH" = 1 ] || [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  [ "$KEY_FILE_PROVIDED" = 1 ] || die 'resume or repair requires --key-file for the existing observability key'
  single_line_file "$KEY_FILE" || die 'resume or repair observability key must be owner-only regular 0600 single-line file'
elif [ -n "$KEY_FILE" ]; then
  if [ -e "$KEY_FILE" ] || [ -L "$KEY_FILE" ]; then safe_file "$KEY_FILE" || die 'provided key must be owner-only regular 0600 single-line file'; else key_parent="$(dirname -- "$KEY_FILE")"; safe_dir "$key_parent" || die 'generated key parent must be owner-only 0700'; tmp_key="$(mktemp "$key_parent/.obs-key.XXXXXXXX")"; chmod 600 "$tmp_key"; openssl rand -base64 48 | tr -d '\n' > "$tmp_key"; chmod 600 "$tmp_key"; mv -f -- "$tmp_key" "$KEY_FILE"; fi
else
  KEY_FILE="$OOB_DIR/fabric-observability-key-bootstrap.b64"; [ ! -e "$KEY_FILE" ] && [ ! -L "$KEY_FILE" ] || die 'default generated key already exists; provide a new path'; tmp_key="$(mktemp "$OOB_DIR/.obs-key.XXXXXXXX")"; chmod 600 "$tmp_key"; openssl rand -base64 48 | tr -d '\n' > "$tmp_key"; chmod 600 "$tmp_key"; mv -f -- "$tmp_key" "$KEY_FILE"
fi
single_line_file "$KEY_FILE" || die 'new observability key must be owner-only regular 0600 single-line file'; log_event 'key=ready value=excluded mode=0600'
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ] || [ "$REPAIR_INTROSPECT_AUTH" = 1 ] || [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  new_introspect_canonical="$(path_canonical "$NEW_INTROSPECT_KEY_FILE")" || die 'cannot canonicalize new introspect key path'
  key_canonical="$(path_canonical "$KEY_FILE")" || die 'cannot canonicalize observability key path'
  [ "$key_canonical" != "$new_introspect_canonical" ] || die 'new introspect key destination must differ from observability key'
  if [ "$RECOVER_INTROSPECT" = 1 ]; then
    old_introspect_canonical="$(path_canonical "$INTROSPECT_KEY_FILE")" || die 'cannot canonicalize old introspect key path'
    [ "$old_introspect_canonical" != "$new_introspect_canonical" ] || die 'new introspect key destination must differ from old key'
  fi
  log_event 'recovery_key=ready value=excluded mode=0600 distinct=true'
fi
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ] || [ "$REPAIR_INTROSPECT_AUTH" = 1 ] || [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  new_introspect_header="$TMP_DIR/new-introspect.header"
  printf 'X-Corelink-Internal-Auth: ' > "$new_introspect_header"
  tr -d '\r\n' < "$NEW_INTROSPECT_KEY_FILE" >> "$new_introspect_header"
  printf '\n' >> "$new_introspect_header"
  chmod 600 "$new_introspect_header"
fi

assert_fleet_quiet() {
  local label="$1" fleet_json="$TMP_DIR/$1-fleet.json" curl_rc
  : > "$fleet_json"; chmod 600 "$fleet_json"
  set +e
  "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$fleet_header" "$FLEET_URL" > "$fleet_json" 2>"$EVIDENCE_DIR/$label-fleet.stderr"
  curl_rc=$?
  set -e
  scrub "$EVIDENCE_DIR/$label-fleet.stderr"
  [ "$curl_rc" = 0 ] || { log_event "$label fleet_request=RED"; return 1; }
  jq -e '((.busy // 0) | tonumber) == 0 and ((.unverifiable // 0) | tonumber) == 0' "$fleet_json" >/dev/null || { log_event "$label fleet_busy=RED"; return 1; }
  log_event "$label fleet_busy=0 fleet_unverifiable=0"
}
probe_introspection() {
  local label="$1" header="$2" body="$3" status_file="$4" curl_rc; local -a access_args=()
  [ -z "$access_header" ] || access_args=(--header "@$access_header")
  : > "$body"
  : > "$status_file"
  chmod 600 "$body" "$status_file"
  set +e
  jq -nc --rawfile token "$PAT_FILE" '{token:($token|sub("\\n$";""))}' |
    "$CURL_BIN" --silent --show-error --connect-timeout 10 --max-time 30 --header "@$header" "${access_args[@]}" --header 'content-type: application/json' --data-binary @- --output "$body" --write-out '%{http_code}' "$CANONICAL_INTROSPECT_URL" > "$status_file" 2>"$EVIDENCE_DIR/$label.stderr"
  curl_rc=$?
  set -e
  scrub "$EVIDENCE_DIR/$label.stderr"
  [ "$curl_rc" = 0 ] || { log_event "$label transport=RED"; return 1; }
  PROBE_STATUS="$(tr -d '\r\n' < "$status_file")"
}

verify_observability_status() {
  local label="$1" header status_json
  header="$TMP_DIR/$label-status.header"
  printf 'X-Corelink-Internal-Auth: ' > "$header"; tr -d '\r\n' < "$KEY_FILE" >> "$header"; printf '\n' >> "$header"; chmod 600 "$header"
  status_json="$TMP_DIR/$label-status.json"; : > "$status_json"; chmod 600 "$status_json"
  "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$header" "$STATUS_URL" > "$status_json" 2> "$EVIDENCE_DIR/$label-status.stderr" || return 1
  scrub "$EVIDENCE_DIR/$label-status.stderr"
  jq -e '(.version | strings | length > 0) and ((.uptime_ms | tonumber) >= 0) and (.ledger_cross_instance_safe | type == "boolean") and ((.num_shards | tonumber) >= 1)' "$status_json" >/dev/null
}

verify_health() {
  local label="$1" health_status health_rc
  set +e
  health_status="$("$CURL_BIN" --silent --show-error --connect-timeout 10 --max-time 30 --output /dev/null --write-out '%{http_code}' "$HEALTH_URL" 2> "$EVIDENCE_DIR/$label-health.stderr")"
  health_rc=$?
  set -e
  scrub "$EVIDENCE_DIR/$label-health.stderr"
  [ "$health_rc" = 0 ] && [ "$health_status" = 200 ]
}

assert_repair_secret_metadata() {
  local metadata
  metadata="$(capture repair-secret-metadata run_wrangler secret list --name "$WORKER_NAME" --format json)" || return 1
  REPAIR_SECRET_METADATA="$(jq -ce '[.. | objects | select(.name? == "FABRIC_INTROSPECT_KEY" or .name? == "FABRIC_INTROSPECT_AUTH_KEY") | {name,version:(.version // .version_id // .id // "metadata-present")}] | if length != 2 or ([.[].name] | sort) != ["FABRIC_INTROSPECT_AUTH_KEY","FABRIC_INTROSPECT_KEY"] then error("required legacy and correct introspection secret metadata is absent or ambiguous") else . end' "$metadata")" || return 1
  log_event 'repair_secret_metadata=GREEN wrong_binding_present=true correct_binding_present=true values=not-read'
}

write_repair_progress() {
  local phase="$1" pre_app="$2" final_app="$3" tmp
  tmp="$REPAIR_PROGRESS.tmp"
  jq -n --arg commit "$COMMIT" --arg pre_version "$REPAIR_PRE_VERSION" --arg final_version "$REPAIR_FINAL_VERSION" --arg digest "$DIGEST" --arg phase "$phase" --arg pre_app "$pre_app" --arg final_app "$final_app" --argjson secret_put "$REPAIR_SECRET_PUT" --argjson recreate "$REPAIR_RECREATE" --argjson secret_put_intent "$REPAIR_SECRET_PUT_INTENT" --argjson secret_put_attempts "$REPAIR_SECRET_PUT_ATTEMPTS" --argjson delete_intent "$REPAIR_DELETE_INTENT" --argjson delete_completed "$REPAIR_DELETE_COMPLETED" --argjson delete_attempts "$REPAIR_DELETE_ATTEMPTS" --argjson recreate_intent "$REPAIR_RECREATE_INTENT" --argjson deploy_attempts "$REPAIR_DEPLOY_ATTEMPTS" --argjson secret_metadata "${REPAIR_SECRET_METADATA:-[]}" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-auth-repair-progress",status:"RED",operation_mode:"introspect-auth-repair",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$pre_version,expected_image_digest:$digest,app_id:$pre_app},historical:{source_commit:"37565619dae31a61f67a095daa0cd15b06386237",version:"c38233a3-4ede-4803-8e1c-1a3b5ad4d667",app_id:"a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7",image_digest:"sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5"},repair:{wrong_name:"FABRIC_INTROSPECT_KEY",correct_name:"FABRIC_INTROSPECT_AUTH_KEY",phase:$phase,pre_repair_app_id:$pre_app,pre_repair_version:$pre_version,final_app_id:$final_app,final_version:$final_version,secret_put_completed:$secret_put,recreate_completed:$recreate,provider_secret_metadata:$secret_metadata},steps:{secret_put:{intent:$secret_put_intent,completed:$secret_put,attempts:$secret_put_attempts},delete:{intent:$delete_intent,completed:$delete_completed,attempts:$delete_attempts},deploy:{intent:$recreate_intent,completed:$recreate,attempts:$deploy_attempts}},secrets:{wrong_binding_name:"FABRIC_INTROSPECT_KEY",correct_binding_name:"FABRIC_INTROSPECT_AUTH_KEY",values:"excluded",hashes:"excluded"},secret_values:"excluded",secret_hashes:"excluded"}' > "$tmp" && chmod 600 "$tmp" && mv -f -- "$tmp" "$REPAIR_PROGRESS"
}

assert_historical_repair_authorization() {
  local artifact="$OOB_DIR/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
  safe_file "$RECOVERY_GUARD" || die 'introspect-auth repair requires the owner-only historical recovery guard'
  [ "$(tr -d '\r\n' < "$RECOVERY_GUARD")" = in-progress ] || die 'historical recovery guard is not armed'
  safe_file "$artifact" || die 'introspect-auth repair requires the owner-only historical RED artifact'
  jq -e --arg source "$HISTORICAL_RECOVERY_SOURCE_SHA" --arg version "$HISTORICAL_RECOVERY_VERSION" --arg app "$HISTORICAL_RECOVERY_APP_ID" --arg digest "$HISTORICAL_RECOVERY_DIGEST" '
    .schema_version == "evidence/v1" and .artifact_id == "fabricd-observability-key-bootstrap-introspect-recovery-failure" and
    .status == "RED" and .operation_mode == "introspect-recovery" and .source.repository == "corelink-runners" and .source.commit_sha == $source and
    .provider.worker == "corelink-fabricd" and .provider.expected_version == $version and .provider.app_id == $app and .provider.expected_image_digest == $digest and
    .phase == "container-recreate" and .secrets.FABRIC_INTROSPECT_KEY_put_completed == true and
    .secrets.FABRIC_OBSERVABILITY_KEY_put_completed == true and .gates.refreeze_result == "green" and .outcome == "RED" and .rerun_guard == "armed" and .secret_values == "excluded" and .secret_hashes == "excluded"
  ' "$artifact" >/dev/null || die 'historical RED artifact does not authorize introspect-auth repair'
}

repair_final_proof() {
  local label="$1" expected_app="$2" expected_version="$3" post post_version post_digest post_app
  post="$(snapshot "$label-provider" "$expected_app" true)" || die 'repair final provider capture failed'
  post_version="$(printf '%s\n' "$post" | cut -f1)"; post_digest="$(printf '%s\n' "$post" | cut -f2)"; post_app="$(printf '%s\n' "$post" | cut -f3)"
  [ "$post_app" = "$expected_app" ] || die 'repair final application id mismatch'
  [ "$post_version" = "$expected_version" ] && [ "$post_digest" = "$DIGEST" ] || die 'repair final provider tuple drift'
  assert_frozen "$post_version" || die 'repair final admission freeze verification failed'
  assert_fleet_quiet "$label-fleet" || die 'repair final fleet quiescence proof failed'
  PROBE_STATUS=''; probe_introspection "$label-introspection" "$new_introspect_header" "$TMP_DIR/$label-introspection.json" "$TMP_DIR/$label-introspection.status" || die 'repair final introspection transport failed'
  [ "$PROBE_STATUS" = 200 ] || die "repair final introspection rejected HTTP $PROBE_STATUS"
  jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant and (.max_concurrency | tonumber) >= 1' "$TMP_DIR/$label-introspection.json" >/dev/null || die 'repair final introspection schema verification failed'
  verify_observability_status "$label" || die 'repair final observability status verification failed'
  verify_health "$label" || die 'repair final health probe failed'
  FINAL_APP_ID="$post_app"; FINAL_FROZEN=1
}

# This is deliberately run before every repair mutation path.  It proves that
# the service is frozen, quiescent and healthy before the persisted intent is
# allowed to authorize a provider command.  A pre-put path must still exhibit
# the defect (the supplied replacement key is rejected); later resume stages
# prove the replacement key instead, because re-requiring a rejection after a
# completed immutable put would make a safe proof-only resume impossible.
repair_preflight() {
  local label="$1" expected_app="$2" auth_expectation="$3" current current_version current_digest current_app
  if [ -n "$expected_app" ]; then
    current="$(snapshot "$label-provider" "$expected_app" false)" || die 'repair provider preflight capture failed'
    current_version="$(printf '%s\n' "$current" | cut -f1)"; current_digest="$(printf '%s\n' "$current" | cut -f2)"; current_app="$(printf '%s\n' "$current" | cut -f3)"
    [ "$current_app" = "$expected_app" ] && [ "$current_version" = "$REPAIR_PRE_VERSION" ] && [ "$current_digest" = "$DIGEST" ] || die 'repair current provider tuple drift'
  else
    current="$(capture "$label-deployments" run_wrangler deployments list --name "$WORKER_NAME" --json)" || die 'repair absent-container deployment capture failed'
    current_version="$(jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$current")" || die 'repair absent-container deployment version missing'
    [ "$current_version" = "$REPAIR_PRE_VERSION" ] || die 'repair absent-container deployment version drift'
    log_event "$label provider_container=absent expected_digest=$DIGEST"
    assert_frozen "$current_version" || die 'repair absent-container admission is not frozen'
    assert_fleet_quiet "$label-fleet" || die 'repair absent-container fleet quiescence request failed'
    log_event "$label container_dependent_probes=deferred_until_deploy"
    return 0
  fi
  assert_frozen "$current_version" || die 'repair fabric admission is not frozen'
  assert_fleet_quiet "$label-fleet" || die 'repair fleet quiescence request failed'
  verify_observability_status "$label" || die 'repair observability status preflight failed'
  verify_health "$label" || die 'repair health preflight failed'
  PROBE_STATUS=''; probe_introspection "$label-introspection" "$new_introspect_header" "$TMP_DIR/$label-introspection.json" "$TMP_DIR/$label-introspection.status" || die 'repair new-key introspection transport failed'
  case "$auth_expectation:$PROBE_STATUS" in
    rejected:401|rejected:403) log_event "repair_root_cause_witness=new_key_auth_rejected_http:$PROBE_STATUS";;
    accepted:200) jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant and (.max_concurrency | tonumber) >= 1' "$TMP_DIR/$label-introspection.json" >/dev/null || die 'repair resumed introspection schema verification failed';;
    rejected:*) die "repair requires new-key root-cause witness HTTP 401 or 403, got $PROBE_STATUS";;
    *) die "repair resumed introspection must be HTTP 200, got $PROBE_STATUS";;
  esac
}

assert_single_repair_target() {
  local label="$1" expected_app="$2" containers named
  containers="$(capture "$label-containers" run_wrangler containers list --json)" || die 'repair target container capture failed'
  named="$(jq -er --arg n "$APP_NAME" '[.. | objects | select(.name? == $n)] | if length == 1 then .[0].id else error("target-name container is absent or ambiguous") end' "$containers")" || die 'repair target-name container is absent or ambiguous'
  [ "$named" = "$expected_app" ] || die 'repair target-name container does not match supplied current app'
}

repair_introspect_auth_main() {
  local current_app post post_app
  REPAIR_PRE_VERSION="$VERSION"; REPAIR_FINAL_VERSION="$VERSION"
  assert_historical_repair_authorization
  [ "$APP_ID" != "$HISTORICAL_RECOVERY_APP_ID" ] || die 'current repair app id must differ from the historical app id'
  assert_repair_secret_metadata || die 'repair provider secret metadata verification failed'
  assert_single_repair_target repair-preflight "$APP_ID"
  repair_preflight repair-preflight "$APP_ID" rejected
  current_app="$APP_ID"
  write_repair_progress preflight "$current_app" '' || die 'cannot write repair progress artifact'
  MUTATION_STARTED=1; REPAIR_PHASE='secret-put'; REPAIR_SECRET_PUT_INTENT=true; REPAIR_SECRET_PUT_ATTEMPTS=1
  write_repair_progress secret-put "$current_app" '' || die 'cannot checkpoint repair secret put intent'
  run_safe repair-introspect-auth-secret-put run_wrangler secret put FABRIC_INTROSPECT_AUTH_KEY --name "$WORKER_NAME" < "$NEW_INTROSPECT_KEY_FILE" || die 'correct introspection auth secret put failed'
  REPAIR_SECRET_PUT=true; REPAIR_SECRET_PUT_INTENT=false; REPAIR_PHASE='delete'; REPAIR_DELETE_INTENT=true; REPAIR_DELETE_ATTEMPTS=1
  write_repair_progress delete "$current_app" '' || die 'cannot checkpoint repair delete intent'
  run_safe repair-container-delete run_wrangler containers delete "$APP_ID" || die 'repair fabricd container delete failed'
  REPAIR_DELETE_COMPLETED=true; REPAIR_DELETE_INTENT=false; REPAIR_PHASE='deploy'; REPAIR_RECREATE_INTENT=true; REPAIR_DEPLOY_ATTEMPTS=1
  write_repair_progress deploy "$current_app" '' || die 'cannot checkpoint repair deploy intent'
  run_safe repair-immediate-recreate run_wrangler deploy --keep-vars --strict --containers-rollout=immediate || die 'repair immediate fabricd recreate failed'
  post="$(snapshot repair-post-recreate "$APP_ID" true)" || die 'repair post-recreate provider capture failed'
  post_app="$(printf '%s\n' "$post" | cut -f3)"; REPAIR_FINAL_VERSION="$(printf '%s\n' "$post" | cut -f1)"; [ "$post_app" != "$APP_ID" ] || die 'repair recreated fabricd application id was reused'
  REPAIR_RECREATE=true; REPAIR_RECREATE_INTENT=false; REPAIR_PHASE='final-proof'; write_repair_progress final-proof "$current_app" "$post_app" || die 'cannot update post-recreate repair progress artifact'
  REPAIR_PHASE='final-proof'; repair_final_proof repair-final "$post_app" "$REPAIR_FINAL_VERSION"
  LINEAGE_OLD_APP_ID="$current_app"; log_event "repair=PASS pre_repair_app_id=$current_app final_app_id=$FINAL_APP_ID"
}

resume_introspect_auth_repair_main() {
  local progress_phase progress_pre progress_final progress_pre_version progress_final_version progress_source failure_source progress_put progress_recreate progress_intent progress_attempts progress_delete_attempts progress_deploy_attempts containers old_count named_count discovered auth_expectation
  assert_historical_repair_authorization
  safe_file "$REPAIR_PROGRESS" || die 'repair resume requires owner-only 0600 progress artifact'
  safe_file "$REPAIR_FAILURE" || die 'repair resume requires owner-only 0600 repair failure artifact'
  [ ! -e "$REPAIR_COMPLETE" ] && [ ! -L "$REPAIR_COMPLETE" ] || die 'repair resume completion marker already exists'
  progress_phase="$(jq -er '.repair.phase' "$REPAIR_PROGRESS")" || die 'repair resume progress phase is missing'
  if [ "$progress_phase" = final-proof ]; then
    # The final-proof checkpoint is a completed mutation journal. Its provider
    # tuple is intentionally the pre-repair tuple; VERSION/APP_ID are the
    # current final deployment supplied by the operator. This branch performs
    # only read-only proofs and never enters the bounded mutation replay below.
    jq -e --arg commit "$COMMIT" --arg digest "$DIGEST" --arg final_app "$APP_ID" --arg final_version "$VERSION" '
      .schema_version == "evidence/v1" and .artifact_id == "fabricd-observability-key-bootstrap-introspect-auth-repair-progress" and .status == "RED" and .operation_mode == "introspect-auth-repair" and
      .source.repository == "corelink-runners" and (.source.commit_sha | type == "string" and test("^[a-f0-9]{40}$")) and .provider.worker == "corelink-fabricd" and .provider.expected_image_digest == $digest and .provider.app_id == .repair.pre_repair_app_id and
      .historical.source_commit == "37565619dae31a61f67a095daa0cd15b06386237" and .historical.version == "c38233a3-4ede-4803-8e1c-1a3b5ad4d667" and .historical.app_id == "a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7" and .historical.image_digest == "sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5" and
      .repair.phase == "final-proof" and .repair.wrong_name == "FABRIC_INTROSPECT_KEY" and .repair.correct_name == "FABRIC_INTROSPECT_AUTH_KEY" and .secrets.wrong_binding_name == "FABRIC_INTROSPECT_KEY" and .secrets.correct_binding_name == "FABRIC_INTROSPECT_AUTH_KEY" and .secrets.values == "excluded" and .secrets.hashes == "excluded" and .secret_values == "excluded" and .secret_hashes == "excluded" and
      (.repair.pre_repair_app_id | type == "string") and (.repair.pre_repair_app_id | length > 0) and (.repair.final_app_id == $final_app) and ($final_app != .repair.pre_repair_app_id) and
      (.provider.expected_version | type == "string") and ((.repair.pre_repair_version // .provider.expected_version) == .provider.expected_version) and
      ((.repair.final_version // $final_version) == $final_version) and
      (.repair.secret_put_completed // .steps.secret_put.completed) == true and (.repair.recreate_completed // .steps.deploy.completed) == true and
      .steps.secret_put.intent == false and .steps.secret_put.completed == true and .steps.secret_put.attempts == 1 and
      .steps.delete.intent == false and .steps.delete.completed == true and .steps.delete.attempts == 1 and
      .steps.deploy.intent == false and .steps.deploy.completed == true and .steps.deploy.attempts == 1
    ' "$REPAIR_PROGRESS" >/dev/null || die 'final-proof progress artifact does not bind the complete pre/final lineage'
    progress_pre="$(jq -er '.repair.pre_repair_app_id' "$REPAIR_PROGRESS")"
    progress_pre_version="$(jq -er '.repair.pre_repair_version // .provider.expected_version' "$REPAIR_PROGRESS")"
    progress_final="$(jq -er '.repair.final_app_id' "$REPAIR_PROGRESS")"
    progress_final_version="$(jq -er --arg current "$VERSION" '.repair.final_version // $current' "$REPAIR_PROGRESS")"
    progress_source="$(jq -er '.source.commit_sha' "$REPAIR_PROGRESS")"; failure_source="$(jq -er '.source.commit_sha' "$REPAIR_FAILURE")"
    [ "$failure_source" = "$progress_source" ] || die 'final-proof artifact and progress source lineage differs'
    if [ "$progress_source" != "$COMMIT" ]; then
      git -C "$ROOT" merge-base --is-ancestor "$progress_source" "$COMMIT" || die 'final-proof source commit is not an ancestor of expected commit'
    fi
    if jq -e '((.repair | type) == "object") and (.repair.pre_repair_app_id? != null) and (.repair.final_app_id? != null) and (.repair.pre_repair_version? != null) and (.repair.final_version? != null)' "$REPAIR_FAILURE" >/dev/null; then
      jq -e --arg digest "$DIGEST" --arg final_app "$APP_ID" --arg final_version "$VERSION" --arg pre_app "$progress_pre" --arg pre_version "$progress_pre_version" '
        .schema_version == "evidence/v1" and .artifact_id == "fabricd-observability-key-bootstrap-introspect-auth-repair-failure" and .status == "RED" and .operation_mode == "introspect-auth-repair" and
        .source.repository == "corelink-runners" and (.source.commit_sha | type == "string" and test("^[a-f0-9]{40}$")) and .provider.worker == "corelink-fabricd" and .provider.expected_version == $pre_version and .provider.expected_image_digest == $digest and .provider.app_id == $pre_app and
        .phase == "final-proof" and .rerun_guard == "armed" and .secret_values == "excluded" and .secret_hashes == "excluded" and
        .repair.pre_repair_app_id == $pre_app and .repair.final_app_id == $final_app and .repair.pre_repair_version == $pre_version and .repair.final_version == $final_version and
        .repair.intents.secret_put == false and .repair.intents.delete == false and .repair.intents.deploy == false and .repair.attempts.secret_put == 1 and .repair.attempts.delete == 1 and .repair.attempts.deploy == 1
      ' "$REPAIR_FAILURE" >/dev/null || die 'final-proof failure artifact does not bind the complete pre/final lineage'
    else
      jq -e --arg legacy "$LEGACY_REPAIR_PROGRESS_SOURCE_SHA" '.artifact_id == "fabricd-observability-key-bootstrap-introspect-auth-repair-failure" and .source.commit_sha == $legacy and .phase == "final-proof" and .status == "RED" and .rerun_guard == "armed" and .repair.wrong_name == "FABRIC_INTROSPECT_KEY" and .repair.correct_name == "FABRIC_INTROSPECT_AUTH_KEY" and .repair.attempts.secret_put == 1 and .repair.attempts.delete == 1 and .repair.attempts.deploy == 1 and .repair.intents.secret_put == false and .repair.intents.delete == false and .repair.intents.deploy == false' "$REPAIR_FAILURE" >/dev/null || die 'legacy final-proof failure artifact is outside the permitted compatibility lineage'
    fi
    [ "$progress_final" = "$APP_ID" ] && [ "$progress_final_version" = "$VERSION" ] || die 'final-proof current provider tuple does not match recorded final lineage'
    [ "$progress_pre" != "$HISTORICAL_RECOVERY_APP_ID" ] || die 'final-proof pre-repair app cannot be historical app'
    assert_repair_secret_metadata || die 'repair final-proof secret metadata verification failed'
    containers="$(capture repair-final-proof-containers run_wrangler containers list --json)" || die 'repair final-proof container capture failed'
    old_count="$(jq -er --arg id "$progress_pre" --arg n "$APP_NAME" '[.. | objects | select(.id? == $id and .name? == $n)] | length' "$containers")"
    named_count="$(jq -er --arg n "$APP_NAME" '[.. | objects | select(.name? == $n)] | length' "$containers")"
    [ "$old_count" = 0 ] && [ "$named_count" = 1 ] || die 'final-proof container lineage is absent or ambiguous'
    REPAIR_PRE_VERSION="$progress_pre_version"; REPAIR_FINAL_VERSION="$VERSION"; REPAIR_SECRET_PUT=true; REPAIR_RECREATE=true; REPAIR_RESUMED=true; REPAIR_PHASE='resume-final-proof'
    LINEAGE_OLD_APP_ID="$progress_pre"
    repair_final_proof repair-resume "$APP_ID" "$REPAIR_FINAL_VERSION"
    log_event "repair_resume=PASS pre_repair_app_id=$progress_pre final_app_id=$FINAL_APP_ID mutations_repeated=false"
    return 0
  fi
  jq -e --arg commit "$COMMIT" --arg version "$VERSION" --arg digest "$DIGEST" '
    .schema_version == "evidence/v1" and .artifact_id == "fabricd-observability-key-bootstrap-introspect-auth-repair-progress" and .status == "RED" and .operation_mode == "introspect-auth-repair" and
    .source.repository == "corelink-runners" and .source.commit_sha == $commit and .provider.worker == "corelink-fabricd" and .provider.expected_version == $version and .provider.expected_image_digest == $digest and .provider.app_id == .repair.pre_repair_app_id and
    .historical.source_commit == "37565619dae31a61f67a095daa0cd15b06386237" and .historical.version == "c38233a3-4ede-4803-8e1c-1a3b5ad4d667" and .historical.app_id == "a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7" and .historical.image_digest == "sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5" and
    .repair.wrong_name == "FABRIC_INTROSPECT_KEY" and .repair.correct_name == "FABRIC_INTROSPECT_AUTH_KEY" and .secrets.wrong_binding_name == "FABRIC_INTROSPECT_KEY" and .secrets.correct_binding_name == "FABRIC_INTROSPECT_AUTH_KEY" and .secrets.values == "excluded" and .secrets.hashes == "excluded" and .secret_values == "excluded" and .secret_hashes == "excluded" and
    (.repair.phase == "preflight" or .repair.phase == "secret-put" or .repair.phase == "delete" or .repair.phase == "deploy" or .repair.phase == "final-proof") and
    (.steps.secret_put.attempts | type == "number") and (.steps.delete.attempts | type == "number") and (.steps.deploy.attempts | type == "number") and
    (if .repair.phase == "preflight" then .steps.secret_put.completed == false and .steps.delete.completed == false and .steps.deploy.completed == false
     elif .repair.phase == "secret-put" then .steps.secret_put.intent == true and .steps.secret_put.completed == false and .steps.secret_put.attempts >= 1
     elif .repair.phase == "delete" then .steps.secret_put.completed == true and .steps.delete.intent == true and .steps.delete.completed == false and .steps.delete.attempts >= 1
     elif .repair.phase == "deploy" then .steps.secret_put.completed == true and .steps.delete.completed == true and .steps.deploy.intent == true and .steps.deploy.completed == false and .steps.deploy.attempts >= 1
     else .steps.secret_put.completed == true and .steps.delete.completed == true and .steps.deploy.completed == true and (.repair.final_app_id | type == "string") and (.repair.final_app_id | length > 0)
     end)
  ' "$REPAIR_PROGRESS" >/dev/null || die 'repair resume progress artifact does not bind the current and historical tuples'
  jq -e --arg commit "$COMMIT" --arg version "$VERSION" --arg digest "$DIGEST" '
    .schema_version == "evidence/v1" and .artifact_id == "fabricd-observability-key-bootstrap-introspect-auth-repair-failure" and .status == "RED" and .operation_mode == "introspect-auth-repair" and
    .source.repository == "corelink-runners" and .source.commit_sha == $commit and .provider.worker == "corelink-fabricd" and .provider.expected_version == $version and .provider.expected_image_digest == $digest and .rerun_guard == "armed" and .secret_values == "excluded" and .secret_hashes == "excluded"
  ' "$REPAIR_FAILURE" >/dev/null || die 'repair resume failure artifact does not bind current tuple'
  progress_pre="$(jq -er '.repair.pre_repair_app_id' "$REPAIR_PROGRESS")"; progress_pre_version="$(jq -er '.repair.pre_repair_version // .provider.expected_version' "$REPAIR_PROGRESS")"; progress_final="$(jq -er '.repair.final_app_id' "$REPAIR_PROGRESS")"; progress_put="$(jq -r '.steps.secret_put.completed' "$REPAIR_PROGRESS")"; progress_recreate="$(jq -r '.steps.deploy.completed' "$REPAIR_PROGRESS")"; progress_intent="$(jq -r '.steps.secret_put.intent' "$REPAIR_PROGRESS")"; progress_attempts="$(jq -er '.steps.secret_put.attempts' "$REPAIR_PROGRESS")"; progress_delete_attempts="$(jq -er '.steps.delete.attempts' "$REPAIR_PROGRESS")"; progress_deploy_attempts="$(jq -er '.steps.deploy.attempts' "$REPAIR_PROGRESS")"
  assert_repair_secret_metadata || die 'repair resume provider secret metadata verification failed'
  REPAIR_PRE_VERSION="$progress_pre_version"; REPAIR_FINAL_VERSION="$VERSION"; REPAIR_SECRET_PUT="$progress_put"; REPAIR_RECREATE="$progress_recreate"; REPAIR_SECRET_PUT_INTENT="$progress_intent"; REPAIR_SECRET_PUT_ATTEMPTS="$progress_attempts"; REPAIR_DELETE_ATTEMPTS="$progress_delete_attempts"; REPAIR_DEPLOY_ATTEMPTS="$progress_deploy_attempts"
  [ "$APP_ID" != "$HISTORICAL_RECOVERY_APP_ID" ] || die 'repair resume current app cannot be historical app'
  auth_expectation=rejected; [ "$progress_put" = true ] && auth_expectation=accepted
  containers="$(capture repair-resume-preflight-containers run_wrangler containers list --json)" || die 'repair resume container discrimination failed'
  old_count="$(jq -er --arg id "$progress_pre" --arg n "$APP_NAME" '[.. | objects | select(.id? == $id and .name? == $n)] | length' "$containers")"; named_count="$(jq -er --arg n "$APP_NAME" '[.. | objects | select(.name? == $n)] | length' "$containers")"
  [ "$named_count" -le 1 ] || die 'repair resume target-name containers are duplicate or ambiguous'
  if [ "$named_count" = 0 ]; then
    # The delete may have completed before its response was durable. There is
    # no container to authenticate through, so prove only worker-level state
    # and the frozen deployment binding before the bounded deploy replay.
    repair_preflight repair-resume-preflight '' "$auth_expectation"
  else
    repair_preflight repair-resume-preflight "$APP_ID" "$auth_expectation"
  fi
  if [ "$REPAIR_SECRET_PUT" != true ]; then
    [ "$REPAIR_SECRET_PUT_INTENT" = true ] && [ "$REPAIR_SECRET_PUT_ATTEMPTS" -lt 2 ] || die 'repair secret put is ambiguous and its idempotent replay cap is exhausted'
    REPAIR_PHASE='secret-put'; REPAIR_SECRET_PUT_ATTEMPTS=$((REPAIR_SECRET_PUT_ATTEMPTS + 1)); REPAIR_SECRET_PUT_INTENT=true
    write_repair_progress secret-put "$progress_pre" "$progress_final" || die 'cannot checkpoint repair secret replay intent'
    MUTATION_STARTED=1; run_safe repair-resume-introspect-auth-secret-put run_wrangler secret put FABRIC_INTROSPECT_AUTH_KEY --name "$WORKER_NAME" < "$NEW_INTROSPECT_KEY_FILE" || die 'idempotent repair secret replay failed'
    REPAIR_SECRET_PUT=true; REPAIR_SECRET_PUT_INTENT=false
  fi
  if [ "$REPAIR_RECREATE" != true ]; then
    containers="$(capture repair-resume-containers run_wrangler containers list --json)" || die 'repair resume container discrimination failed'
    old_count="$(jq -er --arg id "$progress_pre" --arg n "$APP_NAME" '[.. | objects | select(.id? == $id and .name? == $n)] | length' "$containers")"; named_count="$(jq -er --arg n "$APP_NAME" '[.. | objects | select(.name? == $n)] | length' "$containers")"
    if [ "$old_count" = 1 ] && [ "$named_count" = 1 ]; then
      [ "$APP_ID" = "$progress_pre" ] || die 'repair resume must pin the still-present pre-repair app'
      [ "$REPAIR_DELETE_ATTEMPTS" -lt 2 ] || die 'repair delete replay cap is exhausted'
      REPAIR_PHASE='delete'; REPAIR_DELETE_INTENT=true; REPAIR_DELETE_ATTEMPTS=$((REPAIR_DELETE_ATTEMPTS + 1)); write_repair_progress delete "$progress_pre" '' || die 'cannot checkpoint resume delete intent'
      MUTATION_STARTED=1; run_safe repair-resume-container-delete run_wrangler containers delete "$progress_pre" || die 'repair resume container delete failed'
      REPAIR_DELETE_COMPLETED=true; REPAIR_DELETE_INTENT=false
      REPAIR_PHASE='deploy'; REPAIR_RECREATE_INTENT=true; REPAIR_DEPLOY_ATTEMPTS=$((REPAIR_DEPLOY_ATTEMPTS + 1)); [ "$REPAIR_DEPLOY_ATTEMPTS" -le 2 ] || die 'repair deploy replay cap is exhausted'; write_repair_progress deploy "$progress_pre" '' || die 'cannot checkpoint resume deploy intent'
      run_safe repair-resume-immediate-recreate run_wrangler deploy --keep-vars --strict --containers-rollout=immediate || die 'repair resume immediate recreate failed'
      discovered="$(snapshot repair-resume-post-recreate "$progress_pre" true)" || die 'repair resume post-recreate discovery failed'; progress_final="$(printf '%s\n' "$discovered" | cut -f3)"; REPAIR_FINAL_VERSION="$(printf '%s\n' "$discovered" | cut -f1)"; [ "$progress_final" != "$progress_pre" ] || die 'repair resume recreated app id was reused'
      REPAIR_RECREATE=true; REPAIR_RECREATE_INTENT=false; REPAIR_PHASE='final-proof'; write_repair_progress final-proof "$progress_pre" "$progress_final" || die 'cannot checkpoint resume recreate result'
    elif [ "$old_count" = 0 ] && [ "$named_count" = 0 ]; then
      [ "$REPAIR_DEPLOY_ATTEMPTS" -lt 2 ] || die 'repair deploy replay cap is exhausted'
      REPAIR_PHASE='deploy'; REPAIR_RECREATE_INTENT=true; REPAIR_DEPLOY_ATTEMPTS=$((REPAIR_DEPLOY_ATTEMPTS + 1)); write_repair_progress deploy "$progress_pre" '' || die 'cannot checkpoint deploy retry intent'
      MUTATION_STARTED=1; run_safe repair-resume-immediate-recreate run_wrangler deploy --keep-vars --strict --containers-rollout=immediate || die 'repair resume immediate recreate failed'
      discovered="$(snapshot repair-resume-post-recreate "$progress_pre" true)" || die 'repair resume post-recreate discovery failed'; progress_final="$(printf '%s\n' "$discovered" | cut -f3)"; REPAIR_FINAL_VERSION="$(printf '%s\n' "$discovered" | cut -f1)"; [ "$progress_final" != "$progress_pre" ] || die 'repair resume recreated app id was reused'
      REPAIR_RECREATE=true; REPAIR_RECREATE_INTENT=false; REPAIR_PHASE='final-proof'; write_repair_progress final-proof "$progress_pre" "$progress_final" || die 'cannot checkpoint deploy retry result'
    elif [ "$old_count" = 0 ] && [ "$named_count" = 1 ]; then
      progress_final="$(jq -er --arg n "$APP_NAME" '[.. | objects | select(.name? == $n)] | .[0].id' "$containers")"; [ "$progress_final" != "$progress_pre" ] || die 'repair resume container state is ambiguous'
      [ "$APP_ID" = "$progress_final" ] || die 'repair resume must pin the discovered recreated app'
      discovered="$(snapshot repair-resume-discovered "$progress_final" true)" || die 'repair resume recreated app proof failed'; REPAIR_FINAL_VERSION="$(printf '%s\n' "$discovered" | cut -f1)"; REPAIR_RECREATE=true; REPAIR_RECREATE_INTENT=false; REPAIR_PHASE='final-proof'; write_repair_progress final-proof "$progress_pre" "$progress_final" || die 'cannot checkpoint discovered recreate'
    else die 'repair resume container state is absent, duplicate, or ambiguous'; fi
  fi
  [ "$APP_ID" = "$progress_final" ] && [ "$APP_ID" != "$progress_pre" ] || die 'repair resume current app id must equal the recorded post-recreate app id'
  REPAIR_RESUMED=true; REPAIR_PHASE='resume-final-proof'
  repair_final_proof repair-resume "$APP_ID" "$REPAIR_FINAL_VERSION"
  LINEAGE_OLD_APP_ID="$progress_pre"; log_event "repair_resume=PASS pre_repair_app_id=$progress_pre final_app_id=$FINAL_APP_ID mutations_repeated=false"
}

resume_recovery_main() {
  local artifact="$OOB_DIR/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
  local current current_version current_digest current_app_id
  local resumed_after resumed_after_version resumed_after_digest resumed_after_app_id
  local status_header status_json health_status health_rc
  safe_file "$RECOVERY_GUARD" || die 'resume requires owner-only 0600 recovery guard'
  [ ! -e "$RECOVERY_COMPLETE" ] && [ ! -L "$RECOVERY_COMPLETE" ] || die 'resume completion marker already exists'
  [ "$(tr -d '\r\n' < "$RECOVERY_GUARD")" = 'in-progress' ] || die 'resume recovery guard is not armed'
  safe_file "$artifact" || die 'resume requires owner-only 0600 durable RED artifact'
  jq -e --arg recovery_app "$RECOVERY_APP_ID" --arg digest "$DIGEST" --arg source_sha "$RECOVERY_SOURCE_SHA" --arg recovery_version "$RECOVERY_VERSION" '
    .schema_version == "evidence/v1" and
    .artifact_id == "fabricd-observability-key-bootstrap-introspect-recovery-failure" and
    .status == "RED" and .operation_mode == "introspect-recovery" and
    .source.repository == "corelink-runners" and .source.commit_sha == $source_sha and
    .phase == "container-recreate" and
    .secrets.FABRIC_INTROSPECT_AUTH_KEY_put_completed == true and
    .secrets.FABRIC_OBSERVABILITY_KEY_put_completed == true and
    .gates.refreeze_result == "green" and .outcome == "RED" and
    .rerun_guard == "armed" and .provider.worker == "corelink-fabricd" and
    .provider.expected_version == $recovery_version and .provider.app_id == $recovery_app and
    .provider.expected_image_digest == $digest
  ' "$artifact" >/dev/null || die 'durable RED artifact does not authorize resume'

  current="$(snapshot resume-current "$APP_ID" true)" || die 'resume provider capture failed'
  current_version="$(printf '%s\n' "$current" | cut -f1)"
  current_digest="$(printf '%s\n' "$current" | cut -f2)"
  current_app_id="$(printf '%s\n' "$current" | cut -f3)"
  [ "$current_app_id" = "$APP_ID" ] || die 'resume current fabricd application id mismatch'
  [ "$current_version" = "$VERSION" ] || die 'resume provider deployment version drift'
  [ "$current_digest" = "$DIGEST" ] || die 'resume provider container digest drift'
  assert_frozen "$current_version" || die 'resume fabric admission is not frozen'
  assert_fleet_quiet resume-preflight || die 'resume fleet quiescence request failed'

  RECOVERY_PHASE='resume-final-proof'
  PROBE_STATUS=''
  probe_introspection resume-introspection "$new_introspect_header" "$TMP_DIR/resume-introspect.json" "$TMP_DIR/resume-introspect.status" || die 'resume introspection transport failed'
  [ "$PROBE_STATUS" = 200 ] || die "resume introspection rejected HTTP $PROBE_STATUS"
  jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant and (.max_concurrency | tonumber) >= 1' "$TMP_DIR/resume-introspect.json" >/dev/null || die 'resume introspection schema verification failed'
  status_header="$TMP_DIR/resume-status.header"
  printf 'X-Corelink-Internal-Auth: ' > "$status_header"; tr -d '\r\n' < "$KEY_FILE" >> "$status_header"; printf '\n' >> "$status_header"; chmod 600 "$status_header"
  status_json="$TMP_DIR/resume-status.json"; : > "$status_json"; chmod 600 "$status_json"
  "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$status_header" "$STATUS_URL" > "$status_json" 2> "$EVIDENCE_DIR/resume-status.stderr" || die 'resume observability status request failed'; scrub "$EVIDENCE_DIR/resume-status.stderr"
  jq -e '(.version | strings | length > 0) and ((.uptime_ms | tonumber) >= 0) and (.ledger_cross_instance_safe | type == "boolean") and ((.num_shards | tonumber) >= 1)' "$status_json" >/dev/null || die 'resume observability status schema verification failed'
  set +e
  health_status="$("$CURL_BIN" --silent --show-error --connect-timeout 10 --max-time 30 --output /dev/null --write-out '%{http_code}' "$HEALTH_URL" 2> "$EVIDENCE_DIR/resume-health.stderr")"
  health_rc=$?
  set -e
  scrub "$EVIDENCE_DIR/resume-health.stderr"
  [ "$health_rc" = 0 ] && [ "$health_status" = 200 ] || die 'resume health probe failed'
  resumed_after="$(snapshot resume-final "$APP_ID" true)" || die 'resume final provider capture failed'
  resumed_after_version="$(printf '%s\n' "$resumed_after" | cut -f1)"
  resumed_after_digest="$(printf '%s\n' "$resumed_after" | cut -f2)"
  resumed_after_app_id="$(printf '%s\n' "$resumed_after" | cut -f3)"
  [ "$resumed_after" = "$current" ] || die 'resume provider identity changed during proof'
  [ "$resumed_after_version" = "$VERSION" ] && [ "$resumed_after_digest" = "$DIGEST" ] && [ "$resumed_after_app_id" = "$current_app_id" ] || die 'resume final provider pins drifted'
  assert_frozen "$resumed_after_version" || die 'resume final admission freeze verification failed'
  assert_fleet_quiet resume-final || die 'resume final fleet quiescence proof failed'
  FINAL_APP_ID="$current_app_id"
  LINEAGE_OLD_APP_ID="$RECOVERY_APP_ID"
  RESUMED_RECOVERY=true
  FINAL_FROZEN=1
  log_event "resume=PASS old_app_id=$RECOVERY_APP_ID new_app_id=$FINAL_APP_ID version=$current_version digest=$current_digest health=200"
}

if [ "$REPAIR_INTROSPECT_AUTH" = 1 ]; then
  repair_introspect_auth_main
  exit 0
fi
if [ "$RESUME_INTROSPECT_AUTH_REPAIR" = 1 ]; then
  resume_introspect_auth_repair_main
  exit 0
fi
if [ "$RESUME_RECOVERY" = 1 ]; then
  resume_recovery_main
  exit 0
fi

baseline="$(snapshot baseline "$APP_ID" false)" || die 'provider baseline capture failed'; baseline_version="${baseline%%$'\t'*}"; baseline_rest="${baseline#*$'\t'}"; baseline_digest="${baseline_rest%%$'\t'*}"
[ "$baseline_version" = "$VERSION" ] || die 'provider deployment version drift'; [ "$baseline_digest" = "$DIGEST" ] || die 'provider container image digest drift'
assert_frozen "$baseline_version" || die 'fabric admission is not frozen'

assert_fleet_quiet quiescence || die 'fleet quiescence request failed'

introspect_body="$TMP_DIR/introspect.json"
PROBE_STATUS=''
probe_introspection introspect-preflight "$introspect_header" "$introspect_body" "$TMP_DIR/introspect.status" || die 'introspection transport failed'
introspect_status="$PROBE_STATUS"
case "$introspect_status" in
  200)
    jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant' "$introspect_body" >/dev/null || die 'introspection proof failed'
    log_event 'quiescence=GREEN fleet_busy=0 fleet_unverifiable=0 introspection=valid'
    ;;
  401|403)
    [ "$RECOVER_INTROSPECT" = 1 ] || die "introspection auth rejection HTTP $introspect_status"
    log_event "recovery_phase=preflight old_introspection=auth_rejected_http:$introspect_status"
    ;;
  *) die "introspection preflight rejected HTTP $introspect_status";;
esac

stability_sample_1_at="$(date -u +%FT%H:%M:%SZ)"; stability_1="$(snapshot stability-sample-1)" || die 'provider stability sample 1 failed'; log_event "stability_sample_1_at=$stability_sample_1_at value=$stability_1"
if [ "$STABILITY_SECS" -gt 0 ]; then sleep "$STABILITY_SECS"; fi
stability_sample_2_at="$(date -u +%FT%H:%M:%SZ)"; stability_2="$(snapshot stability-sample-2)" || die 'provider stability sample 2 failed'; log_event "stability_sample_2_at=$stability_sample_2_at value=$stability_2"
[ "$stability_1" = "$baseline" ] && [ "$stability_2" = "$baseline" ] && [ "$stability_1" = "$stability_2" ] || die 'provider version or digest changed during stability window'
log_event "provider_stable=GREEN version=$baseline_version digest=$baseline_digest seconds=$STABILITY_SECS"
RECOVERY_PHASE='pre-mutation-fleet'
assert_fleet_quiet pre_mutation || die 'fleet became busy after stability window'

if [ "$RECOVER_INTROSPECT" = 1 ]; then
  printf '%s\n' 'in-progress' > "$RECOVERY_GUARD.tmp"; chmod 600 "$RECOVERY_GUARD.tmp"
  mv -f -- "$RECOVERY_GUARD.tmp" "$RECOVERY_GUARD" || die 'cannot arm recovery rerun guard'
  RECOVERY_PHASE='mutation-guard-armed'
  log_event 'recovery_phase=mutation_guard_armed'
fi
MUTATION_STARTED=1
if [ "$RECOVER_INTROSPECT" = 1 ]; then
  RECOVERY_PHASE='introspect-secret-put'
  run_safe introspect-secret-put run_wrangler secret put FABRIC_INTROSPECT_AUTH_KEY --name "$WORKER_NAME" < "$NEW_INTROSPECT_KEY_FILE" || die 'introspection secret put failed'
  RECOVERY_INTROSPECT_SECRET_PUT=true
  RECOVERY_PHASE='observability-secret-put'
  run_safe observability-secret-put run_wrangler secret put FABRIC_OBSERVABILITY_KEY --name "$WORKER_NAME" < "$KEY_FILE" || die 'observability secret put failed'
  RECOVERY_OBSERVABILITY_SECRET_PUT=true
else
  run_safe secret-put run_wrangler secret put FABRIC_OBSERVABILITY_KEY --name "$WORKER_NAME" < "$KEY_FILE" || die 'secret put failed'
fi
RECOVERY_PHASE='container-recreate'
run_safe container-delete run_wrangler containers delete "$APP_ID" || die 'fabricd container delete failed'
run_safe immediate-recreate run_wrangler deploy --keep-vars --strict --containers-rollout=immediate || die 'immediate fabricd recreate failed'
post="$(snapshot post-recreate "$APP_ID" true)" || die 'post-recreate provider capture failed'; post_version="${post%%$'\t'*}"; post_rest="${post#*$'\t'}"; post_digest="${post_rest%%$'\t'*}"; post_app_id="${post_rest#*$'\t'}"; FINAL_APP_ID="$post_app_id"; [ "$post_app_id" != "$APP_ID" ] || die 'recreated fabricd application id was reused'; [ "$post_digest" = "$DIGEST" ] || die 'recreated fabricd image digest changed'; assert_frozen "$post_version" || die 'admission freeze was not preserved after recreate'
status_header="$TMP_DIR/status.header"; printf 'X-Corelink-Internal-Auth: ' > "$status_header"; tr -d '\r\n' < "$KEY_FILE" >> "$status_header"; printf '\n' >> "$status_header"; chmod 600 "$status_header"
status_json="$TMP_DIR/status.json"; : > "$status_json"; chmod 600 "$status_json"; "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$status_header" "$STATUS_URL" > "$status_json" 2>"$EVIDENCE_DIR/status.stderr" || die 'status verification failed'; scrub "$EVIDENCE_DIR/status.stderr"
jq -e '(.version | strings | length > 0) and ((.uptime_ms | tonumber) >= 0) and (.ledger_cross_instance_safe | type == "boolean") and ((.num_shards | tonumber) >= 1)' "$status_json" >/dev/null || die 'status schema verification failed'; log_event "recovery_phase=final_proof observability_status=valid version=$post_version"
if [ "$RECOVER_INTROSPECT" = 1 ]; then
  RECOVERY_PHASE='final-proof'
  new_introspect_body="$TMP_DIR/new-introspect.json"
  PROBE_STATUS=''
  probe_introspection introspect-final "$new_introspect_header" "$new_introspect_body" "$TMP_DIR/new-introspect.status" || die 'new introspection transport failed'
  new_introspect_status="$PROBE_STATUS"
  [ "$new_introspect_status" = 200 ] || die "new introspection proof rejected HTTP $new_introspect_status"
  jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant and (.max_concurrency | tonumber) >= 1' "$new_introspect_body" >/dev/null || die 'new introspection schema verification failed'
  log_event 'recovery_phase=final_proof introspection_status=valid pat_schema=valid'
  assert_fleet_quiet recovery-final || die 'final fleet quiescence proof failed'
fi
assert_frozen "$post_version" || die 'final admission freeze verification failed'; FINAL_FROZEN=1; log_event 'outcome=PASS admission_paused=1 digest_unchanged=true'
