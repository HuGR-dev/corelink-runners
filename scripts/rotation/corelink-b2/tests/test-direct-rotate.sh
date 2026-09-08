#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
HARNESS="$ROOT/bin/direct-rotate-production.sh"
MOCK_WRANGLER="$ROOT/tests/direct-mock-wrangler.sh"
MOCK_CURL="$ROOT/tests/direct-mock-curl.sh"
pass=0; fail=0
ok() { pass=$((pass + 1)); printf 'ok - %s\n' "$1"; }
bad() { fail=$((fail + 1)); printf 'not ok - %s\n' "$1" >&2; }
new_case() {
  CASE="$(mktemp -d /private/tmp/corelink-direct-rotation.XXXXXX)"
  mkdir -m 700 "$CASE/oob" "$CASE/evidence" "$CASE/repo"
  git -C "$CASE/repo" init -q
  git -C "$CASE/repo" config user.email test@example.invalid
  git -C "$CASE/repo" config user.name test
  printf '{}\n' > "$CASE/repo/spawn.json"; printf '{}\n' > "$CASE/repo/fabric.json"
  git -C "$CASE/repo" add . && git -C "$CASE/repo" commit -qm baseline
  COMMIT="$(git -C "$CASE/repo" rev-parse HEAD)"
  printf '%s\n' fleet-key > "$CASE/oob/fleet-busy-read-key"
  printf '%s\n' canary-pat > "$CASE/oob/corelink-canary-tenant-pat"
  printf '%s\n' OLD-TOKEN > "$CASE/oob/old-token"
  chmod 600 "$CASE/oob/fleet-busy-read-key" "$CASE/oob/corelink-canary-tenant-pat" "$CASE/oob/old-token"
  : > "$CASE/log"
}
run_case() {
  DIRECT_MOCK_LOG="$CASE/log" DIRECT_MOCK_FAIL="${1:-}" "$HARNESS" --mode mock --mock-wrangler "$MOCK_WRANGLER" --curl-bin "$MOCK_CURL" --integration-root "$CASE/repo" --expected-commit "$COMMIT" --spawn-image "registry.example/spawn@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" --fabricd-image "registry.example/fabric@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" --spawn-version v-pin --fabricd-version v-pin --spawn-config "$CASE/repo/spawn.json" --fabricd-config "$CASE/repo/fabric.json" --spawn-config-sha256 "$(openssl dgst -sha256 -r "$CASE/repo/spawn.json" | awk '{print $1}')" --fabricd-config-sha256 "$(openssl dgst -sha256 -r "$CASE/repo/fabric.json" | awk '{print $1}')" --oob-dir "$CASE/oob" --fleet-key-file "$CASE/oob/fleet-busy-read-key" --canary-pat-file "$CASE/oob/corelink-canary-tenant-pat" --old-token-file "$CASE/oob/old-token" --evidence-dir "$CASE/evidence"
}
if plan_output="$(PATH=/nope /bin/bash "$HARNESS")" && grep -q 'PLAN ONLY' <<<"$plan_output"; then ok 'default remains inert'; else bad 'default plan-only'; fi
chmod +x "$MOCK_WRANGLER" "$MOCK_CURL"
new_case
if run_case && grep -q 'outcome=complete_frozen' "$CASE/evidence/events.log" && grep -q 'FABRIC_ADMISSION_PAUSED:1' "$CASE/log" && ! grep -q 'OLD-TOKEN\|canary-pat\|fleet-key' "$CASE/evidence/events.log"; then ok 'success leaves both surfaces frozen with sanitized evidence'; else bad 'success path'; fi
rm -rf "$CASE"
new_case
if run_case canary >/dev/null 2>&1; then bad 'canary failure refuses'; elif grep -q 'freeze=armed' "$CASE/evidence/events.log" && grep -q 'deploy --config .*fabric.*FABRIC_ADMISSION_PAUSED:1' "$CASE/log"; then ok 'canary failure refreezes'; else bad 'canary failure refreezes'; fi
rm -rf "$CASE"
new_case
if "$HARNESS" --mode live --integration-root x >/dev/null 2>&1; then bad 'live requires acknowledgements and pins'; else ok 'live acknowledgements and pins are mandatory'; fi
rm -rf "$CASE"
if shellcheck "$HARNESS" "$MOCK_WRANGLER" "$MOCK_CURL"; then ok 'shellcheck'; else bad 'shellcheck'; fi
[ "$fail" -eq 0 ] || exit 1
printf '%s tests passed\n' "$pass"
