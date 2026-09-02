#!/usr/bin/env bash
# Offline negative matrix for image-pin-freshness.sh. No Docker, registry,
# Cloudflare credential, or network access is used.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
checker="$script_dir/image-pin-freshness.sh"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/image-pin-freshness.XXXXXX")"
trap 'rm -rf -- "$tmp"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
expect_red() {
  local fixture=$1 label=$2 output status
  set +e
  output=$("$checker" --repo "$fixture" --strict 2>&1)
  status=$?
  set -e
  ((status != 0)) || { printf '%s\n' "$output"; fail "$label unexpectedly passed"; }
  printf 'PASS %s (fail-closed)\n' "$label"
}

make_fixture() {
  local fixture=$1
  mkdir -p "$fixture/deploy/runner" "$fixture/deploy/check-host" \
    "$fixture/deploy/cloudflare" "$fixture/deploy/cloudflare-fabricd" \
    "$fixture/deploy/cloudflare-canary" \
    "$fixture/crates/corelink-fabric-server" "$fixture/crates/corelink-fabric" \
    "$fixture/crates/corelink-runners-contracts" "$fixture/.github/workflows"
  git -C "$fixture" init -q
  git -C "$fixture" config user.email selftest@example.invalid
  git -C "$fixture" config user.name image-pin-selftest
  printf 'runner source\n' > "$fixture/deploy/runner/Dockerfile"
  printf 'checkhost source\n' > "$fixture/deploy/check-host/Dockerfile"
  printf 'fabric server\n' > "$fixture/crates/corelink-fabric-server/Dockerfile"
  printf 'fabric\n' > "$fixture/crates/corelink-fabric/lib.rs"
  printf 'contracts\n' > "$fixture/crates/corelink-runners-contracts/lib.rs"
  printf 'workflow\n' > "$fixture/.github/workflows/build-cf-container-images.yml"
  git -C "$fixture" add .
  git -C "$fixture" commit -qm source-build
  local build_sha
  build_sha=$(git -C "$fixture" rev-parse HEAD)
  cat > "$fixture/deploy/cloudflare/wrangler.jsonc" <<EOF
{ "containers": [{ "class_name": "RunnerContainer",
  // build-sha: $build_sha
  "image": "registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-runnercontainer@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }] }
EOF
  cat > "$fixture/deploy/cloudflare-fabricd/wrangler.jsonc" <<EOF
{ "containers": [{ "class_name": "FabricdContainer",
  // build-sha: $build_sha
  "image": "registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-fabricd-fabricdcontainer@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }] }
EOF
  printf '{ "containers": [] }\n' > "$fixture/deploy/cloudflare-canary/wrangler.jsonc"
  git -C "$fixture" add .
  git -C "$fixture" commit -qm pin-config
}

valid="$tmp/valid"
make_fixture "$valid"
"$checker" --repo "$valid" --strict > "$tmp/valid.out" || { cat "$tmp/valid.out"; fail "valid fixture rejected"; }
grep -q 'pins=2 red=0' "$tmp/valid.out" || { cat "$tmp/valid.out"; fail "valid fixture was vacuous"; }
echo 'PASS valid digest pins with recorded build SHA and narrow source paths'

newer="$tmp/newer"
cp -a "$valid" "$newer"
printf 'source mutation after image build\n' >> "$newer/deploy/runner/Dockerfile"
git -C "$newer" add deploy/runner/Dockerfile
git -C "$newer" commit -qm newer-runner-source
expect_red "$newer" 'newer source commit'

tag="$tmp/tag"
cp -a "$valid" "$tag"
sed -i.bak 's#@sha256:[0-9a-f]\{64\}#:#' "$tag/deploy/cloudflare/wrangler.jsonc"
rm -f "$tag/deploy/cloudflare/wrangler.jsonc.bak"
expect_red "$tag" 'mutable tag fallback'

missing="$tmp/missing"
cp -a "$valid" "$missing"
sed -i.bak '/build-sha:/d' "$missing/deploy/cloudflare/wrangler.jsonc"
rm -f "$missing/deploy/cloudflare/wrangler.jsonc.bak"
expect_red "$missing" 'missing build provenance'

empty="$tmp/empty"
make_fixture "$empty"
printf '{ "containers": [] }\n' > "$empty/deploy/cloudflare/wrangler.jsonc"
printf '{ "containers": [] }\n' > "$empty/deploy/cloudflare-fabricd/wrangler.jsonc"
git -C "$empty" add .
git -C "$empty" commit -qm empty-pins
expect_red "$empty" 'vacuous empty pin inventory'

echo 'PASS image-pin-freshness selftest: 5/5 cases'
