#!/usr/bin/env bash
set -euo pipefail
printf 'wrangler %s\n' "$*" >> "${DIRECT_MOCK_LOG:?}"
if [ "${DIRECT_MOCK_FAIL:-}" = deploy ] && [ "$1" = deploy ]; then exit 1; fi
if [ "$1" = deployments ] && [ "$2" = list ]; then
  printf '[{"created_on":"2026-09-08T00:00:00Z","versions":[{"version_id":"v-pin"}]}]\n'
fi
