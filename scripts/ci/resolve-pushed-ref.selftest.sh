#!/usr/bin/env bash
# Offline self-test for resolve-pushed-ref.sh.
#
# Every successful resolution must use a descriptor digest read from the
# registry. Wrangler's tag-only push transcript must never be mistaken for an
# immutable reference.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
resolver="${script_dir}/resolve-pushed-ref.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT

mkdir -p "${work}/bin"
cat >"${work}/bin/docker" <<'MOCK_DOCKER'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" != "manifest inspect -v registry.cloudflare.com/account-fixture/fixture-image:deadbeef" ]]; then
  exit 2
fi
cat "${MOCK_MANIFEST}"
MOCK_DOCKER
chmod +x "${work}/bin/docker"
export DOCKER_BIN="${work}/bin/docker"

export CLOUDFLARE_ACCOUNT_ID='account-fixture'
image='fixture-image'
tag='deadbeef'
base="registry.cloudflare.com/${CLOUDFLARE_ACCOUNT_ID}/${image}"
digest='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'

expect_ref() {
  local label="$1" expected="$2" fixture="$3" actual
  actual="$(${resolver} "${image}" "${tag}" "${fixture}")"
  if [[ "${actual}" != "${expected}" ]]; then
    printf 'resolve-pushed-ref selftest: %s: expected %s, got %s\n' \
      "${label}" "${expected}" "${actual}" >&2
    exit 1
  fi
}

expect_fail() {
  local label="$1" fixture="$2" output rc
  set +e
  output="$(${resolver} "${image}" "${tag}" "${fixture}" 2>&1)"
  rc=$?
  set -e
  if [[ "${rc}" -eq 0 ]]; then
    printf 'resolve-pushed-ref selftest: %s: expected failure, got %s\n' \
      "${label}" "${output}" >&2
    exit 1
  fi
  if [[ "${output}" != *'resolve-pushed-ref:'* ]]; then
    printf 'resolve-pushed-ref selftest: %s: missing fail-closed diagnostic\n' \
      "${label}" >&2
    exit 1
  fi
}

cat >"${work}/remote-manifest.json" <<EOF
{"Descriptor":{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"${digest}"}}
EOF
export MOCK_MANIFEST="${work}/remote-manifest.json"

printf 'Pushed image: %s:%s\n' "${base}" "${tag}" >"${work}/tag-only.out"
expect_ref 'tag-only Wrangler transcript with verified remote digest' \
  "${base}@${digest}" "${work}/tag-only.out"

printf 'Pushed image: %s:wrong-tag\n' "${base}" >"${work}/wrong-tag.out"
expect_fail 'wrong tag transcript' "${work}/wrong-tag.out"

printf '{"Descriptor":{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:bad"}}\n' \
  >"${work}/malformed-remote.json"
export MOCK_MANIFEST="${work}/malformed-remote.json"
expect_fail 'malformed remote digest' "${work}/tag-only.out"

printf '{"Descriptor":{"mediaType":"application/vnd.unknown","digest":"%s"}}\n' \
  "${digest}" >"${work}/unsupported-media.json"
export MOCK_MANIFEST="${work}/unsupported-media.json"
expect_fail 'unsupported remote descriptor' "${work}/tag-only.out"

expect_fail 'missing transcript' "${work}/missing.out"

printf 'resolve-pushed-ref selftest: PASS (5 offline cases)\n'
