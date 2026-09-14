#!/usr/bin/env bash
# Focused, network-free gates for the AU1.8 destructive harness.
set -Eeuo pipefail
umask 077

here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/au1.8-fabricd-cred-ticket-rotation.sh"
repo="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-selftest.XXXXXX")"
test_mint_key="$(mktemp "${TMPDIR:-/tmp}/au1.8-test-mint.XXXXXX")"
observability_key="$(mktemp "${TMPDIR:-/tmp}/au1.8-observability.XXXXXX")"
access_id="$(mktemp "${TMPDIR:-/tmp}/au1.8-access-id.XXXXXX")"
access_secret="$(mktemp "${TMPDIR:-/tmp}/au1.8-access-secret.XXXXXX")"
trap 'rm -rf -- "$repo" "$test_mint_key" "$observability_key" "$access_id" "$access_secret"' EXIT

git -C "$repo" init -q
git -C "$repo" config user.email au1.8-selftest@example.invalid
git -C "$repo" config user.name au1.8-selftest
printf 'tracked\n' > "$repo/tracked"
git -C "$repo" add tracked
git -C "$repo" commit -q -m baseline
head="$(git -C "$repo" rev-parse HEAD)"

dispatch_case() {
  local label="$1" version="${2-}" case_repo case_repo_real case_log case_err sha rc
  case_repo="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-wrangler-dispatch.XXXXXX")"
  case_repo_real="$(cd "$case_repo" && pwd -P)"
  mkdir -p "$case_repo/deploy/cloudflare-fabricd/node_modules/.bin"
  cp "$here/../../deploy/cloudflare-fabricd/wrangler.jsonc" "$case_repo/deploy/cloudflare-fabricd/wrangler.jsonc"
  case "$label" in
    missing) : ;;
    wrong|correct)
      # shellcheck disable=SC2016
      printf '%s\n' '#!/bin/bash' \
        'printf "%s|%s\\n" "$PWD" "$*" >> "${DISPATCH_LOG:?}"' \
        "if [ \"\${1:-}\" = --version ]; then printf '%s\\n' '$version'; exit 0; fi" \
        'if [ "${1:-}" = auth ] && [ "${2:-}" = token ]; then printf '\''{"token":"dispatch-test-token-1234567890"}\n'\''; exit 0; fi' \
        'exit 1' > "$case_repo/deploy/cloudflare-fabricd/node_modules/.bin/wrangler"
      chmod 700 "$case_repo/deploy/cloudflare-fabricd/node_modules/.bin/wrangler"
      ;;
    *) echo "unknown dispatch test case: $label" >&2; exit 1 ;;
  esac
  git -C "$case_repo" init -q
  git -C "$case_repo" config user.email au1.8-dispatch@example.invalid
  git -C "$case_repo" config user.name au1.8-dispatch
  git -C "$case_repo" add -f .
  git -C "$case_repo" commit -qm baseline
  sha="$(git -C "$case_repo" rev-parse HEAD)"
  case_log="$(mktemp "${TMPDIR:-/tmp}/au1.8-wrangler-dispatch-log.XXXXXX")"
  case_err="$(mktemp "${TMPDIR:-/tmp}/au1.8-wrangler-dispatch-err.XXXXXX")"
  set +e
  DISPATCH_LOG="$case_log" AU18_TEST_SHELL_WRANGLER=1 AU18_REPO_ROOT="$case_repo" AU18_SOURCE_COMMIT="$sha" \
    "$harness" --execute --ack-destructive >/dev/null 2>"$case_err"
  rc=$?
  set -e
  case "$label" in
    missing) [[ "$rc" != 0 ]] && rg -q 'missing local Wrangler binary' "$case_err" ;;
    wrong) [[ "$rc" != 0 ]] && rg -q 'local Wrangler version mismatch' "$case_err" ;;
    correct) [[ "$rc" != 0 ]] && grep -F -q "$case_repo_real/deploy/cloudflare-fabricd|--config $case_repo_real/deploy/cloudflare-fabricd/wrangler.jsonc containers list --json" "$case_log" ;;
  esac
  local result=$?
  rm -rf -- "$case_repo" "$case_log" "$case_err"
  return "$result"
}

if dispatch_case missing; then :; else echo 'FAIL: missing local Wrangler must refuse before provider work' >&2; exit 1; fi
if dispatch_case wrong 4.104.0; then :; else echo 'FAIL: wrong local Wrangler version must refuse' >&2; exit 1; fi
if dispatch_case correct 4.105.0; then :; else echo 'FAIL: local Wrangler dispatch must preserve cwd and config' >&2; exit 1; fi
if rg -n '\bnpx\b' "$harness" >/dev/null; then
  echo 'FAIL: AU1.8 must not resolve Wrangler through npx' >&2
  exit 1
fi
if ! rg -n -- '--header "@\$INTROSPECT_HEADER_FILE"' "$harness" >/dev/null ||
   ! rg -n 'INTROSPECT_URL.*corelink-api\.humangr\.com/internal/v1/auth/introspect|INTROSPECT_URL.*CANONICAL' "$harness" >/dev/null; then
  echo 'FAIL: canonical introspection must use the combined temporary header file' >&2
  exit 1
fi

validate() {
  local status_json="${2-}"
  [[ -n "$status_json" ]] || status_json='{"num_shards":1,"ledger_cross_instance_safe":true}'
  AU18_REPO_ROOT="$repo" \
  AU18_SOURCE_COMMIT="${1:-$head}" \
  AU18_VALIDATE_ONLY=1 \
  AU18_STATUS_REPORT_JSON="$status_json" \
    "$harness" --execute --ack-destructive >/dev/null 2>&1
}

validate_files() {
  AU18_REPO_ROOT="$repo" \
  AU18_SOURCE_COMMIT="$head" \
  AU18_VALIDATE_ONLY=1 \
  AU18_VALIDATE_FILE_METADATA_ONLY=1 \
  AU18_TEST_MINT_KEY_FILE="$test_mint_key" \
  AU18_OBSERVABILITY_KEY_FILE="$observability_key" \
    "$harness" --execute --ack-destructive >/dev/null 2>&1
}

validate_stability() {
  AU18_REPO_ROOT="$repo" \
  AU18_SOURCE_COMMIT="$head" \
  AU18_VALIDATE_ONLY=1 \
  AU18_PROVIDER_STABILITY_SECS=0 \
  AU18_PROVIDER_STABILITY_SAMPLE_1="$1" \
  AU18_PROVIDER_STABILITY_SAMPLE_2="$2" \
    "$harness" --execute --ack-destructive >/dev/null 2>&1
}

validate_access_pair() {
  AU18_REPO_ROOT="$repo" \
  AU18_SOURCE_COMMIT="$head" \
  AU18_VALIDATE_ONLY=1 \
  AU18_ACCESS_CLIENT_ID_FILE="$1" \
  AU18_ACCESS_CLIENT_SECRET_FILE="$2" \
    "$harness" --execute --ack-destructive >/dev/null 2>&1
}

