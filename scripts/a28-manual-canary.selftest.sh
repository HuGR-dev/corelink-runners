#!/usr/bin/env bash
# Source-level guard for the direct canary's secret transport.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="${SCRIPT_DIR}/a28-manual-canary.sh"

# A bearer value or JIT document must never be an argument of curl. These
# checks intentionally fail on the old inline forms.
if rg -n 'Authorization: Bearer \$\{|--data[[:space:]]+"\$\(jq.*\$\{JIT\}' "${SCRIPT}"; then
  echo "secret-bearing curl argument found" >&2
  exit 1
fi
if rg -n -- '(-H|--header)[[:space:]]+"Authorization: Bearer|--data([[:space:]]|=)' "${SCRIPT}"; then
  echo "inline bearer or data curl argument found" >&2
  exit 1
fi
if rg -n --fixed-strings -- '--arg jit "${JIT}"' "${SCRIPT}"; then
  echo "JIT secret-bearing jq argument found" >&2
  exit 1
fi

rg -n --fixed-strings 'CORELINK_SPAWN_AUTH_TOKEN_FILE' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'CORELINK_LIFECYCLE_AUTH_TOKEN_FILE' "${SCRIPT}" >/dev/null
[[ "$(rg -n --fixed-strings -- '-H "@${SPAWN_AUTH_HEADER_FILE}"' "${SCRIPT}" | wc -l | tr -d ' ')" == 1 ]]
[[ "$(rg -n --fixed-strings -- '-H "@${LIFECYCLE_AUTH_HEADER_FILE}"' "${SCRIPT}" | wc -l | tr -d ' ')" == 1 ]]
rg -n --fixed-strings -- '--data-binary "@${SPAWN_BODY_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings -- '--rawfile jit "${JIT_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'chmod 600 "${JIT_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'chmod 600 "${SPAWN_AUTH_HEADER_FILE}" "${LIFECYCLE_AUTH_HEADER_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'chmod 600 "${SPAWN_BODY_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'rm -rf -- "${TMP_DIR}"' "${SCRIPT}" >/dev/null

# A missing lifecycle credential must fail during input validation, before the
# launcher creates temporary files or mints a single-use GitHub JIT runner.
VALIDATION_TMP="$(mktemp -d "${TMPDIR:-/tmp}/a28-manual-canary-selftest.XXXXXXXX")"
trap 'rm -rf -- "${VALIDATION_TMP}"' EXIT
if TMPDIR="${VALIDATION_TMP}" GH_REPO=test/repo GH_TOKEN=redacted \
    SPAWN_WORKER_URL=https://example.invalid \
    CORELINK_SPAWN_AUTH_TOKEN_FILE=/dev/null \
    CANARY_IMAGE_DIGEST=registry.invalid/runner@sha256:$(printf '%064d' 0) \
    "${SCRIPT}" >"${VALIDATION_TMP}/output" 2>&1; then
  echo "missing lifecycle credential unexpectedly passed" >&2
  exit 1
fi
rg -n --fixed-strings 'CORELINK_LIFECYCLE_AUTH_TOKEN_FILE is required' "${VALIDATION_TMP}/output" >/dev/null
[[ "$(find "${VALIDATION_TMP}" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" == 0 ]]

# Spawn and lifecycle credentials must differ by content, even when the
# caller accidentally supplies the same path. The check must happen before
# Wrangler, GitHub, or /v1/spawn can mutate anything.
SAME_TOKEN_FILE="${VALIDATION_TMP}/same-token"
printf '%s\n' 'synthetic-token-for-selftest' >"${SAME_TOKEN_FILE}"
chmod 600 "${SAME_TOKEN_FILE}"
if CORELINK_LIFECYCLE_AUTH_TOKEN_FILE="${SAME_TOKEN_FILE}" \
    CORELINK_SPAWN_AUTH_TOKEN_FILE="${SAME_TOKEN_FILE}" \
    GH_REPO=test/repo GH_TOKEN=redacted SPAWN_WORKER_URL=https://example.invalid \
    CANARY_IMAGE_DIGEST=registry.invalid/runner@sha256:$(printf '%064d' 0) \
    "${SCRIPT}" >"${VALIDATION_TMP}/same-output" 2>&1; then
  echo "identical credentials unexpectedly passed" >&2
  exit 1
fi
rg -n --fixed-strings 'CoreLink spawn and lifecycle token files must contain distinct credentials' \
  "${VALIDATION_TMP}/same-output" >/dev/null

# The canary is CoreLink-only; no Hugit secret path or input is permitted.
if rg -ni 'hugit|\.hugit' "${SCRIPT}"; then
  echo "Hugit credential reference found" >&2
  exit 1
fi

echo "a28-manual-canary source secret-transport checks passed"
