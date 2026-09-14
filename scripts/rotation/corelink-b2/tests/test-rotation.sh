#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
HARNESS="$ROOT/bin/rotate-exposed-secrets.sh"
MOCK_ROOT="$ROOT/tests/mock-bin"
PASS=0
FAIL=0
SPAWN_HASH='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
FABRICD_HASH='bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
RUNNER_IMAGE='registry.example/runner@sha256:1111111111111111111111111111111111111111111111111111111111111111'
CHECK_IMAGE='registry.example/check@sha256:2222222222222222222222222222222222222222222222222222222222222222'
FABRIC_IMAGE='registry.example/fabricd@sha256:3333333333333333333333333333333333333333333333333333333333333333'
ok() { PASS=$((PASS + 1)); printf 'ok - %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf 'not ok - %s\n' "$1" >&2; }
new_case() {
  CASE_DIR="$(mktemp -d /private/tmp/corelink-rotation-v2-test.XXXXXX)"
  mkdir "$CASE_DIR/oob" "$CASE_DIR/evidence"
  chmod 700 "$CASE_DIR/oob"
  : > "$CASE_DIR/spawn.conf"; : > "$CASE_DIR/fabricd.conf"
  printf '%s\n' 'EXPOSED-TEST-SPAWN-TOKEN-DO-NOT-LOG' > "$CASE_DIR/old-spawn-token"
  chmod 600 "$CASE_DIR/old-spawn-token"
  printf '%s\n' 'schema=corelink-exposed-secret-rotation-v2' 'spawn_worker=corelink-spawn-worker' 'fabricd_worker=corelink-fabricd' "spawn_config=$CASE_DIR/spawn.conf" "fabricd_config=$CASE_DIR/fabricd.conf" "spawn_runner_image=$RUNNER_IMAGE" "spawn_check_image=$CHECK_IMAGE" "fabricd_image=$FABRIC_IMAGE" "spawn_config_sha256=$SPAWN_HASH" "fabricd_config_sha256=$FABRICD_HASH" > "$CASE_DIR/pins"
  : > "$CASE_DIR/log"; : > "$CASE_DIR/counter"
}
run_mock() {
  local scenario="$1"; shift
  MOCK_SCENARIO="$scenario" MOCK_LOG="$CASE_DIR/log" MOCK_COUNTER_FILE="$CASE_DIR/counter" MOCK_EVIDENCE_DIR="$CASE_DIR/evidence" MOCK_SPAWN_HASH="$SPAWN_HASH" MOCK_FABRICD_HASH="$FABRICD_HASH" MOCK_RUNNER_IMAGE="$RUNNER_IMAGE" MOCK_CHECK_IMAGE="$CHECK_IMAGE" MOCK_FABRIC_IMAGE="$FABRIC_IMAGE" "$HARNESS" --mode mock --mock-root "$MOCK_ROOT" --oob-dir "$CASE_DIR/oob" --evidence-dir "$CASE_DIR/evidence" --pin-manifest "$CASE_DIR/pins" --wrangler-bin "$MOCK_ROOT/wrangler" --control-bin "$MOCK_ROOT/control" --entropy-bin "$MOCK_ROOT/entropy" --derive-bin "$MOCK_ROOT/derive" --hash-bin "$MOCK_ROOT/hash" --mode-bin "$MOCK_ROOT/mode" --owner-bin "$MOCK_ROOT/owner" --auth-probe-bin "$MOCK_ROOT/auth-probe" --spawn-worker-url 'https://spawn.example' --old-spawn-token-file "$CASE_DIR/old-spawn-token" "$@"
}
no_writes() { ! grep -q '^wrangler:' "$CASE_DIR/log"; }
failed_state() { [ "$(sed -n '2p' "$CASE_DIR/evidence/v1/cutover.state")" = 'outcome=failed' ]; }
if PATH=/definitely-not-a-bin /bin/bash "$HARNESS" >/dev/null; then ok 'default plan-only makes no external command lookup'; else bad 'default plan-only makes no external command lookup'; fi
new_case
if run_mock success --lifecycle run >/dev/null && grep -q '^wrangler:<secret><put><CLOUDFLARE_SPAWN_AUTH_TOKEN>' "$CASE_DIR/log" && grep -q '^wrangler:<secret><put><FABRIC_SIGNING_KEY>' "$CASE_DIR/log" && grep -q '^auth-probe:<post-cutover><https://spawn.example>' "$CASE_DIR/log" && grep -q '^wrangler:<deploy><--config><.*<--containers-rollout=immediate>' "$CASE_DIR/log" && grep -q '^control:<release-freeze>' "$CASE_DIR/log" && grep -q '^control:<lifecycle>' "$CASE_DIR/log" && [ "$(sed -n '2p' "$CASE_DIR/evidence/v1/cutover.state")" = 'outcome=complete' ] && ! grep -E 'AAAAAAAA|BBBBBBBB|CCCCCCCC|DDDDDDDD|EXPOSED-TEST-SPAWN-TOKEN' "$CASE_DIR/log" "$CASE_DIR/evidence/v1/"* >/dev/null; then ok 'mock success rotates forward with sanitized evidence'; else bad 'mock success rotates forward with sanitized evidence'; fi
for scenario in preflight_freeze preflight_fleet preflight_unverifiable preflight_pending preflight_unterminalized preflight_impact preflight_work preflight_boxes preflight_stale preflight_identity preflight_image preflight_pin corelink_derivation corelink_contract corelink_attestations corelink_key bad_mode bad_owner; do
  new_case
  if run_mock "$scenario" >/dev/null 2>&1; then bad "$scenario refuses"; elif no_writes && failed_state; then ok "$scenario refuses before mutation"; else bad "$scenario refuses before mutation"; fi
done
new_case
chmod 755 "$CASE_DIR/oob"
if run_mock success >/dev/null 2>&1; then bad 'insecure OOB directory refuses'; elif no_writes && failed_state; then ok 'insecure OOB directory refuses before mutation'; else bad 'insecure OOB directory refuses before mutation'; fi
new_case
mv "$CASE_DIR/oob" "$CASE_DIR/oob-real"
ln -s "$CASE_DIR/oob-real" "$CASE_DIR/oob"
if run_mock success >/dev/null 2>&1; then bad 'symlink OOB directory refuses'; elif no_writes && [ ! -e "$CASE_DIR/evidence/v1/cutover.state" ]; then ok 'symlink OOB directory refuses before mutation'; else bad 'symlink OOB directory refuses before mutation'; fi
for scenario in wrangler_fail auth_new auth_old auth_bound post_health post_ready post_spawn_health post_provider post_image post_config post_boot post_container post_keyshape post_corelink_contract post_corelink_old_key lifecycle_fail release_fail; do
  new_case
  args=()
  [ "$scenario" != lifecycle_fail ] || args+=(--lifecycle run)
  if run_mock "$scenario" "${args[@]}" >/dev/null 2>&1; then bad "$scenario refuses"; elif [ "$scenario" = lifecycle_fail ] && ! grep -q '^control:<refreeze>' "$CASE_DIR/log"; then bad 'lifecycle failure auto-refreezes'; elif failed_state; then ok "$scenario records failed forward attempt"; else bad "$scenario records failed forward attempt"; fi
done
new_case
if run_mock release_evidence_fail >/dev/null 2>&1; then bad 'release evidence failure refuses'; elif grep -q '^control:<release-freeze>' "$CASE_DIR/log" && grep -q '^control:<refreeze>' "$CASE_DIR/log" && failed_state; then ok 'release evidence failure automatically refreezes'; else bad 'release evidence failure automatically refreezes'; fi
new_case
if run_mock success --attempt recovery >/dev/null 2>&1; then bad 'recovery without failed primary refuses'; else ok 'recovery without failed primary refuses'; fi
new_case
if ! run_mock auth_new >/dev/null 2>&1 && run_mock success --attempt recovery --lifecycle run >/dev/null && [ "$(sed -n '2p' "$CASE_DIR/evidence/v1/cutover.state")" = 'outcome=complete' ] && [ "$(<"$CASE_DIR/counter")" = 4 ] && grep -q '^control:<corelink-stage>' "$CASE_DIR/log" && grep -q '^control:<lifecycle>' "$CASE_DIR/log"; then ok 'recovery is forward-only, reuses its pair, and reruns proofs'; else bad 'recovery is forward-only, reuses its pair, and reruns proofs'; fi
[ "$FAIL" -eq 0 ] || { printf '%s/%s tests failed\n' "$FAIL" "$((PASS + FAIL))" >&2; exit 1; }
printf '%s tests passed\n' "$PASS"
