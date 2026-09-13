#!/usr/bin/env bash
# AU1.8 temporary live-probe harness.
#
# This file is intentionally not a runbook. It is an executable change harness
# with a two-part destructive-action gate. It must never be run from CI.
# Secret, PAT, and ticket values are kept in stdin, pipes, or shell memory and
# are never printed. The only retained logs are mode-0600, scrubbed status logs.

set -Eeuo pipefail
set +x
umask 077

usage() {
  sed -n '1,24p' "$0"
  cat >&2 <<'EOF'

Usage:
  au1.8-fabricd-cred-ticket-rotation.sh --execute --ack-destructive [Access OOB options]

The two flags are mandatory. Without both flags this harness performs no
provider, secret, deploy, delete, or network action.
EOF
}

EXECUTE=0
ACK=0
ACCESS_CLIENT_ID_FILE="${AU18_ACCESS_CLIENT_ID_FILE:-${CORELINK_CF_ACCESS_CLIENT_ID_FILE:-}}"
ACCESS_CLIENT_SECRET_FILE="${AU18_ACCESS_CLIENT_SECRET_FILE:-${CORELINK_CF_ACCESS_CLIENT_SECRET_FILE:-}}"
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --execute) EXECUTE=1; shift ;;
    --ack-destructive) ACK=1; shift ;;
    --access-client-id-file|--cf-access-client-id-file)
      [[ "$#" -ge 2 ]] || { echo "missing value for $1" >&2; exit 2; }
      ACCESS_CLIENT_ID_FILE="$2"; shift 2 ;;
    --access-client-secret-file|--cf-access-client-secret-file)
      [[ "$#" -ge 2 ]] || { echo "missing value for $1" >&2; exit 2; }
      ACCESS_CLIENT_SECRET_FILE="$2"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage; exit 2 ;;
  esac
done
if [[ "$EXECUTE" != 1 || "$ACK" != 1 ]]; then
  echo "refusing: both --execute and --ack-destructive are required" >&2
  usage
  exit 2
fi

require_cmd() { command -v "$1" >/dev/null 2>&1 || { echo "missing command: $1" >&2; exit 1; }; }
for cmd in curl git jq node openssl rg sed stat tr mktemp; do require_cmd "$cmd"; done

REPO_ROOT="${AU18_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/../.." && pwd)}"
REPO_ROOT="$(cd -- "$REPO_ROOT" && pwd -P)"
CONFIG="$REPO_ROOT/deploy/cloudflare-fabricd/wrangler.jsonc"
readonly WRANGLER_EXPECTED_VERSION='4.105.0'
WORKER_NAME="corelink-fabricd"
CONTAINER_APP_NAME="corelink-fabricd-fabricdcontainer"
BASE_URL="${AU18_BASE_URL:-https://corelink-fabricd.gmhelmold.workers.dev}"
BASE_URL="${BASE_URL%/}"
HEALTH_URL="${AU18_HEALTH_URL:-$BASE_URL/health}"
OLD_SECRET_FILE="${AU18_OLD_SECRET_FILE:-$HOME/.corelink/secrets/corelink/fabric-cred-ticket-secret}"
TEST_MINT_KEY_FILE="${AU18_TEST_MINT_KEY_FILE:-$HOME/.corelink/secrets/fabric-test-mint-key-OOB.txt}"
PAT_FILE="${AU18_PAT_FILE:-$HOME/Downloads/corelink-dogfood-pat.txt}"
NEW_SECRET_FILE="${AU18_NEW_SECRET_FILE:-$HOME/.corelink/secrets/corelink/fabric-cred-ticket-secret-au1.8-new.txt}"
INTROSPECT_KEY_FILE="${AU18_INTROSPECT_KEY_FILE:-}"
FLEET_BUSY_KEY_FILE="${AU18_FLEET_BUSY_KEY_FILE:-}"
OBSERVABILITY_KEY_FILE="${AU18_OBSERVABILITY_KEY_FILE:-}"
FLEET_BUSY_URL="${AU18_FLEET_BUSY_URL:-https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy}"
OBSERVABILITY_URL="${AU18_OBSERVABILITY_URL:-$BASE_URL/internal/v1/occupancy}"
STATUS_URL="${AU18_STATUS_URL:-$BASE_URL/internal/v1/status}"
USAGE_URL="${AU18_USAGE_URL:-$BASE_URL/v1/usage}"
PROVIDER_STABILITY_SECS="${AU18_PROVIDER_STABILITY_SECS:-5}"
EVIDENCE_PATH="$REPO_ROOT/docs/plan/evidence/au1.8-fabricd-secret-rotation.json"
TENANT="ee30f7ba-fc25-4d71-939e-ebe130b4c6a3"
REPO_FULL_NAME="HuGR-Labs/corelink-runners"
INSTALLATION_ID="150584374"
SOURCE_COMMIT="${AU18_SOURCE_COMMIT:-$(git -C "$REPO_ROOT" rev-parse --verify 'HEAD^{commit}')}"
HEAD="$(git -C "$REPO_ROOT" rev-parse --verify 'HEAD^{commit}')"

