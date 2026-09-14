#!/bin/bash
# Direct, bounded post-cutover probe for the spawn Worker's bearer gate.
# Tokens arrive only on stdin. They are never placed in curl argv, a file, or
# output; curl reads its Authorization header from an in-memory config pipe.
set -euo pipefail

[ "$#" -eq 2 ] && [ "$1" = post-cutover ] || { printf 'usage: direct-auth-probe.sh post-cutover HTTPS_BASE_URL\n' >&2; exit 2; }
base="${2%/}"
[[ "$base" =~ ^https://[^[:space:]]+$ ]] || { printf 'invalid HTTPS base URL\n' >&2; exit 2; }

IFS= read -r new_line
IFS= read -r old_line
[[ "$new_line" = new_token=* && "$old_line" = old_token=* ]] || { printf 'token protocol error\n' >&2; exit 2; }
new_token="${new_line#new_token=}"
old_token="${old_line#old_token=}"
[ -n "$new_token" ] && [ -n "$old_token" ] || { printf 'empty token\n' >&2; exit 2; }
[[ "$new_token" =~ ^[A-Za-z0-9._~+/=-]{16,}$ && "$old_token" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]] \
  || { printf 'token contains an unsafe curl-config character\n' >&2; exit 2; }

probe() {
  local token="$1" status
  status="$(curl --config <(
    printf '%s\n' \
      'silent' \
      'show-error' \
      'request = POST' \
      "url = \"${base}/v1/spawn\"" \
      'header = "content-type: application/json"' \
      "header = \"authorization: Bearer ${token}\"" \
      'data = "{}"' \
      'connect-timeout = 5' \
      'max-time = 10' \
      'output = "/dev/null"' \
      'write-out = "%{http_code}"'
  ) )" || return 1
  [[ "$status" =~ ^[0-9]{3}$ ]] || return 1
  printf '%s' "$status"
}

new_status="$(probe "$new_token")" || { printf 'new-token request failed\n' >&2; exit 1; }
old_status="$(probe "$old_token")" || { printf 'old-token request failed\n' >&2; exit 1; }
printf '%s\n' \
  "new_status=$new_status" \
  "old_status=$old_status" \
  'probe_path=/v1/spawn' \
  'probe_method=POST' \
  'probe_body=empty_object' \
  'max_requests=2' \
  'new_spawned=0' \
  'old_spawned=0'
