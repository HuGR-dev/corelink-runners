#!/usr/bin/env bash
set -Eeuo pipefail
state="${MOCK_STATE:?}"
scenario="${MOCK_SCENARIO:-success}"
digest="${MOCK_DIGEST:?}"
version="${MOCK_VERSION:?}"
app_name='corelink-fabricd-fabricdcontainer'
cmd="${1:-} ${2:-} ${3:-}"
case "$cmd" in
  'deployments list --name')
    n=0; [ -f "$state.deployments" ] && n="$(cat "$state.deployments")"; n=$((n+1)); printf '%s\n' "$n" > "$state.deployments"
    if [ "$scenario" = drift ] && [ "$n" -ge 3 ]; then version='version-drift'; fi
    printf '[{"created_on":"2026-09-08T00:00:00Z","versions":[{"version_id":"%s"}]}]\n' "$version";;
  'containers info '* )
    printf '%s\n' 'unknown option: --json (Wrangler 4.105.0 containers info is not JSON-capable)' >&2
    exit 64;;
  'containers list --json')
    actual="$digest"
    [ "$scenario" = wrong-digest ] && actual='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    container_id="${MOCK_CURRENT_APP_ID:-app-old}"
    [ -f "$state.deleted" ] && container_id="${MOCK_NEW_APP_ID:-app-new}"
    case "$scenario" in
      missing-container)
        printf '{"containers":[{"id":"other-app","name":"other","image":"registry.example/other@%s"}]}\n' "$actual";;
      duplicate-container)
        printf '{"containers":[{"id":"app-old","name":"%s","image":"registry.example/corelink@%s"},{"id":"app-old","name":"%s","image":"registry.example/corelink@%s"}]}\n' "$app_name" "$actual" "$app_name" "$actual";;
      *)
        printf '{"containers":[{"id":"%s","name":"%s","image":"registry.example/corelink@%s"}]}\n' "$container_id" "$app_name" "$actual";;
    esac;;
  'versions view '* )
    printf '{"bindings":[{"name":"FABRIC_ADMISSION_PAUSED","type":"plain_text","text":"1"}]}\n';;
  'secret list --format')
    printf '[{"name":"FABRIC_INTROSPECT_KEY","version":"legacy-v1"},{"name":"FABRIC_INTROSPECT_AUTH_KEY","version":"auth-v1"}]\n';;
  'secret put FABRIC_INTROSPECT_AUTH_KEY')
    [ "$scenario" = partial-first ] && { printf '%s\n' 'first secret put failed' >&2; exit 1; }
    secret="$(cat)"; printf '%s' "$secret" | grep -Eq '^[A-Za-z0-9+/= -]+$' || exit 1; n=0; [ -f "$state.secret-put-introspect-count" ] && n="$(cat "$state.secret-put-introspect-count")"; n=$((n+1)); printf '%s\n' "$n" > "$state.secret-put-introspect-count"; : > "$state.secret-put-introspect"; printf '%s\n' 'introspection secret accepted' >&2;;
  'secret put FABRIC_OBSERVABILITY_KEY')
    [ "$scenario" = partial-second ] && { printf '%s\n' 'second secret put failed' >&2; exit 1; }
    secret="$(cat)"; printf '%s' "$secret" | grep -Eq '^[A-Za-z0-9+/=]+$' || exit 1; : > "$state.secret-put"; : > "$state.secret-put-observability"; printf '%s\n' 'secret accepted' >&2;;
  'containers delete '* )
    : > "$state.deleted"; printf '%s\n' 'container deleted' >&2;;
  deploy\ *)
    n=0; [ -f "$state.deploy" ] && n="$(cat "$state.deploy")"; n=$((n+1)); printf '%s\n' "$n" > "$state.deploy"
    if [ "$scenario" = fail-refreeze ] && [ "$n" -ge 2 ]; then printf '%s\n' 'refreeze failed' >&2; exit 1; fi
    printf '%s\n' 'deployment accepted' >&2;;
  *) printf '%s\n' 'unexpected mock wrangler command' >&2; exit 1;;
esac
