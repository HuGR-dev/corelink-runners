#!/usr/bin/env bash
# The tracked selftest delegates to the verifier; this includes the repository-
# wide static grep that rejects the unsupported Wrangler build-and-push command.
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
"$script_dir/runner-image-static-check.sh"