container_version_case() {
  local expected="$1" payload="$2" expected_value="${3-}" extractor info output rc
  extractor="$(sed -n '/^extract_container_version() {/,/^}$/p' "$harness")"
  info="$(mktemp "${TMPDIR:-/tmp}/au1.8-container-info.XXXXXX")"
  printf '%s\n' "$payload" > "$info"
  set +e
  output="$(bash -u -c '
    set -Eeuo pipefail
    eval "$1"
    extract_container_version "$2"
  ' -- "$extractor" "$info")"
  rc=$?
  set -e
  rm -f -- "$info"
  if [[ "$expected" == pass ]]; then
    [[ "$rc" == 0 && "$output" == "$expected_value" ]]
  else
    [[ "$rc" != 0 ]]
  fi
}

unset_local_regression() {
  local remote_fn header_fn mock key
  remote_fn="$(sed -n '/^capture_remote_bindings() {/,/^}$/p' "$harness")"
  header_fn="$(sed -n '/^make_oob_header_file() {/,/^}$/p' "$harness")"
  mock="$(mktemp "${TMPDIR:-/tmp}/au1.8-mock-wrangler.XXXXXX")"
  key="$(mktemp "${TMPDIR:-/tmp}/au1.8-mock-key.XXXXXX")"
  printf '%s\n' '#!/bin/bash' \
    'printf '\''{"bindings":[{"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","text":""},{"name":"AUTOSCALER_TOKEN","type":"secret_text","text":"secret-literal"},{"name":"UNEXPECTED_OPAQUE","type":"opaque","text":"opaque-literal"}]}'\''' > "$mock"
  printf 'mock-observability-key\n' > "$key"
  chmod 700 "$mock"
  chmod 600 "$key"
  # Execute the production function bodies under nounset with no globals named
  # `label` or `name`; this catches premature expansion in local declarations.
  # shellcheck disable=SC2016
  if ! env -u label -u name bash -u -c '
    set -Eeuo pipefail
    source="$1"
    mock="$2"
    key="$3"
    eval "$source"
    TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-local-regression.XXXXXX")"
    LOG_DIR="$TMP_DIR/log"
    mkdir -p "$LOG_DIR"
    scrub_file() { :; }
    log_event() { :; }
    # Invoke the shell fixture through bash explicitly.  On macOS, executing
    # a temporary text file directly can block in the local process runner
    # before the fixture reaches its first command; this keeps the regression
    # test bounded to the helper under test.
    WRANGLER=(/bin/bash "$mock")
    run_wrangle() { "${WRANGLER[@]}" "$@"; }
    WORKER_NAME=corelink-fabricd
    CONTAINER_APP_NAME=corelink-fabricd-fabriccontainer
    result="$(capture_remote_bindings sample version-a)"
    test -f "$result"
    jq -e "
      length == 3 and
      (map(select(.name == \"FABRIC_TEST_MINT_TENANTS\")) | length) == 1 and
      (map(select(.name == \"AUTOSCALER_TOKEN\")) | .[0].type) == \"secret_text\" and
      (map(select(.name == \"AUTOSCALER_TOKEN\")) | .[0].temporary_value) == null and
      (map(select(.name == \"UNEXPECTED_OPAQUE\")) | .[0].type) == \"opaque\" and
      (map(select(.name == \"UNEXPECTED_OPAQUE\")) | .[0].temporary_value) == null
    " "$result" >/dev/null
    ! rg -q "secret-literal|opaque-literal" "$result"
    header="$(make_oob_header_file observability "$key")"
    test "$(sed -n "1p" "$header")" = "X-Corelink-Internal-Auth: mock-observability-key"
    rm -rf -- "$TMP_DIR"
  ' -- "$remote_fn
$header_fn" "$mock" "$key"; then
    rm -f -- "$mock" "$key"
    return 1
  fi
  rm -f -- "$mock" "$key"
}

admission_pause_case() {
  local expected="$1" payload="$2" capture_fn assert_fn mock rc
  capture_fn="$(sed -n '/^capture_fabricd_admission_pause() {/,/^}$/p' "$harness")"
  assert_fn="$(sed -n '/^assert_fabricd_admission_paused() {/,/^}$/p' "$harness")"
  mock="$(mktemp "${TMPDIR:-/tmp}/au1.8-admission-mock.XXXXXX")"
  # shellcheck disable=SC2016
  printf '%s\n' '#!/bin/bash' 'printf "%s\\n" "${MOCK_BINDINGS_JSON:?}"' > "$mock"
  chmod 700 "$mock"
  set +e
  # shellcheck disable=SC2016
  env -u label -u version_id bash -u -c '
    set -Eeuo pipefail
    capture_fn="$1"
    assert_fn="$2"
    mock="$3"
    payload="$4"
    eval "$capture_fn"
    eval "$assert_fn"
    TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-admission-case.XXXXXX")"
    LOG_DIR="$TMP_DIR/log"
    mkdir -p "$LOG_DIR"
    scrub_file() { :; }
    log_event() { :; }
    run_wrangle() { MOCK_BINDINGS_JSON="$payload" "$mock" "$@"; }
    WORKER_NAME=corelink-fabricd
    assert_fabricd_admission_paused sample version-a
    rm -rf -- "$TMP_DIR"
  ' -- "$capture_fn" "$assert_fn" "$mock" "$payload"
  rc=$?
  set -e
  rm -f -- "$mock"
  if [[ "$expected" == pass ]]; then
    [[ "$rc" == 0 ]]
  else
    [[ "$rc" != 0 ]]
  fi
}

remote_binding_case() {
  local expected="$1" mode="$2" payload="$3" assert_fn predicate tmp_dir baseline_file rc
  assert_fn="$(sed -n '/^assert_remote_bindings() {/,/^}$/p' "$harness")"
  predicate="$(sed -n '/^assert_temp_binding_shape() {/,/^}$/p' "$harness")"
  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-remote-bindings.XXXXXX")"
  baseline_file="$tmp_dir/baseline.json"
  printf '%s\n' '[{"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null}]' > "$baseline_file"
  set +e
  SNAPSHOT="$payload" bash -u -c '
    set -Eeuo pipefail
    eval "$1"
    eval "$2"
    TMP_DIR="$3"
    REMOTE_BASELINE_FILE="$4"
    TENANT="tenant-a"
    REMOTE_TEMP_VAR_STATE=""
    log_event() { :; }
    capture_remote_bindings() { printf "%s\n" "$SNAPSHOT" > "$TMP_DIR/snapshot.json"; printf "%s\n" "$TMP_DIR/snapshot.json"; }
    assert_remote_bindings fixture version-a "$5"
  ' -- "$assert_fn" "$predicate" "$tmp_dir" "$baseline_file" "$mode"
  rc=$?
  set -e
  rm -rf -- "$tmp_dir"
  if [[ "$expected" == pass ]]; then
    [[ "$rc" == 0 ]]
  else
    [[ "$rc" != 0 ]]
  fi
}

remote_baseline_case() {
  local expected="$1" payload="$2" predicate tmp_dir baseline_file rc
  predicate="$(sed -n '/^assert_temp_binding_shape() {/,/^}$/p' "$harness")"
  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-remote-baseline.XXXXXX")"
  baseline_file="$tmp_dir/baseline.json"
  printf '%s\n' "$payload" > "$baseline_file"
  set +e
  bash -u -c '
    set -Eeuo pipefail
    eval "$1"
    assert_temp_binding_shape "$2" baseline
  ' -- "$predicate" "$baseline_file"
  rc=$?
  set -e
  rm -rf -- "$tmp_dir"
  if [[ "$expected" == pass ]]; then
    [[ "$rc" == 0 ]]
  else
    [[ "$rc" != 0 ]]
  fi
}

access_header_case() {
  local header_fn scrub_fn id secret key tmp output
  header_fn="$(sed -n '/^make_introspect_header_file() {/,/^}$/p' "$harness")"
  scrub_fn="$(sed -n '/^scrub_file() {/,/^}$/p' "$harness")"
  id="$(mktemp "${TMPDIR:-/tmp}/au1.8-access-header-id.XXXXXX")"
  secret="$(mktemp "${TMPDIR:-/tmp}/au1.8-access-header-secret.XXXXXX")"
  key="$(mktemp "${TMPDIR:-/tmp}/au1.8-access-header-key.XXXXXX")"
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-access-header-case.XXXXXX")"
  printf 'access-id-value\n' > "$id"
  printf 'access-secret-value\n' > "$secret"
  printf 'internal-key-value\n' > "$key"
  chmod 600 "$id" "$secret" "$key"
  # The helper returns only the temporary path; the scrubber removes both
  # Access header values before an error file can be retained.
  # shellcheck disable=SC2016
  output="$(bash -u -c '
    set -Eeuo pipefail
    eval "$1"
    eval "$2"
    TMP_DIR="$3"
    INTROSPECT_KEY_FILE="$4"
    ACCESS_CLIENT_ID_FILE="$5"
    ACCESS_CLIENT_SECRET_FILE="$6"
    header="$(make_introspect_header_file)"
    grep -F "CF-Access-Client-Id: access-id-value" "$header" >/dev/null
    grep -F "CF-Access-Client-Secret: access-secret-value" "$header" >/dev/null
    printf "CF-Access-Client-Id: access-id-value\nCF-Access-Client-Secret: access-secret-value\n" > "$TMP_DIR/error"
    scrub_file "$TMP_DIR/error"
    ! grep -F "access-id-value" "$TMP_DIR/error" >/dev/null
    ! grep -F "access-secret-value" "$TMP_DIR/error" >/dev/null
    printf "%s" "$header"
  ' -- "$header_fn" "$scrub_fn" "$tmp" "$key" "$id" "$secret")"
  test -n "$output"
  rm -rf -- "$id" "$secret" "$key" "$tmp"
}

usage_fetch_case() {
  local expected="$1" http="$2" curl_rc="$3" fetch_fn header_fn scrub_fn tmp mock args log output rc
  fetch_fn="$(sed -n '/^fetch_usage_response() {/,/^}$/p' "$harness")"
  header_fn="$(sed -n '/^make_pat_header_file() {/,/^}$/p' "$harness")"
  scrub_fn="$(sed -n '/^scrub_file() {/,/^}$/p' "$harness")"
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-usage-case.XXXXXX")"
  mock="$(mktemp "${TMPDIR:-/tmp}/au1.8-usage-curl.XXXXXX")"
  args="$(mktemp "${TMPDIR:-/tmp}/au1.8-usage-args.XXXXXX")"
  log="$(mktemp "${TMPDIR:-/tmp}/au1.8-usage-log.XXXXXX")"
  printf '%s\n' '#!/bin/bash' \
    'printf "%s\n" "$*" > "${MOCK_ARGS:?}"' \
    'out=""; headers=""; while (($#)); do case "$1" in -o) out="${2:?}"; shift 2 ;; -D) headers="${2:?}"; shift 2 ;; -w) shift 2 ;; *) shift ;; esac; done' \
    'printf "%s\n" '\''{"tenant":"ee30f7ba-fc25-4d71-939e-ebe130b4c6a3","active_now":0}'\'' > "${MOCK_BODY:?}"; [[ -z "$out" || "$out" == "$MOCK_BODY" ]] || cp -- "${MOCK_BODY:?}" "$out"' \
    '[[ -z "$headers" ]] || printf "HTTP/1.1 %s\\r\\n\\r\\n" "${MOCK_HTTP:?}" > "$headers"' \
    'printf "%s" "${MOCK_HTTP:?}"; exit "${MOCK_RC:?}"' > "$mock"
  chmod 700 "$mock"
  printf 'usage-secret-value\n' > "$tmp/pat"
  chmod 600 "$tmp/pat"
  set +e
  output="$(MOCK_ARGS="$args" MOCK_BODY="$tmp/usage.body" MOCK_HTTP="$http" MOCK_RC="$curl_rc" PATH="$(dirname "$mock"):$PATH" \
    bash -u -c '
      set -Eeuo pipefail
      eval "$1"; eval "$2"; eval "$3"
      TMP_DIR="$4"; PAT_FILE="$TMP_DIR/pat"; USAGE_URL=https://usage.invalid; EVENT_LOG="$5"
      curl() { command "'"$mock"'" "$@"; }
      log_event() { printf "%s\n" "$*" >> "$EVENT_LOG"; }
      if usage="$(fetch_usage_response)"; then
        [[ "'"$expected"'" == pass ]] || exit 10
        jq -e ".active_now == 0" <<<"$usage" >/dev/null || exit 11
        printf passed > "$TMP_DIR/occupancy"
      else
        [[ "'"$expected"'" == fail ]] || exit 12
      fi
    ' -- "$fetch_fn" "$header_fn" "$scrub_fn" "$tmp" "$log")"
  rc=$?
  set -e
  grep -F 'usage-secret-value' "$args" >/dev/null && rc=1
  [[ "$(wc -l < "$args" | tr -d ' ')" == 1 ]] || rc=1
  if [[ "$expected" == fail ]]; then
    rg -q "usage-failure curl_rc=$curl_rc http_status=$http" "$log" || rc=1
    [[ ! -e "$tmp/occupancy" ]] || rc=1
  else
    [[ "$rc" == 0 && -e "$tmp/occupancy" ]] || rc=1
  fi
  rm -rf -- "$tmp" "$mock" "$args" "$log"
  return "$rc"
}

if ! usage_fetch_case fail 000 26 ||
   ! usage_fetch_case fail 503 0 ||
   ! usage_fetch_case pass 200 0; then
  echo "FAIL: usage must use a stable redacted header file, require HTTP 200, and make one GET" >&2
  exit 1
fi

validate

printf 'untracked\n' > "$repo/untracked"
if validate; then
  echo "FAIL: untracked files must block AU1.8" >&2
  exit 1
fi
rm -f -- "$repo/untracked"

if validate "0000000000000000000000000000000000000000"; then
  echo "FAIL: stale source commit must block AU1.8" >&2
  exit 1
fi

validate "$head" '{"num_shards":1,"ledger_cross_instance_safe":true}'
if validate "$head" '{"num_shards":1,"ledger_cross_instance_safe":false}'; then
  echo 'FAIL: unsafe ledger fixture must stop before mutation' >&2
  exit 1
fi
if validate "$head" '{"num_shards":1}'; then
  echo "FAIL: missing ledger_cross_instance_safe must block AU1.8" >&2
  exit 1
fi

if rg -n '3900|memory_singleton_status_ok|provider_timestamp_epoch|memory_singleton_age_gate|memory-singleton' "$harness" "$here/../../docs/runbook/secret-rotation.md" >/dev/null; then
  echo "FAIL: obsolete in-memory age substitute remains" >&2
  exit 1
fi

capacity_fn="$(sed -n '/^assert_singleton_capacity() {/,/^}$/p' "$harness")"
capacity_case() {
  local expected="$1" payload="$2" info rc
  info="$(mktemp "${TMPDIR:-/tmp}/au1.8-capacity.XXXXXX")"
  printf '%s\n' "$payload" > "$info"
  set +e
  # shellcheck disable=SC2016
  env -u info bash -u -c 'set -Eeuo pipefail; eval "$1"; assert_singleton_capacity "$2"' -- "$capacity_fn" "$info"
  rc=$?
  set -e
  rm -f -- "$info"
  [[ "$expected" == pass && "$rc" == 0 || "$expected" == fail && "$rc" != 0 ]]
}
capacity_case pass '{"name":"corelink-fabricd-fabricdcontainer","max_instances":1}'
capacity_case fail '{"name":"corelink-fabricd-fabricdcontainer","metadata":{"max_instances":1}}'
capacity_case fail '{"name":"corelink-fabricd-fabricdcontainer"}'
capacity_case fail '{"name":"corelink-fabricd-fabricdcontainer","max_instances":"1"}'

# /v1/usage is the first live leaf in quiescence. Exercise its fail-closed
# shape predicate with mocked payloads and verify its HTTP diagnostics retain
# bounded metadata only; the response body and bearer-like values never log.
usage_shape_case() {
  local expected="$1" payload="$2" rc
  set +e
  jq -e --arg tenant tenant-a '.tenant == $tenant and ((.active_now // .activeNow) | tonumber) == 0' <<<"$payload" >/dev/null
  rc=$?
  set -e
  [[ "$expected" == pass && "$rc" == 0 || "$expected" == fail && "$rc" != 0 ]]
}
usage_shape_case pass '{"tenant":"tenant-a","active_now":0}'
usage_shape_case pass '{"tenant":"tenant-a","activeNow":0}'
usage_shape_case fail 'not-json'
usage_shape_case fail '{"tenant":"tenant-a"}'
usage_shape_case fail '{"tenant":"tenant-a","active_now":1}'
usage_log_fn="$(sed -n '/^log_usage_fetch_failure() {/,/^}$/p' "$harness")"
usage_log_dir="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-usage-log.XXXXXX")"
usage_log_headers="$usage_log_dir/headers"
usage_log="$usage_log_dir/events"
printf 'Content-Type: application/json\nCF-Ray: ray-123\nX-Leak: bearer-secret-value\n' > "$usage_log_headers"
# shellcheck disable=SC2016
if ! env -u status bash -u -c '
  set -Eeuo pipefail
  eval "$1"
  EVENT_LOG="$3"
  log_event() { printf "%s\n" "$*" >> "$EVENT_LOG"; }
  log_usage_fetch_failure "$2" "$4" http
  rg -q "leaf=usage_fetch reason=http http_status=503 content_type=application/json cf_ray=ray-123" "$3"
  ! rg -q "bearer-secret-value" "$3"
' -- "$usage_log_fn" 503 "$usage_log" "$usage_log_headers"; then
  rm -rf -- "$usage_log_dir"
  echo "FAIL: usage HTTP failure diagnostics must be sanitized and bounded" >&2
  exit 1
fi
rm -rf -- "$usage_log_dir"

# A usage failure is the first live leaf: one mocked curl call must stop the
# gate before occupancy, status, fleet, or pause reads can occur.
quiescence_fn="$(sed -n '/^quiescence_gate() {/,/^}$/p' "$harness")"
quiescence_header_fn="$(sed -n '/^make_pat_header_file() {/,/^}$/p' "$harness")"
quiescence_log_dir="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-quiescence-order.XXXXXX")"
quiescence_pat="$quiescence_log_dir/pat"
printf 'bearer-secret-value\n' > "$quiescence_pat"
# shellcheck disable=SC2016
if ! env -u response -u fleet -u status_report bash -u -c '
  set -Eeuo pipefail
  eval "$1"
  eval "$2"
  eval "$3"
  TMP_DIR="$4"; LOG_DIR="$TMP_DIR/log"; mkdir -p "$LOG_DIR"
  PAT_FILE="$5"; TENANT=tenant-a; EVENT_LOG="$TMP_DIR/events"; CALLS="$TMP_DIR/calls"
  USAGE_URL=https://usage.invalid/v1/usage
  OBSERVABILITY_URL=https://observability.invalid/occupancy
  STATUS_URL=https://observability.invalid/status
  FLEET_BUSY_URL=https://fleet.invalid/busy
  OBSERVABILITY_KEY_FILE=unused; FLEET_BUSY_KEY_FILE=unused
  scrub_file() { :; }
  make_oob_header_file() { printf "%s/%s-header\n" "$TMP_DIR" "$1"; }
  log_event() { printf "%s\n" "$*" >> "$EVENT_LOG"; }
  curl() {
    printf "%s\n" "$*" >> "$CALLS"
    local header_file= body_file=
    while (($#)); do
      case "$1" in
        -D) header_file="$2"; shift 2 ;;
        -o) body_file="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    printf "Content-Type: application/json\nCF-Ray: ray-123\n" > "$header_file"
    printf "bearer-secret-value" > "$body_file"
    return 22
  }
  : > "$CALLS"; : > "$EVENT_LOG"
  if quiescence_gate; then exit 1; fi
  test "$(wc -l < "$CALLS" | tr -d " ")" -eq 1
  rg -q "usage\\.invalid" "$CALLS"
  ! rg -q "observability\\.invalid|fleet\\.invalid" "$CALLS"
  rg -q "leaf=usage_fetch reason=http http_status=000 content_type=application/json cf_ray=ray-123" "$EVENT_LOG"
  ! rg -q "bearer-secret-value" "$EVENT_LOG"
' -- "$quiescence_fn" "$quiescence_header_fn" "$usage_log_fn" "$quiescence_log_dir" "$quiescence_pat"; then
  rm -rf -- "$quiescence_log_dir"
  echo "FAIL: first usage failure must stop quiescence reads and remain sanitized" >&2
  exit 1
fi
rm -rf -- "$quiescence_log_dir"

# Explicitly reject malformed HTTP status metadata rather than interpolating it.
usage_log_dir="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-usage-malformed.XXXXXX")"
usage_log_headers="$usage_log_dir/headers"
usage_log="$usage_log_dir/events"
printf 'Content-Type: application/json\nCF-Ray: ray-123\n' > "$usage_log_headers"
# shellcheck disable=SC2016
if ! env -u status bash -u -c '
  set -Eeuo pipefail
  eval "$1"
  EVENT_LOG="$3"
  log_event() { printf "%s\n" "$*" >> "$EVENT_LOG"; }
  log_usage_fetch_failure "$2" "$4" http
  rg -q "http_status=000" "$3"
' -- "$usage_log_fn" '503x' "$usage_log" "$usage_log_headers"; then
  echo "FAIL: malformed usage HTTP status must be recorded as 000" >&2
  rm -rf -- "$usage_log_dir"
  exit 1
fi
rm -rf -- "$usage_log_dir"

if jq -e '(.busy | type) == "number" and .busy == 0 and (.unverifiable | type) == "number" and .unverifiable == 0' <<< '{"unverifiable":0}' >/dev/null; then
  echo "FAIL: missing fleet busy must fail closed" >&2; exit 1
fi
occupancy_filter='(.per_tenant | type) == "array" and (.per_tenant | all(.[]; ((.occupied | type) == "number" and .occupied == 0)))'
if ! jq -e "$occupancy_filter" <<< '{"per_tenant":[]}' >/dev/null; then
  echo "FAIL: empty occupancy must pass when the tenant set is empty" >&2; exit 1
fi
if ! jq -e "$occupancy_filter" <<< '{"per_tenant":[{"tenant":"tenant-a","occupied":0,"peak":0}]}' >/dev/null; then
  echo "FAIL: zero occupancy singleton must pass" >&2; exit 1
fi
for fixture in \
  '{"per_tenant":[{"tenant":"tenant-a","occupied":1}]}' \
  '{"per_tenant":[{"tenant":"tenant-a"}]}' \
  '{"per_tenant":[{"tenant":"tenant-a","occupied":"0"}]}' \
  '{"per_tenant":{"tenant-a":{"occupied":0}}}' \
  '{"per_tenant":[[{"tenant":"tenant-a","occupied":0}]]}'; do
  if jq -e "$occupancy_filter" <<< "$fixture" >/dev/null; then
    echo "FAIL: malformed or occupied occupancy must fail closed: $fixture" >&2; exit 1
  fi
done

pause_filter='([.[] | select(.name == "AUTOSCALER_REDRIVE_PAUSED" or .name == "AUTOSCALER_INTAKE_PAUSED")] | sort_by(.name)) as $pauses | ($pauses | length) == 2 and ($pauses | map(.name) | unique | length) == 2 and all($pauses[]; (.type == "plain_text" and (.temporary_value | type) == "string" and .temporary_value == "1"))'
if ! jq -e "$pause_filter" <<< '[{"name":"AUTOSCALER_REDRIVE_PAUSED","type":"plain_text","temporary_value":"1"},{"name":"AUTOSCALER_INTAKE_PAUSED","type":"plain_text","temporary_value":"1"}]' >/dev/null; then
  echo "FAIL: both pause bindings set to string 1 must pass" >&2; exit 1
fi
for fixture in \
  '[{"name":"AUTOSCALER_REDRIVE_PAUSED","type":"plain_text","temporary_value":"1"}]' \
  '[{"name":"AUTOSCALER_REDRIVE_PAUSED","type":"plain_text","temporary_value":"1"},{"name":"AUTOSCALER_INTAKE_PAUSED","type":"plain_text","temporary_value":"0"}]' \
  '[{"name":"AUTOSCALER_REDRIVE_PAUSED","type":"plain_text","temporary_value":1},{"name":"AUTOSCALER_INTAKE_PAUSED","type":"plain_text","temporary_value":"1"}]' \
  '[{"name":"AUTOSCALER_REDRIVE_PAUSED","type":"plain_text","temporary_value":"1"},{"name":"AUTOSCALER_REDRIVE_PAUSED","type":"plain_text","temporary_value":"1"}]' \
  '[{"name":"AUTOSCALER_REDRIVE_PAUSED","type":"secret_text","temporary_value":"1"},{"name":"AUTOSCALER_INTAKE_PAUSED","type":"plain_text","temporary_value":"1"}]'; do
  if jq -e "$pause_filter" <<< "$fixture" >/dev/null; then
    echo "FAIL: missing or malformed pause binding must fail closed: $fixture" >&2; exit 1
  fi
done

printf 'test-mint-key\n' > "$test_mint_key"
printf 'observability-key\n' > "$observability_key"
chmod 600 "$test_mint_key" "$observability_key"
validate_files
printf 'access-id-value\n' > "$access_id"
printf 'access-secret-value\n' > "$access_secret"
chmod 600 "$access_id" "$access_secret"
if validate_access_pair "$access_id" ""; then
  echo "FAIL: incomplete Cloudflare Access pair must block AU1.8" >&2
  exit 1
fi
printf 'access-id-value\nsecond-line\n' > "$access_id"
if validate_access_pair "$access_id" "$access_secret"; then
  echo "FAIL: multiline Cloudflare Access id must block AU1.8" >&2
  exit 1
fi
printf 'access-id-value\n' > "$access_id"
validate_access_pair "$access_id" "$access_secret"
if ! access_header_case; then
  echo "FAIL: direct introspection must carry and scrub Cloudflare Access headers" >&2
  exit 1
fi
chmod 640 "$test_mint_key"
if validate_files; then
  echo "FAIL: FABRIC_TEST_MINT_KEY mode drift must block AU1.8" >&2
  exit 1
fi
chmod 600 "$test_mint_key"
chmod 640 "$observability_key"
if validate_files; then
  echo "FAIL: observability OOB key mode drift must block AU1.8" >&2
  exit 1
fi
chmod 600 "$observability_key"
: > "$test_mint_key"
if validate_files; then
  echo "FAIL: empty FABRIC_TEST_MINT_KEY must block AU1.8" >&2
  exit 1
fi
printf 'test-mint-key\n' > "$test_mint_key"
mv "$observability_key" "${observability_key}.real"
ln -s "${observability_key}.real" "$observability_key"
if validate_files; then
  echo "FAIL: symlinked observability OOB key must block AU1.8" >&2
  exit 1
fi
rm "$observability_key"
mv "${observability_key}.real" "$observability_key"
validate_stability 'worker-a	container-a	sha256:aaa' 'worker-a	container-a	sha256:aaa'
if validate_stability 'worker-a	container-a	sha256:aaa' 'worker-b	container-a	sha256:aaa'; then
  echo "FAIL: provider version drift must block AU1.8" >&2
  exit 1
fi
if validate_stability 'worker-a	container-a	sha256:aaa' ''; then
  echo "FAIL: missing provider stability sample must block AU1.8" >&2
  exit 1
fi

if ! container_version_case pass '{"name":"corelink-fabricd-fabricdcontainer","version":1}' '1' ||
   ! container_version_case pass '{"name":"corelink-fabricd-fabricdcontainer","version_id":"container-v1"}' 'container-v1' ||
   ! container_version_case fail '{"name":"corelink-fabricd-fabricdcontainer","version":{"id":"container-v1"}}' ||
   ! container_version_case fail '{"name":"corelink-fabricd-fabricdcontainer","version":null}' ||
   ! container_version_case fail '{"name":"corelink-fabricd-fabricdcontainer","metadata":{"version":2}}' ; then
  echo "FAIL: containers info version scalar compatibility must remain fail-closed" >&2
  exit 1
fi

if rg -n -- '--containers-rollout=none' "$harness" >/dev/null; then
  echo "FAIL: AU1.8 must recreate with immediate immutable rollout" >&2
  exit 1
fi
if [[ "$(rg -c -- '--containers-rollout=immediate' "$harness")" -lt 2 ]]; then
  echo "FAIL: AU1.8 must use immediate rollout for arm and disarm" >&2
  exit 1
fi
if ! unset_local_regression; then
  echo "FAIL: unset local names must not break mocked AU1.8 helper paths" >&2
  exit 1
fi
if ! admission_pause_case pass '{"bindings":[{"name":"FABRIC_ADMISSION_PAUSED","type":"plain_text","text":"1"}]}' ||
   ! admission_pause_case fail '{"bindings":[]}' ||
   ! admission_pause_case fail '{"bindings":[{"name":"FABRIC_ADMISSION_PAUSED","type":"plain_text","text":"1"},{"name":"FABRIC_ADMISSION_PAUSED","type":"plain_text","text":"1"}]}' ||
   ! admission_pause_case fail '{"bindings":[{"name":"FABRIC_ADMISSION_PAUSED","type":"plain_text","text":"0"}]}' ||
   ! admission_pause_case fail '{"bindings":[{"name":"FABRIC_ADMISSION_PAUSED","type":"secret_text","text":"1"}]}' ; then
  echo "FAIL: Fabricd admission pause must be exactly one authoritative plain-text binding set to 1" >&2
  exit 1
fi
if ! remote_binding_case pass armed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":"tenant-a"},
  {"name":"FABRIC_TEST_MINT_KEY","type":"secret_text","temporary_value":null}
]' ||
   ! remote_binding_case pass disarmed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":""}
]' ||
   ! remote_binding_case fail armed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":"tenant-a"},
  {"name":"FABRIC_TEST_MINT_KEY","type":"secret_text","temporary_value":null},
  {"name":"UNEXPECTED_BINDING","type":"plain_text","temporary_value":null}
]' ||
   ! remote_binding_case fail armed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_KEY","type":"secret_text","temporary_value":null}
]' ||
   ! remote_binding_case fail armed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":"tenant-a"},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":"tenant-a"},
  {"name":"FABRIC_TEST_MINT_KEY","type":"secret_text","temporary_value":null}
]' ||
   ! remote_binding_case fail armed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":"tenant-a"},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":"tenant-a"},
  {"name":"FABRIC_TEST_MINT_KEY","type":"plain_text","temporary_value":null}
]' ||
   ! remote_binding_case fail disarmed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":""},
  {"name":"UNEXPECTED_BINDING","type":"plain_text","temporary_value":null}
]' ||
   ! remote_binding_case fail disarmed '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_KEY","type":"secret_text","temporary_value":null}
]' ||
   ! remote_binding_case fail disarmed '[{"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null}]' ; then
  echo "FAIL: temporary AU1.8 bindings must be exact and mode-scoped" >&2
  exit 1
fi
if ! remote_baseline_case pass '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null}
]' ||
   ! remote_baseline_case pass '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":""}
]' ||
   ! remote_baseline_case fail '[
  {"name":"CORELINK_INTROSPECT_URL","type":"plain_text","temporary_value":null},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":"tenant-a"}
]' ||
   ! remote_baseline_case fail '[
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":""},
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"plain_text","temporary_value":""}
]' ||
   ! remote_baseline_case fail '[
  {"name":"FABRIC_TEST_MINT_TENANTS","type":"secret_text","temporary_value":""}
]' ||
   ! remote_baseline_case fail '[
  {"name":"FABRIC_TEST_MINT_KEY","type":"secret_text","temporary_value":null}
]'; then
  echo 'FAIL: remote baseline temporary bindings must be clean or canonically disarmed' >&2
  exit 1
