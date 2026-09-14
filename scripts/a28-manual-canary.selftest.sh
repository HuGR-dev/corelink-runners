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
rg -n --fixed-strings 'SPAWN_AUTH_TOKEN="$(<"${SPAWN_AUTH_TOKEN_FILE}")"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'LIFECYCLE_AUTH_TOKEN="$(<"${LIFECYCLE_AUTH_TOKEN_FILE}")"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'contains non-printable bytes' "${SCRIPT}" >/dev/null
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

# Command substitution removes trailing newlines. Distinct files containing
# the same effective bearer must therefore be rejected as well.
EFFECTIVE_SPAWN_FILE="${VALIDATION_TMP}/effective-spawn"
EFFECTIVE_LIFECYCLE_FILE="${VALIDATION_TMP}/effective-lifecycle"
printf '%s\n' 'effective-token' >"${EFFECTIVE_SPAWN_FILE}"
printf '%s\n\n' 'effective-token' >"${EFFECTIVE_LIFECYCLE_FILE}"
chmod 600 "${EFFECTIVE_SPAWN_FILE}" "${EFFECTIVE_LIFECYCLE_FILE}"
if CORELINK_LIFECYCLE_AUTH_TOKEN_FILE="${EFFECTIVE_LIFECYCLE_FILE}" \
    CORELINK_SPAWN_AUTH_TOKEN_FILE="${EFFECTIVE_SPAWN_FILE}" \
    GH_REPO=test/repo GH_TOKEN=redacted SPAWN_WORKER_URL=https://example.invalid \
    CANARY_IMAGE_DIGEST=registry.invalid/runner@sha256:$(printf '%064d' 0) \
    "${SCRIPT}" >"${VALIDATION_TMP}/effective-output" 2>&1; then
  echo "effectively identical credentials unexpectedly passed" >&2
  exit 1
fi
rg -n --fixed-strings 'CoreLink spawn and lifecycle token files must contain distinct credentials' \
  "${VALIDATION_TMP}/effective-output" >/dev/null

# A decoded binary secret must be rejected from the raw file before command
# substitution can discard NUL bytes or before any provider mutation occurs.
BINARY_LIFECYCLE_FILE="${VALIDATION_TMP}/binary-lifecycle"
printf 'printable-prefix\001binary-suffix\n' >"${BINARY_LIFECYCLE_FILE}"
chmod 600 "${BINARY_LIFECYCLE_FILE}"
if CORELINK_LIFECYCLE_AUTH_TOKEN_FILE="${BINARY_LIFECYCLE_FILE}" \
    CORELINK_SPAWN_AUTH_TOKEN_FILE="${EFFECTIVE_SPAWN_FILE}" \
    GH_REPO=test/repo GH_TOKEN=redacted SPAWN_WORKER_URL=https://example.invalid \
    CANARY_IMAGE_DIGEST=registry.invalid/runner@sha256:$(printf '%064d' 0) \
    "${SCRIPT}" >"${VALIDATION_TMP}/binary-output" 2>&1; then
  echo "binary lifecycle credential unexpectedly passed" >&2
  exit 1
fi
rg -n --fixed-strings 'CoreLink lifecycle token file contains non-printable bytes' \
  "${VALIDATION_TMP}/binary-output" >/dev/null

# The canary is CoreLink-only; no Hugit secret path or input is permitted.
if rg -ni 'hugit|\.hugit' "${SCRIPT}"; then
  echo "Hugit credential reference found" >&2
  exit 1
fi

echo "a28-manual-canary source secret-transport checks passed"
