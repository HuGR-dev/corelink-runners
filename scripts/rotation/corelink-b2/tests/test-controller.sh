#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROLLER="$ROOT/bin/rotation-controller.sh"
FIXTURE="$ROOT/tests/mock-bin/operation"
CORELINK_STAGE="$ROOT/bin/op_corelink_stage.sh"
CORELINK_POSTFLIGHT="$ROOT/bin/op_postflight.sh"
TOOL="$ROOT/tests/mock-bin/tool"
PASS=0
FAIL=0
RUNNER_IMAGE='registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-runnercontainer@sha256:1111111111111111111111111111111111111111111111111111111111111111'
CHECK_IMAGE='registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-checkhostcontainer@sha256:2222222222222222222222222222222222222222222222222222222222222222'
FABRIC_IMAGE='registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-fabricd-fabricdcontainer@sha256:3333333333333333333333333333333333333333333333333333333333333333'
SPAWN_HASH='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
FABRIC_HASH='bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
KEY_ID='2d16e9ef2102df2a'
PUBKEY='+X0vGNFOSY5t9jo7OTlJNZsoLZOxE172jw/QURNEYw4='
NONCE='rotation-v2-test-nonce-123456'

ok() { PASS=$((PASS + 1)); printf 'ok - %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf 'not ok - %s\n' "$1" >&2; }
sha() { /usr/bin/openssl dgst -sha256 -r "$1" | /usr/bin/awk '{print $1}'; }
trap 'chmod 755 "$CONTROLLER" 2>/dev/null || true' EXIT

new_case() {
  CASE="$(mktemp -d /private/tmp/corelink-b2-local-controller.XXXXXX)"
  MOCK="$CASE/mock"
  mkdir -p "$MOCK/integration/deploy/cloudflare" "$MOCK/integration/deploy/cloudflare-fabricd" "$MOCK/state"
  chmod 700 "$CASE" "$MOCK" "$MOCK/integration" "$MOCK/integration/deploy" "$MOCK/integration/deploy/cloudflare" "$MOCK/integration/deploy/cloudflare-fabricd" "$MOCK/state"
  : > "$MOCK/integration/deploy/cloudflare/wrangler.jsonc"
  : > "$MOCK/integration/deploy/cloudflare-fabricd/wrangler.jsonc"
  chmod 600 "$MOCK/integration/deploy/cloudflare/wrangler.jsonc" "$MOCK/integration/deploy/cloudflare-fabricd/wrangler.jsonc"
  for name in fleet corelink lifecycle; do printf '%s\n' "TEST_TOKEN_${name^^}_ABCDEFGHIJKLMNOP" > "$MOCK/$name.token"; chmod 600 "$MOCK/$name.token"; done
  printf '%s\n' 'schema=cloudflare-oauth-env-v1' 'account_id=6a1fc1c626fc2628823e60b9db01f5cd' 'token_env=CLOUDFLARE_API_TOKEN' > "$MOCK/oauth.conf"
  chmod 600 "$MOCK/oauth.conf"
  for role in preflight release_freeze lifecycle refreeze; do cp "$FIXTURE" "$MOCK/$role"; chmod 700 "$MOCK/$role"; done
  cp "$TOOL" "$MOCK/wrangler"; cp "$TOOL" "$MOCK/curl"; chmod 700 "$MOCK/wrangler" "$MOCK/curl"
  : > "$MOCK/log"
  make_manifest
}

make_manifest() {
  local spawn_hash fabric_hash
  spawn_hash="$(sha "$MOCK/integration/deploy/cloudflare/wrangler.jsonc")"
  fabric_hash="$(sha "$MOCK/integration/deploy/cloudflare-fabricd/wrangler.jsonc")"
  [ "$spawn_hash" = "$SPAWN_HASH" ] || SPAWN_HASH="$spawn_hash"
  [ "$fabric_hash" = "$FABRIC_HASH" ] || FABRIC_HASH="$fabric_hash"
  {
    printf '%s\n' \
      'schema=corelink-b2-rotation-local-controller-v1' \
      "integration_root=$MOCK/integration" \
      'integration_commit=27c08dd76650bc3385c69ce60f7fea5726d05a82' \
      'cloudflare_account_id=6a1fc1c626fc2628823e60b9db01f5cd' \
      'spawn_worker=corelink-spawn-worker' 'fabricd_worker=corelink-fabricd' \
      "spawn_config=$MOCK/integration/deploy/cloudflare/wrangler.jsonc" \
      "fabricd_config=$MOCK/integration/deploy/cloudflare-fabricd/wrangler.jsonc" \
      'spawn_worker_url=https://corelink-spawn-worker.gmhelmold.workers.dev' \
      'fabricd_worker_url=https://corelink-fabricd.gmhelmold.workers.dev' \
      'fleet_busy_url=https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy' \
      "spawn_runner_image=$RUNNER_IMAGE" "spawn_check_image=$CHECK_IMAGE" "fabricd_image=$FABRIC_IMAGE" \
      "spawn_config_sha256=$SPAWN_HASH" "fabricd_config_sha256=$FABRIC_HASH" \
      "oauth_config=$MOCK/oauth.conf" "fleet_read_token_file=$MOCK/fleet.token" "corelink_control_token_file=$MOCK/corelink.token" "lifecycle_token_file=$MOCK/lifecycle.token" \
      "state_file=$MOCK/state/state" "wrangler_bin=$MOCK/wrangler" "curl_bin=$MOCK/curl"
    for role in preflight release_freeze lifecycle refreeze; do printf 'op_%s=%s/%s\nsha_op_%s=%s\n' "$role" "$MOCK" "$role" "$role" "$(sha "$MOCK/$role")"; done
    printf 'op_corelink_stage=%s\nsha_op_corelink_stage=%s\nop_postflight=%s\nsha_op_postflight=%s\n' "$CORELINK_STAGE" "$(sha "$CORELINK_STAGE")" "$CORELINK_POSTFLIGHT" "$(sha "$CORELINK_POSTFLIGHT")"
  } > "$MOCK/manifest"
  chmod 600 "$MOCK/manifest"
}

run_mock() {
  local scenario="$1"; shift
  MOCK_SCENARIO="$scenario" MOCK_LOG="$MOCK/log" MOCK_SPAWN_HASH="$SPAWN_HASH" MOCK_FABRIC_HASH="$FABRIC_HASH" MOCK_RUNNER_IMAGE="$RUNNER_IMAGE" MOCK_CHECK_IMAGE="$CHECK_IMAGE" MOCK_FABRIC_IMAGE="$FABRIC_IMAGE" \
  ROTATION_LOCAL_CONTROLLER_SHA256="$(sha "$CONTROLLER")" ROTATION_LOCAL_MANIFEST_SHA256="$(sha "$MOCK/manifest")" \
  "$CONTROLLER" --mode mock --mock-root "$MOCK" --manifest "$MOCK/manifest" "$@"
}

if PATH=/definitely-not-a-bin /bin/bash "$CONTROLLER" preflight "$NONCE" | grep -q '"mode":"plan-only"'; then ok 'plan-only is inert'; else bad 'plan-only is inert'; fi

new_case
chmod 755 "$CONTROLLER"
if run_mock success preflight "$NONCE" >/dev/null; then ok 'owner-executable 0755 controller is accepted'; else bad 'owner-executable 0755 controller is accepted'; fi
for mode in 775 777; do
  chmod "$mode" "$CONTROLLER"
  if run_mock success preflight "$NONCE" >/dev/null 2>&1; then bad "controller mode $mode is refused"; else ok "controller mode $mode is refused"; fi
done
chmod 755 "$CONTROLLER"

new_case
if run_mock success preflight "$NONCE" >/dev/null && run_mock success corelink-stage "$NONCE" "$KEY_ID" "$PUBKEY" >/dev/null && run_mock success postflight primary "$NONCE" "$KEY_ID" "$PUBKEY" >/dev/null && run_mock success release-freeze "$NONCE" primary >/dev/null && run_mock success lifecycle "$NONCE" "$KEY_ID" | grep -q '"operation":"lifecycle"' && [ "$(tr '\n' ' ' < "$MOCK/log")" = 'preflight release_freeze lifecycle refreeze ' ] && ! rg -q 'TEST_TOKEN_' "$MOCK/log" "$MOCK/state"; then ok 'serialized canary auto-refreezes without token output'; else bad 'serialized canary auto-refreezes without token output'; fi

for scenario in preflight_fail fleet_busy bad_pin; do
  new_case
  if run_mock "$scenario" preflight "$NONCE" >/dev/null 2>&1; then bad "$scenario fails closed"; elif [ ! -s "$MOCK/log" ] || [ "$(head -n 1 "$MOCK/log")" = preflight ]; then ok "$scenario fails closed"; else bad "$scenario fails closed"; fi
done

new_case
if run_mock success preflight "$NONCE" >/dev/null && run_mock success corelink-stage "$NONCE" "$KEY_ID" "$PUBKEY" >/dev/null && run_mock health_fail postflight primary "$NONCE" "$KEY_ID" "$PUBKEY" >/dev/null 2>&1; then bad 'postflight exact health proof fails closed'; else ok 'postflight exact health proof fails closed'; fi

new_case
  if run_mock success preflight "$NONCE" >/dev/null && run_mock success corelink-stage "$NONCE" "$KEY_ID" "$PUBKEY" >/dev/null && run_mock success postflight primary "$NONCE" "$KEY_ID" "$PUBKEY" >/dev/null && run_mock success release-freeze "$NONCE" primary >/dev/null && ! run_mock lifecycle_fail lifecycle "$NONCE" "$KEY_ID" >/dev/null 2>&1 && [ "$(tail -n 1 "$MOCK/log")" = refreeze ] && grep -qx 'phase=refrozen' "$MOCK/state/state"; then ok 'canary failure automatically refreezes'; else bad 'canary failure automatically refreezes'; fi

new_case
chmod 644 "$MOCK/manifest"
if run_mock success preflight "$NONCE" >/dev/null 2>&1; then bad 'manifest mode is enforced'; else ok 'manifest mode is enforced'; fi

new_case
mv "$MOCK/manifest" "$MOCK/manifest.real"; ln -s "$MOCK/manifest.real" "$MOCK/manifest"
if run_mock success preflight "$NONCE" >/dev/null 2>&1; then bad 'manifest symlink is refused'; else ok 'manifest symlink is refused'; fi

new_case
if ROTATION_LOCAL_CONTROLLER_SHA256="$(sha "$CONTROLLER")" ROTATION_LOCAL_MANIFEST_SHA256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa MOCK_LOG="$MOCK/log" MOCK_SCENARIO=success MOCK_SPAWN_HASH="$SPAWN_HASH" MOCK_FABRIC_HASH="$FABRIC_HASH" MOCK_RUNNER_IMAGE="$RUNNER_IMAGE" MOCK_CHECK_IMAGE="$CHECK_IMAGE" MOCK_FABRIC_IMAGE="$FABRIC_IMAGE" "$CONTROLLER" --mode mock --mock-root "$MOCK" --manifest "$MOCK/manifest" preflight "$NONCE" >/dev/null 2>&1; then bad 'manifest hash is externally pinned'; else ok 'manifest hash is externally pinned'; fi

new_case
tampered="$CASE/tampered-manifest"
sed "s#^op_corelink_stage=.*#op_corelink_stage=$MOCK/preflight#" "$MOCK/manifest" > "$tampered"
chmod 600 "$tampered"
tampered_hash="$(sha "$tampered")"
if MOCK_LOG="$MOCK/log" MOCK_SCENARIO=success MOCK_SPAWN_HASH="$SPAWN_HASH" MOCK_FABRIC_HASH="$FABRIC_HASH" MOCK_RUNNER_IMAGE="$RUNNER_IMAGE" MOCK_CHECK_IMAGE="$CHECK_IMAGE" MOCK_FABRIC_IMAGE="$FABRIC_IMAGE" ROTATION_LOCAL_CONTROLLER_SHA256="$(sha "$CONTROLLER")" ROTATION_LOCAL_MANIFEST_SHA256="$tampered_hash" "$CONTROLLER" --mode mock --mock-root "$MOCK" --manifest "$tampered" preflight "$NONCE" >/dev/null 2>&1; then bad 'package operation path is fixed'; else ok 'package operation path is fixed'; fi

if bash -n "$CONTROLLER" && shellcheck -x -S warning "$CONTROLLER" "$FIXTURE" "$TOOL"; then ok 'bash syntax and shellcheck'; else bad 'bash syntax and shellcheck'; fi

[ "$FAIL" -eq 0 ] || { printf '%s/%s tests failed\n' "$FAIL" "$((PASS + FAIL))" >&2; exit 1; }
printf '%s tests passed\n' "$PASS"
