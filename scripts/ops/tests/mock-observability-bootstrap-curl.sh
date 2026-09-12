#!/usr/bin/env bash
set -Eeuo pipefail
scenario="${MOCK_SCENARIO:-success}"
output_file=''
write_out=''
header_file=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output|-o) output_file="${2:?}"; shift 2;;
    --write-out|-w) write_out="${2:?}"; shift 2;;
    --header|-H)
      case "${2:?}" in
        @*) header_file="${2#@}";;
        *) [ -n "$header_file" ] || header_file="${2:?}";;
      esac
      shift 2;;
    --data-binary|-d|--connect-timeout|--max-time) shift 2;;
    *) url="$1"; shift;;
  esac
done

body=''; status=200
case "$url" in
  *fleet/busy)
    fleet_calls=0
    if [ -f "$MOCK_STATE.fleet-calls" ]; then fleet_calls="$(cat "$MOCK_STATE.fleet-calls")"; fi
    fleet_calls=$((fleet_calls + 1))
    printf '%s\n' "$fleet_calls" > "$MOCK_STATE.fleet-calls"
    if [ "$scenario" = busy ] || { [ "$scenario" = busy-after-stability ] && [ "$fleet_calls" -ge 2 ]; }; then
      body='{"busy":1,"unverifiable":0}'
    else
      body='{"busy":0,"unverifiable":0}'
    fi
    ;;
  *auth/introspect)
    old_key=''
    introspect_calls=0
    if [ -n "$header_file" ] && [ -f "$header_file" ]; then
      old_key="$(sed -n 's/^X-Corelink-Internal-Auth: //p' "$header_file")"
    fi
    if [[ "$scenario" == recover-* || "$scenario" == repair-* ]]; then
      [ -f "$MOCK_STATE.introspect-calls" ] && introspect_calls="$(cat "$MOCK_STATE.introspect-calls")"
      introspect_calls=$((introspect_calls + 1))
      printf '%s\n' "$introspect_calls" > "$MOCK_STATE.introspect-calls"
    fi
    if [ -n "$header_file" ] && grep -q '^CF-Access-Client-Id:' "$header_file"; then : > "$MOCK_STATE.access-client-id-header"; fi
    if [ -n "$header_file" ] && grep -q '^CF-Access-Client-Secret:' "$header_file"; then : > "$MOCK_STATE.access-client-secret-header"; fi
    case "$scenario:$old_key:$introspect_calls" in
      repair-final-proof-no-access:*) status=403; body='{"error":"forbidden"}' ;;
      repair-final-proof:*) body="{\"valid\":true,\"tenant_id\":\"${MOCK_TENANT:?}\",\"max_concurrency\":1}" ;;
      repair-zero-named:*) body="{\"valid\":true,\"tenant_id\":\"${MOCK_TENANT:?}\",\"max_concurrency\":1}" ;;
      recover-403:introspect-key:1|recover-403:*:1|auth-403:introspect-key:*) status=403; body='{"error":"forbidden"}' ;;
      recover-401:introspect-key:1|recover-401:*:1|auth-401:introspect-key:*) status=401; body='{"error":"unauthorized"}' ;;
      recover-5xx:*) status=503; body='{"error":"temporarily unavailable"}' ;;
      recover-transport:*) exit 7 ;;
      repair-*:*:1) status=403; body='{"error":"forbidden"}' ;;
      *) body="{\"valid\":true,\"tenant_id\":\"${MOCK_TENANT:?}\",\"max_concurrency\":1}" ;;
    esac
    ;;
  */internal/v1/status)
    if [ "$scenario" = verify-fail ] || [ "$scenario" = fail-refreeze ] || { [ "$scenario" = repair-post-fail ] && [ -f "$MOCK_STATE.deleted" ]; } || { [ "$scenario" = repair-zero-named ] && [ ! -f "$MOCK_STATE.deploy" ]; }; then exit 22; fi
    body='{"version":"0.1.0","uptime_ms":42,"ledger_cross_instance_safe":false,"num_shards":1,"counters":{}}'
    ;;
  */health)
    [ "$scenario" = resume-health-fail ] && exit 22
    body='ok'
    ;;
  *) printf '%s\n' 'unexpected mock curl URL' >&2; exit 1;;
esac

if [ -n "$output_file" ]; then
  printf '%s\n' "$body" > "$output_file"
else
  printf '%s\n' "$body"
fi
if [ "$write_out" = '%{http_code}' ]; then
  printf '%s' "$status"
fi
