#!/usr/bin/env bash
set -euo pipefail
[ "$#" -eq 1 ] || { printf 'usage: sha256-file.sh FILE\n' >&2; exit 2; }
if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi
