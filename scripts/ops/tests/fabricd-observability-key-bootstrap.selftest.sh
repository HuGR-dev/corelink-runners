#!/usr/bin/env bash
# Network-free contract tests for the pre-AU1.8 observability-key bootstrap.
set -Eeuo pipefail
umask 077
here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/../fabricd-observability-key-bootstrap.sh"
wrangler="$here/mock-observability-bootstrap-wrangler.sh"
curl_mock="$here/mock-observability-bootstrap-curl.sh"
recovery_ack='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-RECOVER-INTROSPECT-LIVE-20260909'
resume_ack='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-RESUME-INTROSPECT-LIVE-20260909'
repair_ack='ACK-CORELINK-FABRIC-OBSERVABILITY-REPAIR-INTROSPECT-AUTH-LIVE-20260909'
repair_resume_ack='ACK-CORELINK-FABRIC-OBSERVABILITY-REPAIR-INTROSPECT-AUTH-RESUME-LIVE-20260909'
normal_ack='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908'
tmp="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-bootstrap-test.XXXXXXXX")"
trap 'rm -rf -- "$tmp"' EXIT
chmod 700 "$tmp"

set +e
"$harness" --mode execute --recover-introspect --ack "$recovery_ack" --stability-seconds 120 >/dev/null 2>"$tmp/recovery-ack-valid.stderr"
recovery_ack_valid_rc=$?
"$harness" --mode execute --recover-introspect --ack "$normal_ack" --stability-seconds 120 >/dev/null 2>"$tmp/recovery-ack-wrong.stderr"
recovery_ack_wrong_rc=$?
"$harness" --mode execute --ack "$normal_ack" --stability-seconds 120 >/dev/null 2>"$tmp/normal-ack-valid.stderr"
normal_ack_valid_rc=$?
"$harness" --mode execute --ack "$recovery_ack" --stability-seconds 120 >/dev/null 2>"$tmp/normal-ack-wrong.stderr"
normal_ack_wrong_rc=$?
set -e
test "$recovery_ack_valid_rc" != 0
if rg -q 'acknowledgement required' "$tmp/recovery-ack-valid.stderr"; then exit 1; fi
rg -q 'missing repository/provider pins' "$tmp/recovery-ack-valid.stderr"
test "$recovery_ack_wrong_rc" != 0
rg -q 'exact recovery acknowledgement required' "$tmp/recovery-ack-wrong.stderr"
test "$normal_ack_valid_rc" != 0
if rg -q 'acknowledgement required' "$tmp/normal-ack-valid.stderr"; then exit 1; fi
rg -q 'missing repository/provider pins' "$tmp/normal-ack-valid.stderr"
test "$normal_ack_wrong_rc" != 0
rg -q 'exact live acknowledgement required' "$tmp/normal-ack-wrong.stderr"
printf '%s\n' 'ack-validation=PASS'

for bad_window in 0 1 119 121; do
  if "$harness" --mode execute --ack ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908 --stability-seconds "$bad_window" >/dev/null 2>"$tmp/execute-window-$bad_window.stderr"; then exit 1; fi
  rg -q 'live stability window must be exactly 120 seconds' "$tmp/execute-window-$bad_window.stderr"
done

make_fixture() {
  root="$tmp/root-$1"; oob="$tmp/oob-$1"; state="$tmp/state-$1"; mkdir -p "$root/deploy/cloudflare-fabricd" "$root/evidence" "$oob"; chmod 700 "$root" "$root/evidence" "$oob"
  printf '%s\n' '{"image":"registry.example/corelink@sha256:1111111111111111111111111111111111111111111111111111111111111111"}' > "$root/deploy/cloudflare-fabricd/wrangler.jsonc"
  git -C "$root" init -q; git -C "$root" config user.email bootstrap@example.invalid; git -C "$root" config user.name bootstrap; git -C "$root" add .; git -C "$root" commit -qm baseline; commit="$(git -C "$root" rev-parse HEAD)"
  printf '%s\n' fleet-key > "$oob/fleet"; printf '%s\n' introspect-key > "$oob/introspect"; printf '%s\n' pat > "$oob/pat"; chmod 600 "$oob/fleet" "$oob/introspect" "$oob/pat"
  export root oob state commit MOCK_CURRENT_APP_ID=app-old MOCK_NEW_APP_ID=app-new
}

