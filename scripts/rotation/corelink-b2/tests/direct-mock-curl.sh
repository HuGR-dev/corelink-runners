#!/usr/bin/env bash
set -euo pipefail
args="$*"
url="${!#}"
if [[ "$url" == *'/internal/v1/fleet/busy' ]]; then printf '{"busy":0,"unverifiable":0}\n'; exit 0; fi
if [[ "$url" == *'/v1/attestation/key' ]]; then printf '{"keys":[{"key_id":"0123456789abcdef","pubkey_b64":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","expires_ms":null}]}\n'; exit 0; fi
if [[ "$url" == *'/v1/health' || "$url" == */health ]]; then printf '{}\n'; exit 0; fi
if [[ "$url" == *'/v1/spawn' ]]; then
  if [[ "$args" == *'OLD-TOKEN'* ]]; then printf 401; else printf 503; fi
  exit 0
fi
if [[ "$url" == *'/v1/leases/'* ]] && [[ "$args" == *'DELETE'* ]]; then printf '{}\n'; exit 0; fi
if [[ "$url" == *'/v1/leases' ]]; then
  [ "${DIRECT_MOCK_FAIL:-}" != canary ] || exit 1
  printf '{"lease_id":"lease_mock_12345678"}\n'; exit 0
fi
exit 1
