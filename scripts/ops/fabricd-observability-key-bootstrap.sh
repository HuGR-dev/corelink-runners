#!/usr/bin/env bash
# CoreLink-only bootstrap for a missing local FABRIC_OBSERVABILITY_KEY.
# Default is inert. Execute requires explicit pins and the exact acknowledgement.
set -Eeuo pipefail
set +x
umask 077

readonly ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908'
readonly WORKER_NAME='corelink-fabricd' APP_NAME='corelink-fabricd-fabricdcontainer'
readonly CONFIG_REL='deploy/cloudflare-fabricd/wrangler.jsonc'
MODE=plan ACK_ARG='' ROOT='' COMMIT='' VERSION='' APP_ID='' DIGEST=''
OOB_DIR="${CORELINK_OOB_DIR:-$HOME/.corelink/rotation-b2-20260908}" KEY_FILE=''
FLEET_KEY_FILE='' INTROSPECT_KEY_FILE='' PAT_FILE='' TENANT_ID='' EVIDENCE_FILE=''
STATUS_URL='https://corelink-fabricd.gmhelmold.workers.dev/internal/v1/status'
FLEET_URL='https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy'
INTROSPECT_URL='https://corelink-api.humangr.com/internal/v1/auth/introspect'
STABILITY_SECS=120 MOCK_WRANGLER='' CURL_BIN=curl TMP_DIR='' LOCK_DIR=''
MUTATION_STARTED=0 FINAL_FROZEN=0

die() { printf 'REFUSED: %s\n' "$*" >&2; exit 2; }
usage() { sed -n '1,12p' "$0"; printf '\nPlan is inert. Execute requires --execute, exact --ack, and all provider pins.\n' >&2; }
while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:?}"; shift 2;; --execute) MODE=execute; shift;; --ack) ACK_ARG="${2:?}"; shift 2;;
    --repo-root) ROOT="${2:?}"; shift 2;; --expected-commit) COMMIT="${2:?}"; shift 2;;
    --expected-version) VERSION="${2:?}"; shift 2;; --fabricd-app-id) APP_ID="${2:?}"; shift 2;;
    --expected-image-digest) DIGEST="${2:?}"; shift 2;; --oob-dir) OOB_DIR="${2:?}"; shift 2;;
    --key-file) KEY_FILE="${2:?}"; shift 2;; --fleet-key-file) FLEET_KEY_FILE="${2:?}"; shift 2;;
    --introspect-key-file) INTROSPECT_KEY_FILE="${2:?}"; shift 2;; --introspect-pat-file) PAT_FILE="${2:?}"; shift 2;;
    --tenant-id) TENANT_ID="${2:?}"; shift 2;; --evidence-file) EVIDENCE_FILE="${2:?}"; shift 2;;
    --status-url) STATUS_URL="${2:?}"; shift 2;; --fleet-url) FLEET_URL="${2:?}"; shift 2;;
    --introspect-url) INTROSPECT_URL="${2:?}"; shift 2;; --stability-seconds) STABILITY_SECS="${2:?}"; shift 2;;
    --mock-wrangler) MOCK_WRANGLER="${2:?}"; shift 2;; --curl-bin) CURL_BIN="${2:?}"; shift 2;;
    --help|-h) usage; exit 0;; *) die "unknown argument $1";;
  esac
done
case "$MODE" in
  plan) printf '%s\n' 'PLAN ONLY: no network, file read, key generation, or provider mutation.'; exit 0;;
  execute|mock);; *) die 'mode must be plan, execute, or mock';;
