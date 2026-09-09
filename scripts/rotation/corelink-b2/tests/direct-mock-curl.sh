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
close_response(){
  local capture_incomplete="$1"
  cat <<EOF
{"lease_id":"lease_mock_12345678","released":true,"capture_incomplete":$capture_incomplete,"metrics":{"tokens":{"input":12000,"output":3400,"cache_read":50000,"cache_write":8000,"total":73400},"wall_ms":45000,"active_ms":31000,"tool_calls":17,"tool_breakdown":[{"tool":"Edit","count":9},{"tool":"Bash","count":8}],"model_turns":6,"cost_usd_micros":4200000},"check_result":{"memo_key":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","tree_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","def_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","toolchain_digest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","exit":1,"artifacts":[{"path":"target/release/app","digest":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"},{"path":"dist/report.json","digest":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"}],"stdout_ref":"cas:sha256:1111111111111111111111111111111111111111111111111111111111111111","stderr_ref":"cas:sha256:2222222222222222222222222222222222222222222222222222222222222222","duration_ms":1234,"runner_ref":"runner:corelink-builder-01","produced_at":1700000000000},"attestation":{"tree":"cas:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","def":"cas:sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","runner":"runner:corelink-builder-01","model":"anthropic:claude-opus-4-8","principal":["tenant:acme","agent:claude-01"],"sig":"c2lnbmF0dXJlLWJ5dGVzLWJhc2U2NC1wbGFjZWhvbGRlcg=="},"result_binding_sig":"cmVzdWx0LWJpbmRpbmctc2lnLXYxLWJhc2U2NA==","result_binding_sig_v2":"cmVzdWx0LWJpbmRpbmctc2lnLXYyLWJhc2U2NA==","fabric_key_id":"2d16e9ef2102df2a"}
EOF
}
if [[ "$url" == *'/v1/leases/'*'/close' ]]; then
  case "${DIRECT_MOCK_FAIL:-}" in
    capture_incomplete) close_response true; printf '%s\n' 'close_response=capture_incomplete' >> "${DIRECT_MOCK_LOG:?}"; exit 0;;
    malformed_close) printf '{"lease_id":"lease_mock_12345678","released":true}\n'; printf '%s\n' 'close_response=malformed' >> "${DIRECT_MOCK_LOG:?}"; exit 0;;
    close_ambiguous_held|close_ambiguous_released|close_ambiguous_retry) printf '%s\n' 'close_response=transport_ambiguous' >> "${DIRECT_MOCK_LOG:?}"; exit 7;;
    *) close_response false; printf '%s\n' 'close_response=canonical' >> "${DIRECT_MOCK_LOG:?}"; exit 0;;
  esac
fi
if [[ "$url" == *'/v1/leases/lease_mock_12345678' ]]; then
  counter="${DIRECT_MOCK_LEASE_COUNTER:-${DIRECT_MOCK_LOG}.lease-counter}"
  n=0; [ ! -f "$counter" ] || n="$(<"$counter")"; n=$((n + 1)); printf '%s' "$n" > "$counter"
  state=released
  case "${DIRECT_MOCK_FAIL:-}" in
    close_ambiguous_held) state=held;;
    close_ambiguous_retry) [ "$n" -lt 3 ] && state=held;;
  esac
  printf '%s\n' "lease_get_state=$state poll:$n" >> "${DIRECT_MOCK_LOG:?}"
  printf '{"lease_id":"lease_mock_12345678","state":"%s"}\n' "$state"
  exit 0
fi
if [[ "$url" == *'/v1/leases' ]]; then
  [ "${DIRECT_MOCK_FAIL:-}" != canary ] || exit 1
  printf '{"lease":{"lease_id":"lease_mock_12345678"}}\n'; exit 0
fi
exit 1
