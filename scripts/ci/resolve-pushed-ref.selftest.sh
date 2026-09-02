#!/usr/bin/env bash
# Offline self-test for resolve-pushed-ref.sh.
#
# Every successful resolution must be an immutable digest. In particular, a
# push transcript without a digest must fail instead of returning its mutable
# tag as a deployment reference.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
resolver="${script_dir}/resolve-pushed-ref.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT

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
  if [[ "${output}" != *'did not contain an immutable sha256 digest'* ]]; then
    printf 'resolve-pushed-ref selftest: %s: missing fail-closed diagnostic\n' \
      "${label}" >&2
    exit 1
  fi
}

printf 'manifest-fixture@%s: done\n' "${digest}" >"${work}/manifest.out"
expect_ref 'manifest digest' "${base}@${digest}" "${work}/manifest.out"

printf 'Pushed image: %s@%s\n' "${base}" "${digest}" >"${work}/pushed.out"
expect_ref 'Pushed image digest' "${base}@${digest}" "${work}/pushed.out"

printf 'Pushed image: %s:%s\n' "${base}" "${tag}" >"${work}/tag-only.out"
expect_fail 'tag-only transcript' "${work}/tag-only.out"

printf 'manifest-fixture@sha256:bad: done\n' >"${work}/malformed.out"
expect_fail 'malformed digest' "${work}/malformed.out"

expect_fail 'missing transcript' "${work}/missing.out"

printf 'resolve-pushed-ref selftest: PASS (5 offline cases)\n'
