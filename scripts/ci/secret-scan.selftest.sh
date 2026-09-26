#!/usr/bin/env bash
# Focused real-scanner self-test; creates no credentials and scans no repository history.
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
scanner="$(command -v gitleaks || true)"
[[ -x "$scanner" ]] || { printf 'secret-scan selftest: gitleaks is required\n' >&2; exit 2; }
checker="${script_dir}/secret-scan.sh"; fixture_repo="$(mktemp -d)"; unrelated_repo=''; scoped_repo=''
cleanup() { rm -rf "$fixture_repo"; [[ -z "$unrelated_repo" ]] || rm -rf "$unrelated_repo"; [[ -z "$scoped_repo" ]] || rm -rf "$scoped_repo"; }
trap cleanup EXIT
run_scan_at() { SECRET_SCAN_TEST_MODE=1 "$checker" --repo "$1" --base "$2" --head "$3" --scanner "$scanner" >/dev/null 2>&1; }
run_scan() { run_scan_at "$fixture_repo" "$1" "$2"; }
expect_status() { local expected=$1 actual; shift; set +e; "$@"; actual=$?; set -e; [[ "$actual" -eq "$expected" ]] || { printf 'selftest: expected exit %d, got %d\n' "$expected" "$actual" >&2; exit 1; }; }
git -C "$fixture_repo" init -q; git -C "$fixture_repo" config user.email fixture@example.invalid; git -C "$fixture_repo" config user.name fixture
printf 'clean fixture\n' >"$fixture_repo/README.md"; git -C "$fixture_repo" add .; git -C "$fixture_repo" commit -qm initial
base="$(git -C "$fixture_repo" rev-parse HEAD)"
prefix='ghp_'; suffix='1234567890abcdef1234567890abcdef1234'
printf 'synthetic=%s%s\n' "$prefix" "$suffix" >"$fixture_repo/fixture.txt"; git -C "$fixture_repo" add .; git -C "$fixture_repo" commit -qm synthetic
head="$(git -C "$fixture_repo" rev-parse HEAD)"
expect_status 1 run_scan "$base" "$head"
expect_status 1 run_scan 0000000000000000000000000000000000000000 "$head"
expect_status 2 run_scan 1111111111111111111111111111111111111111 "$head"
git -C "$fixture_repo" rm -q fixture.txt; git -C "$fixture_repo" commit -qm clean; clean_head="$(git -C "$fixture_repo" rev-parse HEAD)"
expect_status 0 run_scan "$head" "$clean_head"
expect_status 2 env SECRET_SCAN_TEST_MODE=1 "$checker" --repo "$fixture_repo" --base "$head" --head "$clean_head" --scanner "$fixture_repo/missing-gitleaks"

# A zero-base scan is bounded by its supplied head, excluding unrelated refs.
unrelated_repo="$(mktemp -d)"
git -C "$unrelated_repo" init -q; git -C "$unrelated_repo" config user.email fixture@example.invalid; git -C "$unrelated_repo" config user.name fixture
printf 'clean\n' >"$unrelated_repo/a"; git -C "$unrelated_repo" add .; git -C "$unrelated_repo" commit -qm clean
unrelated_head="$(git -C "$unrelated_repo" rev-parse HEAD)"
git -C "$unrelated_repo" checkout -qb unrelated
printf 'synthetic=%s%s\n' "$prefix" "$suffix" >"$unrelated_repo/unrelated.txt"; git -C "$unrelated_repo" add .; git -C "$unrelated_repo" commit -qm unrelated-secret
git -C "$unrelated_repo" checkout -q -B mainline "$unrelated_head"
expect_status 0 run_scan_at "$unrelated_repo" 0000000000000000000000000000000000000000 "$unrelated_head"