source_tree_gate() {
  local dirty
  dirty="$(git -C "$REPO_ROOT" status --porcelain=v1 --untracked-files=all)" || {
    echo "refusing: unable to inspect repository status" >&2
    return 1
  }
  if [[ -n "$dirty" ]]; then
    echo "refusing: repository worktree/index is not clean (including untracked files)" >&2
    return 1
  fi
  [[ "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ && "$SOURCE_COMMIT" == "$HEAD" ]] || {
    echo "wrong repository source commit; AU18_SOURCE_COMMIT must match the checked-out HEAD" >&2
    return 1
  }
}

validate_status_report() {
  jq -e '(.num_shards | tonumber) == 1 and
    (.ledger_cross_instance_safe == true or .ledger_cross_instance_safe == false)' <<<"$1" >/dev/null
}

status_ledger_is_safe() {
  jq -er '.ledger_cross_instance_safe == true' <<<"$1" >/dev/null
}

memory_singleton_status_ok() {
  jq -e '(.num_shards | type == "number") and .num_shards == 1 and
    .ledger_cross_instance_safe == false' <<<"$1" >/dev/null
}

provider_timestamp_epoch() {
  local value="$1"
  node -e 'const t=Date.parse(process.argv[1]); if (!Number.isFinite(t)) process.exit(1); process.stdout.write(String(Math.floor(t/1000)))' "$value"
}

memory_singleton_age_gate() {
  local created_on="$1" now created age
  [[ -n "$created_on" ]] || return 1
  created="$(provider_timestamp_epoch "$created_on")" || return 1
  now="$(date +%s)"
  age=$((now - created))
  [[ "$age" -ge 3900 ]]
  log_event "memory-singleton-quiescence-age=$age required=3900"
}

assert_singleton_capacity() {
  local info="$1"
  # The provider's app field is authoritative; unrelated nested metadata must
  # never satisfy the singleton gate.
  jq -e 'has("max_instances") and (.max_instances | type == "number") and .max_instances == 1' "$info" >/dev/null
}

provider_stability_pair_ok() {
  local first="$1" second="$2"
  [[ -n "$first" && -n "$second" && "$first" == "$second" ]]
}

# Wrangler's `containers info` response uses a numeric top-level `version`
# for the current container application, while older/mocked responses expose
# a string top-level `version_id`.  Accept both scalar forms and canonicalize
# to text; inspect only the top level so unrelated nested metadata cannot
# satisfy the identity gate. Reject objects, arrays, booleans, and null.
extract_container_version() {
  local info="$1"
  jq -er 'if has("version") then .version
    elif has("version_id") then .version_id
    else error("top-level container version is absent") end
    | select((type == "string" or type == "number") and ((tostring | length) > 0))
    | tostring' "$info"
}

# The provider's version inventory is the authority for non-secret Worker
# bindings.  Keep only the one binding under test on disk; never retain the
# complete `versions view` response because it may contain unrelated values.
capture_fabricd_admission_pause() {
  local label="$1" version_id="$2" out err rc jq_rc
  out="$TMP_DIR/${label}-fabricd-admission-paused.json"
  err="$LOG_DIR/$(date -u +%s%N)-${label}-fabricd-admission-paused.stderr"
  : > "$out"; chmod 600 "$out"
  : > "$err"; chmod 600 "$err"
  set +e
  run_wrangle versions view "$version_id" --name "$WORKER_NAME" --json 2>"$err" |
    jq -S '[.. | objects | select((.name? | type) == "string" and .name == "FABRIC_ADMISSION_PAUSED") |
      {name, type:(.type // ""), value:((.text // .value // "") | tostring)}]' >"$out"
  local -a pipe_status=("${PIPESTATUS[@]}")
  rc="${pipe_status[0]}"; jq_rc="${pipe_status[1]}"
  set -e
  scrub_file "$err"
  log_event "$label-fabricd-admission-paused rc=$rc jq_rc=$jq_rc"
  [[ "$rc" == 0 && "$jq_rc" == 0 ]] || return 1
  printf '%s\n' "$out"
}

assert_fabricd_admission_paused() {
  local label="$1" version_id="$2" snapshot
  snapshot="$(capture_fabricd_admission_pause "$label" "$version_id")" || return 1
  if ! jq -e '
    length == 1 and
    .[0].name == "FABRIC_ADMISSION_PAUSED" and
    .[0].type == "plain_text" and
    .[0].value == "1"
  ' "$snapshot" >/dev/null; then
    log_event "$label-fabricd-admission-paused=RED"
    return 1
  fi
  log_event "$label-fabricd-admission-paused=GREEN"
}

file_owner_mode_ok() {
  local file="$1" mode owner
  [[ -f "$file" && ! -L "$file" && -r "$file" && -s "$file" ]] || return 1
  case "$(uname -s)" in
    Darwin) mode="$(stat -f '%Lp' "$file")"; owner="$(stat -f '%u' "$file")" ;;
    Linux) mode="$(stat -c '%a' "$file")"; owner="$(stat -c '%u' "$file")" ;;
    *) return 1 ;;
  esac
  [[ "$mode" == 600 && "$owner" == "$(id -u)" ]]
}

single_line_file() {
  local file="$1" newlines carriage_returns
  file_owner_mode_ok "$file" || return 1
  newlines="$(tr -cd '\n' < "$file" | wc -c | tr -d ' ')"
  carriage_returns="$(tr -cd '\r' < "$file" | wc -c | tr -d ' ')"
  [[ "$newlines" -le 1 && "$carriage_returns" == 0 ]]
}

access_pair_gate() {
  if [[ -z "$ACCESS_CLIENT_ID_FILE" && -z "$ACCESS_CLIENT_SECRET_FILE" ]]; then
    return 0
  fi
  [[ -n "$ACCESS_CLIENT_ID_FILE" && -n "$ACCESS_CLIENT_SECRET_FILE" ]] || {
    echo "Cloudflare Access requires both client id and client secret files" >&2
    return 1
  }
  single_line_file "$ACCESS_CLIENT_ID_FILE" || {
    echo "Cloudflare Access client id must be an owner-only regular 0600 single-line file" >&2
    return 1
  }
  single_line_file "$ACCESS_CLIENT_SECRET_FILE" || {
    echo "Cloudflare Access client secret must be an owner-only regular 0600 single-line file" >&2
    return 1
  }
}

oob_file_gate() {
  local label="$1" file="$2"
  file_owner_mode_ok "$file" || {
    echo "$label OOB file must be a non-empty regular 0600 file owned by this operator" >&2
    return 1
  }
}

if ! [[ "$PROVIDER_STABILITY_SECS" =~ ^[0-9]+$ ]]; then
  echo "refusing: AU18_PROVIDER_STABILITY_SECS must be a non-negative integer" >&2
  exit 1
fi

source_tree_gate || exit 1
if [[ "${AU18_VALIDATE_ONLY:-0}" == 1 ]]; then
  access_pair_gate || exit 1
  if [[ "${AU18_VALIDATE_FILE_METADATA_ONLY:-0}" == 1 ]]; then
    oob_file_gate FABRIC_TEST_MINT_KEY "$TEST_MINT_KEY_FILE" || exit 1
    oob_file_gate FABRIC_OBSERVABILITY_KEY "$OBSERVABILITY_KEY_FILE" || exit 1
  fi
  if [[ -n "${AU18_STATUS_REPORT_JSON:-}" ]]; then
    validate_status_report "$AU18_STATUS_REPORT_JSON" || exit 1
  fi
  if [[ -n "${AU18_PROVIDER_STABILITY_SAMPLE_1:-}" || -n "${AU18_PROVIDER_STABILITY_SAMPLE_2:-}" ]]; then
    provider_stability_pair_ok "${AU18_PROVIDER_STABILITY_SAMPLE_1:-}" "${AU18_PROVIDER_STABILITY_SAMPLE_2:-}" || exit 1
  fi
  exit 0
fi

# Validate optional Access credentials before any Wrangler/provider read. This
# keeps a malformed OOB pair fail-closed without revealing provider metadata.
access_pair_gate || exit 1

