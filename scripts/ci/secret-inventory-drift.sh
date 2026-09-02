#!/usr/bin/env bash
# Offline, names-only drift check for tracked credential and binding names.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="${SECRET_INVENTORY_ROOT:-$(git -C "${script_dir}" rev-parse --show-toplevel)}"
inventory_path="${repo_root}/docs/runbook/secret-inventory.md"

if [[ ! -f "${inventory_path}" ]]; then
  printf 'secret inventory missing: docs/runbook/secret-inventory.md\n' >&2
  exit 1
fi

excluded_roots=(
  'deploy/cloudflare/node_modules/' 'deploy/cloudflare/dist/'
  'deploy/cloudflare/vendor/' 'deploy/cloudflare-fabricd/node_modules/'
  'deploy/cloudflare-fabricd/dist/' 'deploy/cloudflare-fabricd/vendor/'
  'deploy/cloudflare-canary/node_modules/' 'deploy/cloudflare-canary/dist/'
  'deploy/cloudflare-canary/vendor/' 'actions/corelink-memoize/node_modules/'
  'integrations/github-actions/node_modules/'
)

is_excluded() {
  local path="$1" root
  for root in "${excluded_roots[@]}"; do
    [[ "${path}" == "${root}"* ]] && return 0
  done
  return 1
}

is_credential_or_binding_name() {
  local name="$1"
  case "${name}" in
    AU7_8_*|CLW_TOKEN|CLW_CRED_TICKET|CLW_LEASE_ID|CLW_FABRIC_ENDPOINT|\
    CORELINK_*TOKEN|CORELINK_*SECRET|CORELINK_*KEY|CORELINK_*PAT|\
    CORELINK_*CREDENTIAL|CORELINK_CF_ACCESS_CLIENT_ID|\
    FABRIC_*TOKEN|FABRIC_*SECRET|FABRIC_*KEY|FABRIC_*PAT|FABRIC_*DIGEST|\
    GITHUB_*TOKEN|\
    GITHUB_*SECRET|GITHUB_*PRIVATE_KEY|GITHUB_APP_ID|CLOUDFLARE_*TOKEN|\
    FLEET_BUSY_READ_KEY|EXEC_SERVER_AUTH_TOKEN|\
    DATABASE_URL|NPM_TOKEN|PYPI_TOKEN|RESEND_API_KEY|PINNED_IMAGE_DIGEST|\
    RUNNER_JOB_PATS|CRED_STASH|CONCURRENCY_SLOTS|WEBHOOK_LIMITER|METRICS|\
    CANARY_KV|FABRICD|FABRICD_SVC|SPAWN_SVC|CHECK_HOST_CONTAINER|RUNNER_CONTAINER|\
    TOOLCHAIN_DIGEST)
      return 0 ;;
  esac
  return 1
}

tmp_names="$(mktemp)"
tmp_names_sorted="$(mktemp)"
tmp_inventory="$(mktemp)"
tmp_paths="$(mktemp)"
trap 'rm -f "${tmp_names}" "${tmp_names_sorted}" "${tmp_inventory}" "${tmp_paths}"' EXIT

git -C "${repo_root}" ls-files -z -- \
  ':(glob)deploy/**/wrangler*.jsonc' \
  ':(glob).github/workflows/*.yml' ':(glob).github/workflows/**/*.yml' \
  ':(glob).github/workflows/*.yaml' ':(glob).github/workflows/**/*.yaml' \
  ':(glob)deploy/**/src/**/*.ts' ':(glob)deploy/**/src/**/*.js' \
  ':(glob)crates/**/src/**/*.rs' ':(glob)deploy/**/*.sh' \
  ':(glob)deploy/**/Dockerfile*' ':(glob)scripts/**/*.sh' \
  ':(glob)actions/**' ':(glob)integrations/**' >"${tmp_paths}"

