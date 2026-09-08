#!/usr/bin/env bash
set -euo pipefail
[ "$#" -eq 1 ] || { printf 'usage: file-owner.sh FILE\n' >&2; exit 2; }
stat -f '%Su' "$1" 2>/dev/null || stat -c '%U' "$1"