[[ -f "$CONFIG" && ! -L "$CONFIG" ]] || { echo "missing or symlinked wrangler config" >&2; exit 1; }
EXPECTED_IMAGE_DIGEST="$(sed -n 's/^[[:space:]]*"image":[[:space:]]*"[^@]*@\(sha256:[0-9a-f]\{64\}\)".*/\1/p' "$CONFIG" | head -n 1)"
CONFIG_INTROSPECT_URL="$(sed -n 's/^[[:space:]]*"CORELINK_INTROSPECT_URL"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$CONFIG" | head -n 1)"
INTROSPECT_URL="${AU18_INTROSPECT_URL:-$CONFIG_INTROSPECT_URL}"
[[ -n "$EXPECTED_IMAGE_DIGEST" ]] || { echo "could not resolve pinned fabricd image digest" >&2; exit 1; }
[[ "$INTROSPECT_URL" == https://corelink-api.humangr.com/internal/v1/auth/introspect ]] || {
  echo "refusing: introspect URL must be the configured corelink-api.humangr.com endpoint" >&2; exit 1;
}
# The temporary tenant binding must not already be tracked in this tip.
if rg -n '^[[:space:]]*"FABRIC_TEST_MINT_TENANTS"[[:space:]]*:' "$CONFIG" >/dev/null; then
  echo "refusing: tenant test var is already active in tracked config" >&2
  exit 1
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
LOG_ROOT="${AU18_LOG_ROOT:-$HOME/.corelink/logs/corelink-au1.8}"
LOG_DIR="$LOG_ROOT/$RUN_ID"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-fabricd.XXXXXX")"
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR" "$TMP_DIR"
EVENT_LOG="$LOG_DIR/events.log"
: > "$EVENT_LOG"
chmod 600 "$EVENT_LOG"

WRANGLER_DIR="${CONFIG%/*}"
WRANGLER_BIN="$WRANGLER_DIR/node_modules/.bin/wrangler"
[[ -x "$WRANGLER_BIN" ]] || { echo "missing local Wrangler binary: $WRANGLER_BIN" >&2; exit 1; }
WRANGLER_VERSION="$(cd "$WRANGLER_DIR" && "$WRANGLER_BIN" --version)" || {
  echo "local Wrangler version probe failed: $WRANGLER_BIN" >&2
  exit 1
}
[[ "$WRANGLER_VERSION" == "$WRANGLER_EXPECTED_VERSION" ]] || {
  echo "local Wrangler version mismatch: expected $WRANGLER_EXPECTED_VERSION, got $WRANGLER_VERSION" >&2
  exit 1
}
run_wrangle() {
  local token
  token="$(cd "$WRANGLER_DIR" && "$WRANGLER_BIN" auth token --json | jq -er '.token // .access_token // .')" || {
    echo "wrangler OAuth unavailable" >&2
    return 1
  }
  [[ "$token" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]] || {
    echo "invalid wrangler OAuth token" >&2
    return 1
  }
  (cd "$WRANGLER_DIR" && CLOUDFLARE_API_TOKEN="$token" "$WRANGLER_BIN" --config "$CONFIG" "$@")
}
APP_ID="${AU18_APP_ID:-}"
HEADER_FILE=""
INTROSPECT_HEADER_FILE=""
FLEET_BUSY_HEADER_FILE=""
OBSERVABILITY_HEADER_FILE=""
MUTATION_STARTED=0
CLEANUP_RUNNING=0
SUCCESS_CLEANUP_DONE=0
WINDOW_START=0
WINDOW_FINISH=0
REMOTE_BASELINE_SHA256=""
REMOTE_BASELINE_FILE=""
REMOTE_TEMP_VAR_STATE="unknown"
QUIESCENCE_STATE="not-checked"
LEDGER_CROSS_INSTANCE_SAFE="not-checked"
PROVIDER_STABILITY_STATE="not-checked"
CAS_PAT_PROOF="not-checked"

log_event() { printf '%s %s\n' "$(date -u +%FT%H:%M:%SZ)" "$*" >> "$EVENT_LOG"; }

log_quiescence_leaf() { log_event "quiescence=RED leaf=$1"; }

log_usage_fetch_failure() {
  local status="$1" headers="$2" reason="$3" content_type cf_ray
  [[ "$status" =~ ^[0-9]{3}$ ]] || status=000
  content_type="$(sed -n 's/^[Cc]ontent-[Tt]ype:[[:space:]]*//p' "$headers" 2>/dev/null | head -n 1 | sed -E 's/[^A-Za-z0-9._;=,+\/ -]//g' | cut -c1-80)"
  cf_ray="$(sed -n 's/^[Cc][Ff]-[Rr]ay:[[:space:]]*//p' "$headers" 2>/dev/null | head -n 1 | sed -E 's/[^A-Za-z0-9._-]//g' | cut -c1-80)"
  log_event "quiescence=RED leaf=usage_fetch reason=$reason http_status=$status content_type=${content_type:-absent} cf_ray=${cf_ray:-absent}"
}

scrub_file() {
  local file="$1"
  local safe="$file.safe"
  # Remove bearer-like values, long opaque values, and any accidental raw
  # Wrangler token fragments before retaining the stderr record.
  sed -E \
    -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1<REDACTED>/g' \
    -e 's/(X-Fabric-Test-Mint-Key:[[:space:]]*)[^[:space:]]+/\1<REDACTED>/g' \
    -e 's/(CF-Access-Client-(Id|Secret):[[:space:]]*)[^[:space:]]+/\1<REDACTED>/g' \
    -e 's/[A-Za-z0-9+\/_=-]{40,}/<REDACTED>/g' \
    "$file" > "$safe" || true
  chmod 600 "$safe"
  mv -f -- "$safe" "$file"
}

run_quiet() {
  local label="$1"; shift
  local err
  err="$LOG_DIR/$(date -u +%s%N)-${label}.stderr"
  : > "$err"; chmod 600 "$err"
  set +e
  "$@" >/dev/null 2>"$err"
  local rc=$?
  set -e
  scrub_file "$err"
  log_event "$label rc=$rc"
  return "$rc"
}

capture_json() {
  local label="$1" output="$2"; shift 2
  local err
  err="$LOG_DIR/$(date -u +%s%N)-${label}.stderr"
  : > "$output"; chmod 600 "$output"
  : > "$err"; chmod 600 "$err"
  set +e
  "$@" >"$output" 2>"$err"
  local rc=$?
  set -e
  scrub_file "$err"
  log_event "$label rc=$rc"
  return "$rc"
}

resolve_app_id() {
  local out="$TMP_DIR/container-list.json"
  capture_json containers-list "$out" run_wrangle containers list --json || return 1
  jq -er --arg app_name "$CONTAINER_APP_NAME" '[.. | objects | select(.name? == $app_name and ((.id? | type) == "string")) | .id] | unique | if length == 1 then .[0] else error("exact fabricd application name is absent or ambiguous") end' "$out"
}

if [[ -z "$APP_ID" ]]; then
  APP_ID="$(resolve_app_id)" || { echo "could not resolve exact $CONTAINER_APP_NAME application id" >&2; exit 1; }
else
  # An operator-supplied id is only accepted after a read-only identity check;
  # never delete an arbitrary container merely because its id was supplied.
  supplied_info="$TMP_DIR/supplied-container-info.json"
  capture_json supplied-container-info "$supplied_info" run_wrangle containers info "$APP_ID" || {
    echo "AU18_APP_ID is not readable" >&2; exit 1;
  }
  jq -e --arg app_name "$CONTAINER_APP_NAME" '.name == $app_name' "$supplied_info" >/dev/null || {
    echo "AU18_APP_ID does not name the exact fabricd application" >&2; exit 1;
  }
fi

for protected in "$OLD_SECRET_FILE" "$PAT_FILE"; do
  oob_file_gate "secret/PAT" "$protected" || exit 1
done
oob_file_gate FABRIC_TEST_MINT_KEY "$TEST_MINT_KEY_FILE" || exit 1
oob_file_gate FABRIC_OBSERVABILITY_KEY "$OBSERVABILITY_KEY_FILE" || exit 1
for gate_key in "$INTROSPECT_KEY_FILE" "$FLEET_BUSY_KEY_FILE" "$OBSERVABILITY_KEY_FILE"; do
  if [[ -z "$gate_key" ]] || ! oob_file_gate internal-auth "$gate_key"; then
    echo "introspection, fleet-busy, and observability OOB key paths must be supplied and 0600" >&2
    exit 1
  fi
done
if [[ -e "$NEW_SECRET_FILE" || -L "$NEW_SECRET_FILE" ]]; then
  echo "refusing to overwrite existing NEW_SECRET_FILE" >&2
  exit 1
fi

assert_test_key_absent() {
  local out="$TMP_DIR/secret-list.json"
  capture_json secret-list "$out" run_wrangle secret list --name "$WORKER_NAME" || return 1
  jq -e '[.. | strings] | any(. == "FABRIC_TEST_MINT_KEY") | not' "$out" >/dev/null
}
assert_test_key_absent || { echo "FABRIC_TEST_MINT_KEY must be absent before this temporary probe" >&2; exit 1; }

capture_state() {
  local label="$1"
  local deploys="$TMP_DIR/$label-deployments.json"
  local info="$TMP_DIR/$label-container.json"
  local health worker container digest
  health="$(curl -sS --connect-timeout 10 --max-time 30 -o /dev/null -w '%{http_code}' "$HEALTH_URL" 2>"$TMP_DIR/$label-health.err")" || health=000
  scrub_file "$TMP_DIR/$label-health.err"
  [[ "$health" == 200 ]] || { log_event "$label health=$health"; return 1; }
  capture_json "$label-deployments" "$deploys" run_wrangle deployments list --name "$WORKER_NAME" --json || return 1
  capture_json "$label-container-info" "$info" run_wrangle containers info "$APP_ID" || return 1
  jq -e --arg app_name "$CONTAINER_APP_NAME" '.name == $app_name' "$info" >/dev/null || {
    log_event "$label application-name-mismatch"; return 1;
  }
  assert_singleton_capacity "$info" || return 1
  worker="$(jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$deploys")" || return 1
  container="$(extract_container_version "$info")" || return 1
  digest="$(jq -er --arg ENV_EXPECTED_DIGEST "$EXPECTED_IMAGE_DIGEST" '[.. | strings | scan("sha256:[0-9a-f]{64}")] | unique | if . == [$ENV_EXPECTED_DIGEST] then .[0] else error("unexpected image digest") end' "$info")" || return 1
  case "$label" in
    before) BEFORE_WORKER_VERSION="$worker"; BEFORE_CONTAINER_VERSION="$container"; BEFORE_DIGEST="$digest" ;;
    rotated) ROTATED_WORKER_VERSION="$worker"; ROTATED_CONTAINER_VERSION="$container"; ROTATED_DIGEST="$digest" ;;
    final) FINAL_WORKER_VERSION="$worker"; FINAL_CONTAINER_VERSION="$container"; FINAL_DIGEST="$digest" ;;
  esac
  CURRENT_WORKER_VERSION="$worker"
  log_event "$label health=200 worker_version_id=$worker container_version_id=$container image_digest=$digest"
}

