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
  printf '{"busy":0,"unverifiable":0}\n'; exit 0
fi
if [[ "$url" == *'/v1/attestation/key' ]]; then printf '{"keys":[{"key_id":"0123456789abcdef","pubkey_b64":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","expires_ms":null}]}\n'; exit 0; fi
if [[ "$url" == *'/v1/health' || "$url" == */health ]]; then printf '{}\n'; exit 0; fi
if [[ "$url" == *'/v1/spawn' ]]; then
  n=0; [ -z "${DIRECT_MOCK_SPAWN_COUNTER:-}" ] || [ ! -f "$DIRECT_MOCK_SPAWN_COUNTER" ] || n="$(<"$DIRECT_MOCK_SPAWN_COUNTER")"
  n=$((n + 1)); [ -z "${DIRECT_MOCK_SPAWN_COUNTER:-}" ] || printf '%s' "$n" > "$DIRECT_MOCK_SPAWN_COUNTER"
  if [ "$n" -eq 1 ]; then printf 503; else printf 401; fi
  exit 0
fi
if [[ "$url" == *'/v1/leases/'*'/close' ]]; then printf '{"lease_id":"lease_mock_12345678","released":true,"capture_incomplete":false}\n'; exit 0; fi
if [[ "$url" == *'/v1/leases' ]]; then
  [ "${DIRECT_MOCK_FAIL:-}" != canary ] || exit 1
  printf '{"lease":{"lease_id":"lease_mock_12345678"}}\n'; exit 0
fi
exit 1
