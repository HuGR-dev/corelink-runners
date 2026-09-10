#!/usr/bin/env bash
# Bounded forward-only rotation for the first-party repository webhook secret.
# Default is an inert plan. Live mode is owner-gated and never prints a secret.
set -Eeuo pipefail
IFS=$'\n\t'; set +x; umask 077
readonly ACK='ACK-CORELINK-REPO-WEBHOOK-ROTATION-LIVE-20260909'
readonly WORKER_NAME='corelink-spawn-worker'
readonly HOOK_REPO='HuGR-Labs/corelink-runners'
readonly HOOK_ID='675536825'
MODE=plan ACK_OK=0 ROOT='' COMMIT='' NEW_SECRET_FILE='' FLEET_KEY_FILE='' EVIDENCE_FILE=''
WORKER_URL='https://corelink-spawn-worker.gmhelmold.workers.dev' WRANGLER_BIN=''
TMP_DIR='' MUTATION_STARTED=0 FINAL_FROZEN=0

die() { printf 'REPO WEBHOOK ROTATION REFUSED: %s\n' "$*" >&2; exit 2; }
usage() {
  sed -n '1,12p' "$0"
  printf '\nPlan is inert. Live mode requires the exact acknowledgement and pins.\n' >&2
}
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) [[ $# -ge 2 ]] || die '--mode requires plan or live'; MODE=$2; shift 2 ;;
    --ack-destructive) [[ $# -ge 2 && "$2" == "$ACK" ]] || die 'destructive acknowledgement mismatch'; ACK_OK=1; shift 2 ;;
    --repo-root) [[ $# -ge 2 ]] || die '--repo-root requires a path'; ROOT=$2; shift 2 ;;
    --expected-commit) [[ $# -ge 2 ]] || die '--expected-commit requires a full SHA'; COMMIT=$2; shift 2 ;;
    --new-secret-file) [[ $# -ge 2 ]] || die '--new-secret-file requires a path'; NEW_SECRET_FILE=$2; shift 2 ;;
    --fleet-key-file) [[ $# -ge 2 ]] || die '--fleet-key-file requires a path'; FLEET_KEY_FILE=$2; shift 2 ;;
    --worker-url) [[ $# -ge 2 ]] || die '--worker-url requires a URL'; WORKER_URL=${2%/}; shift 2 ;;
    --evidence-file) [[ $# -ge 2 ]] || die '--evidence-file requires a path'; EVIDENCE_FILE=$2; shift 2 ;;
    --wrangler-bin) [[ $# -ge 2 ]] || die '--wrangler-bin requires a path'; WRANGLER_BIN=$2; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
[[ "$MODE" == plan || "$MODE" == live ]] || die 'mode must be plan or live'
[[ "$MODE" == live ]] || { printf '%s\n' 'PLAN ONLY: no secret reads, network calls, or provider mutations.'; exit 0; }
[[ "$ACK_OK" == 1 ]] || die 'live mode requires --ack-destructive with the exact acknowledgement'
[[ -n "$ROOT" && "$COMMIT" =~ ^[0-9a-f]{40}$ ]] || die 'live mode requires repo root and full lowercase commit'
[[ -n "$NEW_SECRET_FILE" && -n "$FLEET_KEY_FILE" ]] || die 'live mode requires new-secret and fleet-key files'
[[ -n "${EVIDENCE_FILE:-}" ]] || EVIDENCE_FILE="$HOME/.corelink/rotation-b2-20260909/repo-webhook-rotation.json"
require_cmd() { command -v "$1" >/dev/null 2>&1 || die "missing command: $1"; }
for cmd in curl gh jq node python3 git; do require_cmd "$cmd"; done
ROOT=$(cd -- "$ROOT" && pwd -P)
WRANGLER_BIN="${WRANGLER_BIN:-$ROOT/deploy/cloudflare/node_modules/.bin/wrangler}"
[[ -x "$WRANGLER_BIN" ]] || die 'local Wrangler binary is missing'
CONFIG="$ROOT/deploy/cloudflare/wrangler.jsonc"
[[ -f "$CONFIG" && ! -L "$CONFIG" ]] || die 'Worker config missing or symlinked'
[[ "$(git -C "$ROOT" rev-parse HEAD)" == "$COMMIT" ]] || die 'HEAD does not match expected commit'
[[ -z "$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)" ]] || die 'worktree is not clean'
file_mode() { case "$(uname -s)" in Darwin) stat -f '%Lp' "$1";; Linux) stat -c '%a' "$1";; esac; }
file_uid() { case "$(uname -s)" in Darwin) stat -f '%u' "$1";; Linux) stat -c '%u' "$1";; esac; }
safe_file() { [[ -f "$1" && ! -L "$1" && "$(file_mode "$1")" == 600 && "$(file_uid "$1")" == "$(id -u)" && -s "$1" ]]; }
safe_file "$FLEET_KEY_FILE" || die 'fleet key must be non-empty, regular, owner-only 0600'
safe_file "$NEW_SECRET_FILE" || die 'new secret must be non-empty, regular, owner-only 0600'
new_secret=$(<"$NEW_SECRET_FILE")
[[ "$new_secret" != *$'\r'* && "$new_secret" != *$'\n'*$'\n'* ]] || die 'new secret must be one line without CR'
new_secret="${new_secret%$'\n'}"
[[ -n "$new_secret" ]] || die 'new secret is empty'
mkdir -p -- "$(dirname -- "$EVIDENCE_FILE")"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/corelink-repo-webhook-rotation.XXXXXXXX"); chmod 700 "$TMP_DIR"
EVENTS="$TMP_DIR/events.log"; : > "$EVENTS"; chmod 600 "$EVENTS"
FLEET_HEADER="$TMP_DIR/fleet.header"; printf 'x-corelink-internal-auth: %s\n' "$(<"$FLEET_KEY_FILE")" > "$FLEET_HEADER"; chmod 600 "$FLEET_HEADER"
record() { printf '%s %s\n' "$(date -u +%FT%H:%M:%SZ)" "$*" >> "$EVENTS"; }
scrub() { local f="$1" s="$1.scrubbed"; [[ -f "$f" ]] || return 0; sed -E -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1<REDACTED>/g' -e 's/[A-Za-z0-9+\/_=-]{40,}/<REDACTED>/g' "$f" > "$s" || true; chmod 600 "$s"; mv -f -- "$s" "$f"; }
# Every external operation has a process-group timeout and mode-0600 output.
run_capture() {
  local sec="$1" out="$2" err="$3"; shift 3; : > "$out"; : > "$err"; chmod 600 "$out" "$err"
  python3 - "$sec" "$out" "$err" "$@" <<'PY'
import os, signal, subprocess, sys
seconds=float(sys.argv[1]); out_path=sys.argv[2]; err_path=sys.argv[3]; command=sys.argv[4:]
with open(out_path,"wb") as out, open(err_path,"wb") as err:
    p=subprocess.Popen(command,stdout=out,stderr=err,start_new_session=True)
    try: code=p.wait(timeout=seconds)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid,signal.SIGTERM)
        try: p.wait(timeout=2)
        except subprocess.TimeoutExpired: os.killpg(p.pid,signal.SIGKILL)
        code=124
sys.exit(code)
PY
  local rc=$?; scrub "$err"; return "$rc"
}
run_stdin() {
  local sec="$1" out="$2" err="$3" input="$4"; shift 4; : > "$out"; : > "$err"; chmod 600 "$out" "$err"
  python3 - "$sec" "$out" "$err" "$input" "$@" <<'PY'
import os, signal, subprocess, sys
seconds=float(sys.argv[1]); out_path=sys.argv[2]; err_path=sys.argv[3]; input_path=sys.argv[4]; command=sys.argv[5:]
with open(out_path,"wb") as out, open(err_path,"wb") as err, open(input_path,"rb") as incoming:
    p=subprocess.Popen(command,stdin=incoming,stdout=out,stderr=err,start_new_session=True)
    try: code=p.wait(timeout=seconds)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid,signal.SIGTERM)
        try: p.wait(timeout=2)
        except subprocess.TimeoutExpired: os.killpg(p.pid,signal.SIGKILL)
        code=124
sys.exit(code)
PY
  local rc=$?; scrub "$err"; return "$rc"
}
wrangler() { run_capture 60 "$1" "$2" "$WRANGLER_BIN" --config "$CONFIG" "${@:3}"; }
wrangler_stdin() { run_stdin 60 "$1" "$2" "$3" "$WRANGLER_BIN" --config "$CONFIG" "${@:4}"; }
assert_idle() {
  local out="$TMP_DIR/fleet.json" err="$TMP_DIR/fleet.err"
  run_capture 30 "$out" "$err" curl --silent --show-error --fail-with-body --connect-timeout 10 --max-time 20 -H "@$FLEET_HEADER" "$WORKER_URL/internal/v1/fleet/busy" || return 1
  jq -e '((.busy|tonumber)==0) and ((.unverifiable|tonumber)==0)' "$out" >/dev/null || return 1
  record 'fleet=GREEN busy=0 unverifiable=0'
}
latest_version() {
  local out="$TMP_DIR/deployments.json" err="$TMP_DIR/deployments.err"
  wrangler "$out" "$err" deployments list --name "$WORKER_NAME" --json || return 1
  jq -er 'def rows: if type=="array" then . elif .items? then .items elif .deployments? then .deployments else [] end; [rows[]|{id:(.version_id//.version//.versions[0].version_id//""),created:(.created_on//.created_at//"")}]|map(select(.id!=""))|sort_by(.created)|last.id' "$out"
}
assert_frozen() {
  local version="$1" out="$TMP_DIR/version.json" err="$TMP_DIR/version.err"
  wrangler "$out" "$err" versions view "$version" --name "$WORKER_NAME" --json || return 1
  jq -e '[..|objects|select((.name?|type)=="string" and .name=="FABRIC_ADMISSION_PAUSED")|(.text//.value//"")]|length==1 and .[0]=="1"' "$out" >/dev/null || return 1
  record "frozen=GREEN version=$version"
}
deploy_frozen() {
  local out="$TMP_DIR/deploy.out" err="$TMP_DIR/deploy.err" version
  wrangler "$out" "$err" deploy --keep-vars --strict --var FABRIC_ADMISSION_PAUSED:1 || return 1
  version=$(latest_version) || return 1
  assert_frozen "$version"
}
put_secret() {
  local name out err
  name="$1"; out="$TMP_DIR/put-$name.out"; err="$TMP_DIR/put-$name.err"
  wrangler_stdin "$out" "$err" "$NEW_SECRET_FILE" secret put "$name"
  record "secret_put=$name result=GREEN"
}
delete_next() {
  local confirm="$TMP_DIR/confirm" out="$TMP_DIR/delete.out" err="$TMP_DIR/delete.err"
  printf 'y\n' > "$confirm"; chmod 600 "$confirm"
  wrangler_stdin "$out" "$err" "$confirm" secret delete GITHUB_WEBHOOK_REPO_SECRET_NEXT
  record 'secret_delete=GITHUB_WEBHOOK_REPO_SECRET_NEXT result=GREEN'
}
assert_final_secret_names() {
  local out="$TMP_DIR/secrets.json" err="$TMP_DIR/secrets.err"
  wrangler "$out" "$err" secret list --name "$WORKER_NAME" --format json || return 1
  jq -e '([.[].name] as $names | ($names | index("GITHUB_WEBHOOK_REPO_SECRET")) != null and ($names | index("GITHUB_WEBHOOK_REPO_SECRET_NEXT")) == null)' "$out" >/dev/null || return 1
  record 'secret_names=GREEN primary_present next_absent'
}
probe() {
  local body="$TMP_DIR/probe.body" sig="$TMP_DIR/probe.sig" headers="$TMP_DIR/probe.headers" response="$TMP_DIR/probe.response" status="$TMP_DIR/probe.status" err="$TMP_DIR/probe.err"
  printf '%s\n' '{"action":"queued","workflow_job":{"id":987654321,"labels":["corelink-dogfood"]},"repository":{"full_name":"HuGR-Labs/corelink-runners"}}' > "$body"
  run_capture 20 "$sig" "$TMP_DIR/probe-sign.err" node --input-type=module -e 'import fs from "node:fs"; import crypto from "node:crypto"; const key=fs.readFileSync(process.argv[1],"utf8").trim(); const body=fs.readFileSync(process.argv[2]); process.stdout.write("sha256="+crypto.createHmac("sha256",key).update(body).digest("hex")+"\n");' "$NEW_SECRET_FILE" "$body" || return 1
  printf 'content-type: application/json\nx-github-event: workflow_job\nx-github-delivery: repo-secret-rotation-probe\nx-hub-signature-256: %s\n' "$(<"$sig")" > "$headers"; chmod 600 "$headers"
  run_capture 30 "$status" "$err" curl --silent --show-error --connect-timeout 10 --max-time 20 -o "$response" -w '%{http_code}' -X POST -H "@$headers" --data-binary "@$body" "$WORKER_URL/webhook" || return 1
  [[ "$(<"$status")" == 503 ]] || return 1
  record 'probe_new_secret=GREEN http_status=503 admission_frozen=1'
}
patch_hook() {
  local current="$TMP_DIR/hook.json" payload="$TMP_DIR/hook-patch.json" out="$TMP_DIR/hook.out" err="$TMP_DIR/hook.err"
  run_capture 30 "$current" "$TMP_DIR/hook-get.err" gh api "repos/$HOOK_REPO/hooks/$HOOK_ID" --header 'Accept: application/vnd.github+json' || return 1
  jq --rawfile secret "$NEW_SECRET_FILE" 'def clean: sub("\\n$";""); {config:{url:.config.url,content_type:(.config.content_type//"json"),insecure_ssl:(.config.insecure_ssl//"0"),secret:($secret|clean)}}' "$current" > "$payload"; chmod 600 "$payload"
  run_capture 30 "$out" "$err" gh api --method PATCH "repos/$HOOK_REPO/hooks/$HOOK_ID" --header 'Accept: application/vnd.github+json' --input "$payload" || return 1
  record "hook_patch=GREEN repo=$HOOK_REPO hook_id=$HOOK_ID"
  if run_capture 30 "$TMP_DIR/ping.out" "$TMP_DIR/ping.err" gh api --method POST "repos/$HOOK_REPO/hooks/$HOOK_ID/pings"; then record 'hook_ping=GREEN'; else record 'hook_ping=UNAVAILABLE'; fi
  if run_capture 30 "$TMP_DIR/deliveries.json" "$TMP_DIR/deliveries.err" gh api "repos/$HOOK_REPO/hooks/$HOOK_ID/deliveries?per_page=10"; then record 'hook_deliveries=OBSERVED'; else record 'hook_deliveries=UNAVAILABLE'; fi
}
cleanup() {
  local rc=$?; trap - EXIT
  if [[ "$MUTATION_STARTED" == 1 && "$FINAL_FROZEN" != 1 ]]; then
    if deploy_frozen; then record 'failure_refreeze=GREEN'; else record 'failure_refreeze=RED'; rc=1; fi
  fi
  if [[ "$rc" == 0 ]]; then
    mkdir -p -- "$(dirname -- "$EVIDENCE_FILE")"
    jq -n --arg commit "$COMMIT" --arg events "$EVENTS" --arg repo "$HOOK_REPO" --arg hook "$HOOK_ID" --arg worker "$WORKER_NAME" '{schema_version:"evidence/v1",status:"PASS",source:{repository:"corelink-runners",commit_sha:$commit},worker:$worker,hook:{repository:$repo,id:$hook},gates:{fleet:"busy=0,unverifiable=0",admission_paused:true,probe:"new signed workflow_job accepted by auth and blocked by freeze",primary_promoted:true,next_deleted:true,final_frozen:true},evidence:{events:$events,secrets:"excluded",mode:"0600"}}' > "$EVIDENCE_FILE"; chmod 600 "$EVIDENCE_FILE"
  fi
  rm -rf -- "$TMP_DIR"; exit "$rc"
}
trap cleanup EXIT
assert_idle || die 'fleet is not provably idle'
MUTATION_STARTED=1
deploy_frozen || die 'initial frozen deploy failed'
put_secret GITHUB_WEBHOOK_REPO_SECRET_NEXT || die 'staging NEXT failed'
deploy_frozen || die 'deploy with NEXT failed'
probe || die 'signed queued probe with NEXT failed'
patch_hook || die 'repository hook patch failed'
put_secret GITHUB_WEBHOOK_REPO_SECRET || die 'primary promotion failed'
delete_next || die 'NEXT deletion failed'
deploy_frozen || die 'final frozen deploy failed'
assert_final_secret_names || die 'final secret-name proof failed'
assert_idle || die 'final fleet proof failed'
FINAL_FROZEN=1
record 'outcome=PASS final_frozen=1'
