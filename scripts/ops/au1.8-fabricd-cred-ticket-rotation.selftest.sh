#!/usr/bin/env bash
# Focused, network-free gates for the AU1.8 destructive harness.
set -Eeuo pipefail
umask 077

here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/au1.8-fabricd-cred-ticket-rotation.sh"
repo="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-selftest.XXXXXX")"
trap 'rm -rf -- "$repo"' EXIT

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

echo "AU1.8 focused gates: PASS"
