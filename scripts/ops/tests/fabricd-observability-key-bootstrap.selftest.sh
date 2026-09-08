#!/usr/bin/env bash
# Network-free contract tests for the pre-AU1.8 observability-key bootstrap.
set -Eeuo pipefail
umask 077
here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/../fabricd-observability-key-bootstrap.sh"
wrangler="$here/mock-observability-bootstrap-wrangler.sh"
curl_mock="$here/mock-observability-bootstrap-curl.sh"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-bootstrap-test.XXXXXXXX")"
trap 'rm -rf -- "$tmp"' EXIT
chmod 700 "$tmp"

make_fixture() {
  root="$tmp/root-$1"; oob="$tmp/oob-$1"; state="$tmp/state-$1"; mkdir -p "$root/deploy/cloudflare-fabricd" "$root/evidence" "$oob"; chmod 700 "$root" "$root/evidence" "$oob"
  printf '%s\n' '{"image":"registry.example/corelink@sha256:1111111111111111111111111111111111111111111111111111111111111111"}' > "$root/deploy/cloudflare-fabricd/wrangler.jsonc"
  git -C "$root" init -q; git -C "$root" config user.email bootstrap@example.invalid; git -C "$root" config user.name bootstrap; git -C "$root" add .; git -C "$root" commit -qm baseline; commit="$(git -C "$root" rev-parse HEAD)"
  printf '%s\n' fleet-key > "$oob/fleet"; printf '%s\n' introspect-key > "$oob/introspect"; printf '%s\n' pat > "$oob/pat"; chmod 600 "$oob/fleet" "$oob/introspect" "$oob/pat"
  export root oob state commit
}

run_case() {
  local name="$1" scenario="$2" expected="$3"; make_fixture "$name"
  export MOCK_STATE="$state" MOCK_SCENARIO="$scenario" MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION='version-good' MOCK_TENANT='tenant-test'
  set +e
  "$harness" --mode mock --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version version-good --fabricd-app-id app-old --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --introspect-key-file "$oob/introspect" --introspect-pat-file "$oob/pat" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --status-url https://status.test/internal/v1/status --fleet-url https://spawn.test/internal/v1/fleet/busy --introspect-url https://api.test/internal/v1/auth/introspect --stability-seconds 0 >/dev/null 2>"$tmp/$name.stderr"
  rc=$?; set -e
  if { [ "$expected" = pass ] && [ "$rc" = 0 ]; } || { [ "$expected" = fail ] && [ "$rc" != 0 ]; }; then :; else printf 'unexpected result for %s (rc=%s)\n' "$name" "$rc" >&2; exit 1; fi
  printf '%s\n' "$name=$expected"
}

run_case success success pass
grep -q '"status": "PASS"' "$tmp/root-success/evidence/result.json"
test -f "$tmp/oob-success/fabric-observability-key-bootstrap.b64"
test "$(stat -f '%Lp' "$tmp/oob-success/fabric-observability-key-bootstrap.b64")" = 600
key_value="$(tr -d '\r\n' < "$tmp/oob-success/fabric-observability-key-bootstrap.b64")"
if rg -F "$key_value" "$tmp/root-success/evidence" "$tmp/oob-success" -g '!fabric-observability-key-bootstrap.b64' >/dev/null; then exit 1; fi

run_case drift drift fail
test ! -f "$tmp/state-drift.secret-put"

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

make_fixture lock
mkdir "$oob/.fabricd-observability-key-bootstrap.lock"
export MOCK_STATE="$state" MOCK_SCENARIO=success MOCK_DIGEST='sha256:1111111111111111111111111111111111111111111111111111111111111111' MOCK_VERSION=version-good MOCK_TENANT=tenant-test
set +e
"$harness" --mode mock --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --repo-root "$root" --expected-commit "$commit" --expected-version version-good --fabricd-app-id app-old --expected-image-digest "$MOCK_DIGEST" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --introspect-key-file "$oob/introspect" --introspect-pat-file "$oob/pat" --tenant-id tenant-test --evidence-file "$root/evidence/result.json" --stability-seconds 0 >/dev/null 2>"$tmp/lock.stderr"
rc=$?; set -e; test "$rc" != 0; printf '%s\n' 'lock=fail'

echo 'fabricd observability bootstrap selftest: PASS'
