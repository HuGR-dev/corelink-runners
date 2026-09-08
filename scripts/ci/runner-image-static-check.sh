#!/usr/bin/env bash
# runner-image-static-check.sh — no-network verifier for image-build invariants.
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
dockerfile="$repo/deploy/runner/Dockerfile"
build_workflow="$repo/.github/workflows/build-cf-container-images.yml"
validation_workflow="$repo/.github/workflows/image-build-impact.yml"
fabricd_workflow="$repo/.github/workflows/build-fabricd-image.yml"
shim="$repo/deploy/runner/docker-shim.sh"
disk_guard="$repo/scripts/ci/container-build-disk-guard.sh"
validation_script="$repo/scripts/ci/runner-image-build-validation.sh"

[[ -f "$dockerfile" && -f "$build_workflow" && -f "$validation_workflow" &&
   -f "$fabricd_workflow" && -f "$shim" && -f "$disk_guard" &&
   -x "$validation_script" ]] || {
  echo 'runner-image-static-check: required image files are missing' >&2
  exit 1
}

# The locked Wrangler release does not implement a build-and-push command. Keep
# operational docs and comments aligned with the supported local Docker build
# followed by `wrangler containers push` flow. Assemble the probe so this check
# cannot keep its own obsolete command alive as a false positive.
obsolete_command='wrangler'
obsolete_command+=' containers build'
if git -C "$repo" grep -nFi -- "$obsolete_command" -- .; then
  echo 'runner-image-static-check: obsolete Wrangler container-build command found' >&2
  exit 1
fi

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
# free-space guard, rather than relying on the unsupported Wrangler build-and-
# push shortcut to stream directly on this runner.
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

# The PR validation lane is intentionally a pull_request build-only path. It
# must not become a pull_request_target workflow or inherit a publication token.
if ! grep -qE '^  pull_request:[[:space:]]*$' "$validation_workflow" ||
   ! grep -qE '^  contents:[[:space:]]+read[[:space:]]*$' "$validation_workflow" ||
   ! grep -q 'persist-credentials: false' "$validation_workflow" ||
   ! grep -q 'github.event.pull_request.number' "$validation_workflow" ||
   ! grep -q 'runner-image-build-validation.sh' "$validation_workflow"; then
  echo 'runner-image-static-check: PR build validation trust/concurrency contract missing' >&2
  exit 1
fi
if grep -qE 'pull_request_target|secrets\.|docker push|containers push|wrangler deploy' "$validation_workflow"; then
  echo 'runner-image-static-check: PR validation workflow contains trust or publication escape' >&2
  exit 1
fi

# The validator may load a local image for the Docker build, but it may not log
# in, push, deploy, or call the publication workflow. The manual workflow above
# remains the only registry path.
if ! grep -q 'docker build' "$validation_script" ||
   ! grep -q 'image-build-impact.sh' "$validation_script" ||
   ! grep -q 'env -u GITHUB_TOKEN' "$validation_script" ||
   grep -qE 'docker (login|push)|containers push|wrangler|gh workflow run' "$validation_script"; then
  echo 'runner-image-static-check: PR validator is not a secretless build-only path' >&2
  exit 1
fi

echo 'runner-image-static-check: PASS (labels consumed/removed; registry output bounded)'
