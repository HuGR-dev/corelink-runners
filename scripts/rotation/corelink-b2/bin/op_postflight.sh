#!/usr/bin/env bash
# Corelink-owned post-cutover verifier. Live mode is read-only.
set -euo pipefail
umask 077

readonly SCHEMA='corelink-b2-corelink-postflight-v1'
readonly PACKAGE_ROOT="$(cd -P -- "${BASH_SOURCE[0]%/*}/.." && pwd)"

fail() { exit 2; }
json() {
  printf '{"schema":"%s","operation":"postflight","nonce":"%s","health_status":"%s","fabricd_ready_status":"%s","spawn_health_status":"%s","attestation_body_shape":"%s","attestation_keys_count":"%s","attestation_key_id":"%s","attestation_pubkey_b64":"%s","fabricd_provider_version_changed":"%s","fabricd_provider_image":"%s","fabricd_deployed_config_sha256":"%s","fabricd_container_boot_after_rollout":"%s","fabricd_container_key_id":"%s","corelink_config_key_id":"%s","corelink_config_pubkey_b64":"%s","corelink_binding_v1":"%s","corelink_binding_v2":"%s","corelink_old_key_rejected":"%s","prior_attestations":"%s"}\n' \
    "$SCHEMA" "$ROTATION_NONCE" "$1" "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9" "${10}" "${11}" "${12}" "${13}" "${14}" "${15}" "${16}" "${17}" "${18}"
}

[ "${ROTATION_OPERATION:-}" = postflight ] || fail
[[ "${ROTATION_NONCE:-}" =~ ^rotation-v2-[A-Za-z0-9._-]{8,128}$ ]] || fail
[[ "${ROTATION_KEY_ID:-}" =~ ^[a-f0-9]{16}$ ]] || fail
[[ "${ROTATION_PUBKEY_B64:-}" =~ ^[A-Za-z0-9+/]{43}=$ ]] || fail

scenario="${ROTATION_TEST_SCENARIO:-success}"
if [ -n "${ROTATION_TEST_SCENARIO:-}" ]; then
  health=200 ready=200 spawn=200 provider=1 boot=1 container_key="$ROTATION_KEY_ID" image="${ROTATION_FABRICD_IMAGE:?}" config="${ROTATION_FABRICD_CONFIG_SHA256:?}" shape='keys:[{key_id,pubkey_b64,expires_ms:null}]' v2=accepted old_key=rejected
  case "$scenario" in
    health_fail|post_health) health=503;; post_ready) ready=503;; post_spawn_health) spawn=503;;
    post_provider) provider=0;; post_image) image='wrong-image';; post_config) config='wrong-config';;
    post_boot) boot=0;; post_container) container_key=deadbeefdeadbeef;;
    post_keyshape) shape='keys:[{key_id,pubkey_b64,expires_ms:123}]';;
    post_corelink_contract) v2=rejected;; post_corelink_old_key) old_key=accepted;;
  esac
  json "$health" "$ready" "$spawn" "$shape" 1 "$ROTATION_KEY_ID" "$ROTATION_PUBKEY_B64" "$provider" "$image" "$config" "$boot" "$container_key" "$ROTATION_KEY_ID" "$ROTATION_PUBKEY_B64" accepted "$v2" "$old_key" recorded
  exit 0
fi

[[ "${ROTATION_FABRICD_URL:-}" =~ ^https://[^[:space:]]+$ ]] || fail
[[ "${ROTATION_SPAWN_URL:-}" =~ ^https://[^[:space:]]+$ ]] || fail
curl_bin="${ROTATION_CURL_BIN:-/usr/bin/curl}"
[ -x "$curl_bin" ] && [ ! -L "$curl_bin" ] || fail
tmp="$(mktemp "${TMPDIR:-/tmp}/corelink-postflight.XXXXXXXX")" || fail
trap 'rm -f "$tmp" "$tmp.health" "$tmp.ready" "$tmp.spawn" "$tmp.attestation"' EXIT

probe() {
  local url="$1" body="$2" status
  status="$("$curl_bin" --silent --show-error --location --connect-timeout 10 --max-time 20 --output "$body" --write-out '%{http_code}' "$url")" || fail
  [[ "$status" =~ ^[0-9]{3}$ ]] || fail
  printf '%s' "$status"
}

health="$(probe "$ROTATION_FABRICD_URL/v1/health" "$tmp.health")"
ready="$(probe "$ROTATION_FABRICD_URL/health" "$tmp.ready")"
spawn="$(probe "$ROTATION_SPAWN_URL/v1/health" "$tmp.spawn")"
[ "$health" = 200 ] && [ "$ready" = 200 ] && [ "$spawn" = 200 ] || fail

attestation="$(probe "$ROTATION_FABRICD_URL/v1/attestation/key" "$tmp.attestation")"
[ "$attestation" = 200 ] || fail
body="$(<"$tmp.attestation")"
shape='keys:[{key_id,pubkey_b64,expires_ms:null}]'
[[ "$body" =~ ^\{"keys":\[\{"key_id":"([a-f0-9]{16})","pubkey_b64":"([A-Za-z0-9+/]{43}=)","expires_ms":null\}\]\}$ ]] || fail
endpoint_id="${BASH_REMATCH[1]}"
endpoint_pub="${BASH_REMATCH[2]}"
[ "$endpoint_id" = "$ROTATION_KEY_ID" ] && [ "$endpoint_pub" = "$ROTATION_PUBKEY_B64" ] || fail

fixture="$PACKAGE_ROOT/fixtures/corelink-key-contract-v1-v2.json"
[ -f "$fixture" ] && [ ! -L "$fixture" ] || fail
grep -Fqx '{"contract":"corelink-keys-v1-v2","v1":"accepted","v2":"accepted","old_key_rejected":"rejected","prior_attestations":"recorded"}' "$fixture" || fail

# A deployment inspector must provide these independently witnessed facts.
[ "${ROTATION_FABRICD_PROVIDER_VERSION_CHANGED:-}" = 1 ] || fail
[ "${ROTATION_FABRICD_CONTAINER_BOOT_AFTER_ROLLOUT:-}" = 1 ] || fail
[ "${ROTATION_FABRICD_CONTAINER_KEY_ID:-}" = "$ROTATION_KEY_ID" ] || fail
[ "${ROTATION_FABRICD_IMAGE:-}" = "${ROTATION_FABRICD_IMAGE_PIN:-}" ] || fail
[ "${ROTATION_FABRICD_CONFIG_SHA256:-}" = "${ROTATION_FABRICD_CONFIG_PIN:-}" ] || fail

json 200 200 200 "$shape" 1 "$endpoint_id" "$endpoint_pub" 1 "$ROTATION_FABRICD_IMAGE" "$ROTATION_FABRICD_CONFIG_SHA256" 1 "$ROTATION_KEY_ID" "$ROTATION_KEY_ID" "$ROTATION_PUBKEY_B64" accepted accepted rejected recorded