fi

# A recreated container can briefly return 503 while the Worker is becoming
# ready.  Accept only a bounded transition to 200; a persistent 503 remains
# a visible RED result with bounded diagnostics.
health_wait_fn="$(sed -n '/^wait_for_health() {/,/^}$/p' "$harness")"
health_wait_case() {
  local expected="$1" sequence="$2" max_secs="$3" tmp rc
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-health-wait.XXXXXX")"
  set +e
  SEQUENCE="$sequence" HEALTH_TMP="$tmp" bash -u -c '
    set -Eeuo pipefail
    eval "$1"
    TMP_DIR="$HEALTH_TMP"; HEALTH_URL=https://health.invalid
    HEALTH_READY_MAX_SECS="$2"; HEALTH_READY_INTERVAL_SECS=1; EVENT_LOG="$TMP_DIR/events"
    : > "$TMP_DIR/calls"
    curl() {
      n=$(($(wc -l < "$TMP_DIR/calls") + 1)); printf "%s\n" "$n" >> "$TMP_DIR/calls"
      status=$(printf "%s" "$SEQUENCE" | cut -d, -f"$n")
      [[ -n "$status" ]] || status=503
      printf "%s" "$status"
    }
    scrub_file() { :; }
    log_event() { printf "%s\n" "$*" >> "$EVENT_LOG"; }
    wait_for_health recreated
  ' -- "$health_wait_fn" "$max_secs"
  rc=$?
  set -e
  if [[ "$expected" == pass ]]; then
    [[ "$rc" == 0 ]] && rg -q 'health-ready=GREEN http_status=200 .*attempts=2' "$tmp/events"
  else
    [[ "$rc" != 0 ]] && rg -q 'health-ready=RED http_status=503 .*attempts=' "$tmp/events"
  fi
  rc=$?
  rm -rf -- "$tmp"
  return "$rc"
}
if ! health_wait_case pass '503,200' 3 || ! health_wait_case fail '503' 1; then
  echo 'FAIL: health readiness must accept only bounded 503-to-200 startup transitions' >&2
  exit 1
