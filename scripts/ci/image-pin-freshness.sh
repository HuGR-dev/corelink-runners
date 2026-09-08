#!/usr/bin/env bash
# image-pin-freshness.sh — offline X4 image-pin freshness tripwire.
#
# The check is deliberately report-only by default.  T2-W2b is the owner of
# making this a blocking CI gate after the first repaired image is published.
# Use --strict in a release/fixture gate when a red report must fail.
#
# A pin is fresh only when all of these are true:
#   * it is an exact registry.cloudflare.com ref with @sha256:<64 lowercase hex>;
#   * the surrounding wrangler entry records the source/build commit SHA; and
#   * the entry has an image-provenance comment binding that full 40-char SHA to
#     the exact pinned digest (for example, `digest=sha256:… build-sha=…`); and
#   * no commit after that build SHA touched the image's declared narrow source
#     paths.  The paths are intentionally not the whole repository: the
#     fabricd Dockerfile's COPY . . does not mean every documentation edit
#     requires a rebuild.
#
# No registry, Docker, Cloudflare, or network operation is performed here.
set -euo pipefail

readonly ACCOUNT="6a1fc1c626fc2628823e60b9db01f5cd"
readonly CONTAINER_WORKFLOW=".github/workflows/build-cf-container-images.yml"
readonly FABRICD_WORKFLOW=".github/workflows/build-fabricd-image.yml"

usage() {
  cat <<'EOF'
usage: image-pin-freshness.sh [--repo DIR] [--strict]
       image-pin-freshness.sh --self-test

Default mode prints one report for every configured Cloudflare container image
and exits zero even when the report is RED (T2-W2a is report-only).
--strict makes any RED result exit non-zero; --self-test is always strict.
EOF
}

repo=""
strict=0
self_test=0
while (($#)); do
  case "$1" in
    --repo) [[ $# -ge 2 ]] || { echo "--repo needs a directory" >&2; exit 2; }; repo=$2; shift 2 ;;
    --strict) strict=1; shift ;;
    --self-test) self_test=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if ((self_test)); then
  exec "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/image-pin-freshness.selftest.sh"
fi

if [[ -z "$repo" ]]; then
  repo="$(git rev-parse --show-toplevel)"
fi
repo="$(cd -- "$repo" && pwd)"

declare -a CONFIGS=(
  "deploy/cloudflare/wrangler.jsonc"
  "deploy/cloudflare-fabricd/wrangler.jsonc"
  "deploy/cloudflare-canary/wrangler.jsonc"
)

# image-name -> narrow, declared source paths.  Keep these lists explicit and
# reviewable.  A workflow change is included because it changes the build
# context/recipe even when the Dockerfile is untouched.  CheckHost's image
# context is deploy/check-host, but its binary is compiled by Cargo from the
# repository workspace first; the workspace manifests, lockfile, and package
# therefore belong to the closure too.  Fabricd's Dockerfile uses the root
# context and COPY . ., with .dockerignore selecting the effective inputs; the
# complete crates tree is intentional here because Cargo resolves all workspace
# members and fabricd reaches path dependencies transitively.
declare -a RUNNER_PATHS=("deploy/runner" "$CONTAINER_WORKFLOW")
declare -a CHECKHOST_PATHS=(
  "deploy/check-host"
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "crates/corelink-check-exec-server"
  "$CONTAINER_WORKFLOW"
)
declare -a DEVENVD_PATHS=(
  "deploy/cloudflare/Dockerfile.runner-devenv"
  "deploy/cloudflare/entrypoint.sh"
  "deploy/cloudflare/supervisord.conf"
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "crates"
  "$CONTAINER_WORKFLOW"
)
declare -a FABRICD_PATHS=(
  ".dockerignore"
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "crates"
  "crates/corelink-fabric-server/Dockerfile"
  "$FABRICD_WORKFLOW"
)

paths_for_image() {
  case "$1" in
    corelink-spawn-worker-runnercontainer) printf '%s\n' "${RUNNER_PATHS[@]}" ;;
    corelink-spawn-worker-checkhostcontainer) printf '%s\n' "${CHECKHOST_PATHS[@]}" ;;
    corelink-runner-devenv) printf '%s\n' "${DEVENVD_PATHS[@]}" ;;
    corelink-fabricd-fabricdcontainer) printf '%s\n' "${FABRICD_PATHS[@]}" ;;
    *) return 1 ;;
  esac
}