while IFS= read -r -d '' path; do
  is_excluded "${path}" && continue
  file="${repo_root}/${path}"
  [[ -f "${file}" ]] || continue

  grep -oE 'wrangler[[:space:]]+secret[[:space:]]+put[[:space:]]+[A-Z][A-Z0-9_]+' "${file}" 2>/dev/null \
    | sed -E 's/.*put[[:space:]]+//' >>"${tmp_names}" || true
  grep -oE '"[A-Z][A-Z0-9_]*(PAT|TOKEN|KEY|SECRET)"' "${file}" 2>/dev/null \
    | tr -d '"' >>"${tmp_names}" || true
  grep -oE 'secrets\.[A-Z][A-Z0-9_]+' "${file}" 2>/dev/null \
    | sed 's/.*\.//' >>"${tmp_names}" || true
  grep -oE 'env\.[A-Z][A-Z0-9_]+' "${file}" 2>/dev/null \
    | sed 's/.*\.//' >>"${tmp_names}" || true
  grep -oE 'env\[[A-Z][A-Z0-9_]*\]' "${file}" 2>/dev/null \
    | sed -E 's/.*\[//; s/\].*//' >>"${tmp_names}" || true
  grep -oE 'env\["[A-Z][A-Z0-9_]*"\]' "${file}" 2>/dev/null \
    | sed -E 's/.*env\["//; s/"\].*//' >>"${tmp_names}" || true
  grep -oE "env\\['[A-Z][A-Z0-9_]*'\\]" "${file}" 2>/dev/null \
    | sed -E "s/.*env\\['//; s/'\\].*//" >>"${tmp_names}" || true
  grep -oE 'process\.env\["[A-Z][A-Z0-9_]*"\]' "${file}" 2>/dev/null \
    | sed -E 's/.*env\["//; s/"\].*//' >>"${tmp_names}" || true
  grep -oE "process\\.env\\['[A-Z][A-Z0-9_]*'\\]" "${file}" 2>/dev/null \
    | sed -E "s/.*env\\['//; s/'\\].*//" >>"${tmp_names}" || true
  grep -oE '(std::env::(var|var_os)|get|var)\("[A-Z][A-Z0-9_]*"' "${file}" 2>/dev/null \
    | sed -E 's/.*\("//; s/".*//' >>"${tmp_names}" || true
  grep -oE '\$[{][A-Z][A-Z0-9_]*' "${file}" 2>/dev/null \
    | sed 's/.*{//' >>"${tmp_names}" || true
  grep -oE '"binding"[[:space:]]*:[[:space:]]*"[A-Z][A-Z0-9_]*"' "${file}" 2>/dev/null \
    | sed -E 's/.*"([A-Z][A-Z0-9_]*)"$/\1/' >>"${tmp_names}" || true
done <"${tmp_paths}"

# Inventory entries are names in code spans, never values. Uppercase spans
# keep prose words from becoming entries.
grep -oE "\`[A-Z][A-Z0-9_]{2,}\`" "${inventory_path}" 2>/dev/null \
  | tr -d '`' | sort -u >"${tmp_inventory}" || true

sort -u "${tmp_names}" >"${tmp_names_sorted}"
while IFS= read -r name; do
  [[ -n "${name}" ]] || continue
  is_credential_or_binding_name "${name}" || continue
  if ! grep -Fxq "${name}" "${tmp_inventory}"; then
    printf 'secret-inventory drift: tracked name %s is missing from %s\n' \
      "${name}" "${inventory_path#"${repo_root}"/}" >&2
    exit 1
  fi
done <"${tmp_names_sorted}"

# The inventory must not silently grow stale either. Ordinary non-secret notes
# remain free-form; credential/binding entries are compared symmetrically.
while IFS= read -r name; do
  [[ -n "${name}" ]] || continue
  is_credential_or_binding_name "${name}" || continue
  if ! grep -Fxq "${name}" "${tmp_names_sorted}"; then
    printf 'secret-inventory drift: inventory name %s has no tracked source\n' "${name}" >&2
    exit 1
  fi
done <"${tmp_inventory}"

printf 'secret-inventory drift: PASS (names only; tracked source set is current)\n'
