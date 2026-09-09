#!/usr/bin/env bash
set -euo pipefail
url="${!#}"
if [[ "$url" == *'/internal/v1/fleet/busy' ]]; then
  found=0
  for arg in "$@"; do
    if [[ "$arg" == /dev/fd/* || "$arg" == /proc/self/fd/* ]]; then
      if grep -Fq 'x-corelink-internal-auth:' "$arg" && ! grep -Fq 'authorization:' "$arg"; then found=1; fi
    fi
  done
  [ "$found" = 1 ] || exit 9
  printf '%s\n' 'fleet_header=x-corelink-internal-auth' >> "${DIRECT_MOCK_LOG:?}"
  n=0; [ -z "${DIRECT_MOCK_FLEET_COUNTER:-}" ] || [ ! -f "$DIRECT_MOCK_FLEET_COUNTER" ] || n="$(<"$DIRECT_MOCK_FLEET_COUNTER")"
  n=$((n + 1)); [ -z "${DIRECT_MOCK_FLEET_COUNTER:-}" ] || printf '%s' "$n" > "$DIRECT_MOCK_FLEET_COUNTER"
  if [ "${DIRECT_MOCK_FAIL:-}" = fleet_post_busy ] && [ "$n" -eq 2 ]; then printf '{"busy":1,"unverifiable":0}\n'; else printf '{"busy":0,"unverifiable":0}\n'; fi; exit 0
fi
if [[ "$url" == *'/v1/usage' ]]; then
  found=0
  for arg in "$@"; do
    if [[ "$arg" == /dev/fd/* || "$arg" == /proc/self/fd/* ]]; then
      if grep -Fq 'authorization: Bearer ' "$arg" && ! grep -Fq 'x-corelink-internal-auth:' "$arg"; then found=1; fi
    fi
  done
  [ "$found" = 1 ] || exit 9
  printf '%s\n' 'canary_pat_preflight_request' >> "${DIRECT_MOCK_LOG:?}"
  case "${DIRECT_MOCK_FAIL:-}" in
    canary_pat_401) printf '{}\n401';;
    canary_pat_transport) exit 7;;
    canary_pat_schema) printf '{"tenant":"canary","plan_cap":1,"plan_ceiling_vcpu_h":null,"active_now":0}\n200';;
    *) printf '{"tenant":"canary","plan_cap":1,"plan_ceiling_vcpu_h":null,"active_now":0,"peak_this_instance":0}\n200';;
  esac
  exit 0
fi
if [[ "$url" == *'/v1/attestation/key' ]]; then
  n=0; [ -z "${DIRECT_MOCK_KEY_COUNTER:-}" ] || [ ! -f "$DIRECT_MOCK_KEY_COUNTER" ] || n="$(<"$DIRECT_MOCK_KEY_COUNTER")"
  n=$((n + 1)); [ -z "${DIRECT_MOCK_KEY_COUNTER:-}" ] || printf '%s' "$n" > "$DIRECT_MOCK_KEY_COUNTER"
  if [ "${DIRECT_MOCK_FAIL:-}" = same_key ] || [ "$n" -eq 1 ]; then key_id=0123456789abcdef; else key_id=fedcba9876543210; fi
  printf '{"keys":[{"key_id":"%s","pubkey_b64":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","expires_ms":null}]}\n' "$key_id"; exit 0
fi
if [[ "$url" == *'/v1/health' || "$url" == */health ]]; then
  has_writeout=0; has_fail=0; has_output=0; for arg in "$@"; do [ "$arg" = --write-out ] && has_writeout=1; [ "$arg" = --fail ] && has_fail=1; [ "$arg" = --output ] && has_output=1; done
  [ "$has_writeout" = 1 ] && [ "$has_fail" = 0 ] || exit 12
  if [ "$has_output" = 1 ]; then
    [ -z "${DIRECT_MOCK_WAKE_FILE:-}" ] || : > "$DIRECT_MOCK_WAKE_FILE"
    [ -z "${DIRECT_MOCK_LOG:-}" ] || printf '%s\n' 'fabricd_health_wake_request' >> "$DIRECT_MOCK_LOG"
    case "${DIRECT_MOCK_FAIL:-}" in health_status) printf '500';;health_schema) printf '200';;*) printf '200';;esac
  else
    case "${DIRECT_MOCK_FAIL:-}" in health_status) printf 'ok\n500';;health_schema) printf 'not-ok\n200';;*) printf 'ok\n200';;esac
  fi
  exit 0
fi
if [[ "$url" == *'/v1/spawn' ]]; then
  for arg in "$@"; do [ "$arg" = --fail ] && exit 13; done
  n=0; [ -z "${DIRECT_MOCK_SPAWN_COUNTER:-}" ] || [ ! -f "$DIRECT_MOCK_SPAWN_COUNTER" ] || n="$(<"$DIRECT_MOCK_SPAWN_COUNTER")"
  n=$((n + 1)); [ -z "${DIRECT_MOCK_SPAWN_COUNTER:-}" ] || printf '%s' "$n" > "$DIRECT_MOCK_SPAWN_COUNTER"
  if [ "$n" -eq 1 ]; then printf 503; else printf 401; fi
  exit 0
fi
if [[ "$url" == *'/v1/exec' ]]; then
  counter="${DIRECT_MOCK_LOG}.exec-counter";n=0;[ -f "$counter" ]&&n="$(<"$counter")";n=$((n + 1));printf '%s' "$n" >"$counter";[ "$n" -eq 1 ]&&printf 400||printf 401;exit 0
fi
if [[ "$url" == *'/v1/status/'* ]]; then
  counter="${DIRECT_MOCK_LOG}.lifecycle-counter";n=0;[ -f "$counter" ]&&n="$(<"$counter")";n=$((n + 1));printf '%s' "$n" >"$counter";[ "$n" -eq 1 ]&&printf 404||printf 401;exit 0
fi
if [[ "$url" == *'/v1/leases/'*'/close' ]]; then printf '{"lease_id":"lease_mock_12345678","released":true,"capture_incomplete":false}\n'; exit 0; fi
if [[ "$url" == *'/v1/leases' ]]; then
  [ "${DIRECT_MOCK_FAIL:-}" != canary ] || exit 1
  printf '{"lease":{"lease_id":"lease_mock_12345678"}}\n'; exit 0
fi
exit 1
