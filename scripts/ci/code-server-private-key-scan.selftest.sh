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
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
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

printf '%s\n' \
  '-----BEGIN PRIVATE KEY-----' \
QUJD
  '-----END PRIVATE KEY-----' >"${work_dir}/fixture.pem"
printf "const fixture = '-----BEGIN PRIVATE KEY-----';\n" >"${work_dir}/source.js"
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
printf "const fixture = '-----BEGIN PRIVATE KEY-----';\n" >"${source_dir}/source.js"
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
QUJD