esac
[ "$MODE" != execute ] || [ "$ACK_ARG" = "$ACK" ] || die 'exact live acknowledgement required'
[ "$MODE" != mock ] || [ -x "$MOCK_WRANGLER" ] || die 'mock mode requires --mock-wrangler'
[[ "$STABILITY_SECS" =~ ^[0-9]+$ ]] || die 'stability seconds must be a nonnegative integer'
if [ "$MODE" = mock ]; then :; elif [ "$STABILITY_SECS" = 120 ]; then :; else die 'live stability window must be exactly 120 seconds'; fi
[ -n "$ROOT" ] && [ -n "$COMMIT" ] && [ -n "$VERSION" ] && [ -n "$APP_ID" ] && [ -n "$DIGEST" ] || die 'missing repository/provider pins'
[[ "$COMMIT" =~ ^[a-f0-9]{40}$ ]] || die 'expected commit must be a full SHA-1'
[[ "$DIGEST" =~ ^sha256:[a-f0-9]{64}$ ]] || die 'expected image digest must be sha256:<64 hex>'
[ -d "$ROOT" ] || die 'repository root is not a directory'
command -v "$CURL_BIN" >/dev/null 2>&1 || die 'curl is unavailable'
EVIDENCE_FILE="${EVIDENCE_FILE:-$ROOT/docs/plan/evidence/fabricd-observability-key-bootstrap.json}"
FLEET_KEY_FILE="${FLEET_KEY_FILE:-$OOB_DIR/fleet-busy-read-key}"
INTROSPECT_KEY_FILE="${INTROSPECT_KEY_FILE:-$OOB_DIR/fabric-introspect-key}"
PAT_FILE="${PAT_FILE:-$OOB_DIR/corelink-canary-tenant-pat}"
[ -n "$TENANT_ID" ] || die 'tenant id is required for introspection proof'

file_mode() { case "$(uname -s)" in Darwin) stat -f '%OLp' "$1";; Linux) stat -c '%a' "$1";; *) return 1;; esac; }
file_uid() { case "$(uname -s)" in Darwin) stat -f '%u' "$1";; Linux) stat -c '%u' "$1";; *) return 1;; esac; }
safe_dir() { [ -d "$1" ] && [ ! -L "$1" ] && [ "$(file_mode "$1")" = 700 ] && [ "$(file_uid "$1")" = "$(id -u)" ]; }
safe_file() { [ -f "$1" ] && [ ! -L "$1" ] && [ "$(file_mode "$1")" = 600 ] && [ "$(file_uid "$1")" = "$(id -u)" ] && [ -s "$1" ]; }
single_line_file() { safe_file "$1" || return 1; [ "$(tr -cd '\r' < "$1" | wc -c | tr -d ' ')" = 0 ] || return 1; [ "$(tr -cd '\n' < "$1" | wc -c | tr -d ' ')" -le 1 ]; }
mkdir_private() { local d="$1"; if [ -e "$d" ] || [ -L "$d" ]; then safe_dir "$d" || die "directory must be owner-only 0700: $d"; else mkdir -p "$d"; chmod 700 "$d"; safe_dir "$d" || die "cannot secure directory: $d"; fi; }
mkdir_private "$OOB_DIR"; mkdir_private "$(dirname -- "$EVIDENCE_FILE")"

run_wrangler() {
  if [ "$MODE" = mock ]; then "$MOCK_WRANGLER" "$@"; return; fi
  local token; token="$(npx --no-install wrangler auth token --json | jq -er '.token // .access_token // .')" || die 'wrangler OAuth unavailable'
  [[ "$token" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]] || die 'invalid wrangler OAuth token'
  CLOUDFLARE_API_TOKEN="$token" npx --no-install wrangler "$@"
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
[ -z "$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)" ] || die 'repository worktree is not clean'
grep -Fq "$DIGEST" "$config" || die 'expected immutable digest absent from canonical config'
LOCK_DIR="$OOB_DIR/.fabricd-observability-key-bootstrap.lock"; mkdir "$LOCK_DIR" 2>/dev/null || die 'another bootstrap holds the local lock'; chmod 700 "$LOCK_DIR"; log_event 'lock=acquired'

cleanup() {
  local rc=$?; trap - EXIT
  if [ "$MUTATION_STARTED" = 1 ] && [ "$FINAL_FROZEN" != 1 ]; then
    if run_safe refreeze run_wrangler deploy --config "$config" --keep-vars --strict --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate; then log_event 'refreeze=GREEN admission_paused=1'; else log_event 'refreeze=RED escalation_required'; rc=1; fi
  fi
  rmdir "$LOCK_DIR" 2>/dev/null || true
  if [ "$rc" = 0 ]; then
    stable_events="$OOB_DIR/fabricd-observability-key-bootstrap-$(date -u +%Y%m%dT%H%M%SZ).events.log"
    cp "$EVENT_LOG" "$stable_events" && chmod 600 "$stable_events" && safe_file "$stable_events" || rc=1
    jq -n --arg commit "$COMMIT" --arg version "$VERSION" --arg digest "$DIGEST" --arg app "$APP_ID" --arg key_path "$KEY_FILE" --arg events "$stable_events" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap",status:"PASS",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$app},secret:{name:"FABRIC_OBSERVABILITY_KEY",value:"excluded",local_path:$key_path,mode:"0600"},gates:{quiescence:"GREEN",admission_paused:true,status_endpoint:"200_valid_json",refreeze:"GREEN"},logs:{events:$events,secrets:"excluded",mode:"0600"}}' > "$EVIDENCE_FILE" && chmod 644 "$EVIDENCE_FILE" || rc=1
  else log_event 'outcome=FAILED'; fi
  find "$TMP_DIR" -type f -exec rm -f -- {} + 2>/dev/null || true; rmdir "$EVIDENCE_DIR" "$TMP_DIR" 2>/dev/null || true; exit "$rc"
}
trap cleanup EXIT

snapshot() {
  local label="$1" deploys info version image
  deploys="$(capture "$label-deployments" run_wrangler deployments list --name "$WORKER_NAME" --json)" || return 1
  info="$(capture "$label-container" run_wrangler containers info "$APP_ID" --json)" || return 1
  jq -e --arg n "$APP_NAME" '.name == $n' "$info" >/dev/null || { log_event "$label app_name=RED"; return 1; }
  version="$(jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$deploys")" || return 1
  image="$(jq -er '[.. | strings | scan("sha256:[0-9a-f]{64}")] | unique | if length == 1 then .[0] else error("ambiguous image digest") end' "$info")" || return 1
  printf '%s\t%s\n' "$version" "$image"
}
assert_frozen() {
  local version="$1" vars; vars="$(capture frozen-vars run_wrangler versions view "$version" --name "$WORKER_NAME" --json)" || return 1
  jq -e '[.. | objects | select(.name? == "FABRIC_ADMISSION_PAUSED") | (.text? // .value? // "")] | length == 1 and .[0] == "1"' "$vars" >/dev/null || { log_event 'admission_paused=RED'; return 1; }; log_event 'admission_paused=GREEN'
}