fi

# Mint diagnostics retain only an allowlisted code, status, and restricted
# provider correlation id. Bodies and token-like fields must never leak.
mint_diag_fn="$(sed -n '/^classify_mint_failure() {/,/^}$/p' "$harness")"
mint_diag_tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-mint-diag.XXXXXX")"
if ! MINT_DIAG_TMP="$mint_diag_tmp" bash -u -c '
  set -Eeuo pipefail
  eval "$1"
  headers="$MINT_DIAG_TMP/headers"
  printf "CF-Ray: abc-123/xyz\r\n" > "$headers"
  out="$(classify_mint_failure '\''{"error":"CAS PAT mint failed","token":"token-secret-should-not-leak"}'\'' 503 "$headers")"
  [[ "$out" == "error_code=cas_pat_mint_failed http_status=503 cf_ray=abc-123xyz" ]]
  ! [[ "$out" == *token-secret* ]]
  out="$(classify_mint_failure '\''{"error":"fabricd upstream timeout","secret":"upstream-secret-should-not-leak"}'\'' 503 "$headers")"
  [[ "$out" == "error_code=worker_upstream_timeout http_status=503 cf_ray=abc-123xyz" ]]
  ! [[ "$out" == *upstream-secret* ]]
  out="$(classify_mint_failure '\''{"error":"unexpected","token_plaintext":"pat-secret-should-not-leak"}'\'' 503 "$headers")"
  [[ "$out" == "error_code=unknown http_status=503 cf_ray=abc-123xyz" ]]
  ! [[ "$out" == *pat-secret* ]]
