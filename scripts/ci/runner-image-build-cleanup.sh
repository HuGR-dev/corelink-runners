#!/usr/bin/env bash
# Release the local image and unused containerd/BuildKit state after each
# pushed image. `system prune` is supported by the baked nerdctl-backed docker
# shim; `builder prune` is a Docker-daemon command and is not.
set -euo pipefail

IMAGE_REF="${1:?image reference required}"
if docker image inspect "$IMAGE_REF" >/dev/null 2>&1; then
  docker image rm "$IMAGE_REF" >/dev/null
fi
docker system prune --all --force >/dev/null
echo "Runner image build cleanup: released ${IMAGE_REF} and builder cache"
