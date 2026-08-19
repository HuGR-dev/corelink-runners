#!/bin/sh
# docker(1) drop-in shim for CoreLink runner leases.
#
# A customer job on `runs-on: corelink` runs its UNMODIFIED `docker build` /
# `docker login` / `docker push` steps as the non-root `runner` user. This shim,
# installed as `docker` on PATH, exec's `nerdctl` under `sudo`, LAZILY bringing up
# containerd + buildkitd on the FIRST docker invocation only — so a job that never
# touches docker (a pure Rust build, say) pays zero daemon cost.
#
# Why sudo (rootful) and not rootless: rootless containerd needs a systemd user
# session a one-shot CI lease does not have; the per-lease Firecracker microVM IS
# the isolation boundary (the same root-in-microVM model our own daemonless prod
# build already uses with `sudo buildctl`). The runner user has passwordless sudo.
#
# DOCKER_CONFIG is resolved in the USER's shell (before sudo) and forwarded, so
# the `docker login` -> build -> push credential chain shares one config file.
set -e

# ── `docker buildx` compatibility ────────────────────────────────────────────
# nerdctl exposes BuildKit through `nerdctl build`, NOT a `buildx` subcommand, so
# an unmodified `docker buildx build …` reaches `nerdctl buildx build …` and dies
# with "unknown shorthand flag: 't' in -t". Tooling that shells to buildx —
# notably `wrangler containers build` (used by build-cf-container-images.yml) —
# therefore cannot build on a CoreLink lease unless we translate the two forms it
# emits. Plain `docker build` already works here (containerd image store), so:
#   - `buildx build …`             -> `build …`     (drop the `buildx` word)
#   - `buildx imagetools inspect …`-> best-effort no-op: nerdctl has no imagetools;
#                                     our callers `|| true` and fall back to the
#                                     tag when the metadata query yields nothing.
#   - `buildx version`             -> nerdctl --version (cheap; no daemon needed)
#   - any other `buildx …`         -> pass the remainder through best-effort.
# Placed BEFORE the daemon bring-up so the no-op paths never spin up containerd.
if [ "${1:-}" = "buildx" ]; then
  shift
  case "${1:-}" in
    build) : ;;                                    # falls through: nerdctl build …
    imagetools) exit 0 ;;                          # best-effort; caller falls back
    version|--version) exec sudo nerdctl --version ;;
    *) : ;;                                        # best-effort passthrough
  esac
fi

# The daemons run as root and their sockets live in root-only dirs (/run/buildkit,
# /run/containerd), so the UNPRIVILEGED user cannot stat them — every socket check
# MUST go through `sudo test -S`, or it false-negatives while the daemon is fine.
_sock_up() { sudo test -S "$1"; }
_wait_sock() {
  i=0
  while ! _sock_up "$1" && [ "$i" -lt 120 ]; do sleep 0.5; i=$((i + 1)); done
  _sock_up "$1"
}

# Serialize the lazy start: two concurrent `docker` calls on a cold lease (e.g.
# `docker build & docker build & wait`) would otherwise BOTH try to spawn the
# daemons, and the loser's `containerd`/`buildkitd` fails to bind the fixed socket
# — harmless (the winner's socket serves both) but noisy. An flock on a per-lease
# lockfile makes exactly one invocation do the start; the rest wait then find the
# socket up. The lockfile lives in the single-tenant microVM's /tmp (safe here).
_ensure_daemons() {
  if ! _sock_up /run/containerd/containerd.sock; then
    sudo sh -c 'containerd >/var/log/containerd.log 2>&1 &'
    _wait_sock /run/containerd/containerd.sock || {
      echo "docker-shim: containerd did not start" >&2
      sudo tail -n 20 /var/log/containerd.log >&2 2>/dev/null || true
      return 1
    }
  fi
  if ! _sock_up /run/buildkit/buildkitd.sock; then
    # F3.2 WP-R: --config routes docker.io base-image layer pulls through the
    # CoreLink OCI mirror (deploy/runner/buildkitd.toml, baked at /etc/buildkit).
    # Absent the file (older image) buildkitd ignores the flag's target and runs
    # unmirrored — so this stays safe if the toml ever fails to bake.
    _bk_cfg=""
    if sudo test -f /etc/buildkit/buildkitd.toml; then
      _bk_cfg="--config /etc/buildkit/buildkitd.toml"
    fi
    sudo sh -c "buildkitd --addr unix:///run/buildkit/buildkitd.sock --oci-worker-snapshotter=overlayfs $_bk_cfg >/var/log/buildkitd.log 2>&1 &"
    _wait_sock /run/buildkit/buildkitd.sock || {
      echo "docker-shim: buildkitd did not start" >&2
      sudo tail -n 20 /var/log/buildkitd.log >&2 2>/dev/null || true
      return 1
    }
  fi
  # F3.2 WP-R: arm the CoreLink OCI mirror credential (best-effort, fail-open).
  _arm_mirror_auth || true
}