' -- "$mint_diag_fn"; then
  rm -rf -- "$mint_diag_tmp"
  echo 'FAIL: mint diagnostics must classify safely without body/token leakage' >&2
  exit 1
fi
rm -rf -- "$mint_diag_tmp"

# A failed signer proof is RED, but it must never bypass temporary-key removal,
# disarm/recreate, final capture, or final remote-binding confirmation.
rollback_fn="$(sed -n '/^rollback_old() {/,/^}$/p' "$harness")"
rollback_tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-rollback-cleanup.XXXXXX")"
if ! ROLLBACK_TMP="$rollback_tmp" bash -u -c '
  set -Eeuo pipefail
  eval "$1"
  CURRENT_WORKER_VERSION=worker-final
  OLD_SECRET_FILE=old-secret-file; TEST_MINT_KEY_FILE=test-mint-key-file
  MINT_FIXTURE_FAILURE=transport
  log_event() { printf "%s\n" "$*" >> "$ROLLBACK_TMP/events"; }
  put_secret() { return 0; }
  recreate() { printf "recreate-%s\n" "$1" >> "$ROLLBACK_TMP/calls"; return 0; }
  capture_state() { printf "capture-%s\n" "$1" >> "$ROLLBACK_TMP/calls"; return 0; }
  mint_fixture() { MINT_FIXTURE_FAILURE=transport; return 1; }
  hmac_ticket() { printf ignored; }
  redeem_status() { printf 200; }
  delete_test_key() { printf "delete-key\n" >> "$ROLLBACK_TMP/calls"; return 0; }
  assert_test_key_absent() { printf "key-absent\n" >> "$ROLLBACK_TMP/calls"; return 0; }
  assert_remote_bindings() { printf "remote-%s\n" "$3" >> "$ROLLBACK_TMP/calls"; return 0; }
  if rollback_old; then exit 1; fi
  rg -q '^delete-key$' "$ROLLBACK_TMP/calls"
  rg -q '^recreate-0$' "$ROLLBACK_TMP/calls"
  rg -q '^capture-rollback-final$' "$ROLLBACK_TMP/calls"
  rg -q '^key-absent$' "$ROLLBACK_TMP/calls"
  rg -q '^remote-disarmed$' "$ROLLBACK_TMP/calls"
  rg -q "rollback-signer-proof=RED leaf=mint-transport" "$ROLLBACK_TMP/events"