fleet_header="$TMP_DIR/fleet.header"; introspect_header="$TMP_DIR/introspect.header"
single_line_file "$FLEET_KEY_FILE" || die 'fleet key must be owner-only regular 0600 single-line file'
single_line_file "$INTROSPECT_KEY_FILE" || die 'introspect key must be owner-only regular 0600 single-line file'
single_line_file "$PAT_FILE" || die 'introspection PAT must be owner-only regular 0600 single-line file'
printf 'X-Corelink-Internal-Auth: ' > "$fleet_header"; tr -d '\r\n' < "$FLEET_KEY_FILE" >> "$fleet_header"; printf '\n' >> "$fleet_header"; chmod 600 "$fleet_header"
printf 'X-Corelink-Internal-Auth: ' > "$introspect_header"; tr -d '\r\n' < "$INTROSPECT_KEY_FILE" >> "$introspect_header"; printf '\n' >> "$introspect_header"; chmod 600 "$introspect_header"

if [ -n "$KEY_FILE" ]; then
  if [ -e "$KEY_FILE" ] || [ -L "$KEY_FILE" ]; then safe_file "$KEY_FILE" || die 'provided key must be owner-only regular 0600 single-line file'; else key_parent="$(dirname -- "$KEY_FILE")"; safe_dir "$key_parent" || die 'generated key parent must be owner-only 0700'; tmp_key="$(mktemp "$key_parent/.obs-key.XXXXXXXX")"; chmod 600 "$tmp_key"; openssl rand -base64 48 | tr -d '\n' > "$tmp_key"; chmod 600 "$tmp_key"; mv -f -- "$tmp_key" "$KEY_FILE"; fi
else
  KEY_FILE="$OOB_DIR/fabric-observability-key-bootstrap.b64"; [ ! -e "$KEY_FILE" ] && [ ! -L "$KEY_FILE" ] || die 'default generated key already exists; provide a new path'; tmp_key="$(mktemp "$OOB_DIR/.obs-key.XXXXXXXX")"; chmod 600 "$tmp_key"; openssl rand -base64 48 | tr -d '\n' > "$tmp_key"; chmod 600 "$tmp_key"; mv -f -- "$tmp_key" "$KEY_FILE"
fi
single_line_file "$KEY_FILE" || die 'new observability key must be owner-only regular 0600 single-line file'; log_event 'key=ready value=excluded mode=0600'

