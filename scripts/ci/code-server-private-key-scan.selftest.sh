#!/usr/bin/env bash
set -euo pipefail

# The image Dockerfile treats grep's status as a three-state contract:
# 0 = finding (fail), 1 = clean (pass), >1 = scanner error (fail).
# This self-test uses this script as a grep stub so it never handles key data.
if [[ "${STUB_MODE:-0}" == 1 ]]; then
  case "${STUB_RC:?}" in
    0) printf '%s\n' "${STUB_PATH:?}"; exit 0 ;;
    1) exit 1 ;;
    *) exit "${STUB_RC}" ;;
  esac
fi

scan_tree() {
  local grep_bin=$1 root=$2 private_key_files scan_rc
  set +e
  private_key_files="$(STUB_MODE=1 STUB_RC="${STUB_RC}" STUB_PATH="${root}/fixture.key" \
    "${grep_bin}" -RIlE -- '-----BEGIN [A-Z0-9][A-Z0-9 ]*PRIVATE KEY-----' "${root}")"
  scan_rc=$?
  set -e
  if [[ "${scan_rc}" -eq 0 ]]; then
    printf '%s\n' "${private_key_files}"
    return 1
  elif [[ "${scan_rc}" -ne 1 ]]; then
    printf 'private-key scanner failed (grep rc=%s)\n' "${scan_rc}" >&2
    return 1
  fi
}

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/corelink-code-server-key-scan.XXXXXX")"
trap 'rm -rf -- "${work_dir}"' EXIT
touch "${work_dir}/fixture.key"

STUB_RC=0
finding_output="$(scan_tree "${BASH_SOURCE[0]}" "${work_dir}")" && {
  printf 'expected rc=0 finding to fail\n' >&2
  exit 1
}
[[ "${finding_output}" == "${work_dir}/fixture.key" ]]

STUB_RC=1
clean_output="$(scan_tree "${BASH_SOURCE[0]}" "${work_dir}")"
[[ -z "${clean_output}" ]]

STUB_RC=2
error_output="$(scan_tree "${BASH_SOURCE[0]}" "${work_dir}" 2>&1)" && {
  printf 'expected scanner error to fail\n' >&2
  exit 1
}
[[ "${error_output}" == *"grep rc=2"* ]]

printf 'code-server private-key scan selftest: PASS (grep rc 0/1/2)\n'
