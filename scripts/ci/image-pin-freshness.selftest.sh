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
    "$fixture/.github/workflows" "$fixture/crates/corelink-check-exec-server/src" \
    "$fixture/crates/corelink-fabric-api/src" "$fixture/crates/corelink-runner/src" \
    "$fixture/crates/corelink-cloud-engine/src" \
    "$fixture/crates/corelink-fabric-server" "$fixture/crates/corelink-fabric" \
    "$fixture/crates/corelink-runners-contracts" "$fixture/.github/workflows"
  git -C "$fixture" init -q
  git -C "$fixture" config user.email selftest@example.invalid
  git -C "$fixture" config user.name image-pin-selftest
  printf 'runner source\n' > "$fixture/deploy/runner/Dockerfile"
  printf 'checkhost source\n' > "$fixture/deploy/check-host/Dockerfile"
  printf '[workspace]\nmembers = ["crates/*"]\nresolver = "2"\n' > "$fixture/Cargo.toml"
  printf '# fixture lockfile\n' > "$fixture/Cargo.lock"
  printf 'channel = "1.96.0"\n' > "$fixture/rust-toolchain.toml"
  printf 'ignore deploy and github\n' > "$fixture/.dockerignore"
  printf 'check exec server\n' > "$fixture/crates/corelink-check-exec-server/src/main.rs"
  printf 'fabric api\n' > "$fixture/crates/corelink-fabric-api/src/lib.rs"
  printf 'runner\n' > "$fixture/crates/corelink-runner/src/lib.rs"
  printf 'cloud engine\n' > "$fixture/crates/corelink-cloud-engine/src/lib.rs"
  printf 'fabric server\n' > "$fixture/crates/corelink-fabric-server/Dockerfile"
  printf 'fabric\n' > "$fixture/crates/corelink-fabric/lib.rs"
  printf 'contracts\n' > "$fixture/crates/corelink-runners-contracts/lib.rs"
  printf 'workflow\n' > "$fixture/.github/workflows/build-cf-container-images.yml"
  printf 'fabricd workflow\n' > "$fixture/.github/workflows/build-fabricd-image.yml"
  git -C "$fixture" add .
  git -C "$fixture" commit -qm source-build
  local build_sha
  build_sha=$(git -C "$fixture" rev-parse HEAD)
  local runner_digest fabricd_digest
  runner_digest="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  fabricd_digest="sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  cat > "$fixture/deploy/cloudflare/wrangler.jsonc" <<EOF
{ "containers": [{ "class_name": "RunnerContainer",
  // build-sha: $build_sha
  // image-provenance: digest=$runner_digest build-sha=$build_sha
  "image": "registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-runnercontainer@$runner_digest" }] }
EOF
  cat > "$fixture/deploy/cloudflare-fabricd/wrangler.jsonc" <<EOF
{ "containers": [{ "class_name": "FabricdContainer",
  // build-sha: $build_sha
  // image-provenance: digest=$fabricd_digest build-sha=$build_sha
  "image": "registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-fabricd-fabricdcontainer@$fabricd_digest" }] }
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

closure="$tmp/closure"
cp -a "$valid" "$closure"
printf 'transitive workspace mutation after image build\n' >> "$closure/crates/corelink-fabric-api/src/lib.rs"
git -C "$closure" add crates/corelink-fabric-api/src/lib.rs
git -C "$closure" commit -qm newer-fabricd-transitive-source
expect_red "$closure" 'transitive fabricd closure source'

lock="$tmp/lock"
cp -a "$valid" "$lock"
printf 'lockfile mutation after image build\n' >> "$lock/Cargo.lock"
git -C "$lock" add Cargo.lock
git -C "$lock" commit -qm newer-checkhost-lockfile
expect_red "$lock" 'workspace lockfile closure source'

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
sed -i.bak '/image-provenance:/d' "$missing/deploy/cloudflare/wrangler.jsonc"
rm -f "$missing/deploy/cloudflare/wrangler.jsonc.bak"
expect_red "$missing" 'missing build provenance'

wrong_digest="$tmp/wrong-digest"
cp -a "$valid" "$wrong_digest"
sed -i.bak '/image-provenance:/s/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc/' \
  "$wrong_digest/deploy/cloudflare/wrangler.jsonc"
rm -f "$wrong_digest/deploy/cloudflare/wrangler.jsonc.bak"
expect_red "$wrong_digest" 'digest provenance mismatch'

wrong_sha="$tmp/wrong-sha"
cp -a "$valid" "$wrong_sha"
wrong_sha_value=$(git -C "$wrong_sha" rev-parse HEAD~1)
wrong_sha_replacement=$(git -C "$wrong_sha" rev-parse HEAD)
sed -i.bak "/image-provenance:/s/build-sha=$wrong_sha_value/build-sha=$wrong_sha_replacement/" \
  "$wrong_sha/deploy/cloudflare/wrangler.jsonc"
rm -f "$wrong_sha/deploy/cloudflare/wrangler.jsonc.bak"
expect_red "$wrong_sha" 'build SHA provenance mismatch'

report_only="$tmp/report-only"
cp -a "$valid" "$report_only"
sed -i.bak '/image-provenance:/d' "$report_only/deploy/cloudflare/wrangler.jsonc"
rm -f "$report_only/deploy/cloudflare/wrangler.jsonc.bak"
set +e
report_output=$("$checker" --repo "$report_only" 2>&1)
report_status=$?
set -e
((report_status == 0)) || { printf '%s\n' "$report_output"; fail "report-only mode unexpectedly failed"; }
grep -q 'mode=report-only' <<<"$report_output" || { printf '%s\n' "$report_output"; fail "report-only mode was not reported"; }
echo 'PASS default report-only mode remains non-blocking'

empty="$tmp/empty"
make_fixture "$empty"
printf '{ "containers": [] }\n' > "$empty/deploy/cloudflare/wrangler.jsonc"
printf '{ "containers": [] }\n' > "$empty/deploy/cloudflare-fabricd/wrangler.jsonc"
git -C "$empty" add .
git -C "$empty" commit -qm empty-pins
expect_red "$empty" 'vacuous empty pin inventory'

echo 'PASS image-pin-freshness selftest: 10/10 cases'