' -- "$rollback_fn"; then
  rm -rf -- "$rollback_tmp"
  echo 'FAIL: rollback signer-proof failure must still complete deterministic disarm cleanup' >&2
  exit 1
fi
rm -rf -- "$rollback_tmp"

# Safety contract surfaces: stable lock refusal, fail-closed in-memory ledger,
# no-replace secret publication, and bounded per-state evidence fields.
safety_lock_dir="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-lock.XXXXXX")"
mkdir -p "$safety_lock_dir/au1.8-fabricd-cred-ticket-rotation.lock"
printf 'pid=foreign\n' > "$safety_lock_dir/au1.8-fabricd-cred-ticket-rotation.lock/owner"
chmod 700 "$safety_lock_dir" "$safety_lock_dir/au1.8-fabricd-cred-ticket-rotation.lock"
lock_fn="$(sed -n '/^acquire_lock() {/,/^}/p' "$harness")"
if LOCK_DIR="$safety_lock_dir" LOCK_PATH="$safety_lock_dir/au1.8-fabricd-cred-ticket-rotation.lock" RUN_ID=test SOURCE_COMMIT="$head" \
    bash -u -c 'set -Eeuo pipefail; eval "$1"; acquire_lock' -- "$lock_fn" >/dev/null 2>&1; then
  echo 'FAIL: foreign AU1.8 lock must fail closed before provider work' >&2
  exit 1
