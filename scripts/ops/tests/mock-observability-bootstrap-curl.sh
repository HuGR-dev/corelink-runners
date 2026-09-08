#!/usr/bin/env bash
set -Eeuo pipefail
scenario="${MOCK_SCENARIO:-success}"
url="${!#}"
case "$url" in
  *fleet/busy) [ "$scenario" = busy ] && printf '{"busy":1,"unverifiable":0}\n' || printf '{"busy":0,"unverifiable":0}\n' ;;
  *auth/introspect) printf '{"valid":true,"tenant_id":"%s","max_concurrency":1}\n' "${MOCK_TENANT:?}" ;;
  */internal/v1/status)
    if [ "$scenario" = verify-fail ] || [ "$scenario" = fail-refreeze ]; then exit 22; fi
    printf '{"version":"0.1.0","uptime_ms":42,"ledger_cross_instance_safe":false,"num_shards":1,"counters":{}}\n';;
  *) printf '%s\n' 'unexpected mock curl URL' >&2; exit 1;;
esac
