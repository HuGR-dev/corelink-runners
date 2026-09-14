#!/usr/bin/env bash
set -euo pipefail

die() {
  printf 'stress input error: %s\n' "$1" >&2
  exit 2
}

decimal_in_range() {
  local name=$1 value=$2 minimum=$3 maximum=$4 number
  [[ -n "$value" ]] || die "$name must be a decimal integer"
  # Capture at most three significant digits so oversized input cannot wrap in
  # Bash arithmetic. Leading zeroes remain valid decimal notation.
  [[ "$value" =~ ^0*([0-9]{1,3})$ ]] || die "$name must be a decimal integer"
  number=$((10#${BASH_REMATCH[1]}))
  (( number >= minimum && number <= maximum )) ||
    die "$name must be between $minimum and $maximum"
}

validate() {
  : "${COUNT:?COUNT is required}"
  : "${HOLD_SECONDS:?HOLD_SECONDS is required}"
  decimal_in_range COUNT "$COUNT" 1 100
  decimal_in_range HOLD_SECONDS "$HOLD_SECONDS" 1 300
}

matrix() {
  validate
  [[ "$COUNT" =~ ^0*([0-9]{1,3})$ ]] || die 'COUNT must be a decimal integer'
  local count=$((10#${BASH_REMATCH[1]})) n matrix='['
  for ((n = 1; n <= count; n++)); do
    [[ $n -eq 1 ]] || matrix+=','
    matrix+="$n"
  done
  matrix+=']'
  printf 'matrix=%s\n' "$matrix"
}

case "${1:-}" in
  validate) validate ;;
  matrix) matrix ;;
  *) die 'usage: stress-inputs.sh validate|matrix' ;;
esac