pin_image_name() {
  local ref=$1 repo_name=${1%@*}
  [[ "$ref" == *"@sha256:"* ]] || { printf '%s\n' ""; return; }
  printf '%s\n' "${repo_name##*/}"
}

source_build_sha() {
  local file=$1 line=$2 before start
  ((line > 1)) || { printf '%s\n' ""; return; }
  # Start at this container entry's class_name.  Without this boundary a
  # neighboring entry's old provenance comment could accidentally certify a
  # pin that has no build record of its own (especially CheckHostContainer).
  start="$(awk -v limit="$line" 'NR < limit && /"class_name"[[:space:]]*:/ {last=NR} END {print last + 1}' "$file")"
  [[ "$start" -ge 1 && "$start" -lt "$line" ]] || { printf '%s\n' ""; return; }
  before="$(sed -n "${start},$((line - 1))p" "$file")"
  # Only accept an explicitly keyed build/git/tag SHA from the entry's own
  # nearby comments.  Do not treat the image digest or an arbitrary prose hash
  # as build provenance.
  printf '%s\n' "$before" \
    | grep -Eiv 'image[-_[:space:]]provenance' \
    | grep -Eio '(build[-_ ]sha|git[-_ ]sha|git[[:space:]]+commit|tag)[[:space:]:=]+[0-9a-f]{40}([^0-9a-f]|$)' \
    | grep -Eio '[0-9a-f]{40}' | tail -n 1 || true
}

entry_comments() {
  local file=$1 line=$2 start
  ((line > 1)) || return 0
  start="$(awk -v limit="$line" 'NR < limit && /"class_name"[[:space:]]*:/ {last=NR} END {print last + 1}' "$file")"
  [[ "$start" -ge 1 && "$start" -lt "$line" ]] || return 0
  sed -n "${start},$((line - 1))p" "$file"
}

provenance_digest() {
  local file=$1 line=$2 comment digest
  while IFS= read -r comment; do
    # The explicit image-provenance record is deliberately required to bind
    # this pin's digest to its recorded build SHA.  Accept either the digest
    # alone or a complete registry ref, but never infer it from prose.
    if [[ "$comment" =~ image[-_[:space:]]provenance ]] &&
      [[ "$comment" =~ (digest|image[-_[:space:]]digest)[=:][[:space:]]*([^[:space:]]+) ]]; then
      digest="${BASH_REMATCH[2]}"
      printf '%s\n' "$digest"
    fi
  done < <(entry_comments "$file" "$line")
}

provenance_build_sha() {
  local file=$1 line=$2 comment sha
  while IFS= read -r comment; do
    if [[ "$comment" =~ image[-_[:space:]]provenance ]] &&
      [[ "$comment" =~ build[-_[:space:]]sha[=:][[:space:]]*([0-9A-Fa-f]{40})([^0-9A-Fa-f]|$) ]]; then
      sha="${BASH_REMATCH[1]}"
      printf '%s\n' "$sha"
    fi
  done < <(entry_comments "$file" "$line")
}

red=0
pins=0
printf 'image-pin-freshness: repo=%s mode=%s\n' "$repo" "$([[ $strict -eq 1 ]] && echo strict || echo report-only)"

