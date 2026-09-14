#!/usr/bin/env bash
# Fast, network-free contract test for AU1.8's operational-window gates.
set -Eeuo pipefail

here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/au1.8-fabricd-cred-ticket-rotation.sh"
window_fn="$(sed -n '/^check_window() {/,/^}$/p' "$harness")"
credential_fn="$(sed -n '/^require_credential_mutation_budget() {/,/^}$/p' "$harness")"

run_case() {
  local name="$1" now="$2" expect_rc="$3" expect_put="$4" tmp rc
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/au1.8-window-selftest.XXXXXX")"
  set +e
  MOCK_NOW="$now" CASE_TMP="$tmp" bash -u -c '
    set -Eeuo pipefail
    eval "$1"; eval "$2"
    OPERATIONAL_WINDOW_MAX_SECS=2400
    CREDENTIAL_MUTATION_MIN_REMAINING_SECS=900
    WINDOW_START=0; EVENT_LOG="$CASE_TMP/events"
    date() { printf "%s\n" "$MOCK_NOW"; }
    log_event() { printf "%s\n" "$*" >> "$EVENT_LOG"; }
    put_secret() { printf "put\n" >> "$CASE_TMP/puts"; }
    case "$3" in
      window) check_window ;;
      credential) require_credential_mutation_budget && put_secret FABRIC_CRED_TICKET_SECRET new-secret ;;
    esac
  ' -- "$window_fn" "$credential_fn" "$name"
  rc=$?
  set -e
  if [[ "$expect_rc" == pass ]]; then
    [[ "$rc" == 0 ]] || { rm -rf -- "$tmp"; return 1; }
  else
    [[ "$rc" != 0 ]] || { rm -rf -- "$tmp"; return 1; }
  fi
  if [[ "$expect_put" == yes ]]; then
    [[ "$(wc -l < "$tmp/puts")" == 1 ]]
  else
    [[ ! -e "$tmp/puts" ]]
  fi
  rm -rf -- "$tmp"
}

run_case window 601 pass no || { echo 'FAIL: >600 and <2400 seconds must pass' >&2; exit 1; }
run_case window 2400 pass no || { echo 'FAIL: exactly 2400 seconds must pass' >&2; exit 1; }
run_case window 2401 fail no || { echo 'FAIL: >2400 seconds must be RED' >&2; exit 1; }
run_case credential 1501 fail no || { echo 'FAIL: <900 seconds remaining must be RED before put' >&2; exit 1; }
run_case credential 1500 pass yes || { echo 'FAIL: >=900 seconds remaining must allow put' >&2; exit 1; }
if ! rg -q '^readonly OPERATIONAL_WINDOW_MAX_SECS=2400$' "$harness" ||
   ! rg -q -- '--argjson maximum_seconds "\$OPERATIONAL_WINDOW_MAX_SECS"' "$harness" ||
   ! rg -F -q 'timing:{maximum_seconds:$maximum_seconds' "$harness"; then
  echo 'FAIL: evidence maximum_seconds must use the named window constant' >&2
  exit 1
fi
printf 'AU1.8 operational-window selftest PASS\n'
