#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then
  printf '%s\n' '4.105.0'
  exit 0
fi
printf 'dispatch=fabricd cwd=%s args=%s\n' "$PWD" "$*" >> "${DIRECT_DISPATCH_LOG:?}"
exec "${DIRECT_MOCK_WRANGLER:?}" "$@"