# _arm_mirror_auth — give buildkit a CoreLink OCI bearer so mirror pulls of
# allowlisted _public base layers authenticate. STRICTLY fail-open: any failure
# here MUST NOT break the build — on a miss/401/unreachable mirror, buildkit
# falls back to docker.io. Runs at most once per lease (flag file).
_arm_mirror_auth() {
  _flag=/tmp/corelink-mirror-login.done
  [ -f "$_flag" ] && return 0
  # Only meaningful when the CoreLink moat is armed for this lease.
  _pat=""
  if [ -n "${CLW_TOKEN:-}" ]; then
    _pat="$CLW_TOKEN"
  elif [ -n "${CLW_CRED_TICKET:-}" ] && [ -n "${CLW_LEASE_ID:-}" ] \
    && { [ -n "${CLW_FABRIC_ENDPOINT:-}" ] || [ -n "${CLW_ENDPOINT:-}" ]; } \
    && command -v curl >/dev/null 2>&1; then
    _fe="${CLW_FABRIC_ENDPOINT:-$CLW_ENDPOINT}"
    _resp="$(curl -sS -m 15 -X POST "$_fe/v1/leases/$CLW_LEASE_ID/cas-cred" \
      -H 'content-type: application/json' \
      -d "{\"ticket\":\"$CLW_CRED_TICKET\"}" 2>/dev/null || true)"
    # Extract .cas_pat without a JSON dep (grep/sed) — best-effort.
    _pat="$(printf '%s' "$_resp" | sed -n 's/.*"cas_pat"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
  fi
  if [ -z "$_pat" ]; then
    # No credential → do NOT touch docker config; unmirrored/anon pulls fall
    # through to docker.io. Mark done so we don't retry every invocation.
    : > "$_flag" 2>/dev/null || true
    return 0
  fi
  # Write auth into the SAME DOCKER_CONFIG the build exec uses (below).
  _dc="${DOCKER_CONFIG:-$HOME/.docker}"
  printf '%s' "$_pat" | sudo DOCKER_CONFIG="$_dc" nerdctl login corelink-api.humangr.com \
    -u x --password-stdin >/dev/null 2>&1 \
    && echo "docker-shim: CoreLink OCI mirror auth armed" >&2 \
    || echo "docker-shim: CoreLink OCI mirror auth unavailable — builds fall back to docker.io" >&2
  : > "$_flag" 2>/dev/null || true
  return 0
}

if command -v flock >/dev/null 2>&1; then
  # subshell holds the lock only during the start; released before the exec below
  ( flock -x 9 || exit 1; _ensure_daemons ) 9>/tmp/corelink-docker-shim.lock || exit 1
else
  _ensure_daemons || exit 1
fi

exec sudo \
  DOCKER_CONFIG="${DOCKER_CONFIG:-$HOME/.docker}" \
  BUILDKIT_HOST="unix:///run/buildkit/buildkitd.sock" \
  CONTAINERD_ADDRESS=/run/containerd/containerd.sock \
  nerdctl "$@"