dispatch_case() {
  local label="$1" version="${2-}" case_root case_real case_oob case_evidence_dir digest case_log case_err rc
  case_root="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-dispatch.XXXXXXXX")"
  case_real="$(cd "$case_root" && pwd -P)"
  case_oob="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-dispatch-oob.XXXXXXXX")"
  case_evidence_dir="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-dispatch-evidence.XXXXXXXX")"
  mkdir -p "$case_root/deploy/cloudflare-fabricd/node_modules/.bin"
  chmod 700 "$case_root" "$case_oob" "$case_evidence_dir"
  cp "$here/../../../deploy/cloudflare-fabricd/wrangler.jsonc" "$case_root/deploy/cloudflare-fabricd/wrangler.jsonc"
  digest="$(sed -n 's/.*@\(sha256:[0-9a-f]\{64\}\).*/\1/p' "$case_root/deploy/cloudflare-fabricd/wrangler.jsonc" | head -n 1)"
  case "$label" in
    missing) : ;;
    wrong|correct)
      # shellcheck disable=SC2016
      printf '%s\n' '#!/usr/bin/env bash' \
        'printf "%s|%s\\n" "$PWD" "$*" >> "${DISPATCH_LOG:?}"' \
        "if [ \"\${1:-}\" = --version ]; then printf '%s\\n' '$version'; exit 0; fi" \
        'if [ "${1:-}" = auth ] && [ "${2:-}" = token ]; then printf '\''{"token":"dispatch-test-token-1234567890"}\n'\''; exit 0; fi' \
        'exit 1' > "$case_root/deploy/cloudflare-fabricd/node_modules/.bin/wrangler"
      chmod 700 "$case_root/deploy/cloudflare-fabricd/node_modules/.bin/wrangler"
      ;;
    *) printf 'unknown dispatch test case: %s\n' "$label" >&2; exit 1 ;;
  esac
  git -C "$case_root" init -q
  git -C "$case_root" config user.email obs-dispatch@example.invalid
  git -C "$case_root" config user.name obs-dispatch
  git -C "$case_root" add -f .
  git -C "$case_root" commit -qm baseline
  commit="$(git -C "$case_root" rev-parse HEAD)"
  for key in fleet introspect pat; do printf '%s\n' dispatch-test-key > "$case_oob/$key"; chmod 600 "$case_oob/$key"; done
  case_log="$(mktemp "${TMPDIR:-/tmp}/corelink-obs-dispatch-log.XXXXXXXX")"
  case_err="$(mktemp "${TMPDIR:-/tmp}/corelink-obs-dispatch-err.XXXXXXXX")"
  set +e
  DISPATCH_LOG="$case_log" "$harness" --mode execute --ack ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908 \
    --repo-root "$case_root" --expected-commit "$commit" --expected-version version-good \
    --fabricd-app-id 22222222-2222-2222-2222-222222222222 --expected-image-digest "$digest" \
    --oob-dir "$case_oob" --fleet-key-file "$case_oob/fleet" \
    --introspect-key-file "$case_oob/introspect" --introspect-pat-file "$case_oob/pat" \
    --tenant-id tenant-test --evidence-file "$case_evidence_dir/result.json" --stability-seconds 120 \
    >/dev/null 2>"$case_err"
  rc=$?
  set -e
  case "$label" in
    missing) [[ "$rc" != 0 ]] && rg -q 'local Wrangler binary missing' "$case_err" ;;
    wrong) [[ "$rc" != 0 ]] && rg -q 'local Wrangler version mismatch' "$case_err" ;;
    correct) [[ "$rc" != 0 ]] && grep -F -q "$case_real/deploy/cloudflare-fabricd|--config $case_real/deploy/cloudflare-fabricd/wrangler.jsonc deployments list --name corelink-fabricd --json" "$case_log" ;;
  esac
  local result=$?
  rm -rf -- "$case_root" "$case_oob" "$case_evidence_dir" "$case_log" "$case_err"
  return "$result"
}

dispatch_case missing
dispatch_case wrong 4.104.0
dispatch_case correct 4.105.0
if rg -n '\bnpx\b' "$harness" >/dev/null; then
  printf '%s\n' 'unexpected npx Wrangler resolution' >&2
  exit 1
fi

# Reproduce the provider CLI failure that motivated this path: pinned
# Wrangler 4.105.0 rejects the legacy machine-readable `info` flag.
unsupported_state="$tmp/unsupported-info"
set +e
MOCK_STATE="$unsupported_state" MOCK_SCENARIO=unsupported-info MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION=version-good \
  "$wrangler" containers info app-old --json >/dev/null 2>"$tmp/unsupported-info.stderr"
