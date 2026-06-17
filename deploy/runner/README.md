# CoreLink Ephemeral GitHub Actions Runner Image

ADR-0007 Stage A — the runner image the CoreLink fabric provisions per queued
GitHub Actions job.

---

## What this image is

A cache-warm, one-shot GitHub Actions runner that:

1. Receives a **JIT config token** at provision time via the `CORELINK_RUNNER_JITCONFIG`
   environment variable.
2. Starts the official GitHub Actions runner agent in `--ephemeral` JIT mode
   (`./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG"`).
3. Executes exactly **one job** from the customer's unmodified workflow.
4. **Self-deregisters** and exits cleanly; the fabric tears down the microVM.

No credential, secret, or runner registration is baked into the image.  The JIT
config is the only per-job credential, and it is one-time-use.

---

## Contents

| Component | Version | Notes |
|---|---|---|
| Base OS | ubuntu:24.04 | Digest-pinned (`@sha256:…`) — see below |
| GitHub Actions runner | v2.335.1 | SHA-256 verified at download time; must track a currently-supported release (GitHub deprecates old runners) |
| Rust toolchain | 1.96.0 | Matches `rust-toolchain.toml` |
| rustfmt | (toolchain component) | Gate: `cargo fmt --check` |
| clippy | (toolchain component) | Gate: `cargo clippy -D warnings` |
| cargo-deny | 0.19.8 | Matches CI (`taiki-e/install-action`) |
| cargo-audit | 0.22.2 | Matches CI (`taiki-e/install-action`) |
| git | distro package | Required by `actions/checkout` |

The Rust toolchain is included so that this repo's own CI gate (the dogfood use
case) runs warm on the first job without downloading the toolchain.

---

## `$CORELINK_RUNNER_JITCONFIG` contract

| Property | Value |
|---|---|
| **Name** | `CORELINK_RUNNER_JITCONFIG` |
| **Provider** | CoreLink fabric — injected at container provision time |
| **Contents** | Base64-encoded JIT config blob from the GitHub Actions API (`actions/generateRunnerJitconfig`) |
| **Lifetime** | Single-use; valid for one job registration; expires after the job completes |
| **If absent** | Container exits with code 1 and a clear error message — never idles |
| **Never echoed** | The entrypoint passes it as an argument; the value is never printed |

The fabric obtains this token via the GitHub API before provisioning the box and
injects it via the container runtime's env mechanism (never via a file, never
baked into the image).

---

## How the fabric uses this image

```
RunnerLease {
  image: "ghcr.io/humangr-labs/corelink-runner@sha256:<digest>",  // X4: pinned
  env: {
    "CORELINK_RUNNER_JITCONFIG": "<jit-config-token>",            // per-job credential
  },
  ephemeral: true,
}
```

The fabric always pins the image by digest in the `RunnerLease.image` field.
Using a floating tag (`latest`) in production violates the X4 supply-chain
floor — always pin.

---

## X4 supply-chain: digest pinning requirement

All `FROM` lines in the Dockerfile are `@sha256:`-digest-pinned (PR #75), and
`build-and-push.sh` fails closed if any `FROM` is ever left unpinned. A digest
pin is intentionally frozen — that is the X4 guarantee.

To **refresh** the pinned base (e.g. for a base-image security update),
re-resolve the current digest:

```sh
# Resolve ubuntu:24.04 digest (amd64):
docker buildx imagetools inspect ubuntu:24.04 \
  --format '{{json .Manifest.Manifests}}' \
  | jq -r '.[] | select(.Platform.Architecture=="amd64") | .Digest'

# Or the multi-arch manifest digest:
docker buildx imagetools inspect ubuntu:24.04 \
  --format '{{json .Manifest}}' | jq -r '.digest'
```

Replace both `@sha256:` digests in `Dockerfile` with that digest, commit the
change, and record the new digest in the build log.

---

## Building and pushing

```sh
# 1. (Only when refreshing the base) re-resolve the digest — see X4 section.

# 2. Log in to GHCR:
echo "$GHCR_PAT" | docker login ghcr.io -u "$GITHUB_ACTOR" --password-stdin

# 3. Build and push:
export REGISTRY=ghcr.io
export IMAGE=humangr-labs/corelink-runner
export TAG=2.335.1-rust1.96.0   # recommended: encode runner + Rust versions
./deploy/runner/build-and-push.sh
```

The script prints the resulting `@sha256:` digest.  Record it and pin it
in the fabric's runner-lease config.

### Overridable env vars for `build-and-push.sh`

| Variable | Default | Notes |
|---|---|---|
| `REGISTRY` | `ghcr.io` | Container registry host |
| `IMAGE` | `humangr-labs/corelink-runner` | Image name (no tag) |
| `TAG` | `latest` | Image tag |
| `PLATFORM` | `linux/amd64` | Build platform |

No credentials are hard-coded.  Auth is assumed from `docker login` before the
script runs.

---

## Updating the runner version

1. Find the new release at <https://github.com/actions/runner/releases>.
2. Download the `actions-runner-linux-x64-<VERSION>.tar.gz` asset and get its
   SHA-256:
   ```sh
   sha256sum actions-runner-linux-x64-<VERSION>.tar.gz
   ```
3. Update `RUNNER_VERSION` and `RUNNER_SHA256` in `Dockerfile`.
4. Re-resolve the base image digest (it may have moved) and update both
   `@sha256:` occurrences in `Dockerfile`.
5. Build, push, record the new image digest, pin it in the fabric.

---

## Security notes

- **No secrets in the image.** The JIT config is the only per-job credential
  and arrives at runtime via env injection.
- **Non-root runner.** The runner agent runs as the `runner` user (uid chosen
  by `useradd`).  Sudo is available for `actions/setup-*` tooling that needs it.
- **Ephemeral + self-deregistering.** After one job the runner removes itself
  from the pool; the fabric tears down the microVM.  There is no persistent
  runner state.
- **Digest-pinned base.** Both `FROM` lines are `@sha256:`-pinned and
  `build-and-push.sh` fails the build if any `FROM` is unpinned; any build from
  an unpinned base is non-compliant under X4.
