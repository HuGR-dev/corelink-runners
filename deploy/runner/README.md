# CoreLink Ephemeral GitHub Actions Runner Image

ADR-0007 Stage A — the runner image the CoreLink fabric provisions per queued
GitHub Actions job.

---

## What this image is

A cache-warm, one-shot GitHub Actions runner that:

1. Receives a **JIT config token** at provision time via the
   `CORELINK_RUNNER_JITCONFIG` environment variable in the short-lived initial
   entrypoint stage.
2. Seals it into a private mode-0600 bridge, removes the value-bearing
   environment, and cleanly re-execs the entrypoint. The pinned Python
   bootstrap opens that bridge without following symlinks, unlinks it
   immediately, and materializes the official runner config files mode 0600.
3. Starts `./run.sh` with zero arguments and without any JIT value or bridge
   pathname in its environment.
4. Executes exactly **one job** from the customer's unmodified workflow.
5. **Self-deregisters** and exits cleanly; the fabric tears down the microVM.

No credential, secret, or runner registration is baked into the image.  The JIT
config is the only per-job credential, and it is one-time-use.

---

## Contents

| Component | Version | Notes |
|---|---|---|
| Base OS | ubuntu:24.04 | Digest-pinned (`@sha256:…`) — see below |
| GitHub Actions runner | v2.335.1 | SHA-256 verified at download time; must track a currently-supported release (GitHub deprecates old runners) |
| Rust toolchain | 1.96.0 (default) + 1.91.1 | 1.96.0 = this repo's own dogfood CI (its `rust-toolchain.toml`); 1.91.1 baked alongside for corelink-server (ADR-0015 pin) so its `cargo` resolves warm with zero per-spawn download. Both carry rustfmt+clippy; 1.91.1 also carries rust-src + wasm32/musl targets |
| rustfmt | (toolchain component) | Gate: `cargo fmt --check` |
| clippy | (toolchain component) | Gate: `cargo clippy -D warnings` |
| cargo-deny | 0.19.8 | Matches CI (`taiki-e/install-action`) |
| cargo-audit | 0.22.2 | Matches CI (`taiki-e/install-action`) |
| sccache | 0.17.0 | Static musl binary, SHA-256 verified before extraction; inert unless the workflow sets `RUSTC_WRAPPER=sccache` + `SCCACHE_WEBDAV_*` |
| Node.js | 22.23.2 | linux-x64, SHA-256 verified against `SHASUMS256.txt`; `include/` headers stripped. Matches `node-version: 22` in corelink-server's workflows |
| npm / npx / corepack | (bundled with Node) | On `PATH` via `/usr/local/bin` symlinks |
| pnpm | 10.32.1 | npm-tarball form, SHA-256 verified; must match corelink-server's root `packageManager` exactly |
| git | distro package | Required by `actions/checkout` |
| python3 | distro package (stdlib, no pip) | Required by corelink-server's ~48 `validate_*` / OKF / secrets-matrix gates (stdlib scripts) — absent → `exit 127`. This is the dominant unlock for migrating those gates to `runs-on: corelink` |

The Rust toolchain is included so that this repo's own CI gate (the dogfood use
case) runs warm on the first job without downloading the toolchain.

### sccache — the compile cache that dogfoods CoreLink's own cache

