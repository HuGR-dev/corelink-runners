#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
guard="$script_dir/container-build-disk-guard.sh"
"$guard" --help >/dev/null
if "$guard" --minimum-free-mb invalid >/dev/null 2>&1; then
  echo 'disk guard accepted malformed minimum-free-mb' >&2
  exit 1
fi
if "$guard" --prune-only >/dev/null 2>&1; then
  echo 'disk guard accepted prune-only without an exact image' >&2
  exit 1
fi
echo 'PASS container-build-disk-guard argument selftest'
