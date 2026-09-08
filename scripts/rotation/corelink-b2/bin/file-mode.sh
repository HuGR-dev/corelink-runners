#!/usr/bin/env bash
set -euo pipefail
[ "$#" -eq 1 ] || { printf 'usage: file-mode.sh FILE\n' >&2; exit 2; }
stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"
