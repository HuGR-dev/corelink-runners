#!/usr/bin/env bash
set -euo pipefail
printf 'wrangler %s\n' "$*" >> "${DIRECT_MOCK_LOG:?}"
if [ "${DIRECT_MOCK_FAIL:-}" = release_fabricd ] && [ "$1" = deploy ] && [[ "$*" == *'FABRIC_ADMISSION_PAUSED:0'* ]] && [[ "$*" == *'fabricd'* ]]; then exit 1; fi
if [ "${DIRECT_MOCK_FAIL:-}" = deploy ] && [ "$1" = deploy ]; then exit 1; fi
if [ "$1" = deployments ] && [ "$2" = list ]; then
  printf '[{"created_on":"2026-09-08T00:00:00Z","versions":[{"version_id":"v-pin"}]}]\n'
fi
if [ "$1" = versions ] && [ "$2" = view ]; then
  printf '%s\n' '{"bindings":[{"name":"FABRIC_ADMISSION_PAUSED","type":"plain_text","text":"1"}]}'
fi
if [ "$1" = secret ] && [ "$2" = put ]; then
  secret_file="${DIRECT_MOCK_LOG}.secret-input"
  cat > "$secret_file"
  if [ ! -s "$secret_file" ] || [ "$(wc -l < "$secret_file" | tr -d ' ')" != 0 ] || ! LC_ALL=C grep -Eq '^[A-Za-z0-9+/=]+$' "$secret_file"; then
    rm -f "$secret_file"
    exit 1
  fi
  rm -f "$secret_file"
  printf '%s\n' 'secret_put_valid_single_line' >> "${DIRECT_MOCK_LOG}"
  if [ "${DIRECT_MOCK_FAIL:-}" = mutate_after_secret ] && [ "$(grep -c 'secret_put_valid_single_line' "${DIRECT_MOCK_LOG}")" = 1 ]; then
    printf '%s\n' 'mutated-after-first-put' > "${DIRECT_MOCK_MUTATE_FILE:?}"
  fi
fi
if [ "$1" = containers ] && [ "$2" = list ]; then
  if [ -e "${DIRECT_MOCK_LOG}.fabricd-deleted" ]; then
    printf '%s\n' 'fabricd_absence_confirmed' >> "${DIRECT_MOCK_LOG}"
    printf '%s\n' '[]'
  else
    printf '%s\n' '{"id":"22222222-2222-2222-2222-222222222222","name":"corelink-fabricd-fabricdcontainer"}'
  fi
fi
if [ "$1" = containers ] && [ "$2" = info ]; then
  if [ -e "${DIRECT_MOCK_LOG}.fabricd-deleted" ] && [ "$3" = 22222222-2222-2222-2222-222222222222 ]; then exit 1; fi
  if [ "$3" = 22222222-2222-2222-2222-222222222222 ]; then
    printf '%s\n' '{"name":"corelink-fabricd-fabricdcontainer","image":"registry.example/fabric@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","version_id":"v-pin"}'
    exit 0
  fi
  printf '%s\n' 'registry.example/spawn@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' 'registry.example/fabric@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
fi
if [ "$1" = containers ] && [ "$2" = delete ]; then
  if [ "${DIRECT_MOCK_DELETE_MODE:-}" = timeout-absent ]; then
    : > "${DIRECT_MOCK_LOG}.fabricd-deleted"
    sleep "${DIRECT_MOCK_DELETE_DELAY:-2}"
  elif [ "${DIRECT_MOCK_DELETE_MODE:-}" = timeout-unconfirmed ]; then
    sleep "${DIRECT_MOCK_DELETE_DELAY:-2}"
  elif [ "${DIRECT_MOCK_DELETE_MODE:-}" = unconfirmed ]; then
    :
  else
    : > "${DIRECT_MOCK_LOG}.fabricd-deleted"
  fi
  printf '%s\n' 'fabricd_delete' >> "${DIRECT_MOCK_LOG}"
fi
if [ "$1" = deploy ] && [[ "$*" == *'cloudflare-fabricd'* ]]; then
  if [ -e "${DIRECT_MOCK_LOG}.fabricd-deleted" ]; then
    rm -f "${DIRECT_MOCK_LOG}.fabricd-deleted"
    printf '%s\n' 'fabricd_recreate' >> "${DIRECT_MOCK_LOG}"
  fi
fi
