#!/usr/bin/env bash
# Fail-closed preflight for the local container-image build lane.
#
# The runner image build can exhaust the containerd filesystem while importing
# the exported layers. Refuse to start a build when the measured filesystem
# budget is already below the conservative floor; this is a bound, not a claim
# that a successful preflight proves the build will fit.
set -euo pipefail

MIN_FREE_MB="${RUNNER_IMAGE_MIN_FREE_MB:-8192}"
DISK_ROOT="${RUNNER_IMAGE_DISK_ROOT:-/var/lib/containerd}"

if ! [[ "$MIN_FREE_MB" =~ ^[0-9]+$ ]] || [ "$MIN_FREE_MB" -lt 1 ]; then
  echo "::error::RUNNER_IMAGE_MIN_FREE_MB must be a positive integer" >&2
  exit 1
fi

if [ ! -d "$DISK_ROOT" ]; then
  echo "::warning::$DISK_ROOT is absent; measuring the runner filesystem instead"
  DISK_ROOT=/
fi

available_mb="$(df -Pm "$DISK_ROOT" | awk 'NR == 2 { print $4 }')"
if ! [[ "$available_mb" =~ ^[0-9]+$ ]]; then
  echo "::error::could not measure free space at $DISK_ROOT" >&2
  exit 1
fi
if [ "$available_mb" -lt "$MIN_FREE_MB" ]; then
  echo "::error::insufficient image-build disk: ${available_mb} MiB free at ${DISK_ROOT}; need ${MIN_FREE_MB} MiB" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "::error::docker shim is absent; refusing to start an image build" >&2
  exit 1
fi
docker version >/dev/null 2>&1 || {
  echo "::error::docker shim/containerd is unavailable; refusing to start an image build" >&2
  exit 1
}

echo "Runner image build preflight: ${available_mb} MiB free at ${DISK_ROOT}; floor ${MIN_FREE_MB} MiB"