fi
rm -rf -- "$safety_lock_dir"

publish_fn="$(sed -n '/^publish_new_secret() {/,/^}$/p' "$harness")"
file_mode_fn="$(sed -n '/^file_owner_mode_ok() {/,/^}$/p' "$harness")"
publish_failure_case() {
  local kind="$1" tmp rc
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-publish.XXXXXX")"
  set +e
  PUBLISH_TMP="$tmp" bash -u -c '
    set -Eeuo pipefail
    eval "$1"; eval "$2"
    dir="$PUBLISH_TMP/destination"; mkdir -p "$dir"
    NEW_SECRET_FILE="$dir/new-secret"; NEW_SECRET_TMP=""
    puts="$PUBLISH_TMP/puts"
    put_secret() { printf called >> "$puts"; }
    case "$3" in
      symlink) printf sentinel > "$dir/sentinel"; ln -s "$dir/sentinel" "$NEW_SECRET_FILE" ;;
      race) ln() { printf sentinel > "$NEW_SECRET_FILE"; command ln "$@"; } ;;
      generator) openssl() { return 1; } ;;
    esac
    if publish_new_secret; then put_secret FABRIC_CRED_TICKET_SECRET "$NEW_SECRET_FILE"; exit 1; fi
    [[ ! -e "$puts" && "$NEW_SECRET_TMP" == "" ]]
    [[ -z "$(find "$dir" -name ".au18-new-secret.*" -print -quit)" ]]
    case "$3" in
      symlink) [[ "$(cat "$dir/sentinel")" == sentinel ]] ;;
      race) [[ "$(cat "$NEW_SECRET_FILE")" == sentinel ]] ;;
      generator) [[ ! -e "$NEW_SECRET_FILE" && ! -L "$NEW_SECRET_FILE" ]] ;;
    esac
  ' -- "$file_mode_fn" "$publish_fn" "$kind"
  rc=$?
  set -e
  rm -rf -- "$tmp"
  return "$rc"
}
if ! publish_failure_case symlink || ! publish_failure_case race || ! publish_failure_case generator; then
  echo 'FAIL: publication failures must preserve the destination, clean temp files, and skip secret put' >&2
  exit 1