# Repository-local config, ignore entries, and inline allow comments cannot waive findings.
printf 'synthetic=%s%s # gitleaks:allow\n' "$prefix" "$suffix" >"$fixture_repo/waived.txt"
printf 'waived.txt\n' >"$fixture_repo/.gitleaksignore"
printf '[rules]\n' >"$fixture_repo/.gitleaks.toml"
git -C "$fixture_repo" add .; git -C "$fixture_repo" commit -qm waivers; waivers_head="$(git -C "$fixture_repo" rev-parse HEAD)"
expect_status 1 run_scan "$clean_head" "$waivers_head"

# A secret introduced while resolving a merge must be caught through -m.
git -C "$fixture_repo" checkout -qb merge-side "$waivers_head"
printf 'side\n' >"$fixture_repo/merge.txt"; git -C "$fixture_repo" add merge.txt; git -C "$fixture_repo" commit -qm side
git -C "$fixture_repo" checkout -q -B mainline "$waivers_head"
printf 'main\n' >"$fixture_repo/merge.txt"; git -C "$fixture_repo" add merge.txt; git -C "$fixture_repo" commit -qm mainline
merge_base="$(git -C "$fixture_repo" rev-parse HEAD)"
set +e; git -C "$fixture_repo" merge --no-ff merge-side -m merge >/dev/null 2>&1; merge_status=$?; set -e
[[ "$merge_status" -ne 0 ]] || { printf 'selftest: merge fixture did not conflict\n' >&2; exit 1; }
printf 'synthetic=%s%s\n' "$prefix" "$suffix" >"$fixture_repo/merge.txt"; git -C "$fixture_repo" add merge.txt; git -C "$fixture_repo" commit -qm merge-resolution
merge_head="$(git -C "$fixture_repo" rev-parse HEAD)"
expect_status 1 run_scan "$merge_base" "$merge_head"

# The production config has narrow fingerprint exceptions for historical
# synthetic fixtures. A new value in the same path must still fail.
scoped_repo="$(mktemp -d)"
git -C "$scoped_repo" init -q; git -C "$scoped_repo" config user.email fixture@example.invalid; git -C "$scoped_repo" config user.name fixture
printf 'clean\n' >"$scoped_repo/README.md"; git -C "$scoped_repo" add .; git -C "$scoped_repo" commit -qm initial; scoped_base="$(git -C "$scoped_repo" rev-parse HEAD)"
mkdir -p "$scoped_repo/deploy/cloudflare/test"
printf 'const token = "%s%s";\n' 'ghp_' '1234567890abcdef1234567890abcdef1234' >"$scoped_repo/deploy/cloudflare/test/compute-terminal-test-helpers.ts"
git -C "$scoped_repo" add .; git -C "$scoped_repo" commit -qm fresh-allowlisted-path-value; scoped_head="$(git -C "$scoped_repo" rev-parse HEAD)"
expect_status 1 run_scan_at "$scoped_repo" "$scoped_base" "$scoped_head"

python3 - <<'PY'
import re
import subprocess
from pathlib import Path

assert subprocess.run(
    ["git", "ls-files", "--error-unmatch", "scripts/ci/secret-scan.selftest.sh"],
    check=False,
    capture_output=True,
).returncode == 0
secret_scan = Path("scripts/ci/secret-scan.sh").read_text()
workflow = Path(".github/workflows/selftests.yml").read_text()
version = re.search(r"readonly GITLEAKS_VERSION='([^']+)'", secret_scan).group(1)
checksum = re.search(r"readonly GITLEAKS_SHA256='([^']+)'", secret_scan).group(1)
assert f"GITLEAKS_VERSION: '{version}'" in workflow
assert f"GITLEAKS_SHA256: '{checksum}'" in workflow
install_at = workflow.index("- name: Install pinned gitleaks for tracked selftests")
discovery_at = workflow.index("- name: Discover and run every tracked selftest")
assert install_at < discovery_at
PY
printf 'secret-scan selftest: PASS (detection, zero-base, waivers, merge, clean, missing refs, scanner error)\n'
