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
    sudo sh -c 'buildkitd --addr unix:///run/buildkit/buildkitd.sock --oci-worker-snapshotter=overlayfs >/var/log/buildkitd.log 2>&1 &'
    _wait_sock /run/buildkit/buildkitd.sock || {
      echo "docker-shim: buildkitd did not start" >&2
      sudo tail -n 20 /var/log/buildkitd.log >&2 2>/dev/null || true
      return 1
    }
  fi
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
