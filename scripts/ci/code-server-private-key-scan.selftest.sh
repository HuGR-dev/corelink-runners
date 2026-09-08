#!/usr/bin/env bash
set -euo pipefail

# The image Dockerfile accepts a source tree string only when it is not a PEM
# header line. A real PEM block requires the header plus base64 payload or END.
# grep status remains three-state: 0 = candidates, 1 = clean, >1 = error.
if [[ "${STUB_MODE:-0}" == 1 ]]; then
  case "${STUB_RC:?}" in
    0) printf '%s\n' "${STUB_PATH:?}"; exit 0 ;;
    1) exit 1 ;;
    *) exit "${STUB_RC}" ;;
  esac
fi

scan_tree() {
  local root=$1 grep_bin=${2:-grep}
  local candidate_files scan_rc scanner_error=0 private_key_files='' private_name_files find_rc

  set +e
  candidate_files="$(${grep_bin} -RIlE --binary-files=without-match -- \
    '^[[:space:]]*-----BEGIN (RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----[[:space:]]*$' "${root}")"
  scan_rc=$?
  if (( scan_rc > 1 )); then
    scanner_error=${scan_rc}
  elif (( scan_rc == 0 )); then
    while IFS= read -r candidate; do
      [[ -n "${candidate}" ]] || continue
      awk 'BEGIN { in_pem=0; found=0 }
        {
          line=$0; sub(/\r$/, "", line)
          if (line ~ /^[[:space:]]*-----BEGIN (RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----[[:space:]]*$/) { in_pem=1; next }
          if (in_pem && line ~ /^[[:space:]]*-----END (RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----[[:space:]]*$/) { found=1; exit }
          if (in_pem) {
            payload=line; gsub(/[[:space:]]/, "", payload)
            if (length(payload) >= 16 && payload ~ /^[A-Za-z0-9+\/=]+$/) { found=1; exit }
          }
        }
        END { exit(found ? 0 : 1) }' "${candidate}" >/dev/null 2>&1
      candidate_rc=$?
      if (( candidate_rc == 0 )); then
        private_key_files+="${candidate}"$'\n'
      elif (( candidate_rc > 1 )); then
        scanner_error=${candidate_rc}
      fi
    done <<< "${candidate_files}"
  fi
  private_name_files="$(find "${root}" -type f \( -iname '*.key' -o -iname '*.key.pem' \
    -o -iname 'id_rsa' -o -iname 'id_dsa' -o -iname 'id_ecdsa' \
    -o -iname 'id_ed25519' -o -iname 'private.key' -o -iname 'private.pem' \) -print)"
  find_rc=$?
  if (( find_rc != 0 )); then scanner_error=${find_rc}; fi
  private_key_files+="${private_name_files}"
  set -e

  if (( scanner_error != 0 )); then
    printf 'private-key scanner failed (rc=%s)\n' "${scanner_error}" >&2
    return 1
  elif [[ -n "${private_key_files}" ]]; then
    printf '%s\n' "${private_key_files}"
    return 1
  fi
}

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/corelink-code-server-key-scan.XXXXXX")"
trap 'rm -rf -- "${work_dir}"' EXIT

pem_begin='-----BEGIN'
pem_end='-----END'
pem_kind=' PRIVATE KEY-----'
pem_payload='QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo='
printf '%s%s\n%s\n%s%s\n' \
  "${pem_begin}" "${pem_kind}" "${pem_payload}" "${pem_end}" "${pem_kind}" \
  >"${work_dir}/fixture.pem"
printf "const fixture = '%s%s';\n" "${pem_begin}" "${pem_kind}" >"${work_dir}/source.js"

real_output="$(scan_tree "${work_dir}")" && {
  printf 'expected real PEM fixture to fail\n' >&2
  exit 1
}
[[ "${real_output}" == *"${work_dir}/fixture.pem"* ]]
[[ "${real_output}" != *"QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo="* ]]

source_dir="$(mktemp -d "${TMPDIR:-/tmp}/corelink-code-server-source.XXXXXX")"
trap 'rm -rf -- "${work_dir}" "${source_dir}"' EXIT
printf "const fixture = '-----BEGIN PRIVATE KEY-----';\n" >"${source_dir}/source.js"
source_output="$(scan_tree "${source_dir}")"
[[ -z "${source_output}" ]]

stub_output="$(STUB_MODE=1 STUB_RC=2 scan_tree "${source_dir}" "${BASH_SOURCE[0]}" 2>&1)" && {
  printf 'expected scanner error to fail\n' >&2
  exit 1
}
[[ "${stub_output}" == *"rc=2"* ]]

printf 'code-server private-key scan selftest: PASS (real PEM, source literal, scanner error)\n'