unsupported_rc=$?
set -e
test "$unsupported_rc" = 64
rg -q 'unknown option: --json' "$tmp/unsupported-info.stderr"
printf '%s\n' 'unsupported-info-flag=refused'

run_case() {
  local name="$1" scenario="$2" expected="$3" stability="${4:-0}"; make_fixture "$name"
  export MOCK_STATE="$state" MOCK_SCENARIO="$scenario" MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION='version-good' MOCK_TENANT='tenant-test'
  set +e
  "$harness" --mode mock --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version version-good --fabricd-app-id app-old --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --introspect-key-file "$oob/introspect" --introspect-pat-file "$oob/pat" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --fleet-url https://spawn.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds "$stability" >/dev/null 2>"$tmp/$name.stderr"
  rc=$?; set -e
  if { [ "$expected" = pass ] && [ "$rc" = 0 ]; } || { [ "$expected" = fail ] && [ "$rc" != 0 ]; }; then :; else printf 'unexpected result for %s (rc=%s)\n' "$name" "$rc" >&2; exit 1; fi
  printf '%s\n' "$name=$expected"
}

run_recovery_case() {
  local name="$1" scenario="$2" expected="$3" layout="${4:-valid}"; make_fixture "$name"
  new_introspect="$oob/new-introspect"
  case "$layout" in
    valid|same-as-old|same-as-observability)
      if [ "$layout" = same-as-old ]; then new_introspect="$oob/introspect"; fi
      if [ "$layout" = same-as-observability ]; then new_introspect="$oob/fabric-observability-key-bootstrap.b64"; fi
      printf '%s\n' bmV3LWludHJvc3BlY3Qta2V5 > "$new_introspect"; chmod 600 "$new_introspect";;
    missing) : ;;
    unsafe) printf '%s\n' new-introspect-key > "$new_introspect"; chmod 640 "$new_introspect";;
    *) printf 'unknown recovery fixture layout: %s\n' "$layout" >&2; exit 1;;
  esac
  export MOCK_STATE="$state" MOCK_SCENARIO="$scenario" MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION=version-good MOCK_TENANT=tenant-test
  set +e
  "$harness" --mode mock --recover-introspect --ack "$recovery_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version version-good --fabricd-app-id app-old --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --introspect-key-file "$oob/introspect" --new-introspect-key-file "$new_introspect" --introspect-pat-file "$oob/pat" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --fleet-url https://spawn.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0 >/dev/null 2>"$tmp/$name.stderr"
  rc=$?; set -e
  if { [ "$expected" = pass ] && [ "$rc" = 0 ]; } || { [ "$expected" = fail ] && [ "$rc" != 0 ]; }; then :; else printf 'unexpected recovery result for %s (rc=%s)\n' "$name" "$rc" >&2; exit 1; fi
  printf '%s\n' "$name=$expected"
}

run_resume_artifact_mismatch() {
  local name="$1" field="$2" artifact_filter
  make_fixture "$name"
  printf '%s\n' bm9uLXJvdGF0ZWQtb2JzZXJ2YWJpbGl0eS1rZXk= > "$oob/observability-key"; chmod 600 "$oob/observability-key"
  printf '%s\n' bmV3LWludHJvc3BlY3Qta2V5 > "$oob/new-introspect"; chmod 600 "$oob/new-introspect"
  case "$field" in
    schema_version) artifact_filter='.schema_version = "wrong"';;
    artifact_id) artifact_filter='.artifact_id = "wrong"';;
    source.repository) artifact_filter='.source.repository = "wrong"';;
    source.commit_sha) artifact_filter='.source.commit_sha = "0000000000000000000000000000000000000000"';;
    provider.worker) artifact_filter='.provider.worker = "wrong"';;
    provider.expected_version) artifact_filter='.provider.expected_version = "wrong"';;
    provider.app_id) artifact_filter='.provider.app_id = "wrong"';;
    *) printf 'unknown artifact mismatch: %s\n' "$field" >&2; exit 1;;
  esac
  jq -n --arg commit "$commit" --arg digest "$MOCK_DIGEST" --arg version "$recovery_version" --arg app "$recovery_app" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$app},phase:"container-recreate",secrets:{FABRIC_INTROSPECT_AUTH_KEY_put_completed:true,FABRIC_OBSERVABILITY_KEY_put_completed:true},gates:{refreeze_result:"green"},outcome:"RED",rerun_guard:"armed"}' | jq "$artifact_filter" > "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
  printf '%s\n' in-progress > "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
  chmod 600 "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
  export MOCK_STATE="$state" MOCK_SCENARIO=resume-mismatch MOCK_NEW_APP_ID="$current_app" MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION="$current_version" MOCK_TENANT=tenant-test
  set +e
  "$harness" --mode mock --resume-recovery --ack "$resume_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --recovery-source-sha "$commit" --recovery-expected-version "$recovery_version" --recovery-fabricd-app-id "$recovery_app" --expected-version "$current_version" --fabricd-app-id "$current_app" --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability-key" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://status.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0 >/dev/null 2>"$tmp/resume-$name.stderr"
  mismatch_rc=$?
  set -e
  test "$mismatch_rc" != 0
  test -f "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
  test ! -e "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.complete"
  printf '%s\n' "resume-$name=fail-closed"
}

