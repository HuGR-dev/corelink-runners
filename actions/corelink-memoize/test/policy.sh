#!/usr/bin/env bash
set -euo pipefail

# Focused T6-W2 fixtures. The internal fixture is grounded in installed clw
# 0.1.5: `clw run --json` says --json does not apply, emits no JSON stdout,
# and uses exit 125 for an internal error.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/corelink-memoize-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
bin="$tmp/bin"
mkdir -p "$bin"
system_path="${PATH}"

if command -v clw >/dev/null 2>&1; then
  case "$(clw --version 2>/dev/null || true)" in
    *"clw 0.1.5"*) ;;
    *)
      echo "FAIL: this fixture is pinned to the observed clw 0.1.5 run contract" >&2
      exit 1
      ;;
  esac
fi

cat >"$bin/clw" <<'FAKE_CLW'
#!/usr/bin/env bash
set -e
printf '%s\n' '[clw] note: --json does not apply to `run` (it proxies the child' >&2
printf '%s\n' 'stdout/stderr); ignoring it' >&2
exit 125
FAKE_CLW
chmod +x "$bin/clw"

run_case() {
  local name="$1" policy="$2" expected_rc="$3" moat="$4"
  local marker="$tmp/$name.marker" out="$tmp/$name.out" err="$tmp/$name.err"
  rm -f "$marker"
  set +e
  (
    export CL_RUN="printf '%s' ran > '$marker'"
    export CL_INPUTS="fixture"
    export CL_ENVNAMES=""
    export CL_TOOLS=""
    export CL_CACHE_POLICY="$policy"
    if [ "$moat" = present ]; then
      export CLW_ENDPOINT=fake CLW_TOKEN=fake
      export PATH="$bin:$system_path"
    else
      unset CLW_ENDPOINT CLW_TOKEN CLW_CRED_TICKET
      export PATH="$system_path"
    fi
    "$root/memoize.sh"
  ) >"$out" 2>"$err"
  local rc=$?
  set -e
  if [ "$rc" -ne "$expected_rc" ]; then
    echo "FAIL $name: expected rc=$expected_rc got rc=$rc" >&2
    cat "$out" "$err" >&2 || true
    exit 1
  fi
  printf '%s: rc=%s\n' "$name" "$rc"
}

assert_marker() {
  local name="$1" expected="$2"
  local marker="$tmp/$name.marker"
  if [ "$expected" = yes ]; then
    [ -s "$marker" ] || { echo "FAIL $name: wrapped command did not run" >&2; exit 1; }
  else
    [ ! -e "$marker" ] || { echo "FAIL $name: wrapped command ran unexpectedly" >&2; exit 1; }
  fi
}

run_case optional-cold optional 0 absent
assert_marker optional-cold yes
run_case optional-internal-fallback optional 0 present
assert_marker optional-internal-fallback yes
run_case required-absence required-hit 78 absent
assert_marker required-absence no
run_case required-no-json-api required-hit 78 present
assert_marker required-no-json-api no
run_case invalid-policy invalid 78 absent
assert_marker invalid-policy no

echo "T6-W2 policy fixtures: all green"
