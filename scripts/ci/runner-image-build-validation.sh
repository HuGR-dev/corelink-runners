#!/usr/bin/env bash
# runner-image-build-validation.sh — build changed runner-image inputs on PRs.
#
# This is deliberately a build-only path. It is invoked from a pull_request
# workflow, where the checkout is untrusted, so it receives no repository or
# Cloudflare secrets and never invokes a registry publisher or deploy tool.
set -euo pipefail

usage() {
  cat <<'EOF'
usage: runner-image-build-validation.sh --base SHA --head SHA [--repo DIR]

Build the runner image locally when the PR changes one of its inputs. The
image is tagged only in the local Docker store and is removed on exit.
EOF
}

repo=""
base=""
head=""
while (($#)); do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || { echo '--repo needs a directory' >&2; exit 2; }
      repo=$2
      shift 2
      ;;
    --base)
      [[ $# -ge 2 ]] || { echo '--base needs a SHA' >&2; exit 2; }
      base=$2
      shift 2
      ;;
    --head)
      [[ $# -ge 2 ]] || { echo '--head needs a SHA' >&2; exit 2; }
      head=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[[ -n "$base" && -n "$head" ]] || { usage >&2; exit 2; }
if [[ -z "$repo" ]]; then
  repo="$(git rev-parse --show-toplevel)"
fi
repo="$(cd -- "$repo" && pwd -P)"
repo="$(git -C "$repo" rev-parse --show-toplevel)"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
impact_output="$(bash "$script_dir/image-build-impact.sh" --repo "$repo" --base "$base" --head "$head")"

# Whitelist the detector's fixed image identifier. Changed paths are data and
# must never become shell syntax, options, or a command argument outside the
# fixed context below.
if ! grep -q '^IMAGE_BUILD_REQUIRED image=corelink-spawn-worker-runnercontainer ' <<<"$impact_output"; then
  printf '%s\n' "$impact_output"
  echo 'RUNNER_IMAGE_BUILD_NOT_REQUIRED'
  exit 0
fi

context="$repo/deploy/runner"
dockerfile="$context/Dockerfile"
if [[ ! -d "$context" || -L "$context" || ! -f "$dockerfile" || -L "$dockerfile" ]]; then
  echo 'runner-image-build-validation: unsafe or missing deploy/runner context' >&2
  exit 1
fi
if [[ -n "$(find -P "$context" -type l -print -quit)" ]]; then
  echo 'runner-image-build-validation: symlink found in deploy/runner context' >&2
  exit 1
fi

run_id="${GITHUB_RUN_ID:-local}"
attempt="${GITHUB_RUN_ATTEMPT:-0}"
[[ "$run_id" =~ ^[[:alnum:]_.-]+$ && "$attempt" =~ ^[[:alnum:]_.-]+$ ]] || {
  echo 'runner-image-build-validation: invalid workflow identity' >&2
  exit 1
}
tag="corelink-runner-pr-${run_id}-${attempt}"

cleanup() {
  # The tag is generated locally above and is never a path or a registry ref.
  docker image rm --force "$tag" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "RUNNER_IMAGE_BUILD_REQUIRED context=deploy/runner"
echo "Building local validation image ${tag} (no push/deploy)"

# Explicitly remove common credential variables in case a local caller has
# them set. The pull_request workflow does not provide them, and no build
# secret or registry output is configured here.
env -u GITHUB_TOKEN -u GH_TOKEN -u CLOUDFLARE_API_TOKEN \
  DOCKER_BUILDKIT=1 docker build --pull=false --tag "$tag" "$context"
echo "RUNNER_IMAGE_BUILD_PASSED tag=${tag}"
