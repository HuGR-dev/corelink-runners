#!/usr/bin/env bash
# Fast, network-free contract test for AU1.8's operational-window gates.
set -Eeuo pipefail

here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/au1.8-fabricd-cred-ticket-rotation.sh"
window_fn="$(sed -n '/^check_window() {/,/^}$/p' "$harness")"

run_case() {
  local now="$1" expect_rc="$2" tmp rc
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-window-selftest.XXXXXX")"
  set +e
  MOCK_NOW="$now" CASE_TMP="$tmp" bash -u -c '
    set -Eeuo pipefail
    eval "$1"
    OPERATIONAL_WINDOW_MAX_SECS=600
    WINDOW_START=0; EVENT_LOG="$CASE_TMP/events"
    date() { printf "%s\n" "$MOCK_NOW"; }
    log_event() { printf "%s\n" "$*" >> "$EVENT_LOG"; }
    check_window
  ' -- "$window_fn"
  rc=$?
  set -e
  if [[ "$expect_rc" == pass ]]; then
    [[ "$rc" == 0 ]] || { rm -rf -- "$tmp"; return 1; }
  else
    [[ "$rc" != 0 ]] || { rm -rf -- "$tmp"; return 1; }
  fi
  rm -rf -- "$tmp"
}

run_case 600 pass || { echo 'FAIL: exactly 600 seconds must pass' >&2; exit 1; }
run_case 601 fail || { echo 'FAIL: 601 seconds must be RED' >&2; exit 1; }
if ! rg -q '^readonly OPERATIONAL_WINDOW_MAX_SECS=600$' "$harness" ||
   ! rg -q -- '--argjson maximum_seconds "\$OPERATIONAL_WINDOW_MAX_SECS"' "$harness" ||
   ! rg -F -q 'timing:{maximum_seconds:$maximum_seconds' "$harness"; then
  echo 'FAIL: evidence maximum_seconds must use the named window constant' >&2
  exit 1
fi
printf 'AU1.8 operational-window selftest PASS\n'
