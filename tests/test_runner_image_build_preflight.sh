#!/usr/bin/env bash
# Focused behavior + mutation test for the bounded image-build preflight.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFLIGHT="$ROOT/scripts/ci/runner-image-build-preflight.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin"
cat > "$TMP/bin/docker" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = "version" ]; then exit 0; fi
exit 0
EOF
chmod +x "$TMP/bin/docker"

PATH="$TMP/bin:$PATH" RUNNER_IMAGE_DISK_ROOT="$TMP" RUNNER_IMAGE_MIN_FREE_MB=1 \
  "$PREFLIGHT" >/dev/null

if PATH="$TMP/bin:$PATH" RUNNER_IMAGE_DISK_ROOT="$TMP" RUNNER_IMAGE_MIN_FREE_MB=999999999 \
  "$PREFLIGHT" >/dev/null 2>&1; then
  echo "FAIL: high disk floor was accepted" >&2
  exit 1
fi

# A weakened disk comparison makes the high-floor case pass. That is the
# expected observable failure of the behavioral assertion, so the harness kills
# the mutant by requiring this changed behavior explicitly.
MUTANT="$TMP/preflight-mutant.sh"
cp "$PREFLIGHT" "$MUTANT"
# The single-quoted sed expression intentionally preserves the script's
# parameter literals while replacing the comparison in the copied mutant.
# shellcheck disable=SC2016
sed -i.bak 's/if \[ "$available_mb" -lt "$MIN_FREE_MB" \]; then/if false; then/' "$MUTANT"
if PATH="$TMP/bin:$PATH" RUNNER_IMAGE_DISK_ROOT="$TMP" RUNNER_IMAGE_MIN_FREE_MB=999999999 \
  "$MUTANT" >/dev/null 2>&1; then
  echo "PASS: disk-guard mutation is killed (weakened guard accepts an over-floor)"
else
  echo "FAIL: disk-guard mutation was still rejected" >&2
  exit 1
fi

echo "PASS: image-build preflight accepts budget, rejects low budget, and kills mutation"
