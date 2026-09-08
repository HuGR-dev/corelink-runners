#!/usr/bin/env bash
# Corelink-owned pre-cutover key-contract verifier.
set -euo pipefail
umask 077

readonly SCHEMA='corelink-b2-corelink-stage-v1'
PACKAGE_ROOT="$(cd -P -- "${BASH_SOURCE[0]%/*}/.." && pwd)"
readonly PACKAGE_ROOT

fail() { exit 2; }
json() {
  printf '{"schema":"%s","operation":"corelink_stage","nonce":"%s","corelink_expected_key_id":"%s","corelink_expected_pubkey_b64":"%s","corelink_material_derived":"%s","corelink_contract_version":"%s","corelink_prior_attestations":"%s"}\n' \
    "$SCHEMA" "$ROTATION_NONCE" "$OUTPUT_KEY_ID" "$ROTATION_PUBKEY_B64" "$1" "$2" "$3"
}

[ "${ROTATION_OPERATION:-}" = corelink_stage ] || fail
[[ "${ROTATION_NONCE:-}" =~ ^rotation-v2-[A-Za-z0-9._-]{8,128}$ ]] || fail
[[ "${ROTATION_KEY_ID:-}" =~ ^[a-f0-9]{16}$ ]] || fail
[[ "${ROTATION_PUBKEY_B64:-}" =~ ^[A-Za-z0-9+/]{43}=$ ]] || fail

# Tests select only named package-owned failure fixtures. Production executes
# the native checks below.
scenario="${ROTATION_TEST_SCENARIO:-success}"
OUTPUT_KEY_ID="$ROTATION_KEY_ID"
case "$scenario" in
  corelink_key) OUTPUT_KEY_ID='deadbeefdeadbeef'; json 1 corelink-keys-v1-v2 recorded; exit 0 ;;
  corelink_derivation) json 0 corelink-keys-v1-v2 recorded; exit 0 ;;
  corelink_contract) json 1 wrong-contract recorded; exit 0 ;;
  corelink_attestations) json 1 corelink-keys-v1-v2 missing; exit 0 ;;
esac

# Corelink's key_id is lower_hex(SHA-256(raw_ed25519_public_key))[..16].
[ "$(printf '%s' "$ROTATION_PUBKEY_B64" | /usr/bin/base64 --decode 2>/dev/null | /usr/bin/wc -c | /usr/bin/tr -d ' ')" = 32 ] || fail
derived_id="$(printf '%s' "$ROTATION_PUBKEY_B64" | /usr/bin/base64 --decode 2>/dev/null | /usr/local/bin/openssl dgst -sha256 -r 2>/dev/null | /usr/bin/awk '{print substr($1,1,16)}')" || fail
[ "$derived_id" = "$ROTATION_KEY_ID" ] || fail

fixture="$PACKAGE_ROOT/fixtures/corelink-key-contract-v1-v2.json"
[ -f "$fixture" ] && [ ! -L "$fixture" ] || fail
grep -Fqx '{"contract":"corelink-keys-v1-v2","v1":"accepted","v2":"accepted","old_key_rejected":"rejected","prior_attestations":"recorded"}' "$fixture" || fail

json 1 corelink-keys-v1-v2 recorded
