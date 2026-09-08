#!/usr/bin/env bash
# Offline tests for image-build-impact.sh. No Docker, registry, or credentials.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
checker="$script_dir/image-build-impact.sh"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/image-build-impact.XXXXXX")"
trap 'rm -rf -- "$fixture"' EXIT

git -C "$fixture" init -q
git -C "$fixture" config user.email selftest@example.invalid
git -C "$fixture" config user.name image-build-impact-selftest
mkdir -p "$fixture/deploy/runner" "$fixture/crates/corelink-check-exec-server/src" \
  "$fixture/deploy/cloudflare" "$fixture/crates/corelink-fabric-server" \
  "$fixture/.github/workflows"
for path in \
  deploy/runner/Dockerfile \
  crates/corelink-check-exec-server/src/main.rs \
  crates/corelink-fabric-server/Dockerfile \
  deploy/cloudflare/Dockerfile.runner-devenv \
  .github/workflows/build-cf-container-images.yml \
  .github/workflows/build-fabricd-image.yml; do
  mkdir -p "$fixture/$(dirname "$path")"
  printf '%s\n' "$path" > "$fixture/$path"
done
git -C "$fixture" add .
git -C "$fixture" commit -qm base
base="$(git -C "$fixture" rev-parse HEAD)"

printf 'changed\n' >> "$fixture/deploy/runner/Dockerfile"
printf 'changed\n' >> "$fixture/crates/corelink-check-exec-server/src/main.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm image-inputs
head="$(git -C "$fixture" rev-parse HEAD)"
output="$($checker --repo "$fixture" --base "$base" --head "$head")"
grep -q 'image=corelink-spawn-worker-runnercontainer' <<< "$output"
grep -q 'image=corelink-spawn-worker-checkhostcontainer' <<< "$output"
grep -q 'image=corelink-fabricd-fabricdcontainer' <<< "$output"
grep -q 'image=corelink-runner-devenv' <<< "$output"
echo 'PASS image-build-impact detects transitive image closures'

rename="$fixture/rename"
mkdir -p "$rename/deploy/runner" "$rename/docs"
printf 'runner recipe\n' > "$rename/deploy/runner/Dockerfile"
git -C "$rename" init -q
git -C "$rename" config user.email selftest@example.invalid
git -C "$rename" config user.name image-build-impact-rename-selftest
git -C "$rename" add .
git -C "$rename" commit -qm rename-base
rename_base="$(git -C "$rename" rev-parse HEAD)"
git -C "$rename" mv deploy/runner/Dockerfile docs/runner-recipe.txt
git -C "$rename" commit -qm rename-out-of-tree
rename_head="$(git -C "$rename" rev-parse HEAD)"
output="$($checker --repo "$rename" --base "$rename_base" --head "$rename_head")"
grep -q 'image=corelink-spawn-worker-runnercontainer' <<< "$output"
echo 'PASS image-build-impact detects out-of-tree Dockerfile rename'

printf 'docs\n' > "$fixture/README.md"
git -C "$fixture" add README.md
git -C "$fixture" commit -qm docs
base="$(git -C "$fixture" rev-parse HEAD~1)"
head="$(git -C "$fixture" rev-parse HEAD)"
output="$($checker --repo "$fixture" --base "$base" --head "$head")"
grep -q 'IMAGE_BUILD_NOT_REQUIRED' <<< "$output"
echo 'PASS image-build-impact ignores unrelated changes'