run_case success success pass
grep -q '"status": "PASS"' "$tmp/root-success/evidence/result.json"
test -f "$tmp/oob-success/fabric-observability-key-bootstrap.b64"
test "$(stat -f '%Lp' "$tmp/oob-success/fabric-observability-key-bootstrap.b64")" = 600
key_value="$(tr -d '\r\n' < "$tmp/oob-success/fabric-observability-key-bootstrap.b64")"
if rg -F "$key_value" "$tmp/root-success/evidence" "$tmp/oob-success" -g '!fabric-observability-key-bootstrap.b64' >/dev/null; then exit 1; fi

start="$(date +%s)"; run_case elapsed success pass 1; elapsed_seconds=$(( $(date +%s) - start )); test "$elapsed_seconds" -ge 1
events="$(jq -r '.logs.events' "$tmp/root-elapsed/evidence/result.json")"
line_one="$(rg -n 'stability_sample_1_at=' "$events" | cut -d: -f1)"
line_two="$(rg -n 'stability_sample_2_at=' "$events" | cut -d: -f1)"
line_put="$(rg -n 'secret-put rc=0' "$events" | cut -d: -f1)"
line_key="$(rg -n 'key=ready value=excluded' "$events" | cut -d: -f1)"
test "$line_key" -lt "$line_one" && test "$line_one" -lt "$line_two" && test "$line_two" -lt "$line_put"

run_case drift drift fail
test ! -f "$tmp/state-drift.secret-put"
run_case busy-after-stability busy-after-stability fail
test ! -f "$tmp/state-busy-after-stability.secret-put"

make_fixture bad-file
printf '%s\n' provided-key > "$oob/bad-key"; chmod 640 "$oob/bad-key"
export MOCK_STATE="$state" MOCK_SCENARIO=success MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION=version-good MOCK_TENANT=tenant-test
set +e
"$harness" --mode mock --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version version-good --fabricd-app-id app-old --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --introspect-key-file "$oob/introspect" --introspect-pat-file "$oob/pat" --tenant-id tenant-test --key-file "$oob/bad-key" --evidence-file "$root/evidence/result.json" --stability-seconds 0 >/dev/null 2>"$tmp/bad-file.stderr"
rc=$?; set -e; test "$rc" != 0; test ! -f "$state.secret-put"
printf '%s\n' 'bad-file=fail'

run_case wrong-digest wrong-digest fail
run_case verify-fail verify-fail fail
test -f "$tmp/state-verify-fail.secret-put"
run_case failed-refreeze fail-refreeze fail
test -f "$tmp/state-failed-refreeze.secret-put"
run_case missing-container missing-container fail
run_case duplicate-container duplicate-container fail