for rel in "${CONFIGS[@]}"; do
  file="$repo/$rel"
  if [[ ! -f "$file" ]]; then
    printf 'RED config=%s reason=missing-config\n' "$rel"
    red=1
    continue
  fi

  mapfile -t image_lines < <(grep -nE '"image"[[:space:]]*:' "$file" || true)
  for numbered in "${image_lines[@]}"; do
    line=${numbered%%:*}
    raw=${numbered#*:}
    ref=$(printf '%s\n' "$raw" | sed -nE 's/.*"image"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p')
    [[ -n "$ref" ]] || continue
    pins=$((pins + 1))
    name=$(pin_image_name "$ref")
    reasons=()
    if [[ ! "$ref" =~ ^registry\.cloudflare\.com/${ACCOUNT}/[^@[:space:]]+@sha256:[0-9a-f]{64}$ ]]; then
      reasons+=("not-an-exact-cloudflare-digest")
    fi
    if [[ -z "$name" ]]; then
      reasons+=("unrecognised-image-ref")
    else
      mapfile -t declared_paths < <(paths_for_image "$name" || true)
      if ((${#declared_paths[@]} == 0)); then
        reasons+=("no-declared-source-path-list")
      fi
    fi

    build_sha_raw=$(source_build_sha "$file" "$line")
    provenance_digest=$(provenance_digest "$file" "$line" | tail -n 1)
    provenance_sha_raw=$(provenance_build_sha "$file" "$line" | tail -n 1)
    build_sha="$build_sha_raw"
    if [[ -z "$build_sha" ]]; then
      reasons+=("missing-recorded-build-sha")
    elif ! git -C "$repo" cat-file -e "${build_sha}^{commit}" 2>/dev/null; then
      reasons+=("recorded-build-sha-not-in-repository")
    elif ! git -C "$repo" merge-base --is-ancestor "$build_sha" HEAD; then
      reasons+=("recorded-build-sha-not-ancestor")
    else
      build_sha="$(git -C "$repo" rev-parse "${build_sha}^{commit}")"
      if [[ -z "$provenance_digest" || -z "$provenance_sha_raw" ]]; then
        reasons+=("missing-digest-build-provenance")
      else
        pin_digest="${ref##*@}"
        provenance_digest_value="${provenance_digest##*@}"
        if [[ "$provenance_digest_value" != "$pin_digest" ]]; then
          reasons+=("provenance-digest-mismatch")
        fi
        if ! provenance_sha="$(git -C "$repo" rev-parse "${provenance_sha_raw}^{commit}" 2>/dev/null)" ||
          [[ "$provenance_sha" != "$build_sha" ]]; then
          reasons+=("provenance-build-sha-mismatch")
        fi
      fi
      for source_path in "${declared_paths[@]}"; do
        if ! git -C "$repo" ls-files --error-unmatch -- "$source_path" >/dev/null 2>&1; then
          reasons+=("undeclared-or-untracked-source-path:$source_path")
          continue
        fi
        newer=$(git -C "$repo" log -1 --format=%H "${build_sha}..HEAD" -- "$source_path" || true)
        if [[ -n "$newer" ]]; then
          reasons+=("source-newer-than-build:$source_path:$newer")
        fi
      done
    fi

    if ((${#reasons[@]})); then
      printf 'RED file=%s line=%s image=%s build_sha=%s reason=%s\n' \
        "$rel" "$line" "${ref%%@*}" "${build_sha:-<missing>}" "$(IFS=,; echo "${reasons[*]}")"
      red=1
    else
      printf 'GREEN file=%s line=%s image=%s build_sha=%s source_paths=%s\n' \
        "$rel" "$line" "${ref%%@*}" "$build_sha" "$(IFS=,; echo "${declared_paths[*]}")"
    fi
  done
done

if ((pins == 0)); then
  printf 'RED reason=no-configured-container-images\n'
  red=1
fi
printf 'image-pin-freshness: pins=%d red=%d\n' "$pins" "$red"
if ((strict && red)); then
  exit 1
fi
exit 0
