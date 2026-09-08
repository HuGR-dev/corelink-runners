#!/usr/bin/env bash
# image-build-impact.sh — offline PR detector for container build inputs.
#
# This command only compares Git paths. It never invokes Docker/BuildKit,
# Wrangler, a registry, or a credential. The PR workflow uses it to tell a
# reviewer which manually-dispatched image build is required after a context,
# Dockerfile, or build recipe change.
set -euo pipefail

usage() {
  cat <<'EOF'
usage: image-build-impact.sh --base SHA --head SHA [--repo DIR]

Print one line for every affected production image. Exit zero even when no
image is affected; this is a detector, not a publication gate.
EOF
}

repo=""
base=""
head=""
while (($#)); do
  case "$1" in
    --repo) [[ $# -ge 2 ]] || { echo "--repo needs a directory" >&2; exit 2; }; repo=$2; shift 2 ;;
    --base) [[ $# -ge 2 ]] || { echo "--base needs a SHA" >&2; exit 2; }; base=$2; shift 2 ;;
    --head) [[ $# -ge 2 ]] || { echo "--head needs a SHA" >&2; exit 2; }; head=$2; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ -n "$base" && -n "$head" ]] || { usage >&2; exit 2; }
if [[ -z "$repo" ]]; then
  repo="$(git rev-parse --show-toplevel)"
fi
repo="$(cd -- "$repo" && pwd)"

# Keep these closures in sync with image-pin-freshness.sh. A workflow is an
# input because changing build flags, context paths, or publication behavior
# changes the resulting image even when its Dockerfile is untouched.
# shellcheck disable=SC2034
declare -a RUNNER_PATHS=(
  "deploy/runner"
  ".github/workflows/build-cf-container-images.yml"
)
# shellcheck disable=SC2034
declare -a CHECKHOST_PATHS=(
  "deploy/check-host"
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "crates/corelink-check-exec-server"
  ".github/workflows/build-cf-container-images.yml"
)
# shellcheck disable=SC2034
declare -a DEVENVD_PATHS=(
  "deploy/cloudflare/Dockerfile.runner-devenv"
  "deploy/cloudflare/entrypoint.sh"
  "deploy/cloudflare/supervisord.conf"
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "crates"
  ".github/workflows/build-cf-container-images.yml"
)
# shellcheck disable=SC2034
declare -a FABRICD_PATHS=(
  ".dockerignore"
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "crates"
  ".github/workflows/build-fabricd-image.yml"
)

changed=()
# Use status-aware output and inspect BOTH sides of renames/copies. A Dockerfile
# moved out of its old tree still invalidates the old image pin, even when the
# destination is outside the ordinary context path list.
while IFS=$'\t' read -r change_status old_path new_path; do
  [[ -n "$old_path" ]] || continue
  changed+=("$old_path")
  if [[ "$change_status" =~ ^[RC] ]]; then
    [[ -n "$new_path" ]] && changed+=("$new_path")
  fi
done < <(git -C "$repo" diff --name-status --find-renames --find-copies "$base...$head")

matches_path() {
  local changed_path=$1 declared
  shift
  for declared in "$@"; do
    if [[ "$changed_path" == "$declared" || "$changed_path" == "$declared"/* ]]; then
      return 0
    fi
  done
  return 1
}

affected=0
for image_and_paths in \
  "corelink-spawn-worker-runnercontainer RUNNER_PATHS" \
  "corelink-spawn-worker-checkhostcontainer CHECKHOST_PATHS" \
  "corelink-runner-devenv DEVENVD_PATHS" \
  "corelink-fabricd-fabricdcontainer FABRICD_PATHS"; do
  read -r image path_var <<< "$image_and_paths"
  declare -n paths="$path_var"
  image_changed=()
  for path in "${changed[@]}"; do
    if matches_path "$path" "${paths[@]}"; then
      image_changed+=("$path")
    fi
  done
  if ((${#image_changed[@]})); then
    affected=1
    printf 'IMAGE_BUILD_REQUIRED image=%s changed=%s\n' "$image" "$(IFS=,; echo "${image_changed[*]}")"
  fi
done

if ((affected == 0)); then
  echo 'IMAGE_BUILD_NOT_REQUIRED changed_paths=0'
else
  echo 'image-build-impact: detector-only; no image build or publication was started'
fi