baseline="$(snapshot baseline)" || die 'provider baseline capture failed'; baseline_version="${baseline%%$'\t'*}"; baseline_digest="${baseline#*$'\t'}"
[ "$baseline_version" = "$VERSION" ] || die 'provider deployment version drift'; [ "$baseline_digest" = "$DIGEST" ] || die 'provider container image digest drift'
assert_frozen "$baseline_version" || die 'fabric admission is not frozen'

fleet_json="$TMP_DIR/fleet.json"; : > "$fleet_json"; chmod 600 "$fleet_json"
set +e; "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$fleet_header" "$FLEET_URL" > "$fleet_json" 2>"$EVIDENCE_DIR/fleet.stderr"; curl_rc=$?; set -e; scrub "$EVIDENCE_DIR/fleet.stderr"; [ "$curl_rc" = 0 ] || die 'fleet quiescence request failed'
jq -e '((.busy // 0) | tonumber) == 0 and ((.unverifiable // 0) | tonumber) == 0' "$fleet_json" >/dev/null || die 'fleet is busy or unverifiable'
introspect_body="$TMP_DIR/introspect.json"; : > "$introspect_body"; chmod 600 "$introspect_body"
jq -nc --rawfile token "$PAT_FILE" '{token:($token|sub("\\n$";""))}' | "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$introspect_header" --header 'content-type: application/json' --data-binary @- "$INTROSPECT_URL" > "$introspect_body" 2>"$EVIDENCE_DIR/introspect.stderr" || die 'introspection quiescence request failed'
scrub "$EVIDENCE_DIR/introspect.stderr"; jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant' "$introspect_body" >/dev/null || die 'introspection proof failed'; log_event 'quiescence=GREEN fleet_busy=0 fleet_unverifiable=0 introspection=valid'

stability_sample_1_at="$(date -u +%FT%H:%M:%SZ)"; stability_1="$(snapshot stability-sample-1)" || die 'provider stability sample 1 failed'; log_event "stability_sample_1_at=$stability_sample_1_at value=$stability_1"
if [ "$STABILITY_SECS" -gt 0 ]; then sleep "$STABILITY_SECS"; fi
stability_sample_2_at="$(date -u +%FT%H:%M:%SZ)"; stability_2="$(snapshot stability-sample-2)" || die 'provider stability sample 2 failed'; log_event "stability_sample_2_at=$stability_sample_2_at value=$stability_2"
[ "$stability_1" = "$baseline" ] && [ "$stability_2" = "$baseline" ] && [ "$stability_1" = "$stability_2" ] || die 'provider version or digest changed during stability window'
log_event "provider_stable=GREEN version=$baseline_version digest=$baseline_digest seconds=$STABILITY_SECS"

MUTATION_STARTED=1
run_safe secret-put run_wrangler secret put FABRIC_OBSERVABILITY_KEY --name "$WORKER_NAME" < "$KEY_FILE" || die 'secret put failed'
run_safe container-delete run_wrangler containers delete "$APP_ID" || die 'fabricd container delete failed'
run_safe immediate-recreate run_wrangler deploy --config "$config" --keep-vars --strict --containers-rollout=immediate || die 'immediate fabricd recreate failed'
post="$(snapshot post-recreate)" || die 'post-recreate provider capture failed'; post_version="${post%%$'\t'*}"; post_digest="${post#*$'\t'}"; [ "$post_digest" = "$DIGEST" ] || die 'recreated fabricd image digest changed'; assert_frozen "$post_version" || die 'admission freeze was not preserved after recreate'
status_header="$TMP_DIR/status.header"; printf 'X-Corelink-Internal-Auth: ' > "$status_header"; tr -d '\r\n' < "$KEY_FILE" >> "$status_header"; printf '\n' >> "$status_header"; chmod 600 "$status_header"
status_json="$TMP_DIR/status.json"; : > "$status_json"; chmod 600 "$status_json"; "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$status_header" "$STATUS_URL" > "$status_json" 2>"$EVIDENCE_DIR/status.stderr" || die 'status verification failed'; scrub "$EVIDENCE_DIR/status.stderr"
jq -e '(.version | strings | length > 0) and ((.uptime_ms | tonumber) >= 0) and (.ledger_cross_instance_safe | type == "boolean") and ((.num_shards | tonumber) >= 1)' "$status_json" >/dev/null || die 'status schema verification failed'; log_event "status=GREEN endpoint=/internal/v1/status version=$post_version"
assert_frozen "$post_version" || die 'final admission freeze verification failed'; FINAL_FROZEN=1; log_event 'outcome=PASS admission_paused=1 digest_unchanged=true'
