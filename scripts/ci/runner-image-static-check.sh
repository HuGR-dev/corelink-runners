#!/usr/bin/env bash
# runner-image-static-check.sh — no-network verifier for image-build invariants.
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
dockerfile="$repo/deploy/runner/Dockerfile"
build_workflow="$repo/.github/workflows/build-cf-container-images.yml"
fabricd_workflow="$repo/.github/workflows/build-fabricd-image.yml"
shim="$repo/deploy/runner/docker-shim.sh"

[[ -f "$dockerfile" && -f "$build_workflow" && -f "$fabricd_workflow" && -f "$shim" ]] || {
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

# Production publication must use Wrangler's direct BuildKit registry output;
# a local `docker build` followed by `containers push` recreates the containerd
# unpack/disk-exhaustion path this check guards against.
if grep -nE '(^|[[:space:]])docker build([[:space:]\\]|$)|containers push' \
    "$build_workflow" "$fabricd_workflow"; then
  echo 'runner-image-static-check: local build or second-step push path found' >&2
  exit 1
fi
if ! grep -q 'containers build' "$build_workflow" ||
   ! grep -q -- '--push' "$build_workflow" ||
   ! grep -q 'containers build' "$fabricd_workflow"; then
  echo 'runner-image-static-check: direct registry-output build is missing' >&2
  exit 1
fi
if ! grep -q 'type=image,name=' "$shim" ||
   ! grep -q '^ *--push)' "$shim"; then
  echo 'runner-image-static-check: docker buildx --push is not translated to direct registry output' >&2
  exit 1
fi

echo 'runner-image-static-check: PASS (labels consumed/removed; registry output bounded)'
