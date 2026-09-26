#!/usr/bin/env bash
# Export RunnerContainer as OCI, evict BuildKit cache, then import only when its
# measured archive/layer footprint fits the remaining box disk budget.
set -euo pipefail

usage() { echo 'usage: container-build-export-load.sh IMAGE TAG deploy/runner [--reserve-mb N]' >&2; }
if (($# < 3)); then usage; exit 2; fi
image=$1
tag=$2
context=$3
shift 3
reserve_mb=4096
while (($#)); do
  case "$1" in
    --reserve-mb)
      [[ $# -ge 2 ]] || { echo '--reserve-mb needs a value' >&2; exit 2; }
      reserve_mb=$2
      shift 2
      ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

[[ "$image" == corelink-spawn-worker-runnercontainer ]] || {
  echo 'only the RunnerContainer image is in this issue scope' >&2; exit 2;
}
[[ "$context" == deploy/runner ]] || {
  echo 'only deploy/runner is in this issue scope' >&2; exit 2;
}
[[ "$tag" =~ ^[0-9a-f]{40}$ ]] || {
  echo 'image tag must be the exact 40-character source SHA' >&2; exit 2;
}
if [[ -n "${GITHUB_SHA:-}" && "$tag" != "$GITHUB_SHA" ]]; then
  echo 'image tag must equal GITHUB_SHA' >&2
  exit 2
fi
[[ "$reserve_mb" =~ ^[0-9]+$ ]] || { echo 'reserve must be a non-negative MiB value' >&2; exit 2; }
(( reserve_mb >= 4096 )) || { echo 'minimum post-import disk reserve is 4096 MiB' >&2; exit 2; }

repo="$(git rev-parse --show-toplevel)"
context_path="$repo/$context"
[[ -f "$context_path/Dockerfile" && ! -L "$context_path/Dockerfile" ]] || {
  echo 'runner image Dockerfile is missing or unsafe' >&2; exit 1;
}
nightly_date="$(sed -n 's/^ARG NIGHTLY_DATE=//p' "$context_path/Dockerfile" | head -n1)"
[[ "$nightly_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || {
  echo 'could not resolve pinned nightly date from runner Dockerfile' >&2; exit 1;
}

runner_temp="${RUNNER_TEMP:-/tmp}"
[[ -d "$runner_temp" && ! -L "$runner_temp" ]] || {
  echo 'RUNNER_TEMP is missing or a symlink' >&2; exit 1;
}
temp_dir="$(mktemp -d "$runner_temp/corelink-runner-image.XXXXXXXX")"
archive="$temp_dir/runner-image.oci.tar"
metadata="$temp_dir/buildkit-metadata.json"
receipt="$temp_dir/archive-receipt.json"
image_ref="$image:$tag"
buildkit_addr="${BUILDKIT_HOST:-unix:///run/buildkit/buildkitd.sock}"
disk_guard="$repo/scripts/ci/container-build-disk-guard.sh"
archive_verifier="$repo/scripts/ci/verify_runner_image_oci_archive.py"

cleanup() {
  rm -rf -- "$temp_dir"
}
trap cleanup EXIT

# Start the already-provisioned daemonless services through the shim, then
# clear any prior image/cache state before beginning this measured build.
docker version >/dev/null
sudo buildctl --addr "$buildkit_addr" prune --all --force
bash "$disk_guard" --minimum-free-mb 8192

echo "runner-image-build: exporting OCI archive for source SHA $tag"
sudo buildctl --addr "$buildkit_addr" build \
  --frontend dockerfile.v0 \
  --local context="$context_path" \
  --local dockerfile="$context_path" \
  --opt filename=Dockerfile \
  --output "type=oci,name=$image_ref,dest=$archive,compression=gzip" \
  --metadata-file "$metadata"

# The archive is the only exported image representation. Drop BuildKit's
# intermediate snapshots/content before containerd unpacks the same layers.
sudo buildctl --addr "$buildkit_addr" prune --all --force
buildkit_bytes="$(sudo du -sb /var/lib/buildkit | awk '{print $1}')"
[[ "$buildkit_bytes" =~ ^[0-9]+$ ]] || {
  echo 'could not measure the post-prune BuildKit store' >&2; exit 1;
}
(( buildkit_bytes <= 268435456 )) || {
  echo "post-prune BuildKit store exceeds 256 MiB: ${buildkit_bytes} bytes" >&2; exit 1;
}

python3 "$archive_verifier" \
  --archive "$archive" --metadata "$metadata" --image-ref "$image_ref" \
  > "$receipt"
read -r total_kib free_kib < <(df -Pk "$archive" | awk 'NR == 2 {print $2, $4}')
[[ "$total_kib" =~ ^[0-9]+$ && "$free_kib" =~ ^[0-9]+$ ]] || {
  echo 'could not measure archive filesystem capacity' >&2; exit 1;
}
archive_device="$(df -Pk "$archive" | awk 'NR == 2 {print $1}')"
buildkit_device="$(sudo df -Pk /var/lib/buildkit | awk 'NR == 2 {print $1}')"
[[ -n "$archive_device" && "$archive_device" == "$buildkit_device" ]] || {
  echo 'archive and BuildKit store must share the measured disk budget' >&2; exit 1;
}
archive_bytes="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["archive_bytes"])' "$receipt")"
layers_bytes="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["uncompressed_layer_bytes"])' "$receipt")"
manifest_digest="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["manifest_digest"])' "$receipt")"
config_digest="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["config_digest"])' "$receipt")"
free_bytes=$((free_kib * 1024))
filesystem_bytes=$((total_kib * 1024))
(( filesystem_bytes >= 17179869184 && filesystem_bytes <= 21474836480 )) || {
  echo "::error::expected the 18 GB CoreLink box filesystem (16–20 GiB); measured ${filesystem_bytes} bytes" >&2
  exit 1
}
reserve_bytes=$((reserve_mb * 1024 * 1024))
required_import_bytes=$((layers_bytes + buildkit_bytes + reserve_bytes))
if ((free_bytes < required_import_bytes)); then
  echo "::error::refusing containerd import: free=${free_bytes} bytes, layers=${layers_bytes}, BuildKit=${buildkit_bytes}, reserve=${reserve_bytes}" >&2
  exit 1
fi

echo "runner-image-disk-budget: filesystem_bytes=${filesystem_bytes} archive_bytes=${archive_bytes} uncompressed_layer_bytes=${layers_bytes} buildkit_after_prune_bytes=${buildkit_bytes} free_before_import_bytes=${free_bytes} reserved_after_import_bytes=${reserve_bytes}"
docker load --input "$archive"
loaded_id="$(docker image inspect "$image_ref" --format '{{.Id}}')"
[[ "$loaded_id" == "$config_digest" || "$loaded_id" == "$manifest_digest" ]] || {
  echo "loaded image digest does not match BuildKit metadata: ${loaded_id}" >&2; exit 1;
}
docker image inspect "$image_ref" --format '{{range .Config.Env}}{{println .}}{{end}}' \
  | grep -Fxq "CORELINK_NIGHTLY=nightly-${nightly_date}" || {
    echo 'loaded image lost the pinned CORELINK_NIGHTLY runtime environment' >&2; exit 1;
  }

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "build_digest=${manifest_digest}"
    echo "config_digest=${config_digest}"
    echo "disk_archive_bytes=${archive_bytes}"
    echo "disk_filesystem_bytes=${filesystem_bytes}"
    echo "disk_uncompressed_layer_bytes=${layers_bytes}"
    echo "disk_buildkit_bytes=${buildkit_bytes}"
    echo "disk_free_before_import_bytes=${free_bytes}"
  } >> "$GITHUB_OUTPUT"
fi
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo '### RunnerContainer bounded build receipt'
    echo "- Source SHA: `$tag`"
    echo "- BuildKit manifest digest: `$manifest_digest`"
    echo "- BuildKit config digest, verified after import: `$config_digest`"
    echo "- OCI archive: `$archive_bytes` bytes; uncompressed layers: `$layers_bytes` bytes"
    echo "- Filesystem capacity: `$filesystem_bytes` bytes"
    echo "- BuildKit store after prune: `$buildkit_bytes` bytes"
    echo "- Free disk before import: `$free_bytes` bytes; required reserve after import: `$reserve_bytes` bytes"
  } >> "$GITHUB_STEP_SUMMARY"
fi
echo "runner-image-build: loaded $image_ref with BuildKit digest $manifest_digest"
