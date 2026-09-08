#!/usr/bin/env bash
# runner-image-static-check.sh — no-network verifier for image-build invariants.
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
dockerfile="$repo/deploy/runner/Dockerfile"
build_workflow="$repo/.github/workflows/build-cf-container-images.yml"
fabricd_workflow="$repo/.github/workflows/build-fabricd-image.yml"
shim="$repo/deploy/runner/docker-shim.sh"
disk_guard="$repo/scripts/ci/container-build-disk-guard.sh"

[[ -f "$dockerfile" && -f "$build_workflow" && -f "$fabricd_workflow" && -f "$shim" && -f "$disk_guard" ]] || {
  echo 'runner-image-static-check: required image files are missing' >&2
  exit 1
}

# These labels had no runtime consumer. Keeping them would make a stale image
# appear to carry a trustworthy toolchain version, so the metadata contract is
# explicit: the image may not reintroduce them without a reviewed consumer.
if grep -nE '^[[:space:]]*corelink\.(rust|gh|node)\.' "$dockerfile"; then
  echo 'runner-image-static-check: unused corelink.rust/gh/node OCI label found' >&2
  exit 1
fi

# Nightly and cargo-fuzz are real image inputs. Verify their single-source
# values are consumed by the install commands, rather than relying on labels.
for variable in NIGHTLY_DATE CARGO_FUZZ_VERSION; do
  pattern="\${${variable}}"
  count="$(grep -F -c "$pattern" "$dockerfile" || true)"
  if [[ "$count" -lt 2 ]]; then
    echo "runner-image-static-check: ${variable} is not consumed by both metadata/runtime paths" >&2
    exit 1
  fi
done

# Locked Wrangler requires a locally loaded image. Verify the production path
# explicitly prunes the exact tag and unreferenced content, with a pre-build
# free-space guard, rather than pretending `containers build --push` streams
# directly on this runner.
if ! grep -q 'docker build' "$build_workflow" ||
   ! grep -q 'containers push' "$build_workflow" ||
   ! grep -q 'docker build' "$fabricd_workflow" ||
   ! grep -q 'containers push' "$fabricd_workflow" ||
   ! grep -q 'container-build-disk-guard.sh' "$build_workflow" ||
   ! grep -q 'container-build-disk-guard.sh' "$fabricd_workflow"; then
  echo 'runner-image-static-check: bounded local build/push path is missing' >&2
  exit 1
fi
if grep -q 'type=image,name=' "$shim" || grep -q '^ *--push)' "$shim"; then
  echo 'runner-image-static-check: unsupported direct-registry shim translation found' >&2
  exit 1
fi
# shellcheck disable=SC2016
mutable_ref='REF="${IMG}:${GITHUB_SHA}"'
if grep -qF "$mutable_ref" "$fabricd_workflow" ||
   grep -q 'imagetools) exit 0' "$shim"; then
  echo 'runner-image-static-check: mutable digest fallback or silent imagetools failure found' >&2
  exit 1
fi

echo 'runner-image-static-check: PASS (labels consumed/removed; registry output bounded)'
