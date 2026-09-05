#!/usr/bin/env bash
set -euo pipefail

# Focused, dependency-light contract fixtures for T6-W2.  The fake clw models
# the only relevant boundary: a miss invokes the supplied command, a hit returns
# a structured result without invoking it, and an internal error exits 125.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/corelink-memoize-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
bin="$tmp/bin"
mkdir -p "$bin"
system_path="${PATH}"

cat >"$bin/clw" <<'FAKE_CLW'
#!/usr/bin/env bash
set -e
while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do shift; done
[ "$#" -gt 0 ] && shift
case "${CLW_FAKE_MODE:-miss}" in
  hit)
    printf '%s\n' '{"schema_version":1,"verdict":"hit","authenticated":true}'
    exit 0
    ;;
  miss)
    printf '%s\n' '{"schema_version":1,"verdict":"miss","authenticated":true}'
    "$@"
    exit "$?"
    ;;
  malformed)
    printf '%s\n' 'cache hit (human text is not a contract)'
    exit 0
    ;;
  untrusted)
    printf '%s\n' '{"schema_version":1,"verdict":"hit","authenticated":false}'
    exit 0
    ;;
  internal)
    printf '%s\n' '{"schema_version":1,"verdict":"error","authenticated":false}'
    exit 125
    ;;
  *)
    printf 'unknown fake mode\n' >&2
    exit 125
    ;;
esac
FAKE_CLW
chmod +x "$bin/clw"

run_case() {
  local name="$1" policy="$2" mode="$3" expected_rc="$4" moat="$5"
  local marker="$tmp/$name.marker" out="$tmp/$name.out" err="$tmp/$name.err"
  rm -f "$marker"
  set +e
  (
    export CL_RUN="printf '%s' ran > '$marker'"
    export CL_INPUTS="fixture"
    export CL_ENVNAMES=""
    export CL_TOOLS=""
    export CL_CACHE_POLICY="$policy"
    export CLW_FAKE_MODE="$mode"
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
  printf '%s\n' "$name: rc=$rc"
  printf '%s\n' "$rc"
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

run_case optional-cold optional miss 0 absent
assert_marker optional-cold yes
run_case optional-miss optional miss 0 present
assert_marker optional-miss yes
run_case optional-internal optional internal 0 present
assert_marker optional-internal yes

run_case required-absence required-hit miss 78 absent
assert_marker required-absence no
run_case required-miss required-hit miss 78 present
assert_marker required-miss no
run_case required-internal required-hit internal 78 present
assert_marker required-internal no
run_case required-malformed required-hit malformed 78 present
assert_marker required-malformed no
run_case required-untrusted required-hit untrusted 78 present
assert_marker required-untrusted no
run_case required-hit required-hit hit 0 present
assert_marker required-hit no
run_case invalid-policy invalid miss 78 absent
assert_marker invalid-policy no

echo "T6-W2 policy fixtures: all green"
