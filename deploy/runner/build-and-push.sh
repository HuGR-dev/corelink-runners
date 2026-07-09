#!/usr/bin/env bash
# build-and-push.sh — build and push the CoreLink ephemeral runner image (ADR-0007)
#
# ⚠️ LEGACY MANUAL PATH — NOT the live build. Cloudflare-first (ADR-0008): the
# LIVE runner image is built by `wrangler containers build` (from
# deploy/runner/Dockerfile) and pushed to the CF managed registry — the runtime
# image is
# `registry.cloudflare.com/<account>/corelink-spawn-worker-runnercontainer@sha256:…`
# (see deploy/cloudflare/wrangler.jsonc; CI: build-cf-container-images.yml). The
# runner runs on Cloudflare. This script exists ONLY for the ADR-0008 fallback
# substrate (Northflank), which cannot pull from the CF-internal registry and so
# needs a plain public OCI image. It is registry-NEUTRAL: `REGISTRY` is REQUIRED
# (no default) so nothing is ever pushed to a surprise host.
#
# USAGE:
#   export REGISTRY=registry.example.com   # REQUIRED — your OCI registry host
#   export IMAGE=corelink-runner           # image name (default below)
#   export TAG=latest                      # default: latest
#   export PLATFORM=linux/amd64            # default: linux/amd64
#   ./build-and-push.sh
#
# AUTHENTICATION:
#   You must be logged in to the target registry before running this script
#   (`docker login <REGISTRY> …`). No credentials are hard-coded in this script
#   or the Dockerfile.
#
# OUTPUT:
#   Prints the image digest (@sha256:…) after push.
#   Pin that digest in the fabric's runner-lease `image` field (X4 requirement).
#
# PREREQUISITES:
#   - Docker with buildx enabled (docker buildx version)
#   - Logged in to the target registry (see AUTHENTICATION above)
#   - Every FROM in the Dockerfile must be digest-pinned (@sha256:<64-hex>);
#     the guard below enforces this (X4). See Dockerfile / deploy/runner/README.md.
set -euo pipefail

# ── Configuration — override via env ─────────────────────────────────────────
# REGISTRY is REQUIRED (registry-neutral by design — no ghcr/default host).
REGISTRY="${REGISTRY:?set REGISTRY to your OCI registry host (this script is the ADR-0008 fallback-substrate builder; the LIVE image is built by build-cf-container-images.yml to the CF registry)}"
IMAGE="${IMAGE:-corelink-runner}"
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

# ── X4 floor: every FROM must be digest-pinned with a real 64-hex sha256 ──────
# Checks the FROM lines specifically (NOT comments), so a resolved pin passes
# cleanly and only a genuinely unpinned/placeholder base fails the build closed.
if grep -E '^[[:space:]]*FROM[[:space:]]' Dockerfile | grep -qvE '@sha256:[0-9a-f]{64}([[:space:]]|$)'; then
  echo "ERROR: a FROM line is not digest-pinned with a 64-hex sha256 (X4 floor)." >&2
  echo "       Offending FROM line(s):" >&2
  grep -nE '^[[:space:]]*FROM[[:space:]]' Dockerfile >&2
  echo "       Resolve the ubuntu:24.04 digest and pin BOTH FROM lines:" >&2
  echo "         docker buildx imagetools inspect ubuntu:24.04 \\" >&2
  echo "           --format '{{json .Manifest}}' | jq -r '.digest'" >&2
  echo "       A build from an unpinned base is NON-COMPLIANT under X4 — failing closed." >&2
  exit 1
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