provider_snapshot() {
  local label="$1"
  local deploys="$TMP_DIR/$label-deployments.json"
  local info="$TMP_DIR/$label-container.json"
  local worker container digest
  capture_json "$label-deployments" "$deploys" run_wrangle deployments list --name "$WORKER_NAME" --json || return 1
  capture_json "$label-container-info" "$info" run_wrangle containers info "$APP_ID" || return 1
  jq -e --arg app_name "$CONTAINER_APP_NAME" '.name == $app_name' "$info" >/dev/null || return 1
  assert_singleton_capacity "$info" || return 1
  worker="$(jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$deploys")" || return 1
  container="$(extract_container_version "$info")" || return 1
  digest="$(jq -er --arg expected "$EXPECTED_IMAGE_DIGEST" '[.. | strings | scan("sha256:[0-9a-f]{64}")] | unique | if . == [$expected] then .[0] else error("unexpected image digest") end' "$info")" || return 1
  assert_fabricd_admission_paused "$label" "$worker" || return 1
  printf '%s\t%s\t%s\n' "$worker" "$container" "$digest"
}

provider_stability_gate() {
  local expected_worker="${1:-}" expected_container="${2:-}" expected_digest="${3:-}"
  local first second
  first="$(provider_snapshot provider-stability-1)" || { log_quiescence_leaf stability; return 1; }
  if [[ "$PROVIDER_STABILITY_SECS" -gt 0 ]]; then
    sleep "$PROVIDER_STABILITY_SECS"
  fi
  second="$(provider_snapshot provider-stability-2)" || { log_quiescence_leaf stability; return 1; }
  provider_stability_pair_ok "$first" "$second" || {
    log_event "provider-stability=RED sample_mismatch"
    log_quiescence_leaf stability
    return 1
  }
  if [[ -n "$expected_worker" && "$first" != "$expected_worker"$'\t'"$expected_container"$'\t'"$expected_digest" ]]; then
    log_event "provider-stability=RED baseline_mismatch"
    log_quiescence_leaf stability
    return 1
  fi
  PROVIDER_STABILITY_STATE="green"
  log_event "provider-stability=GREEN samples=2 worker_container_digest=$second interval_seconds=$PROVIDER_STABILITY_SECS"
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}';
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}