fi

release_lock_fn="$(sed -n '/^release_lock() {/,/^}$/p' "$harness")"
lock_owner_mismatch_case() {
  local field="$1" tmp rc
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-lock-owner.XXXXXX")"
  set +e
  LOCK_TMP="$tmp" bash -u -c '
    set -Eeuo pipefail
    eval "$1"
    LOCK_PATH="$LOCK_TMP/lock"; mkdir "$LOCK_PATH"
    RUN_ID=ours; SOURCE_COMMIT=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; LOCK_HELD=1
    printf "pid=%s\nrun_id=ours\nsource_commit=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n" "$$" > "$LOCK_PATH/owner"
    case "$2" in run) sed -i.bak "s/run_id=ours/run_id=foreign/" "$LOCK_PATH/owner"; rm -f "$LOCK_PATH/owner.bak" ;; source) sed -i.bak "s/source_commit=.*/source_commit=foreign/" "$LOCK_PATH/owner"; rm -f "$LOCK_PATH/owner.bak" ;; esac
    if release_lock; then exit 1; fi
    [[ -d "$LOCK_PATH" && -f "$LOCK_PATH/owner" ]]
  ' -- "$release_lock_fn" "$field"
  rc=$?
  set -e
  rm -rf -- "$tmp"
  return "$rc"
}
if ! lock_owner_mismatch_case run || ! lock_owner_mismatch_case source; then
  echo 'FAIL: mismatched lock owner metadata must retain the foreign lock' >&2
  exit 1
fi

snapshot_fn="$(sed -n '/^provider_snapshot() {/,/^}$/p' "$harness")"
container_fn="$(sed -n '/^extract_container_version() {/,/^}$/p' "$harness")"
capacity_fn="$(sed -n '/^assert_singleton_capacity() {/,/^}$/p' "$harness")"
stability_binding_case() {
  local expected="$1" state="$2" tmp rc
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-stability-binding.XXXXXX")"
  set +e
  STABILITY_TMP="$tmp" bash -u -c '
    set -Eeuo pipefail
    eval "$1"; eval "$2"; eval "$3"
    TMP_DIR="$STABILITY_TMP"; APP_ID=app-a; WORKER_NAME=worker; CONTAINER_APP_NAME=app
    REMOTE_BASELINE_FILE="$TMP_DIR/baseline.json"; printf "[]" > "$REMOTE_BASELINE_FILE"
    EXPECTED_IMAGE_DIGEST=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    capture_json() {
      case "$1" in
        *deployments) printf "[{\"created_on\":\"2026-01-01T00:00:00Z\",\"versions\":[{\"version_id\":\"worker-v\"}]}]" > "$2" ;;
        *container-info) printf "{\"id\":\"app-provider\",\"created_on\":\"2026-01-01T00:00:00Z\",\"name\":\"app\",\"max_instances\":1,\"version\":\"container-v\",\"image\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}" > "$2" ;;
      esac
    }
    assert_fabricd_admission_paused() { :; }
    assert_remote_bindings() { REMOTE_TEMP_VAR_STATE="$STABILITY_STATE"; }
    STABILITY_STATE="$4"
    provider_snapshot sample
  ' -- "$snapshot_fn" "$container_fn" "$capacity_fn" "$state" > "$tmp/out"
  rc=$?
  set -e
  if [[ "$expected" == pass ]]; then
    [[ "$rc" == 0 && "$(cat "$tmp/out")" == $'worker-v\tcontainer-v\tsha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tdisarmed\tapp-provider\t2026-01-01T00:00:00Z' ]]
  else
    [[ "$rc" != 0 ]]
  fi
  rc=$?
  rm -rf -- "$tmp"
  return "$rc"
}
if ! stability_binding_case pass empty-disabled || ! stability_binding_case fail armed; then
  echo 'FAIL: stability binding state must be provider-attested and disarmed' >&2
  exit 1
fi

# Behavioral evidence validator: omitted/mismatched tuples and missing disarm
# cannot qualify a PASS-capable result.
evidence_fn="$(sed -n '/^evidence_pass_ready() {/,/^}/p' "$harness")"
evidence_env='BEFORE_APP_ID=app-a BEFORE_CREATED_ON=created-a ARMED_CREATED_ON=created-b ROTATED_CREATED_ON=created-c FINAL_CREATED_ON=created-d BEFORE_WORKER_VERSION=worker-a ARMED_WORKER_VERSION=worker-b ROTATED_WORKER_VERSION=worker-c FINAL_WORKER_VERSION=worker-d BEFORE_CONTAINER_VERSION=container-a ARMED_CONTAINER_VERSION=container-b ROTATED_CONTAINER_VERSION=container-c FINAL_CONTAINER_VERSION=container-d BEFORE_DIGEST=digest-a ARMED_DIGEST=digest-b ROTATED_DIGEST=digest-c FINAL_DIGEST=digest-d FINAL_DISARM_STATE=green REMOTE_TEMP_VAR_STATE=disarmed'
if bash -u -c 'set -Eeuo pipefail; eval "$1"; eval "$2"; STABILITY_SAMPLE_1=$'\''worker-a\tcontainer-a\tdigest-a\tdisarmed\tapp-a\tcreated-a'\''; STABILITY_SAMPLE_2=$'\''worker-a\tcontainer-a\tdigest-a\tdisarmed\tapp-b\tcreated-a'\''; evidence_pass_ready' -- "$evidence_fn" "$evidence_env"; then
  echo 'FAIL: mismatched stability tuple must fail evidence validation' >&2; exit 1
fi
if bash -u -c 'set -Eeuo pipefail; eval "$1"; eval "$2"; STABILITY_SAMPLE_1=$'\''worker-a\tcontainer-a\tdigest-a\tdisarmed\tapp-b\tcreated-a'\''; STABILITY_SAMPLE_2=$STABILITY_SAMPLE_1; evidence_pass_ready' -- "$evidence_fn" "$evidence_env"; then
  echo 'FAIL: stability app metadata must match the baseline tuple' >&2; exit 1
fi
if bash -u -c 'set -Eeuo pipefail; eval "$1"; eval "$2"; STABILITY_SAMPLE_1=$'\''worker-a\tcontainer-a\tdigest-a\tdisarmed\tapp-a\tcreated-a'\''; STABILITY_SAMPLE_2=$STABILITY_SAMPLE_1; FINAL_DISARM_STATE=not-proven; evidence_pass_ready' -- "$evidence_fn" "$evidence_env"; then
  echo 'FAIL: absent final disarm must fail evidence validation' >&2; exit 1
fi

echo "AU1.8 focused gates: PASS"
