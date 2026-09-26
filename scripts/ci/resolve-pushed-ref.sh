#!/usr/bin/env bash
# resolve-pushed-ref.sh — verify the tag reported by `wrangler containers
# push`, then resolve its immutable registry descriptor for pinning.
#
# Wrangler reports the pushed registry URI with its mutable tag. Query the
# registry through Docker's verbose manifest inspection and use the returned
# descriptor digest; never infer a digest from progress text or the tag.
#
# The resolver emits only the immutable digest form
#     registry.cloudflare.com/<account>/<image>@sha256:<64hex>
# A missing, mismatched, or malformed remote descriptor is an error. Falling
# back to the tag would make a successful push look pin-ready while leaving
# subsequent deployments mutable, violating the X4 floor.
#
# Usage: resolve-pushed-ref.sh <image-name> <tag> <push-output-file>
set -euo pipefail

IMAGE="${1:?image name required}"
TAG="${2:?tag required}"
OUT="${3:?push-output file required}"
ACCOUNT="${CLOUDFLARE_ACCOUNT_ID:?CLOUDFLARE_ACCOUNT_ID must be set}"
DOCKER_BIN="${DOCKER_BIN:-docker}"

base="registry.cloudflare.com/${ACCOUNT}/${IMAGE}"
expected_ref="${base}:${TAG}"

if ! grep -Fqx "Pushed image: ${expected_ref}" "${OUT}" 2>/dev/null; then
  printf 'resolve-pushed-ref: push transcript did not identify the expected registry tag %s\n' \
    "${expected_ref}" >&2
  exit 1
fi

manifest_json="$("${DOCKER_BIN}" manifest inspect -v "${expected_ref}" 2>/dev/null)" || {
  printf 'resolve-pushed-ref: registry did not return a verifiable manifest for %s\n' \
    "${expected_ref}" >&2
  exit 1
}

digest="$(printf '%s' "${manifest_json}" | python3 -c '
import json
import re
import sys

try:
    descriptor = json.load(sys.stdin).get("Descriptor", {})
except (json.JSONDecodeError, AttributeError):
    raise SystemExit(1)
digest = descriptor.get("digest")
media_type = descriptor.get("mediaType")
allowed_media_types = {
    "application/vnd.oci.image.index.v1+json",
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.list.v2+json",
    "application/vnd.docker.distribution.manifest.v2+json",
}
if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
    raise SystemExit(1)
if not isinstance(media_type, str) or media_type not in allowed_media_types:
    raise SystemExit(1)
print(digest)
')" || {
  printf 'resolve-pushed-ref: registry manifest descriptor was missing a supported immutable sha256 digest for %s\n' \
    "${expected_ref}" >&2
  exit 1
}

printf '%s@%s\n' "${base}" "${digest}"