run_case auth-403 auth-403 fail
run_recovery_case recovery-403 recover-403 pass
grep -q '"operation_mode": "introspect-recovery"' "$tmp/root-recovery-403/evidence/result.json"
recovery_events="$(jq -r '.logs.events' "$tmp/root-recovery-403/evidence/result.json")"
rg -q 'old_introspection=auth_rejected_http:403' "$recovery_events"
rg -q 'introspection_status=valid pat_schema=valid' "$recovery_events"
test -f "$tmp/state-recovery-403.secret-put-introspect"
test -f "$tmp/state-recovery-403.secret-put-observability"
test -f "$tmp/oob-recovery-403/.fabricd-observability-key-bootstrap-introspect-recovery.complete"
run_recovery_case recovery-401 recover-401 pass
run_recovery_case recovery-5xx recover-5xx fail
run_recovery_case recovery-transport recover-transport fail
run_recovery_case recovery-missing-key recover-403 fail missing
test ! -f "$tmp/state-recovery-missing-key.secret-put-introspect"
run_recovery_case recovery-unsafe-key recover-403 fail unsafe
test ! -f "$tmp/state-recovery-unsafe-key.secret-put-introspect"
run_recovery_case recovery-first-secret-failure partial-first fail
test ! -f "$tmp/state-recovery-first-secret-failure.secret-put-introspect"
failure_artifact="$tmp/oob-recovery-first-secret-failure/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
test -f "$failure_artifact"
test "$(stat -f '%Lp' "$failure_artifact")" = 600
test "$(jq -r '.status' "$failure_artifact")" = RED
test "$(jq -r '.phase' "$failure_artifact")" = introspect-secret-put
test "$(jq -r '.secrets.FABRIC_INTROSPECT_AUTH_KEY_put_completed' "$failure_artifact")" = false
test "$(jq -r '.secrets.FABRIC_OBSERVABILITY_KEY_put_completed' "$failure_artifact")" = false
test "$(jq -r '.gates.refreeze_result' "$failure_artifact")" = green
test -f "$tmp/oob-recovery-first-secret-failure/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
run_recovery_case recovery-second-secret-failure partial-second fail
test -f "$tmp/state-recovery-second-secret-failure.secret-put-introspect"
test ! -f "$tmp/state-recovery-second-secret-failure.secret-put-observability"
failure_artifact="$tmp/oob-recovery-second-secret-failure/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
test -f "$failure_artifact"
test "$(stat -f '%Lp' "$failure_artifact")" = 600
test "$(jq -r '.status' "$failure_artifact")" = RED
test "$(jq -r '.phase' "$failure_artifact")" = observability-secret-put
test "$(jq -r '.secrets.FABRIC_INTROSPECT_AUTH_KEY_put_completed' "$failure_artifact")" = true
test "$(jq -r '.secrets.FABRIC_OBSERVABILITY_KEY_put_completed' "$failure_artifact")" = false
test "$(jq -r '.gates.refreeze_result' "$failure_artifact")" = green
test -f "$tmp/oob-recovery-second-secret-failure/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
run_recovery_case recovery-same-old recover-403 fail same-as-old
run_recovery_case recovery-same-observability recover-403 fail same-as-observability

make_repair_authorization() {
  jq -n '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:"37565619dae31a61f67a095daa0cd15b06386237"},provider:{worker:"corelink-fabricd",expected_version:"c38233a3-4ede-4803-8e1c-1a3b5ad4d667",app_id:"a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7",expected_image_digest:"sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5"},phase:"container-recreate",secrets:{FABRIC_INTROSPECT_KEY_put_completed:true,FABRIC_OBSERVABILITY_KEY_put_completed:true},gates:{refreeze_result:"green"},outcome:"RED",rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
  printf '%s\n' in-progress > "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
  chmod 600 "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
}

run_repair_case() {
  local name="$1" scenario="$2" expected="$3"; make_fixture "$name"; make_repair_authorization
  printf '%s\n' observability-existing-key > "$oob/observability-key"; printf '%s\n' new-introspect-auth-key > "$oob/new-introspect"; chmod 600 "$oob/observability-key" "$oob/new-introspect"
  export MOCK_STATE="$state" MOCK_SCENARIO="$scenario" MOCK_CURRENT_APP_ID='13e744e1-921e-4dfb-8423-7662a804222e' MOCK_NEW_APP_ID='a035e7f9-26d7-4537-9863-568872efc6ba' MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION='13e744e1-921e-4dfb-8423-7662a804222e' MOCK_TENANT=tenant-test
  set +e
  "$harness" --mode mock --repair-introspect-auth --ack "$repair_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version "$MOCK_VERSION" --fabricd-app-id "$MOCK_CURRENT_APP_ID" --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability-key" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://status.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0 >/dev/null 2>"$tmp/$name.stderr"
  rc=$?; set -e
  if { [ "$expected" = pass ] && [ "$rc" = 0 ]; } || { [ "$expected" = fail ] && [ "$rc" != 0 ]; }; then :; else printf 'unexpected repair result for %s (rc=%s)\n' "$name" "$rc" >&2; exit 1; fi
}

