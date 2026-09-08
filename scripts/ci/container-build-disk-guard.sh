#!/usr/bin/env bash
# container-build-disk-guard.sh — bound the local containerd image store.
#
# Wrangler's locked Containers build path requires a locally loaded image before
# its authenticated push. The ephemeral runner has a finite disk allocation, so
# image publication must remove the exact temporary tag and prune unreferenced
# BuildKit/containerd content before the next image starts.
set -euo pipefail

minimum_free_mb=4096
prune_only=0
image=""
while (($#)); do
  case "$1" in
    --minimum-free-mb)
      [[ $# -ge 2 ]] || { echo '--minimum-free-mb needs a value' >&2; exit 2; }
      minimum_free_mb=$2
      shift 2
      ;;
    --image)
      [[ $# -ge 2 ]] || { echo '--image needs a reference' >&2; exit 2; }
      image=$2
      shift 2
      ;;
    --prune-only)
      prune_only=1
      shift
      ;;
    -h|--help)
      echo 'usage: container-build-disk-guard.sh [--minimum-free-mb N] [--image REF] [--prune-only]'
      exit 0
      ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$minimum_free_mb" =~ ^[0-9]+$ ]] || {
  echo "minimum free space must be a non-negative integer MiB: $minimum_free_mb" >&2
  exit 2
}
if (( prune_only )) && [[ -z "$image" ]]; then
  echo '--image is required with --prune-only' >&2
  exit 2
fi

if [[ -n "$image" ]]; then
  # The image is an exact CI-generated tag supplied by the caller; never use a
  # broad glob or a repository/workspace path as a cleanup target.
  docker image rm --force "$image" >/dev/null 2>&1 || true
fi

# This runs on the disposable CoreLink build lease. The image store is not a
# customer workspace and pruning is required to release unreferenced layers.
docker system prune --all --force --volumes >/dev/null

free_kib="$(df -Pk / | awk 'NR == 2 { print $4 }')"
[[ "$free_kib" =~ ^[0-9]+$ ]] || { echo 'could not read free disk space' >&2; exit 1; }
minimum_kib=$((minimum_free_mb * 1024))
if ((free_kib < minimum_kib)); then
  echo "::error::container build lease has ${free_kib} KiB free; required ${minimum_kib} KiB" >&2
  exit 1
fi
echo "container-build-disk-guard: ${free_kib} KiB free (minimum ${minimum_kib} KiB)"
