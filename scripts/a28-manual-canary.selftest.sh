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

rg -n --fixed-strings 'CORELINK_SPAWN_AUTH_TOKEN_FILE' "${SCRIPT}" >/dev/null
rg -n --fixed-strings -- '-H "@${AUTH_HEADER_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings -- '--data-binary "@${SPAWN_BODY_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'chmod 600 "${AUTH_HEADER_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'chmod 600 "${SPAWN_BODY_FILE}"' "${SCRIPT}" >/dev/null
rg -n --fixed-strings 'rm -rf -- "${TMP_DIR}"' "${SCRIPT}" >/dev/null

# The canary is CoreLink-only; no Hugit secret path or input is permitted.
if rg -ni 'hugit|\.hugit' "${SCRIPT}"; then
  echo "Hugit credential reference found" >&2
  exit 1
fi

echo "a28-manual-canary source secret-transport checks passed"
