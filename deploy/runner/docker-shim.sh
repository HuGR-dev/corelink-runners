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

_wait_sock() {
  i=0
  while [ ! -S "$1" ] && [ "$i" -lt 40 ]; do sleep 0.25; i=$((i + 1)); done
  [ -S "$1" ]
}

if [ ! -S /run/containerd/containerd.sock ]; then
  sudo sh -c 'containerd >/var/log/containerd.log 2>&1 &'
  _wait_sock /run/containerd/containerd.sock || {
    echo "docker-shim: containerd did not start" >&2
    sudo tail -n 20 /var/log/containerd.log >&2 2>/dev/null || true
    exit 1
  }
fi

if [ ! -S /run/buildkit/buildkitd.sock ]; then
  sudo sh -c 'buildkitd --addr unix:///run/buildkit/buildkitd.sock --oci-worker-snapshotter=overlayfs >/var/log/buildkitd.log 2>&1 &'
  _wait_sock /run/buildkit/buildkitd.sock || {
    echo "docker-shim: buildkitd did not start" >&2
    sudo tail -n 20 /var/log/buildkitd.log >&2 2>/dev/null || true
    exit 1
  }
fi

exec sudo \
  DOCKER_CONFIG="${DOCKER_CONFIG:-$HOME/.docker}" \
  BUILDKIT_HOST="unix:///run/buildkit/buildkitd.sock" \
  CONTAINERD_ADDRESS=/run/containerd/containerd.sock \
  nerdctl "$@"
