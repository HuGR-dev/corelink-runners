#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BRIDGE="$ROOT/bin/control-bridge.sh"
CASE="$(mktemp -d /private/tmp/corelink-b2-bridge.XXXXXX)"
trap 'chmod 755 "$CASE/bin/rotation-controller.sh" 2>/dev/null || true; rm -rf "$CASE"' EXIT
mkdir -p "$CASE/bin"
cp "$BRIDGE" "$CASE/bin/control-bridge.sh"
cat > "$CASE/bin/rotation-controller.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' '{"schema":"corelink-b2-rotation-local-controller-v1","operation":"preflight","nonce":"rotation-v2-bridge-test","admission_freeze_deployed":"1","admission_paused":"1","fleet_endpoint":"/internal/v1/fleet/busy","fleet_busy":"0","fleet_unverifiable":"0","ledger_backend":"in_memory","pending_terminalized":"1","held_terminalized":"1","maintenance_impact_recorded":"1","durable_pending":"0","durable_held":"0","active_boxes":"0","dynamic_current":"1","spawn_worker":"corelink-spawn-worker","fabricd_worker":"corelink-fabricd","spawn_runner_image":"runner","spawn_check_image":"check","fabricd_image":"fabricd","spawn_config_sha256":"spawn","fabricd_config_sha256":"fabric"}'
EOF
chmod 755 "$CASE/bin/rotation-controller.sh"
printf '%s\n' manifest > "$CASE/manifest"
chmod 600 "$CASE/manifest"

run_bridge() {
  ROTATION_CONTROL_MANIFEST="$CASE/manifest" \
  ROTATION_LOCAL_CONTROLLER_SHA256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  ROTATION_LOCAL_MANIFEST_SHA256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \
  "$CASE/bin/control-bridge.sh" preflight rotation-v2-bridge-test
}

chmod 755 "$CASE/bin/rotation-controller.sh"
if run_bridge >/dev/null; then
  printf '%s\n' 'ok - bridge accepts owner-executable 0755 controller'
else
  printf '%s\n' 'not ok - bridge accepts owner-executable 0755 controller' >&2
  exit 1
fi

for mode in 775 777; do
  chmod "$mode" "$CASE/bin/rotation-controller.sh"
  if run_bridge >/dev/null 2>&1; then
    printf 'not ok - bridge rejects controller mode %s\n' "$mode" >&2
    exit 1
  fi
  printf 'ok - bridge rejects controller mode %s\n' "$mode"
done

printf '%s\n' '3 bridge mode tests passed'
