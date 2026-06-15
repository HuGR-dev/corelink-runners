#!/usr/bin/env bash
# build-and-push.sh — build and push the CoreLink ephemeral runner image (ADR-0007)
#
# USAGE:
#   export REGISTRY=ghcr.io          # default: ghcr.io
#   export IMAGE=humangr-labs/corelink-runner  # default: humangr-labs/corelink-runner
#   export TAG=latest                # default: latest
#   export PLATFORM=linux/amd64     # default: linux/amd64
#   ./build-and-push.sh
#
# AUTHENTICATION:
#   You must be logged in to the target registry before running this script.
#   For GHCR (default):
#     echo "$GHCR_PAT" | docker login ghcr.io -u "$GITHUB_ACTOR" --password-stdin
#   No credentials are hard-coded in this script or the Dockerfile.
#
# OUTPUT:
#   Prints the image digest (@sha256:…) after push.
#   Pin that digest in the fabric's runner-lease `image` field (X4 requirement).
#
# PREREQUISITES:
#   - Docker with buildx enabled (docker buildx version)
#   - Logged in to the target registry (see AUTHENTICATION above)
#   - The base image digest placeholder in Dockerfile must be resolved first
#     (see comments in Dockerfile / deploy/runner/README.md)
set -euo pipefail

# ── Configuration — override via env ─────────────────────────────────────────
REGISTRY="${REGISTRY:-ghcr.io}"
IMAGE="${IMAGE:-humangr-labs/corelink-runner}"
TAG="${TAG:-latest}"
PLATFORM="${PLATFORM:-linux/amd64}"

FULL_IMAGE="${REGISTRY}/${IMAGE}:${TAG}"

# ── Ensure we build from the directory containing the Dockerfile ──────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "==> Building CoreLink runner image"
echo "    Registry : ${REGISTRY}"
echo "    Image    : ${IMAGE}"
echo "    Tag      : ${TAG}"
echo "    Platform : ${PLATFORM}"
echo "    Full ref : ${FULL_IMAGE}"
echo ""

# ── Sanity: warn if the base digest placeholder has not been resolved ─────────
if grep -q '<PIN-AT-BUILD>' Dockerfile; then
  echo "WARNING: Dockerfile still contains <PIN-AT-BUILD> placeholder(s)." >&2
  echo "         Resolve the ubuntu:24.04 digest first:" >&2
  echo "           docker buildx imagetools inspect ubuntu:24.04 \\" >&2
  echo "             --format '{{json .Manifest}}' | jq -r '.digest'" >&2
  echo "         Replace every occurrence of @sha256:<PIN-AT-BUILD> in Dockerfile." >&2
  echo "" >&2
  echo "         Building anyway (will produce an unverifiable base) — in production" >&2
  echo "         treat an image built from an unpinned base as NON-COMPLIANT (X4)." >&2
  echo "" >&2
fi

# ── Build and push in one pass (avoids a second pull from the registry) ───────
# --provenance=false keeps the manifest simple (single-arch index).
# --sbom=false avoids an extra layer annotation that can confuse older clients.
# Adjust these flags if your registry supports SLSA provenance attestations.
DIGEST=$(docker buildx build \
  --platform "${PLATFORM}" \
  --tag "${FULL_IMAGE}" \
  --push \
  --provenance=false \
  --sbom=false \
  --file Dockerfile \
  --metadata-file /tmp/corelink-runner-build-metadata.json \
  . && \
  jq -r '."containerimage.digest"' /tmp/corelink-runner-build-metadata.json)

echo ""
echo "==> Push complete."
echo ""
echo "IMAGE DIGEST (pin this in the fabric's runner-lease image field):"
echo "  ${REGISTRY}/${IMAGE}@${DIGEST}"
echo ""
echo "Fully-qualified pinned reference:"
echo "  ${FULL_IMAGE} -> ${REGISTRY}/${IMAGE}@${DIGEST}"