set +e
"$harness" --mode execute --repair-introspect-auth --ack "$repair_ack" --stability-seconds 120 >/dev/null 2>"$tmp/repair-ack-valid.stderr"; repair_ack_rc=$?
"$harness" --mode execute --repair-introspect-auth --ack "$normal_ack" --stability-seconds 120 >/dev/null 2>"$tmp/repair-ack-wrong.stderr"; repair_ack_wrong_rc=$?
set -e
test "$repair_ack_rc" != 0; rg -q 'missing repository/provider pins' "$tmp/repair-ack-valid.stderr"
test "$repair_ack_wrong_rc" != 0; rg -q 'exact introspect-auth repair acknowledgement required' "$tmp/repair-ack-wrong.stderr"

run_repair_case repair-success repair-403 pass
repair_evidence="$tmp/root-repair-success/evidence/result.json"
test "$(jq -r '.repair.wrong_name' "$repair_evidence")" = FABRIC_INTROSPECT_KEY
test "$(jq -r '.repair.correct_name' "$repair_evidence")" = FABRIC_INTROSPECT_AUTH_KEY
test "$(jq -r '.repair.historical.source_commit' "$repair_evidence")" = 37565619dae31a61f67a095daa0cd15b06386237
test "$(jq -r '.repair.pre_repair_app_id' "$repair_evidence")" = 13e744e1-921e-4dfb-8423-7662a804222e
test "$(jq -r '.repair.final_app_id' "$repair_evidence")" = a035e7f9-26d7-4537-9863-568872efc6ba
test -f "$tmp/state-repair-success.secret-put-introspect"
test ! -f "$tmp/state-repair-success.secret-put-observability"
test -f "$tmp/oob-repair-success/.fabricd-observability-key-bootstrap-introspect-auth-repair.complete"
test ! -e "$tmp/oob-repair-success/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
if rg -F 'new-introspect-auth-key' "$tmp/root-repair-success/evidence" "$tmp/oob-repair-success" -g '!new-introspect' >/dev/null; then exit 1; fi

run_repair_case repair-post-failure repair-post-fail fail
repair_oob="$tmp/oob-repair-post-failure"; repair_state="$tmp/state-repair-post-failure"
test -f "$repair_oob/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json"
test -f "$repair_oob/fabricd-observability-key-bootstrap-introspect-auth-repair-failure.json"
test "$(jq -r '.repair.recreate_completed' "$repair_oob/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json")" = true
test "$(jq -r '.repair.final_app_id' "$repair_oob/fabricd-observability-key-bootstrap-introspect-auth-repair-failure.json")" = a035e7f9-26d7-4537-9863-568872efc6ba
test -f "$repair_oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
secret_put_before="$(cat "$repair_state.secret-put-introspect-count")"; deploy_before="$(cat "$repair_state.deploy")"
export MOCK_STATE="$repair_state" MOCK_SCENARIO=repair-403 MOCK_CURRENT_APP_ID='13e744e1-921e-4dfb-8423-7662a804222e' MOCK_NEW_APP_ID='a035e7f9-26d7-4537-9863-568872efc6ba' MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION='13e744e1-921e-4dfb-8423-7662a804222e' MOCK_TENANT=tenant-test
"$harness" --mode mock --resume-introspect-auth-repair --ack "$repair_resume_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$tmp/root-repair-post-failure" --expected-commit "$(git -C "$tmp/root-repair-post-failure" rev-parse HEAD)" --expected-version "$MOCK_VERSION" --fabricd-app-id a035e7f9-26d7-4537-9863-568872efc6ba --expected-image-digest "$MOCK_DIGEST" --oob-dir "$repair_oob" --fleet-key-file "$repair_oob/fleet" --new-introspect-key-file "$repair_oob/new-introspect" --introspect-pat-file "$repair_oob/pat" --key-file "$repair_oob/observability-key" --tenant-id tenant-test --evidence-file "$tmp/root-repair-post-failure/evidence/resumed.json" --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://status.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0 >/dev/null
test "$(cat "$repair_state.secret-put-introspect-count")" = "$secret_put_before"
test "$(cat "$repair_state.deploy")" = "$deploy_before"
test "$(jq -r '.resumed' "$tmp/root-repair-post-failure/evidence/resumed.json")" = true
test ! -e "$repair_oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
test -f "$repair_oob/.fabricd-observability-key-bootstrap-introspect-auth-repair.complete"
printf '%s\n' 'introspect-auth-repair=pass'