`sccache` is baked at `/usr/local/bin/sccache` (root-owned, world-executable, on
the `runner` user's `PATH`). It is the client half of the `runs-on: corelink`
compile-cache pilot: a workflow that exports `RUSTC_WRAPPER=sccache` plus
`SCCACHE_WEBDAV_ENDPOINT` / `SCCACHE_WEBDAV_TOKEN` /
`SCCACHE_IGNORE_SERVER_IO_ERROR=1` routes every `rustc` invocation through
CoreLink's own `/cargo/<tenant>` WebDAV cache surface.

Two deliberate choices:

- **Baked, not fetched at job time.** The download happens once at image-build
  time on a hosted builder, so an ephemeral box needs **no runtime egress to
  GitHub** to obtain it. Its only required egress stays `corelink-api.humangr.com`
  — the cache itself (ADR-0003 egress posture).
- **Never `cargo install sccache`.** Compiling the cache client would cost more
  build time than the cache it enables saves. The prebuilt musl binary is static:
  no runtime deps, no glibc floor.

The binary's mere presence changes nothing — without `RUSTC_WRAPPER` it is never
invoked. Refresh the pin by bumping `SCCACHE_VERSION` + `SCCACHE_SHA256` in the
`Dockerfile` from the release's published `<artifact>.sha256` sidecar, recomputing
the digest of the downloaded tarball yourself. Never fabricate the checksum.

### Node.js + pnpm — for `run:` steps, not for `uses:` actions

The actions-runner agent already ships its own Node under `externals/node20` /
`externals/node24`. That is why JS-based `uses:` actions (`actions/checkout`,
`actions/setup-node`, …) have always worked on this box. **That Node is private
to the agent and is not on `PATH`** — a workflow `run:` step calling `node`,
`npm` or `pnpm` saw nothing. The baked toolchain at `/usr/local/node` +
`/usr/local/pnpm` (symlinked into `/usr/local/bin`) is what makes those steps work.

Scope, stated honestly: this does **not** unblock corelink-server's JS workflows.
They already carry `actions/setup-node`, and their `setup-pnpm` composite has a
`pnpm/action-setup` fallback written for exactly this case, so they could move to
`runs-on: corelink` today and pay a per-run download. Baking buys two things:
it removes that download from every run (~287 runs / 3 days across the four heavy
workflows), and it stops the move depending on a fallback branch that has never
executed. The pnpm version must match corelink-server's root `packageManager`
**exactly** — the composite compares `pnpm --version` to the pinned string and
falls back to downloading on any mismatch.

**Cost, measured:** node ≈ 145 MiB on disk (≈ 45 MiB gzipped) after stripping the
62 MiB of `include/` headers (node-gyp fetches its own); pnpm ≈ 21 MiB (≈ 4.6 MiB
gzipped). ≈ 166 MiB / ≈ 50 MiB compressed, paid by **every** spawn including Rust
jobs. Measured spawn latency is 8–10 s — re-measure after the roll.

**Browsers are not baked, deliberately.** Playwright's browser set at the pinned
1.61.x revisions is ~395 MiB of download / ~1 GiB on disk (chromium ~187 MiB,
firefox ~105 MiB, webkit ~101 MiB), i.e. 6–8× the whole node+pnpm cost, on every
spawn, to serve two workflows — and it is version-locked to the `@playwright/test`
pin (admin-ui 1.61.1 vs apps/docs 1.61.0 are already skewed), so it rots into a
re-download on the next bump. `playwright install` at job time stays correct; if
that download becomes the bottleneck the answer is a separate label-selected image
variant or caching the browsers in CoreLink's own CAS. Same reasoning for Chrome:
`lighthouse-ci` and docs-ci's axe jobs need a system Chrome and are **not** served
by this image.

Refresh by bumping `NODE_VERSION` + `NODE_SHA256` (from
`https://nodejs.org/dist/v<V>/SHASUMS256.txt`) and `PNPM_VERSION` + `PNPM_SHA256`
(recompute over `https://registry.npmjs.org/pnpm/-/pnpm-<V>.tgz`, cross-checked
against the registry's published `dist.integrity`). Never fabricate a checksum.

---

## `$CORELINK_RUNNER_JITCONFIG` contract

| Property | Value |
|---|---|
| **Name** | `CORELINK_RUNNER_JITCONFIG` |
| **Provider** | CoreLink fabric — injected at container provision time |
| **Contents** | Base64-encoded JIT config blob from the GitHub Actions API (`actions/generateRunnerJitconfig`) |
| **Lifetime** | Single-use; valid for one job registration; expires after the job completes |
| **If absent** | Container exits with code 1 and a clear error message — never idles |
| **Process surface** | The initial entrypoint seals the value before a clean re-exec; `run.sh` receives zero arguments and inherits neither the value nor the bridge pathname |

The fabric obtains this token via the GitHub API before provisioning the box and
injects it via the container runtime's env mechanism. That environment is only
the ingress to the short-lived sealing stage; the private bridge is unlinked on
open before `run.sh` starts. The value is never baked into the image.

---

## How the fabric uses this image

```
RunnerLease {
  // Cloudflare-first: the runtime image lives in the CF managed registry
  // (registry.cloudflare.com/<account>/corelink-spawn-worker-runnercontainer),
  // built from this Dockerfile via `wrangler containers build`. NOT ghcr.
  image: "registry.cloudflare.com/<account>/corelink-spawn-worker-runnercontainer@sha256:<digest>",  // X4: pinned
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

Replace **every** `@sha256:` base digest in `Dockerfile` with that digest (they must
all stay identical), commit the change, and record the new digest in the build log.

---

## Building and pushing

**Rolling a change to prod is a four-step manual procedure** — build, re-pin,
deploy, **roll** — and a green deploy does not reboot a running container. The
exact steps, with the Cloudflare Containers API calls and how to verify the new
image is actually running, are in
**[`docs/runbook/runner-image-rollout.md`](../../docs/runbook/runner-image-rollout.md)**.

**Cloudflare-first (live path):** the runner image is built from THIS Dockerfile by
`wrangler containers build` (in `deploy/cloudflare/`, CI workflow
`build-cf-container-images.yml`) and pushed to the CF managed registry — that
CF-registry `@sha256` is what the fabric pins. No ghcr anywhere in the loop.

The image workflow is intentionally split: pull requests build all image
contexts on the `corelink` runner without registry credentials or a push;
operators use `workflow_dispatch` for the credentialed build-and-push. Before
materializing layers it requires a configurable free-space floor
(`RUNNER_IMAGE_MIN_FREE_MB`, default 8192 MiB), and it releases the local image
and BuildKit cache after each successful push. A successful preflight is only a
capacity bound, not proof that a particular image will fit.

**Fallback-substrate manual path** (`build-and-push.sh`) — ONLY for the ADR-0008
Northflank fallback, which cannot pull from the CF-internal registry and so needs
a plain public OCI image. The script is registry-neutral: `REGISTRY` is REQUIRED
(no default host):

```sh
# 1. (Only when refreshing the base) re-resolve the digest — see X4 section.
# 2. Log in to your registry, then:
export REGISTRY=registry.example.com    # REQUIRED — your OCI registry host
export IMAGE=corelink-runner
export TAG=2.335.1-rust1.96.0           # recommended: encode runner + Rust versions
./deploy/runner/build-and-push.sh
```

The script prints the resulting `@sha256:` digest.  Record it and pin it
in the fabric's runner-lease config.

### Overridable env vars for `build-and-push.sh`

| Variable | Default | Notes |
|---|---|---|
| `REGISTRY` | **(required — no default)** | OCI registry host; the script fails closed if unset (CF-first builds go via `wrangler containers build`, not this script) |
| `IMAGE` | `corelink-runner` | Image name (no tag) |
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
