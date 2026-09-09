#!/usr/bin/env bash
set -Eeuo pipefail
root="$(cd -- "$(dirname -- "$0")/../.." && pwd -P)"
exec "$root/scripts/ops/t2-w4-a2.9-post-merge-probe.sh" --selftest