make_fixture recovery-rerun
rerun_new="$oob/new-introspect"; printf '%s\n' bmV3LWludHJvc3BlY3Qta2V5 > "$rerun_new"; chmod 600 "$rerun_new"
export MOCK_STATE="$state" MOCK_SCENARIO=recover-403 MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION=version-good MOCK_TENANT=tenant-test
recovery_args=(--mode mock --recover-introspect --ack "$recovery_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version version-good --fabricd-app-id app-old --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --introspect-key-file "$oob/introspect" --new-introspect-key-file "$rerun_new" --introspect-pat-file "$oob/pat" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --fleet-url https://spawn.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0)
"$harness" "${recovery_args[@]}" >/dev/null 2>"$tmp/recovery-rerun-first.stderr"
test -f "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.complete"
set +e
"$harness" "${recovery_args[@]}" >/dev/null 2>"$tmp/recovery-rerun-second.stderr"
rerun_rc=$?; set -e
test "$rerun_rc" != 0
rg -q 'recovery already started; refusing rerun' "$tmp/recovery-rerun-second.stderr"
printf '%s\n' 'recovery-rerun=fail-closed'

make_fixture resume-success
printf '%s\n' recovery-followup > "$root/recovery-followup.txt"
git -C "$root" add recovery-followup.txt
git -C "$root" commit -qm recovery-followup
recovery_source_sha="$(git -C "$root" rev-parse HEAD^)"
commit="$(git -C "$root" rev-parse HEAD)"
recovery_version='c38233a3-4ede-4803-8e1c-1a3b5ad4d667'
recovery_app='a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7'
current_version='13e744e1-921e-4dfb-8423-7662a804222e'
current_app='a035e7f9-26d7-4537-9863-568872efc6ba'
printf '%s\n' bm9uLXJvdGF0ZWQtb2JzZXJ2YWJpbGl0eS1rZXk= > "$oob/observability-key"; chmod 600 "$oob/observability-key"
printf '%s\n' bmV3LWludHJvc3BlY3Qta2V5 > "$oob/new-introspect"; chmod 600 "$oob/new-introspect"
: > "$state.deleted"
jq -n --arg commit "$recovery_source_sha" --arg digest "$MOCK_DIGEST" --arg version "$recovery_version" --arg app "$recovery_app" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$app},phase:"container-recreate",secrets:{FABRIC_INTROSPECT_AUTH_KEY_put_completed:true,FABRIC_OBSERVABILITY_KEY_put_completed:true},gates:{refreeze_result:"green"},outcome:"RED",rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
printf '%s\n' in-progress > "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
chmod 600 "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
export MOCK_STATE="$state" MOCK_SCENARIO=resume-success MOCK_NEW_APP_ID="$current_app" MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION="$current_version" MOCK_TENANT=tenant-test
"$harness" --mode mock --resume-recovery --ack "$resume_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --recovery-source-sha "$recovery_source_sha" --recovery-expected-version "$recovery_version" --recovery-fabricd-app-id "$recovery_app" --expected-version "$current_version" --fabricd-app-id "$current_app" --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability-key" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://status.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0 >/dev/null 2>"$tmp/resume-success.stderr"
grep -q '"status": "PASS"' "$tmp/root-resume-success/evidence/result.json"
test "$(jq -r '.resumed' "$tmp/root-resume-success/evidence/result.json")" = true
test "$(jq -r '.provider.expected_version' "$tmp/root-resume-success/evidence/result.json")" = "$current_version"
test "$(jq -r '.provider.app_id' "$tmp/root-resume-success/evidence/result.json")" = "$current_app"
test "$(jq -r '.lineage.old_app_id' "$tmp/root-resume-success/evidence/result.json")" = "$recovery_app"
test "$(jq -r '.lineage.new_app_id' "$tmp/root-resume-success/evidence/result.json")" = "$current_app"
test ! -e "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
test -f "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.complete"
test ! -f "$state.secret-put-introspect"
test ! -f "$state.secret-put-observability"
test ! -f "$state.deploy"
printf '%s\n' 'resume-success=pass'

