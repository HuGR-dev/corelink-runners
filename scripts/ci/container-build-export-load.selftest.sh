#!/usr/bin/env bash
# Hosted, credentialless contract test for the RunnerContainer disk strategy.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo="$(cd -- "$script_dir/../.." && pwd -P)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/runner-image-oci-fixture.XXXXXXXX")"
trap 'rm -rf -- "$fixture"' EXIT

workflow="$repo/.github/workflows/build-cf-container-images.yml"
builder="$script_dir/container-build-export-load.sh"
contract="$script_dir/verify_runner_image_disk_contract.py"
archive="$fixture/image.oci.tar"
metadata="$fixture/metadata.json"

python3 - "$archive" "$metadata" <<'PY'
import gzip
import hashlib
import io
import json
import sys
import tarfile

archive_path, metadata_path = sys.argv[1:]
ref = "corelink-spawn-worker-runnercontainer:" + "a" * 40
blobs = {}

def put(value):
    digest = "sha256:" + hashlib.sha256(value).hexdigest()
    blobs[digest] = value
    return digest

config = json.dumps({"architecture": "amd64", "os": "linux", "config": {"Env": ["CORELINK_NIGHTLY=nightly-2026-08-31"]}}, separators=(",", ":")).encode()
config_digest = put(config)
layer_tar = io.BytesIO()
with tarfile.open(fileobj=layer_tar, mode="w") as layer:
    payload = b"runner-image-test-layer"
    info = tarfile.TarInfo("opt/fixture")
    info.size = len(payload)
    layer.addfile(info, io.BytesIO(payload))
layer_raw = gzip.compress(layer_tar.getvalue(), mtime=0)
layer_digest = put(layer_raw)
manifest = json.dumps({"schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json", "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": config_digest, "size": len(config)}, "layers": [{"mediaType": "application/vnd.oci.image.layer.v1.tar+gzip", "digest": layer_digest, "size": len(layer_raw)}]}, separators=(",", ":")).encode()
manifest_digest = put(manifest)
index = json.dumps({"schemaVersion": 2, "manifests": [{"mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": manifest_digest, "size": len(manifest), "annotations": {"org.opencontainers.image.ref.name": ref}}]}, separators=(",", ":")).encode()
with tarfile.open(archive_path, "w") as output:
    for name, value in [("oci-layout", b'{"imageLayoutVersion":"1.0.0"}'), ("index.json", index)]:
        info = tarfile.TarInfo(name)
        info.size = len(value)
        output.addfile(info, io.BytesIO(value))
    for digest, value in blobs.items():
        name = "blobs/sha256/" + digest.split(":", 1)[1]
        info = tarfile.TarInfo(name)
        info.size = len(value)
        output.addfile(info, io.BytesIO(value))
with open(metadata_path, "w", encoding="utf-8") as metadata_file:
    json.dump({"containerimage.digest": manifest_digest, "containerimage.config.digest": config_digest, "containerimage.descriptor": {"digest": manifest_digest}}, metadata_file)
PY

python3 "$script_dir/verify_runner_image_oci_archive.py" \
  --archive "$archive" --metadata "$metadata" \
  --image-ref "corelink-spawn-worker-runnercontainer:$(printf 'a%.0s' {1..40})" \
  > "$fixture/receipt.json"
grep -q '"manifest_digest"' "$fixture/receipt.json"
grep -q '"uncompressed_layer_bytes"' "$fixture/receipt.json"
echo 'PASS OCI archive digest and expanded-layer receipt'

if python3 "$script_dir/verify_runner_image_oci_archive.py" \
  --archive "$archive" --metadata "$metadata" --image-ref wrong:tag \
  > /dev/null 2>&1; then
  echo 'OCI archive verifier accepted a mismatched image ref' >&2
  exit 1
fi
echo 'PASS OCI archive verifier rejects mismatched image ref'

python3 "$contract" --workflow "$workflow" --builder "$builder"
mutated="$fixture/builder-without-prune.sh"
sed '/prune --all --force/d' "$builder" > "$mutated"
if python3 "$contract" --workflow "$workflow" --builder "$mutated"; then
  echo 'disk contract mutation removing BuildKit prune was not detected' >&2
  exit 1
fi
echo 'PASS disk contract mutation rejects missing cache eviction'

# The caller is the existing pull_request workflow on ubuntu-latest. Keep the
# production workflow manual-only and lint its YAML from that hosted job.
if [[ "${GITHUB_ACTIONS:-}" == true ]]; then
  go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.7
  "$(go env GOPATH)/bin/actionlint" "$workflow"
fi
