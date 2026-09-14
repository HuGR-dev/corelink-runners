#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
validator=$script_dir/stress-inputs.sh
tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/corelink-stress-inputs.XXXXXX")
sentinel=$tmp_dir/SENTINEL_EXECUTED
trap 'rm -rf -- "$tmp_dir"' EXIT

expect_ok() {
  local count=$1 hold=$2
  COUNT=$count HOLD_SECONDS=$hold bash "$validator" validate
}

expect_bad() {
  local count=$1 hold=$2 marker=$3 output
  if output=$(COUNT=$count HOLD_SECONDS=$hold bash "$validator" validate 2>&1); then
    printf 'expected rejection (%s)\n' "$marker" >&2
    exit 1
  fi
  [[ ! -e "$sentinel" ]] || {
    printf 'malicious payload executed at %s (%s)\n' "$sentinel" "$marker" >&2
    exit 1
  }
  printf '%s\n' "$output" > "$tmp_dir/$marker"
}

expect_ok 1 1
expect_ok 100 300
literal_dollar='$'
literal_tick='`'
malicious_count=$(printf '1; touch %s\n%s touch %s%s "%s(%s)"' \
  "$sentinel" "$literal_tick" "$sentinel" "$literal_tick" "$literal_dollar" "$sentinel")
malicious_hold=$(printf '25; touch %s\n%s touch %s%s "%s(%s)"' \
  "$sentinel" "$literal_tick" "$sentinel" "$literal_tick" "$literal_dollar" "$sentinel")
expect_bad "$malicious_count" 25 malicious-count
expect_bad 40 "$malicious_hold" malicious-hold
expect_bad '' 25 blank-count
expect_bad 40 '' blank-hold
expect_bad 0 25 low-count
expect_bad 101 25 high-count
expect_bad 40 0 low-hold
expect_bad 40 301 high-hold
expect_bad 1.5 25 fractional-count
expect_bad 40 2.5 fractional-hold
expect_bad 999999999999999999999999999999999999999 25 huge-count
expect_bad 18446744073709551617 25 overflowing-count

matrix_output=$(COUNT=3 HOLD_SECONDS=2 bash "$validator" matrix)
[[ "$matrix_output" == 'matrix=[1,2,3]' ]] || {
  printf 'unexpected matrix: %s\n' "$matrix_output" >&2
  exit 1
}

[[ ! -e "$tmp_dir/SENTINEL_EXECUTED" ]] || exit 1
printf 'stress input selftest: ok\n'
