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
    if [ "$scenario" = drift ] && [ "$n" -ge 2 ]; then version='version-drift'; fi
    printf '[{"created_on":"2026-09-08T00:00:00Z","versions":[{"version_id":"%s"}]}]\n' "$version";;
  'containers info '* )
    actual="$digest"
    [ "$scenario" = wrong-digest ] && actual='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    printf '{"name":"%s","image":"registry.example/corelink@%s"}\n' "$app_name" "$actual";;
  'versions view '* )
    printf '{"bindings":[{"name":"FABRIC_ADMISSION_PAUSED","type":"plain_text","text":"1"}]}\n';;
  'secret put FABRIC_OBSERVABILITY_KEY')
    secret="$(cat)"; printf '%s' "$secret" | grep -Eq '^[A-Za-z0-9+/=]+$' || exit 1; : > "$state.secret-put"; printf '%s\n' 'secret accepted' >&2;;
  'containers delete '* )
    : > "$state.deleted"; printf '%s\n' 'container deleted' >&2;;
  deploy\ *)
    n=0; [ -f "$state.deploy" ] && n="$(cat "$state.deploy")"; n=$((n+1)); printf '%s\n' "$n" > "$state.deploy"
    if [ "$scenario" = fail-refreeze ] && [ "$n" -ge 2 ]; then printf '%s\n' 'refreeze failed' >&2; exit 1; fi
    printf '%s\n' 'deployment accepted' >&2;;
  *) printf '%s\n' 'unexpected mock wrangler command' >&2; exit 1;;
esac