make_fixture resume-same-id
printf '%s\n' bm9uLXJvdGF0ZWQtb2JzZXJ2YWJpbGl0eS1rZXk= > "$oob/observability-key"; chmod 600 "$oob/observability-key"
printf '%s\n' bmV3LWludHJvc3BlY3Qta2V5 > "$oob/new-introspect"; chmod 600 "$oob/new-introspect"
jq -n --arg commit "$commit" --arg digest "$MOCK_DIGEST" --arg version "$recovery_version" --arg app "$recovery_app" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$app},phase:"container-recreate",secrets:{FABRIC_INTROSPECT_AUTH_KEY_put_completed:true,FABRIC_OBSERVABILITY_KEY_put_completed:true},gates:{refreeze_result:"green"},outcome:"RED",rerun_guard:"armed"}' > "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
printf '%s\n' in-progress > "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
chmod 600 "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
export MOCK_STATE="$state" MOCK_SCENARIO=resume-same-id MOCK_NEW_APP_ID="$recovery_app" MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION="$current_version" MOCK_TENANT=tenant-test
set +e
"$harness" --mode mock --resume-recovery --ack "$resume_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --recovery-source-sha "$commit" --recovery-expected-version "$recovery_version" --recovery-fabricd-app-id "$recovery_app" --expected-version "$current_version" --fabricd-app-id "$current_app" --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability-key" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://spawn.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0 >/dev/null 2>"$tmp/resume-same-id.stderr"
resume_same_id_rc=$?
set -e
test "$resume_same_id_rc" != 0
rg -q 'current fabricd application id mismatch' "$tmp/resume-same-id.stderr"
test -f "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
test ! -e "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.complete"
printf '%s\n' 'resume-same-id=fail-closed'
for mismatch in schema_version artifact_id source.repository source.commit_sha provider.worker provider.expected_version provider.app_id; do
  run_resume_artifact_mismatch "$(printf '%s' "$mismatch" | tr . _)" "$mismatch"
done

make_fixture resume-nonancestor
printf '%s\n' bm9uLXJvdGF0ZWQtb2JzZXJ2YWJpbGl0eS1rZXk= > "$oob/observability-key"; chmod 600 "$oob/observability-key"
printf '%s\n' bmV3LWludHJvc3BlY3Qta2V5 > "$oob/new-introspect"; chmod 600 "$oob/new-introspect"
nonancestor_sha="$(printf '%s\n' nonancestor | git -C "$root" commit-tree "$(git -C "$root" rev-parse 'HEAD^{tree}')")"
jq -n --arg commit "$nonancestor_sha" --arg digest "$MOCK_DIGEST" --arg version "$recovery_version" --arg app "$recovery_app" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$app},phase:"container-recreate",secrets:{FABRIC_INTROSPECT_AUTH_KEY_put_completed:true,FABRIC_OBSERVABILITY_KEY_put_completed:true},gates:{refreeze_result:"green"},outcome:"RED",rerun_guard:"armed"}' > "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
printf '%s\n' in-progress > "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
chmod 600 "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
export MOCK_STATE="$state" MOCK_SCENARIO=resume-nonancestor MOCK_NEW_APP_ID="$current_app" MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION="$current_version" MOCK_TENANT=tenant-test
set +e
"$harness" --mode mock --resume-recovery --ack "$resume_ack" --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --recovery-source-sha "$nonancestor_sha" --recovery-expected-version "$recovery_version" --recovery-fabricd-app-id "$recovery_app" --expected-version "$current_version" --fabricd-app-id "$current_app" --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability-key" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://spawn.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0 >/dev/null 2>"$tmp/resume-nonancestor.stderr"
nonancestor_rc=$?
set -e
test "$nonancestor_rc" != 0
rg -q 'not an ancestor' "$tmp/resume-nonancestor.stderr"
printf '%s\n' 'resume-nonancestor=fail-closed'

make_fixture lock
mkdir "$oob/.fabricd-observability-key-bootstrap.lock"
export MOCK_STATE="$state" MOCK_SCENARIO=success MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION=version-good MOCK_TENANT=tenant-test
set +e
"$harness" --mode mock --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version version-good --fabricd-app-id app-old --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --introspect-key-file "$oob/introspect" --introspect-pat-file "$oob/pat" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --stability-seconds 0 >/dev/null 2>"$tmp/lock.stderr"
rc=$?; set -e; test "$rc" != 0; printf '%s\n' 'lock=fail'

echo 'fabricd observability bootstrap selftest: PASS'
