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
if [ "$1" = secret ] && [ "$2" = list ]; then
  if [ "${3:-}" != --name ] || [ -z "${4:-}" ] || [ "${5:-}" != --format ] || [ "${6:-}" != json ] || [ "$#" -ne 6 ]; then
    printf '%s\n' 'unknown or unsupported secret list format; use --format json' >&2
    exit 2
  fi
  if [ "${DIRECT_MOCK_SECRET_LIST:-present}" = missing ]; then
    printf '%s\n' '[]'
  elif [ "${DIRECT_MOCK_SECRET_LIST:-present}" = partial ]; then
    printf '%s\n' '[{"name":"CLOUDFLARE_SPAWN_AUTH_TOKEN","type":"secret_text"}]'
  elif [ "${DIRECT_MOCK_SECRET_LIST:-present}" = malformed ]; then
    printf '%s\n' '{not-json}'
  else
    printf '%s\n' '[{"name":"CLOUDFLARE_SPAWN_AUTH_TOKEN","type":"secret_text"},{"name":"CLOUDFLARE_EXEC_AUTH_TOKEN","type":"secret_text"},{"name":"CLOUDFLARE_LIFECYCLE_AUTH_TOKEN","type":"secret_text"}]'
  fi
  if [ "${DIRECT_MOCK_SECRET_LIST:-present}" = stderr ]; then
    printf '%s\n' 'synthetic warning stays on stderr' >&2
  fi
  exit 0
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
    info_digest=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
    [ "${DIRECT_MOCK_INFO_MODE:-exact}" = wrong ] && info_digest=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
    printf '%s\n' "{\"name\":\"corelink-fabricd-fabricdcontainer\",\"image\":\"registry.example/fabric@sha256:$info_digest\",\"version_id\":\"v-pin\"}"
    exit 0
  fi
  printf '%s\n' 'registry.example/spawn@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' 'registry.example/fabric@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
fi
if [ "$1" = containers ] && [ "$2" = instances ]; then
  n=0; [ -z "${DIRECT_MOCK_INSTANCE_COUNTER:-}" ] || [ ! -f "$DIRECT_MOCK_INSTANCE_COUNTER" ] || n="$(<"$DIRECT_MOCK_INSTANCE_COUNTER")"
  n=$((n + 1)); [ -z "${DIRECT_MOCK_INSTANCE_COUNTER:-}" ] || printf '%s' "$n" > "$DIRECT_MOCK_INSTANCE_COUNTER"
  if [ -n "${DIRECT_MOCK_INSTANCE_DELAY:-}" ]; then
    [ -z "${DIRECT_MOCK_INSTANCE_PID_FILE:-}" ] || printf '%s\n' "$$" > "$DIRECT_MOCK_INSTANCE_PID_FILE"
    sleep "$DIRECT_MOCK_INSTANCE_DELAY"
  fi
  state=running; digest=sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  case "${DIRECT_MOCK_CONVERGENCE_MODE:-ready}" in
    delayed) [ "$n" -lt "${DIRECT_MOCK_CONVERGENCE_AFTER:-2}" ] && state=starting ;;
    never) state=starting ;;
    wrong-digest) digest=sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc ;;
    failed) state=failed ;;
  esac
  if [ "${DIRECT_MOCK_INSTANCE_DIGEST:-present}" = absent ]; then
    printf '%s\n' "{\"instances\":[{\"id\":\"fabricd-instance\",\"state\":\"$state\"}]}"
  elif [ "${DIRECT_MOCK_INSTANCE_DIGEST:-present}" = empty ]; then
    printf '%s\n' "{\"instances\":[{\"id\":\"fabricd-instance\",\"state\":\"$state\",\"digest\":\"\"}]}"
  elif [ "${DIRECT_MOCK_INSTANCE_DIGEST:-present}" = malformed ]; then
    printf '%s\n' "{\"instances\":[{\"id\":\"fabricd-instance\",\"state\":\"$state\",\"digest\":123}]}"
  else
    printf '%s\n' "{\"instances\":[{\"id\":\"fabricd-instance\",\"state\":\"$state\",\"digest\":\"$digest\"}]}"
  fi
  exit 0
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
