#!/usr/bin/env bash
# Focused, network-free gates for the AU1.8 destructive harness.
set -Eeuo pipefail
umask 077

here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/au1.8-fabricd-cred-ticket-rotation.sh"
repo="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-selftest.XXXXXX")"
test_mint_key="$(mktemp "${TMPDIR:-/tmp}/au1.8-test-mint.XXXXXX")"
observability_key="$(mktemp "${TMPDIR:-/tmp}/au1.8-observability.XXXXXX")"
trap 'rm -rf -- "$repo" "$test_mint_key" "$observability_key"' EXIT

git -C "$repo" init -q
git -C "$repo" config user.email au1.8-selftest@example.invalid
git -C "$repo" config user.name au1.8-selftest
printf 'tracked\n' > "$repo/tracked"
git -C "$repo" add tracked
git -C "$repo" commit -q -m baseline
head="$(git -C "$repo" rev-parse HEAD)"

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
  echo "FAIL: ledger_cross_instance_safe=false must block AU1.8" >&2
  exit 1
fi
if validate "$head" '{"num_shards":1}'; then
  echo "FAIL: missing ledger_cross_instance_safe must block AU1.8" >&2
  exit 1
fi

printf 'test-mint-key\n' > "$test_mint_key"
printf 'observability-key\n' > "$observability_key"
chmod 600 "$test_mint_key" "$observability_key"
validate_files
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

if rg -n -- '--containers-rollout=none' "$harness" >/dev/null; then
  echo "FAIL: AU1.8 must recreate with immediate immutable rollout" >&2
  exit 1
fi
if [[ "$(rg -c -- '--containers-rollout=immediate' "$harness")" -lt 2 ]]; then
  echo "FAIL: AU1.8 must use immediate rollout for arm and disarm" >&2
  exit 1
fi

echo "AU1.8 focused gates: PASS"
