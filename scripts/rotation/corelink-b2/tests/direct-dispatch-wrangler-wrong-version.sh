#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then
  printf '%s\n' '4.102.0'
  exit 0
fi
exec "${DIRECT_MOCK_WRANGLER:?}" "$@"
