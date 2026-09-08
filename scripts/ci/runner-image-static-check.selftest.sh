#!/usr/bin/env bash
# The tracked selftest delegates to the verifier; source mutations are covered
# by its exact metadata/build-path checks and by actionlint in the repository gate.
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
"$script_dir/runner-image-static-check.sh"
