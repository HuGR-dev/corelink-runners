#!/usr/bin/env bash
# Focused offline self-test for secret-inventory-drift.sh.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
checker="${script_dir}/secret-inventory-drift.sh"
fixture_repo="$(mktemp -d)"
trap 'rm -rf "${fixture_repo}"' EXIT

au_prefix='AU7'"_8_"
# The matrix below constructs these exact names at runtime, keeping fixture
# tokens out of the real repository's own source scan:
# AU7_8_WRANGLER_MISSING, AU7_8_WORKFLOW_MISSING,
# AU7_8_DEPLOY_SOURCE_MISSING, AU7_8_RUST_SOURCE_MISSING,
# AU7_8_DEPLOY_SHELL_MISSING, AU7_8_SCRIPT_MISSING,
# AU7_8_ACTION_MISSING, AU7_8_INTEGRATION_MISSING,
# Plus bracketed env access and Dockerfile* path coverage.
# AU7_8_STALE_INVENTORY, AU7_8_GENERATED_IGNORED, AU7_8_VENDOR_IGNORED.

reset_fixture() {
  local path="$1" body="$2" include_name="$3" name="$4"
  rm -rf "${fixture_repo}"
  mkdir -p "${fixture_repo}/$(dirname -- "${path}")" "${fixture_repo}/docs/runbook"
  printf '%s\n' "${body}" >"${fixture_repo}/${path}"
  if [[ "${include_name}" == 1 ]]; then
    printf "# fixture names only\n\n\`%s\`\n" "${name}" >"${fixture_repo}/docs/runbook/secret-inventory.md"
  else
    : >"${fixture_repo}/docs/runbook/secret-inventory.md"
  fi
  git -C "${fixture_repo}" init -q
  git -C "${fixture_repo}" add -A
}

expect_pass() {
  local label="$1"
  if ! SECRET_INVENTORY_ROOT="${fixture_repo}" "${checker}" >/dev/null 2>&1; then
    printf 'selftest: expected pass: %s\n' "${label}" >&2
    exit 1
  fi
}

expect_fail() {
  local label="$1"
  if SECRET_INVENTORY_ROOT="${fixture_repo}" "${checker}" >/dev/null 2>&1; then
    printf 'selftest: expected fail: %s\n' "${label}" >&2
    exit 1
  fi
}

run_negative_case() {
  local suffix="$1" path="$2" body="$3"
  local name="${au_prefix}${suffix}"
  reset_fixture "${path}" "${body}" 1 "${name}"
  expect_pass "${suffix} present"
  reset_fixture "${path}" "${body}" 0 "${name}"
  expect_fail "${suffix} missing"
}

run_negative_case WRANGLER_MISSING \
  deploy/fixture/wrangler.jsonc \
  "// wrangler secret put ${au_prefix}WRANGLER_MISSING"
run_negative_case WORKFLOW_MISSING \
  .github/workflows/fixture.yml \
  "name: fixture\n# secrets.${au_prefix}WORKFLOW_MISSING"
run_negative_case DEPLOY_SOURCE_MISSING \
  deploy/fixture/src/index.ts \
  "const token = env.${au_prefix}DEPLOY_SOURCE_MISSING;"
run_negative_case DEPLOY_SOURCE_BRACKET_DOUBLE_MISSING \
  deploy/fixture/src/bracket-double.ts \
  "const token = env[\"${au_prefix}DEPLOY_SOURCE_BRACKET_DOUBLE_MISSING\"];"
run_negative_case DEPLOY_SOURCE_BRACKET_SINGLE_MISSING \
  deploy/fixture/src/bracket-single.ts \
  "const token = env['${au_prefix}DEPLOY_SOURCE_BRACKET_SINGLE_MISSING'];"
run_negative_case RUST_SOURCE_MISSING \
  crates/fixture/src/lib.rs \
  "fn fixture() { let _ = std::env::var(\"${au_prefix}RUST_SOURCE_MISSING\"); }"
run_negative_case DOCKERFILE_SUFFIX_MISSING \
  deploy/fixture/Dockerfile.ci \
  "RUN printf '%s' \"\${${au_prefix}DOCKERFILE_SUFFIX_MISSING:-}\""
run_negative_case DEPLOY_SHELL_MISSING \
  deploy/fixture.sh \
  "printf '%s' \"\${${au_prefix}DEPLOY_SHELL_MISSING:-}\""
run_negative_case SCRIPT_MISSING \
  scripts/fixture.sh \
  "printf '%s' \"\${${au_prefix}SCRIPT_MISSING:-}\""
run_negative_case ACTION_MISSING \
  actions/fixture/action.yml \
  "runs: echo \${${au_prefix}ACTION_MISSING}"
run_negative_case INTEGRATION_MISSING \
  integrations/fixture/action.yml \
  "runs: echo \${${au_prefix}INTEGRATION_MISSING}"

# Real-world names must be classified by their credential suffix even when
# they do not carry a CORELINK/FABRIC/GITHUB prefix.  In particular, removing
# this inventory entry must fail for the cold-organic tenant PAT binding.
real_name='COLD_ORGANIC_TENANT_PAT'
reset_fixture deploy/fixture/src/index.ts \
  "const token = env.${real_name};" 1 "${real_name}"
expect_pass COLD_ORGANIC_TENANT_PAT_PRESENT
reset_fixture deploy/fixture/src/index.ts \
  "const token = env.${real_name};" 0 "${real_name}"
expect_fail COLD_ORGANIC_TENANT_PAT_MISSING

# A near miss must remain ordinary application configuration rather than
# broadening the matcher to every uppercase environment variable.
near_miss='COLD_ORGANIC_TENANT_PATS'
reset_fixture deploy/fixture/src/index.ts \
  "const token = env.${near_miss};" 0 "${near_miss}"
expect_pass COLD_ORGANIC_TENANT_PATS_NEAR_MISS

# Inventory-only drift is also an error.
stale_name="${au_prefix}STALE_INVENTORY"
reset_fixture docs/runbook/fixture.txt "fixture" 1 "${stale_name}"
git -C "${fixture_repo}" rm -q -f docs/runbook/fixture.txt
expect_fail STALE_INVENTORY

# These exact rooted generated/vendor paths are ignored by the checker.
ignored_generated="${au_prefix}GENERATED_IGNORED"
reset_fixture deploy/cloudflare/dist/src/generated.ts \
  "const token = env.${ignored_generated};" 0 "${ignored_generated}"
expect_pass GENERATED_IGNORED
reset_fixture deploy/cloudflare/src/moved-generated.ts \
  "const token = env.${ignored_generated};" 0 "${ignored_generated}"
expect_fail GENERATED_MOVED_TO_SOURCE

ignored_vendor="${au_prefix}VENDOR_IGNORED"
reset_fixture deploy/cloudflare/vendor/src/vendor.ts \
  "const token = env.${ignored_vendor};" 0 "${ignored_vendor}"
expect_pass VENDOR_IGNORED
reset_fixture deploy/cloudflare/src/moved-vendor.ts \
  "const token = env.${ignored_vendor};" 0 "${ignored_vendor}"
expect_fail VENDOR_MOVED_TO_SOURCE

printf 'secret-inventory drift selftest: PASS (offline names-only matrix)\n'