# `versions view` is the provider's authoritative remote binding inventory. We
# stream it through jq and retain only names/types plus the temporary tenant's
# value; secret values and arbitrary binding payloads never reach disk or logs.
capture_remote_bindings() {
  local label version_id worker_name out
  label="$1"
  version_id="$2"
  worker_name="${3:-$WORKER_NAME}"
  out="$TMP_DIR/${label}-remote-bindings.json"
  local err rc
  err="$LOG_DIR/$(date -u +%s%N)-${label}-versions.stderr"
  : > "$out"; chmod 600 "$out"; : > "$err"; chmod 600 "$err"
  set +e
  run_wrangle versions view "$version_id" --name "$worker_name" --json 2>"$err" |
    jq -S --arg temp "FABRIC_TEST_MINT_TENANTS" '
      [ .. | objects | select((.name? | type) == "string" and (.type? | type) == "string") |
        select(.type | test("^(plain_text|secret_text|json|kv_namespace|durable_object_namespace|service|wasm_module|plain_text_blob)$")) |
        {name, type, temporary_value:(if .name == $temp or (.name | startswith("AUTOSCALER_")) then (.text // .value // "") else null end)}
      ] | sort_by([.name,.type])
    ' >"$out"
  local -a pipe_status=("${PIPESTATUS[@]}")
  rc="${pipe_status[0]}"; local jq_rc="${pipe_status[1]}"
  set -e
  scrub_file "$err"
  log_event "$label-remote-bindings rc=$rc jq_rc=$jq_rc"
  [[ "$rc" == 0 && "$jq_rc" == 0 ]] || return 1
  printf '%s\n' "$out"
}

assert_remote_bindings() {
  local label="$1" version_id="$2" mode="$3" snapshot current
  snapshot="$(capture_remote_bindings "$label" "$version_id")" || return 1
  current="$TMP_DIR/$label-remote-bindings-no-temp.json"
  jq 'map(select(.name != "FABRIC_TEST_MINT_TENANTS"))' "$snapshot" > "$current"
  chmod 600 "$current"
  jq -e --slurpfile baseline "$REMOTE_BASELINE_FILE" '$baseline[0] == .' "$current" >/dev/null || {
    log_event "$label remote-binding-drift=RED"; return 1;
  }
  if [[ "$mode" == armed ]]; then
    jq -e --arg tenant "$TENANT" 'map(select(.name == "FABRIC_TEST_MINT_TENANTS")) | length == 1 and .[0].temporary_value == $tenant' "$snapshot" >/dev/null || return 1
    REMOTE_TEMP_VAR_STATE="armed"
  else
    # `--keep-vars --strict` protects unknown bindings. An empty override is
    # accepted as disarmed because it removes the effective tenant allowlist
    # without deleting an operator-owned remote binding we cannot reconstruct.
    jq -e 'map(select(.name == "FABRIC_TEST_MINT_TENANTS")) | length == 0 or (length == 1 and (.[0].temporary_value // "") == "")' "$snapshot" >/dev/null || return 1
    REMOTE_TEMP_VAR_STATE="empty-disabled"
  fi
  log_event "$label remote-binding-baseline=GREEN mode=$mode"
}

delete_and_confirm() {
  local old_id="$APP_ID" i info_status list_status list="$TMP_DIR/delete-list.json" info="$TMP_DIR/delete-info.json"
  app_is_absent() {
    set +e
    run_wrangle containers info "$old_id" >"$info" 2>"$TMP_DIR/delete-info.err"; info_status=$?
    run_wrangle containers list --json >"$list" 2>"$TMP_DIR/delete-list.err"; list_status=$?
    set -e
    scrub_file "$TMP_DIR/delete-info.err"; scrub_file "$TMP_DIR/delete-list.err"
    [[ "$info_status" != 0 && "$list_status" == 0 ]] &&
      ! jq -e --arg id "$old_id" '[.. | objects | select(.id? == $id)] | length > 0' "$list" >/dev/null
  }
  # Rollback can begin after an operator/provider timeout has already removed
  # the app. In that state the safe transition is deploy + rediscover; never
  # issue a second delete against an absent id.
  if app_is_absent; then
    log_event "container-already-absent; deploy-and-rediscover"
    return 0
  fi
  for i in 1 2; do
    if ! run_quiet container-delete run_wrangle containers delete "$old_id"; then
      if app_is_absent; then
        log_event "container-absence-confirmed-after-delete-error attempt=$i"
        return 0
      fi
      return 1
    fi
    if app_is_absent; then
      log_event "container-absence-confirmed attempt=$i"
      return 0
    fi
    sleep 1
  done
  log_event "container-absence-confirmed rc=1"
  return 1
}

recreate() {
  local arm="$1" new_id i
  delete_and_confirm || return 1
  if [[ "$arm" == 1 ]]; then
    assert_remote_bindings pre-deploy-arm "$CURRENT_WORKER_VERSION" "$([[ "$REMOTE_TEMP_VAR_STATE" == armed ]] && echo armed || echo baseline)" || return 1
    # `none` only updates Worker configuration and can leave a deleted
    # Containers application absent. Immediate rollout recreates the exact
    # configured application from the immutable digest and waits for provider
    # readiness; capture_state below verifies that digest after every rollout.
    run_quiet deploy-arm run_wrangle deploy --keep-vars --strict --var "FABRIC_TEST_MINT_TENANTS:$TENANT" --containers-rollout=immediate || return 1
  else
    # Keep every unknown remote binding and explicitly blank only the temporary
    # tenant allowlist. Strict mode rejects accidental config drift.
    assert_remote_bindings pre-deploy-disarm "$CURRENT_WORKER_VERSION" armed || return 1
    run_quiet deploy-disarm run_wrangle deploy --keep-vars --strict --var "FABRIC_TEST_MINT_TENANTS:" --containers-rollout=immediate || return 1
  fi
  for i in $(seq 1 20); do
    if new_id="$(resolve_app_id 2>/dev/null)"; then APP_ID="$new_id"; return 0; fi
    sleep 2
  done
  return 1
}

put_secret() {
  local name="$1" file="$2"
  local err
  err="$LOG_DIR/$(date -u +%s%N)-secret-put.stderr"
  : > "$err"; chmod 600 "$err"
  set +e
  run_wrangle secret put "$name" --name "$WORKER_NAME" < "$file" >/dev/null 2>"$err"
  local rc=$?
  set -e
  scrub_file "$err"
  log_event "secret-put-$name rc=$rc"
  return "$rc"
}

delete_test_key() { run_quiet secret-delete-test-mint-key run_wrangle secret delete FABRIC_TEST_MINT_KEY --name "$WORKER_NAME"; }

make_header_file() {
  local out="$TMP_DIR/test-mint-header"
  local key
  IFS= read -r key < "$TEST_MINT_KEY_FILE"
  printf 'X-Fabric-Test-Mint-Key: %s\n' "$key" > "$out"
  chmod 600 "$out"
  printf '%s\n' "$out"
}

make_oob_header_file() {
  local name file out key
  name="$1"
  file="$2"
  out="$TMP_DIR/${name}-header"
  IFS= read -r key < "$file"
  printf 'X-Corelink-Internal-Auth: %s\n' "$key" > "$out"
  chmod 600 "$out"
  printf '%s\n' "$out"
}

make_pat_header_file() {
  local out="$TMP_DIR/pat-header"
  local pat
  pat="$(tr -d '\r\n' < "$PAT_FILE")"
  [[ -n "$pat" ]] || return 1
  printf 'Authorization: Bearer %s\n' "$pat" > "$out"
  chmod 600 "$out"
  printf '%s\n' "$out"
}

fetch_usage_response() {
  local header body err http_status curl_rc
  header="$(make_pat_header_file)" || {
    log_event "usage-failure curl_rc=26 http_status=000"
    return 1
  }
  body="$TMP_DIR/usage.body"
  err="$TMP_DIR/usage.err"
  : > "$body"; chmod 600 "$body"
  : > "$err"; chmod 600 "$err"
  set +e
  http_status="$(curl -sS --connect-timeout 10 --max-time 30 \
    --header "@$header" -o "$body" -w '%{http_code}' "$USAGE_URL" 2>"$err")"
  curl_rc=$?
  set -e
  scrub_file "$err"
  http_status="${http_status:-000}"
  if [[ "$curl_rc" != 0 || "$http_status" != 200 ]]; then
    log_event "usage-failure curl_rc=$curl_rc http_status=$http_status"
    return 1
  fi
  cat "$body"
}

make_introspect_header_file() {
  local out="$TMP_DIR/introspect-header" key
  IFS= read -r key < "$INTROSPECT_KEY_FILE"
  {
    printf 'X-Corelink-Internal-Auth: %s\n' "$key"
    if [[ -n "$ACCESS_CLIENT_ID_FILE" ]]; then
      printf 'CF-Access-Client-Id: '
      tr -d '\r\n' < "$ACCESS_CLIENT_ID_FILE"
      printf '\nCF-Access-Client-Secret: '
      tr -d '\r\n' < "$ACCESS_CLIENT_SECRET_FILE"
      printf '\n'
    fi
  } > "$out"
  chmod 600 "$out"
  printf '%s\n' "$out"
}

preflight_dogfood_pat() {
  INTROSPECT_HEADER_FILE="$(make_introspect_header_file)"
  local body err raw rc
  err="$TMP_DIR/pat-introspect.err"; : > "$err"; chmod 600 "$err"
  set +e
  body="$(jq -nc --rawfile pat "$PAT_FILE" '{token:($pat|sub("\\n$";""))}' |
    curl -fsS --connect-timeout 10 --max-time 30 --header "@$INTROSPECT_HEADER_FILE" \
      --header 'content-type: application/json' --data-binary @- "$INTROSPECT_URL" 2>>"$err")"
  rc=$?
  set -e
  scrub_file "$err"
  [[ "$rc" == 0 ]] || return 1
  jq -e --arg tenant "$TENANT" '
    .valid == true and .tenant_id == $tenant and
    ((.scopes // .scope // .plan // .max_concurrency // null) != null) and
    ((.max_concurrency // 0) | tonumber) > 0 and
    ((.max_vcpu_h // 0) | tonumber) >= 0
  ' <<<"$body" >/dev/null || {
    log_event "dogfood-pat-preflight=RED"; return 1;
  }
  log_event "dogfood-pat-preflight=GREEN tenant=$TENANT scopes=entitlements-present"
}

quiescence_gate() {
  # These are authoritative reads immediately before the first mutation:
  # fabric usage proves no active lease for the dogfood tenant, occupancy proves
  # the singleton has no held job, and the spawn fleet endpoint proves no busy
  # or unverifiable fleet item while intake/redispatch are paused remotely.
  OBSERVABILITY_HEADER_FILE="$(make_oob_header_file observability "$OBSERVABILITY_KEY_FILE")"
  FLEET_BUSY_HEADER_FILE="$(make_oob_header_file fleet-busy "$FLEET_BUSY_KEY_FILE")"
  local usage fleet response status_report rc usage_http_status usage_body usage_headers pat_header
  pat_header="$(make_pat_header_file)" || {
    log_usage_fetch_failure 000 "$TMP_DIR/usage.headers" http
    return 1
  }
  usage_body="$TMP_DIR/usage.body"; usage_headers="$TMP_DIR/usage.headers"
  : > "$usage_body"; : > "$usage_headers"; chmod 600 "$usage_body" "$usage_headers"
  set +e
  usage_http_status="$(curl -sS --connect-timeout 10 --max-time 30 \
    --header "@$pat_header" -D "$usage_headers" -o "$usage_body" -w '%{http_code}' "$USAGE_URL" 2>"$TMP_DIR/usage.err")"
  rc=$?
  set -e
  scrub_file "$TMP_DIR/usage.err"
  usage="$(<"$usage_body")"
  if [[ "$rc" != 0 || "${usage_http_status:-000}" != 200 ]]; then
    log_usage_fetch_failure "$usage_http_status" "$usage_headers" http
    scrub_file "$usage_body"; scrub_file "$usage_headers"
    return 1
  fi
  if ! jq -e --arg tenant "$TENANT" '.tenant == $tenant and ((.active_now // .activeNow) | tonumber) == 0' <<<"$usage" >/dev/null; then
    log_usage_fetch_failure "$usage_http_status" "$usage_headers" shape
    scrub_file "$usage_body"; scrub_file "$usage_headers"
    return 1
  fi
  scrub_file "$usage_body"; scrub_file "$usage_headers"

  response="$(curl -fsS --connect-timeout 10 --max-time 30 --header "@$OBSERVABILITY_HEADER_FILE" "$OBSERVABILITY_URL" 2>"$TMP_DIR/occupancy.err")" || { log_quiescence_leaf occupancy; return 1; }
  scrub_file "$TMP_DIR/occupancy.err"
  jq -e '(.per_tenant | type) == "array" and
    (.per_tenant | all(.[]; ((.occupied | type) == "number" and .occupied == 0)))' <<<"$response" >/dev/null || { log_quiescence_leaf occupancy; return 1; }
  status_report="$(curl -fsS --connect-timeout 10 --max-time 30 --header "@$OBSERVABILITY_HEADER_FILE" "$STATUS_URL" 2>"$TMP_DIR/status.err")" || { log_quiescence_leaf status; return 1; }
  scrub_file "$TMP_DIR/status.err"
  validate_status_report "$status_report" || { log_quiescence_leaf status; return 1; }
  if status_ledger_is_safe "$status_report"; then
    LEDGER_CROSS_INSTANCE_SAFE="true"
  else
    # In-memory state is acceptable only during a provider-attested, paused
    # singleton window.  The deployment timestamp comes from the provider
    # response above; caller-supplied age/version assertions are ignored.
    LEDGER_CROSS_INSTANCE_SAFE="false"
    memory_singleton_status_ok "$status_report" || { log_quiescence_leaf stability; return 1; }
    assert_singleton_capacity "$TMP_DIR/before-container.json" || { log_quiescence_leaf capacity; return 1; }
    local before_created_on before_worker
    before_created_on="$(jq -er 'sort_by(.created_on // "") | last | .created_on // empty' "$TMP_DIR/before-deployments.json")" || { log_quiescence_leaf age; return 1; }
    memory_singleton_age_gate "$before_created_on" || { log_quiescence_leaf age; return 1; }
    before_worker="$(jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$TMP_DIR/before-deployments.json")" || { log_quiescence_leaf pause; return 1; }
    assert_fabricd_admission_paused quiescence "$before_worker" || { log_quiescence_leaf pause; return 1; }
  fi

  fleet="$(curl -fsS --connect-timeout 10 --max-time 30 --header "@$FLEET_BUSY_HEADER_FILE" "$FLEET_BUSY_URL" 2>"$TMP_DIR/fleet-busy.err")" || { log_quiescence_leaf fleet; return 1; }
  scrub_file "$TMP_DIR/fleet-busy.err"
  jq -e '(.busy | type) == "number" and .busy == 0 and
    (.unverifiable | type) == "number" and .unverifiable == 0' <<<"$fleet" >/dev/null || { log_quiescence_leaf fleet; return 1; }

  # The spawn Worker version is checked from provider state, not inferred from
  # a local config file. Both pause bindings must be remotely set to "1".
  local spawn_version spawn_bindings
  spawn_version="$(capture_json spawn-deployments "$TMP_DIR/spawn-deployments.json" run_wrangle deployments list --name corelink-spawn-worker --json >/dev/null; jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$TMP_DIR/spawn-deployments.json")" || { log_quiescence_leaf pause; return 1; }
  spawn_bindings="$(capture_remote_bindings spawn-paused "$spawn_version" corelink-spawn-worker)" || { log_quiescence_leaf pause; return 1; }
  jq -e '([.[] | select(.name == "AUTOSCALER_REDRIVE_PAUSED" or .name == "AUTOSCALER_INTAKE_PAUSED")] | sort_by(.name)) as $pauses |
    ($pauses | length) == 2 and ($pauses | map(.name) | unique | length) == 2 and
    all($pauses[]; (.type == "plain_text" and (.temporary_value | type) == "string" and .temporary_value == "1"))' "$spawn_bindings" >/dev/null || { log_quiescence_leaf pause; return 1; }
  QUIESCENCE_STATE="green"
  log_event "quiescence=GREEN active_now=0 fabric_occupied=0 fleet_busy=0 fleet_unverifiable=0 ledger_cross_instance_safe=$LEDGER_CROSS_INSTANCE_SAFE intake_paused=1 redrive_paused=1"
}

mint_fixture() {
  local body_err="$TMP_DIR/mint.err" raw
  : > "$body_err"; chmod 600 "$body_err"
  set +e
  raw="$(jq -nc --arg tenant "$TENANT" --arg repo "$REPO_FULL_NAME" --arg installation "$INSTALLATION_ID" --rawfile pat "$PAT_FILE" \
    '{tenant:$tenant,repo_full_name:$repo,installation_id:$installation,acquiring_pat:($pat|sub("\\n$";""))}' \
    | curl -fsS --connect-timeout 10 --max-time 45 --header "@$HEADER_FILE" --header 'content-type: application/json' --data-binary @- "$BASE_URL/v1/test/mint-cred-ticket" 2>>"$body_err" \
    | jq -er '[.ticket,.lease_id] | @tsv' 2>>"$body_err")"
  local rc=$?
  set -e
  scrub_file "$body_err"
  [[ "$rc" == 0 ]] || return 1
  IFS=$'\t' read -r FIXTURE_TICKET FIXTURE_LEASE_ID <<< "$raw"
  [[ -n "$FIXTURE_TICKET" && -n "$FIXTURE_LEASE_ID" ]] || return 1
  # Success here is the live server's validation of the exact dogfood PAT,
  # tenant, GitHub repository, and installation tuple sent above.
  log_event "dogfood-mint-preflight=GREEN tenant=$TENANT repo=$REPO_FULL_NAME installation=$INSTALLATION_ID"
}

hmac_ticket() {
  local secret_file="$1" lease_id="$2"
  node -e '
    const crypto = require("crypto");
    const lease = process.argv[1];
    const chunks = [];
    process.stdin.on("data", c => chunks.push(c));
    process.stdin.on("end", () => {
      let key = Buffer.concat(chunks);
      if (key[key.length - 1] === 10) key = key.subarray(0, key.length - 1);
      if (key[key.length - 1] === 13) key = key.subarray(0, key.length - 1);
      const msg = Buffer.concat([Buffer.from("corelink/cred-ticket/v1:"), Buffer.from(lease)]);
      process.stdout.write(crypto.createHmac("sha256", key).update(msg).digest("base64"));
    });
  ' "$lease_id" < "$secret_file"
}

redeem_status() {
  local lease_id="$1" ticket="$2" err="$TMP_DIR/redeem.err" status
  : > "$err"; chmod 600 "$err"
  set +e
  status="$(printf '%s' "$ticket" | node -e 'let s=""; process.stdin.on("data",c=>s+=c); process.stdin.on("end",()=>process.stdout.write(JSON.stringify({ticket:s})));' \
    | curl -sS --connect-timeout 10 --max-time 30 -o /dev/null -w '%{http_code}' --header 'content-type: application/json' --data-binary @- "$BASE_URL/v1/leases/$lease_id/cas-cred" 2>>"$err")"
  local rc=$?
  set -e
  scrub_file "$err"
  [[ "$rc" == 0 ]] || return 1
  printf '%s' "$status"
}

redeem_and_probe_clw() {
  local lease_id="$1" ticket="$2" err="$TMP_DIR/redeem-success.err" raw body status
  local cas_pat clw_endpoint clw_tenant clw_header_fd clw_status
  : > "$err"; chmod 600 "$err"
  set +e
  raw="$(printf '%s' "$ticket" | node -e 'let s=""; process.stdin.on("data",c=>s+=c); process.stdin.on("end",()=>process.stdout.write(JSON.stringify({ticket:s})));' |
    curl -sS --connect-timeout 10 --max-time 30 --header 'content-type: application/json' --data-binary @- -w '\n%{http_code}' \
      "$BASE_URL/v1/leases/$lease_id/cas-cred" 2>>"$err")"
  local rc=$?
  set -e
  scrub_file "$err"
  [[ "$rc" == 0 ]] || return 1
  status="${raw##*$'\n'}"
  body="${raw%$'\n'*}"
  [[ "$status" == 200 ]] || return 1
  cas_pat="$(jq -er '.cas_pat | strings | select(length > 0)' <<<"$body")" || return 1
  clw_endpoint="$(jq -er '.clw_endpoint | strings | select(startswith("https://"))' <<<"$body")" || return 1
  clw_tenant="$(jq -er '.clw_tenant | strings' <<<"$body")" || return 1
  [[ "$clw_tenant" == "$TENANT" ]] || return 1
  clw_header_fd=<(printf 'Authorization: Bearer %s\n' "$cas_pat")
  # A tenant-bearing CAS read is authenticated with the redeemed per-job PAT;
  # the deliberately absent digest may return 404, but 401/403 proves the PAT
  # was not accepted. The PAT remains only in shell memory and this pipe.
  clw_status="$(curl -sS --connect-timeout 10 --max-time 30 -o /dev/null -w '%{http_code}' \
    --header "@$clw_header_fd" "$clw_endpoint/v1/cas/$clw_tenant/0000000000000000000000000000000000000000000000000000000000000000" \
    2>>"$TMP_DIR/clw-probe.err")" || return 1
  scrub_file "$TMP_DIR/clw-probe.err"
  [[ "$clw_status" != 401 && "$clw_status" != 403 ]] || return 1
  CAS_PAT_PROOF="GREEN tenant=$clw_tenant status=$clw_status"
  unset cas_pat clw_endpoint clw_tenant clw_header_fd raw body status
  log_event "redeemed-cas-pat-clw-probe=GREEN tenant=$TENANT status=$clw_status"
}

check_window() {
  local elapsed=$(( $(date +%s) - WINDOW_START ))
  [[ "$elapsed" -le 600 ]] || { log_event "window-seconds=$elapsed RED"; return 1; }
  log_event "window-seconds=$elapsed"
}

rollback_old() {
  # Re-arm the known OOB test key only long enough to prove the restored signer;
  # this is followed by a disarm/recreate before returning to the caller.
  put_secret FABRIC_CRED_TICKET_SECRET "$OLD_SECRET_FILE" || return 1
  put_secret FABRIC_TEST_MINT_KEY "$TEST_MINT_KEY_FILE" || return 1
  recreate 1 || return 1
  capture_state rollback || return 1
  mint_fixture || return 1
  local expected
  expected="$(hmac_ticket "$OLD_SECRET_FILE" "$FIXTURE_LEASE_ID")"
  [[ "$expected" == "$FIXTURE_TICKET" ]] || return 1
  [[ "$(redeem_status "$FIXTURE_LEASE_ID" "$expected")" == 200 ]] || return 1
  delete_test_key || return 1
  recreate 0 || return 1
  capture_state rollback-final || return 1
  assert_test_key_absent || return 1
  return 0
}

write_evidence() {
  local rc="$1" status observed elapsed out="$TMP_DIR/au1.8-evidence.json"
  observed="$(date -u +%FT%H:%M:%SZ)"
  if [[ "$WINDOW_START" != 0 ]]; then
    [[ "$WINDOW_FINISH" != 0 ]] || WINDOW_FINISH="$(date +%s)"
    elapsed=$((WINDOW_FINISH - WINDOW_START))
  else elapsed=0; fi
  [[ "$rc" == 0 && "$SUCCESS_CLEANUP_DONE" == 1 ]] && status=PASS || status=FAILED
  jq -n \
    --arg status "$status" --arg observed "$observed" --arg commit "$SOURCE_COMMIT" \
    --arg app_name "$CONTAINER_APP_NAME" --arg app_id_before "${BEFORE_APP_ID:-}" \
    --arg worker_before "${BEFORE_WORKER_VERSION:-}" --arg worker_rotated "${ROTATED_WORKER_VERSION:-}" --arg worker_final "${FINAL_WORKER_VERSION:-}" \
    --arg container_before "${BEFORE_CONTAINER_VERSION:-}" --arg container_rotated "${ROTATED_CONTAINER_VERSION:-}" --arg container_final "${FINAL_CONTAINER_VERSION:-}" \
    --arg digest_before "${BEFORE_DIGEST:-}" --arg digest_rotated "${ROTATED_DIGEST:-}" --arg digest_final "${FINAL_DIGEST:-}" \
    --arg baseline_sha256 "$REMOTE_BASELINE_SHA256" --arg log_path "$EVENT_LOG" \
    --arg quiescence "$QUIESCENCE_STATE" --arg ledger_safe "$LEDGER_CROSS_INSTANCE_SAFE" --arg stability "$PROVIDER_STABILITY_STATE" --arg cas_probe "$CAS_PAT_PROOF" --arg temp_state "$REMOTE_TEMP_VAR_STATE" \
    --argjson stability_interval "$PROVIDER_STABILITY_SECS" --argjson elapsed "$elapsed" \
    '{schema_version:"evidence/v1", artifact_id:"au1.8-fabricd-secret-rotation", kind:"probe", status:$status, observed_at:$observed,
      source:{repository:"corelink-runners", commit_sha:$commit, path:"docs/plan/evidence/au1.8-fabricd-secret-rotation.json"},
      claims:["AU1.8","AU1.8:secret-rotation"], version:{id:$commit},
      evidence:{operation:{app_name:$app_name, app_id_before:$app_id_before, worker_versions:{before:$worker_before, rotated:$worker_rotated, final:$worker_final}, container_versions:{before:$container_before, rotated:$container_rotated, final:$container_final}, image_digests:{before:$digest_before, rotated:$digest_rotated, final:$digest_final}},
      proofs:{old_hmac_prevalidated:true, old_hmac_redeem_status:401, new_hmac_redeem_status:200, replay_status:410, redeemed_cas_pat_clw:$cas_probe},
      quiescence:{gate:$quiescence, active_leases:0, active_jobs:0, fleet_busy:0, fleet_unverifiable:0, ledger_cross_instance_safe:($ledger_safe == "true"), intake_paused:true, redrive_paused:true},
      provider_stability:{gate:$stability, samples:2, interval_seconds:$stability_interval},
      remote_variables:{baseline_snapshot_sha256:$baseline_sha256, unknown_bindings_preserved:true, drift_refusal:true, temporary_tenant_binding:$temp_state, deploy_flags:["--keep-vars","--strict","--containers-rollout=immediate"]},
      secret_handling:{old_rollback_path:$ENV_OLD_SECRET_FILE, new_operational_path:$ENV_NEW_SECRET_FILE, values:"excluded", oob_mode:"0600"},
      timing:{maximum_seconds:600, elapsed_seconds:$elapsed, clock_starts_before_first_test_key_put:true, clock_ends_after_final_provider_capture:true},
      logs:{status_path:$log_path, secrets:"excluded", mode:"0600"}}}' \
    --arg ENV_OLD_SECRET_FILE "$OLD_SECRET_FILE" --arg ENV_NEW_SECRET_FILE "$NEW_SECRET_FILE" > "$out"
  chmod 600 "$out"
  mkdir -p "$(dirname -- "$EVIDENCE_PATH")"
  chmod 755 "$(dirname -- "$EVIDENCE_PATH")"
  mv -f -- "$out" "$EVIDENCE_PATH"
  chmod 644 "$EVIDENCE_PATH"
}

on_exit() {
  local rc=$?
  trap - EXIT
  if [[ "$CLEANUP_RUNNING" == 0 && "$MUTATION_STARTED" == 1 && "$SUCCESS_CLEANUP_DONE" == 0 ]]; then
    CLEANUP_RUNNING=1
    log_event "primary-result=RED; starting rollback"
    if rollback_old; then
      log_event "rollback=GREEN old-signer-proved test-mint-disarmed"
    else
      log_event "rollback=RED escalation-required"
      rc=1
    fi
  fi
  if [[ "$MUTATION_STARTED" == 1 ]]; then
    write_evidence "$rc" || rc=1
  fi
  find "$TMP_DIR" -type f -exec rm -f -- {} + 2>/dev/null || true
  rmdir "$TMP_DIR" 2>/dev/null || true
  exit "$rc"
}
trap on_exit EXIT

# Baseline is captured after resolving the exact app and before any secret put.
capture_state before || { echo "baseline health/provider capture failed" >&2; exit 1; }
BEFORE_DIGEST="$EXPECTED_IMAGE_DIGEST"
BEFORE_APP_ID="$APP_ID"
REMOTE_BASELINE_FILE="$(capture_remote_bindings baseline "$BEFORE_WORKER_VERSION")" || {
  echo "remote binding baseline snapshot failed" >&2; exit 1;
}
jq -e 'map(select(.name == "FABRIC_TEST_MINT_TENANTS")) | length == 0' "$REMOTE_BASELINE_FILE" >/dev/null || {
  echo "temporary tenant binding is already present remotely" >&2; exit 1;
}
REMOTE_BASELINE_SHA256="$(hash_file "$REMOTE_BASELINE_FILE")"
REMOTE_TEMP_VAR_STATE="baseline"
log_event "remote-binding-baseline=GREEN sha256=$REMOTE_BASELINE_SHA256"
preflight_dogfood_pat || { echo "dogfood PAT tenant/scope preflight failed" >&2; exit 1; }
quiescence_gate || { echo "authoritative quiescence gate failed; refusing mutation" >&2; exit 1; }
# Provider state is sampled twice immediately before the first secret write.
# Both the Worker deployment and the Containers application version/digest must
# remain byte-identical to the captured baseline; a concurrent deploy is RED.
provider_stability_gate "$BEFORE_WORKER_VERSION" "$BEFORE_CONTAINER_VERSION" "$BEFORE_DIGEST" || {
  echo "provider deployment changed or was unstable before mutation; refusing AU1.8" >&2
  exit 1
}

MUTATION_STARTED=1
WINDOW_START="$(date +%s)"
put_secret FABRIC_TEST_MINT_KEY "$TEST_MINT_KEY_FILE" || exit 1
recreate 1 || exit 1
capture_state armed || exit 1
assert_remote_bindings armed "$CURRENT_WORKER_VERSION" armed || exit 1
HEADER_FILE="$(make_header_file)"

# This fixture is deliberately retained only in memory until rotation. It binds
# the old lease to the deployed old signer, then becomes the 401 proof.
mint_fixture || exit 1
OLD_FIXTURE_TICKET="$FIXTURE_TICKET"
OLD_FIXTURE_LEASE_ID="$FIXTURE_LEASE_ID"
OLD_HMAC="$(hmac_ticket "$OLD_SECRET_FILE" "$OLD_FIXTURE_LEASE_ID")"
[[ "$OLD_HMAC" == "$OLD_FIXTURE_TICKET" ]] || { echo "old HMAC does not match deployed signer" >&2; exit 1; }
log_event "prevalidate-old-hmac=GREEN ticket-redacted"

umask 077
mkdir -p "$(dirname -- "$NEW_SECRET_FILE")"
openssl rand -base64 48 | tr -d '\n' > "$NEW_SECRET_FILE"
chmod 600 "$NEW_SECRET_FILE"
file_owner_mode_ok "$NEW_SECRET_FILE" || { echo "generated NEW secret is not mode 0600" >&2; exit 1; }
put_secret FABRIC_CRED_TICKET_SECRET "$NEW_SECRET_FILE" || exit 1
check_window || exit 1
recreate 1 || exit 1
capture_state rotated || exit 1
assert_remote_bindings rotated "$CURRENT_WORKER_VERSION" armed || exit 1
[[ "$ROTATED_DIGEST" == "$BEFORE_DIGEST" ]] || { echo "image digest changed during rotation" >&2; exit 1; }

[[ "$(redeem_status "$OLD_FIXTURE_LEASE_ID" "$OLD_HMAC")" == 401 ]] || { echo "old HMAC was not rejected with 401" >&2; exit 1; }
log_event "old-hmac-redeem=401 GREEN"

mint_fixture || exit 1
NEW_SERVER_TICKET="$FIXTURE_TICKET"
NEW_HMAC="$(hmac_ticket "$NEW_SECRET_FILE" "$FIXTURE_LEASE_ID")"
[[ "$NEW_HMAC" == "$NEW_SERVER_TICKET" ]] || { echo "new HMAC does not match server ticket" >&2; exit 1; }
redeem_and_probe_clw "$FIXTURE_LEASE_ID" "$NEW_HMAC" || { echo "new HMAC/CAS PAT authenticated CLW proof failed" >&2; exit 1; }
[[ "$(redeem_status "$FIXTURE_LEASE_ID" "$NEW_HMAC")" == 410 ]] || { echo "new ticket replay was not rejected with 410" >&2; exit 1; }
log_event "new-hmac-redeem=200 replay=410 GREEN"
check_window || exit 1

[[ "$ROTATED_DIGEST" == "$BEFORE_DIGEST" ]] || exit 1
delete_test_key || exit 1
recreate 0 || exit 1
capture_state final || exit 1
assert_remote_bindings final "$CURRENT_WORKER_VERSION" disarmed || exit 1
[[ "$FINAL_DIGEST" == "$BEFORE_DIGEST" ]] || { echo "final image digest changed" >&2; exit 1; }
assert_test_key_absent || { echo "temporary test-mint key remained armed" >&2; exit 1; }
WINDOW_FINISH="$(date +%s)"
check_window || exit 1
log_event "AU1.8=PASS new-secret-preserved old-rollback-preserved"
SUCCESS_CLEANUP_DONE=1
echo "AU1.8 PASS; status log: $EVENT_LOG; NEW secret preserved at configured OOB path"
