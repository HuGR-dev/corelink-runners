#!/usr/bin/env bash
set -euo pipefail

# Focused fixtures for clw 0.1.12 required-hit. The fake CLI proves the action
# wire; these tests do not claim a real CAS hit.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/corelink-memoize-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
bin="$tmp/bin"; mkdir -p "$bin"; system_path="${PATH}"

cat >"$bin/clw" <<'FAKE_CLW'
#!/bin/bash
set -e
if [ "${1:-}" = "--version" ]; then printf '%s\n' "${FAKE_CLW_VERSION:-clw 0.1.12}"; exit 0; fi
printf '%q ' "$@" > "$FAKE_CLW_ARGS"; printf '\n' >> "$FAKE_CLW_ARGS"
printf '%s\n' "${__CL_TOOLVERS:-}" > "$FAKE_CLW_TOOLVERS"
case "${FAKE_CLW_MODE:-config78}" in
  cached0) printf 'cached stdout\n'; printf 'cached stderr\n' >&2; exit 0 ;;
  cached125) printf 'cached stderr\n' >&2; exit 125 ;;
  miss78|config78) exit 78 ;;
  unsupported) exit 2 ;;
  *) exit 125 ;;
esac
FAKE_CLW
chmod +x "$bin/clw"

run_case() {
  local name="$1" policy="$2" expected_rc="$3" moat="$4" version="$5" mode="$6"
  local marker="$tmp/$name.marker" out="$tmp/$name.out" err="$tmp/$name.err"
  rm -f "$marker"; set +e
  (
    export CL_RUN="printf '%s' ran > '$marker'"
    export CL_INPUTS="fixture second" CL_ENVNAMES="CACHE_KEY" CACHE_KEY="cache-value"
    export CL_TOOLS="node" CL_CACHE_POLICY="$policy"
    export FAKE_CLW_VERSION="$version" FAKE_CLW_MODE="$mode"
    export FAKE_CLW_ARGS="$tmp/$name.args" FAKE_CLW_TOOLVERS="$tmp/$name.toolvers"
    if [ "$moat" = present ]; then export CLW_ENDPOINT=fake CLW_TOKEN=fake PATH="$bin:$system_path"; else unset CLW_ENDPOINT CLW_TOKEN CLW_CRED_TICKET; export PATH="$system_path"; fi
    "$root/memoize.sh"
  ) >"$out" 2>"$err"
  local rc=$?; set -e
  if [ "$rc" -ne "$expected_rc" ]; then echo "FAIL $name: expected rc=$expected_rc got rc=$rc" >&2; cat "$out" "$err" >&2 || true; exit 1; fi
  printf '%s: rc=%s\n' "$name" "$rc"
}

assert_marker() {
  local name="$1" expected="$2" marker="$tmp/$1.marker"
  if [ "$expected" = yes ]; then [ -s "$marker" ] || { echo "FAIL $name: wrapped command did not run" >&2; exit 1; }; else [ ! -e "$marker" ] || { echo "FAIL $name: wrapped command ran unexpectedly" >&2; exit 1; }; fi
}

run_case optional-cold optional 0 absent "clw 0.1.12" config78; assert_marker optional-cold yes
run_case optional-internal-fallback optional 0 present "clw 0.1.5" cached125; assert_marker optional-internal-fallback yes
run_case optional-old-fallback optional 0 present "clw 0.1.5" cached125; assert_marker optional-old-fallback yes
run_case required-cached-zero required-hit 0 present "clw 0.1.12" cached0; assert_marker required-cached-zero no
run_case required-cached-nonzero required-hit 125 present "clw 0.1.12" cached125; assert_marker required-cached-nonzero no
run_case required-miss required-hit 78 present "clw 0.1.12" miss78; assert_marker required-miss no
run_case required-config-error required-hit 78 present "clw 0.1.12" config78; assert_marker required-config-error no
run_case required-absence required-hit 78 absent "clw 0.1.12" config78; assert_marker required-absence no
run_case required-old-version required-hit 78 present "clw 0.1.5" cached0; assert_marker required-old-version no
run_case required-unknown-version required-hit 78 present "clw 0.1.12-dev" cached0; assert_marker required-unknown-version no
run_case required-unsupported-version required-hit 78 present "clw 0.1.12-no-require-hit" cached0; assert_marker required-unsupported-version no
run_case invalid-policy invalid 78 absent "clw 0.1.12" config78; assert_marker invalid-policy no

grep -F -- 'run --require-hit --input fixture --input second --env CACHE_KEY --env __CL_TOOLVERS -- bash -c' "$tmp/required-cached-zero.args" >/dev/null
grep -F -- 'node=' "$tmp/required-cached-zero.toolvers" >/dev/null
grep -F -- 'run --input fixture --input second --env CACHE_KEY --env __CL_TOOLVERS -- bash -c' "$tmp/optional-internal-fallback.args" >/dev/null

echo "T6-W2 policy fixtures: all green"
