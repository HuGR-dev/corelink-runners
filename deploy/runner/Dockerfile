# -----------------------------------------------------------------------------
# CoreLink ephemeral GitHub Actions runner image — ADR-0007 Stage A
#
# X4 supply-chain floor: every FROM is digest-pinned.
#
# BASE DIGEST (PINNED — X4 supply-chain floor):
#   EVERY FROM line below is digest-pinned to the same ubuntu:24.04 manifest
#   (resolved with `docker buildx imagetools inspect`; pinned in PR #75). This is
#   NOT a placeholder — a digest pin is INTENTIONALLY frozen: the X4 floor
#   verifies it before spawn, so the base is reproducible and tamper-evident.
#
#   To REFRESH the pin (e.g. to pick up a base-image security update), re-resolve
#   the digest and replace it in EVERY FROM line (they must stay identical):
#
#     docker buildx imagetools inspect ubuntu:24.04 \
#       --format '{{json .Manifest}}' | jq -r '.digest'
#
#   For a single-arch build, pin the per-arch digest instead:
#
#     docker buildx imagetools inspect ubuntu:24.04 \
#       --format '{{json .Manifest.Manifests}}' \
#       | jq -r '.[] | select(.Platform.Architecture=="amd64") | .Digest'
#
# RUNNER VERSION:
#   Pinned to v2.335.1 (latest stable as of 2026-06-16). GitHub DEPRECATES old
#   runner versions and refuses to dispatch jobs to them ("Runner version
#   vX.Y.Z is deprecated and cannot receive messages"), so this must track a
#   currently-supported release — observed live 2026-06-16 with v2.322.0.
#   To update: check https://github.com/actions/runner/releases, update
#   RUNNER_VERSION and RUNNER_SHA256 (from the release page checksums).
#
# RUST TOOLCHAIN:
#   Matches rust-toolchain.toml (1.96.0) + components from CI (rustfmt, clippy).
#   cargo-deny@0.19.8 and cargo-audit@0.22.2 match .github/workflows/ci.yml.
#
# CLW BINARY (the moat drive — snapshot/hydrate/run):
#   Pinned to clw v0.1.4, digest-verified (X4 floor). The sha256 below is from
#   the minisign-signed SHA256SUMS of the v0.1.4 GitHub Release
#   (HuGR-Labs/clw-releases, minisign key id 4B57B8B54A0E396D), so pinning
#   the digest transitively trusts the signed manifest. To refresh: bump
#   CLW_VERSION + CLW_SHA256 from the signed SHA256SUMS of the new release.
#   The runner drives `clw` per the frozen CLI/exit contract (125 = clw-internal).
#
# SCCACHE (the Rust compile-cache client — dogfoods CoreLink's OWN cache):
#   Pinned to sccache v0.17.0 (x86_64-unknown-linux-musl), digest-verified before
#   extraction (X4 floor). This is the client half of the `runs-on: corelink`
#   compile-cache pilot: a workflow that exports RUSTC_WRAPPER=sccache plus
#   SCCACHE_WEBDAV_ENDPOINT / SCCACHE_WEBDAV_TOKEN / SCCACHE_IGNORE_SERVER_IO_ERROR
#   sends every rustc invocation through CoreLink's own `/cargo/<tenant>` WebDAV
#   cache surface — the cache earning its keep on the compute it sells. The v0.17.0
#   build carries the WebDAV backend (those three env names are present in the
#   binary itself), and it is a STATIC musl executable: no runtime deps, no glibc
#   floor, nothing to install alongside it.
#
#   It is BAKED, never fetched at job time. The download happens here, once, on a
#   hosted image builder — so an ephemeral runner box needs NO runtime egress to
#   GitHub to get it. The box's only required egress stays
#   `corelink-api.humangr.com` (the cache itself), which is what keeps the egress
#   posture (ADR-0003) narrow and auditable. It is also why we do NOT
#   `cargo install sccache`: compiling the cache client would cost far more build
#   time than the cache it enables ever saves.
#
#   Absence is FAIL-OPEN by design on the workflow side (the lane just compiles
#   cold), so a missing binary breaks nothing — it merely makes the pilot measure
#   nothing. That is exactly the state this stanza ends.
#
#   To refresh: bump SCCACHE_VERSION + SCCACHE_SHA256 from the release's published
#   `<artifact>.sha256` sidecar at https://github.com/mozilla/sccache/releases.
#   The pin below was verified the same way the rustup-init pin was — the
#   9,561,816-byte tarball was downloaded over TLS, `shasum -a 256` recomputed
#   locally, and the result matched upstream's published `.sha256` sidecar
#   byte-for-byte. NEVER fabricate this value.
#
# NODE / PNPM (the JS toolchain — so a workflow `run:` step can call node/pnpm):
#   Node.js v22.23.2 (linux-x64) + pnpm 10.32.1, both digest-verified before use
#   (X4 floor). Pinned to what corelink-server actually asks for, never "latest":
#     - node 22 — `actions/setup-node` uses `node-version: 22` in admin-ui-ci.yml,
#       admin-ui-e2e.yml and lighthouse-ci.yml (docs-ci.yml still asks for 20;
#       node 22 also satisfies apps/docs' own `engines.node` ">=22.0.0 <23.0.0",
#       which its own workflow currently violates). v22.23.2 is the current 22.x
#       LTS ("Jod", 2026-07-28) — i.e. what `node-version: 22` resolves to today.
#     - pnpm 10.32.1 — corelink-server's ROOT package.json `packageManager`, and
#       the default of its `.github/actions/setup-pnpm` composite. It must match
#       EXACTLY: that composite takes its offline fast path only when
#       `pnpm --version` equals the pinned string, else it downloads pnpm anyway.
#
#   ⚠️ WHAT THIS DOES *NOT* DO — read before repeating the claim. It does not
#   "unblock" those workflows. The actions-runner agent already bundles its OWN
#   Node under `externals/node20` / `externals/node24`, which is why JS-based
#   `uses:` actions (actions/checkout, actions/setup-node, …) already work here.
#   That Node is private to the agent and is NOT on PATH, so a workflow `run:`
#   step calling `node`/`npm`/`pnpm` has never worked — THAT is what this fixes.
#   Those four workflows could move to `runs-on: corelink` without this change,
#   paying a per-run setup-node + pnpm download. What baking buys is exactly two
#   things: (a) it removes that download from every run, and (b) it stops the move
#   depending on the setup-pnpm composite's fallback branch, which was written for
#   "a future hosted/Linux runner" and has never once executed. Both are real;
#   neither is "unblocking".
#
#   THE PRIZE IT SERVES (measured on HuGR-Labs/corelink-server, 2026-08-01→03):
#   4429 billed ubuntu-latest minutes in 3 days — admin-ui-e2e 1112 min / $6.67,
#   admin-ui-ci 811 / $4.87, docs-ci 712 / $4.27, lighthouse-ci 308 / $1.85; 66%
#   of the bill, every job `runs-on: ubuntu-latest`. A minute on `runs-on:
#   corelink` is $0 of GitHub billing. The `runs-on:` change lives in THAT repo;
#   this stanza only makes the box somewhere those jobs can land cheaply.
#
#   NOT ALL FOUR FIT. node+pnpm is the entire requirement for admin-ui-ci and for
#   docs-ci's build / i18n-coverage / reapi-gen-drift jobs. admin-ui-e2e also runs
#   `playwright install --with-deps` — apt-get and passwordless sudo are both
#   present here, so it works; it just pays the browser download every run.
#   lighthouse-ci and docs-ci's axe / a11y-baseline-diff jobs need a SYSTEM CHROME
#   on PATH (LHCI's chrome-launcher auto-discovery, and an explicit
#   `command -v google-chrome-stable` that fails the job hard). This image ships
#   no Chrome, so those stay hosted until that is decided separately. docs-ci also
#   has a Docker-container action (lychee) and two jobs that `npm install --global`
#   — the global prefix here is the root-owned /usr/local/node, so that needs
#   `sudo` (available, NOPASSWD), another reason those jobs are not the first movers.
#
#   SIZE / LATENCY — the cost side, paid on every spawn by every job in the fleet,
#   including the Rust ones that will never call node. Measured on the real
#   artifacts, not estimated:
#     node v22.23.2 linux-x64: 31,058,332 B download → 208 MiB extracted, of which
#       the `include/` C++ headers are 62 MiB. Those are dropped in the stage below
#       (node-gyp fetches its own per-version headers into ~/.cache/node-gyp and
#       never reads this directory), leaving ~145 MiB on disk / ~45 MiB gzipped.
#     pnpm 10.32.1 npm tarball: 4,534,444 B download → ~21 MiB on disk /
#       ~4.6 MiB gzipped.
#     TOTAL ≈ 166 MiB uncompressed, ≈ 50 MiB compressed. Measured spawn latency is
#     8–10 s today. Re-measure it after the roll
#     (docs/runbook/runner-image-rollout.md) instead of assuming this is free.
#
#   ⛔ BROWSERS ARE DELIBERATELY *NOT* BAKED. Measured against the Playwright CDN
#   at the revisions pinned by @playwright/test 1.61.1: chromium ~187 MiB, firefox
#   ~105 MiB, webkit ~101 MiB, ffmpeg ~2 MiB — ~395 MiB of download and ~1 GiB on
#   disk, i.e. 6–8× the entire node+pnpm cost, imposed on every Rust spawn to serve
#   two workflows. And a baked browser set is version-locked to the Playwright
#   package (admin-ui pins 1.61.1, apps/docs pins 1.61.0 — already skewed), so it
#   silently rots into a re-download on the next bump. If the per-run browser
#   download ever becomes the bottleneck, the answer is a SEPARATE image variant
#   selected by runner label, or caching the browsers in CoreLink's own CAS — not
#   fattening the one image every Rust job also pulls.
#
#   To refresh: bump NODE_VERSION + NODE_SHA256 from the
#   `node-v<V>-linux-x64.tar.xz` line of https://nodejs.org/dist/v<V>/SHASUMS256.txt
#   (the per-release sidecar, additionally GPG-signed as SHASUMS256.txt.asc); and
#   PNPM_VERSION + PNPM_SHA256 by downloading
#   https://registry.npmjs.org/pnpm/-/pnpm-<V>.tgz, checking its sha512 against the
#   registry's published `dist.integrity` for that exact version, then recomputing
#   `shasum -a 256` over those same bytes. NEVER fabricate either value.
# -----------------------------------------------------------------------------

# ── Stage 1: runner binary download + checksum verification ──────────────────
# Base digest PINNED (see header for the refresh procedure).
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54 AS runner-download

# GitHub Actions runner release to pin.
# Verify sha256 from: https://github.com/actions/runner/releases/tag/v2.335.1
ARG RUNNER_VERSION=2.335.1
ARG RUNNER_SHA256=4ef2f25285f0ae4477f1fe1e346db76d2f3ebf03824e2ddd1973a2819bf6c8cf

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL \
      "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
      -o /tmp/actions-runner.tar.gz && \
    echo "${RUNNER_SHA256}  /tmp/actions-runner.tar.gz" | sha256sum -c - && \
    mkdir -p /opt/actions-runner && \
    tar -xzf /tmp/actions-runner.tar.gz -C /opt/actions-runner && \
    rm /tmp/actions-runner.tar.gz

# ── Stage 1b: clw binary download + checksum verification ────────────────────
# Same PINNED base digest. clw is a BARE executable (not archived): download,
# verify the sha256 (X4 floor; sourced from the minisign-signed SHA256SUMS of
# the v0.1.4 release), chmod +x.
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54 AS clw-download

# clw release to pin (x86_64-unknown-linux-gnu). Refresh both from the signed
# SHA256SUMS at https://github.com/HuGR-Labs/clw-releases/releases.
ARG CLW_VERSION=0.1.4
ARG CLW_SHA256=9ec443d173c6088978cb73caf7b15983ac7161879b3c5663059fea7a4fdf5d8c

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL \
      "https://github.com/HuGR-Labs/clw-releases/releases/download/v${CLW_VERSION}/clw-${CLW_VERSION}-x86_64-unknown-linux-gnu" \
      -o /tmp/clw && \
    echo "${CLW_SHA256}  /tmp/clw" | sha256sum -c - && \
    install -m 0755 /tmp/clw /usr/local/bin/clw && \
    rm /tmp/clw && \
    /usr/local/bin/clw --version

# ── Stage 1c: sccache binary download + checksum verification ────────────────
# Same PINNED base digest. sccache ships as a TARBALL (like the runner agent, not
# like bare clw): download, verify the sha256 BEFORE extracting anything, then
# extract and install the single static musl binary.
#
# The fetch lives HERE, at image-build time on a hosted builder, deliberately:
# baking it means the ephemeral runner box never has to reach GitHub at job time
# to get its compile cache client. The box's only required egress remains
# corelink-api.humangr.com — the cache surface itself.
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54 AS sccache-download

# sccache release to pin (x86_64-unknown-linux-musl — static, no runtime deps).
# Refresh both from the release's published `<artifact>.sha256` sidecar at
# https://github.com/mozilla/sccache/releases (re-verify by recomputing the
# digest of the downloaded tarball — NEVER fabricate it).
ARG SCCACHE_VERSION=0.17.0
ARG SCCACHE_SHA256=67c4a96dd237c1f518f6b36083f270f9976d516f1e57fce891755ea782e50006

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL \
      "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
      -o /tmp/sccache.tar.gz && \
    echo "${SCCACHE_SHA256}  /tmp/sccache.tar.gz" | sha256sum -c - && \
    tar -xzf /tmp/sccache.tar.gz -C /tmp && \
    install -m 0755 \
      "/tmp/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl/sccache" \
      /usr/local/bin/sccache && \
    rm -rf /tmp/sccache.tar.gz "/tmp/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl" && \
    /usr/local/bin/sccache --version

# ── Stage 1d: Node.js download + checksum verification ───────────────────────
# Same PINNED base digest. Node ships as a TARBALL (like the runner agent and
# sccache) with a per-release SHASUMS256.txt sidecar, itself GPG-signed as
# SHASUMS256.txt.asc — verify BEFORE extracting anything.
#
# Fetched HERE, at image-build time on a hosted builder, for the same reason as
# sccache: an ephemeral runner box then needs no runtime egress to nodejs.org (or
# to GitHub's setup-node tool mirror) just to obtain a JS toolchain.
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54 AS node-download

# Node.js release to pin (linux-x64). NODE_VERSION tracks what corelink-server's
# workflows ask `actions/setup-node` for (`node-version: 22`); v22.23.2 is the
# current 22.x LTS. Refresh NODE_SHA256 from the `node-v${V}-linux-x64.tar.xz`
# line of https://nodejs.org/dist/v${V}/SHASUMS256.txt, re-verified by recomputing
# the digest of the downloaded tarball — NEVER fabricate it.
ARG NODE_VERSION=22.23.2
ARG NODE_SHA256=d60acfe00a2932254bb0ad20e01b0d74397a0875595de719654b214f4b03f307

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates xz-utils && \
    rm -rf /var/lib/apt/lists/*

# The `rm -rf` drops 62 MiB of C++ embedder headers plus release docs. node-gyp
# does NOT read them — it downloads its own per-version headers into
# ~/.cache/node-gyp — so native-module builds are unaffected. Every megabyte here
# is paid on every spawn of every job, including the Rust ones (see header).
RUN curl -fsSL \
      "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
      -o /tmp/node.tar.xz && \
    echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum -c - && \
    mkdir -p /opt/node && \
    tar -xJf /tmp/node.tar.xz -C /opt/node --strip-components=1 && \
    rm -f /tmp/node.tar.xz && \
    rm -rf /opt/node/include /opt/node/share/doc \
           /opt/node/CHANGELOG.md /opt/node/README.md && \
    /opt/node/bin/node --version

# ── Stage 1e: pnpm download + checksum verification ──────────────────────────
# Same PINNED base digest. FORM CHOICE, deliberately: pnpm's GitHub release
# publishes standalone binaries but NO checksum sidecar of any kind, so pinning
# one would mean fabricating its digest — forbidden. The npm registry DOES publish
# a per-version sidecar (`dist.integrity`, a sha512 over the exact tarball), so the
# npm tarball is the only form of pnpm with a verifiable published digest. It is
# also 4.5 MB against the standalone binary's 66 MB, and it is a Node bundle —
# fine, because Stage 1d bakes the Node it needs.
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54 AS pnpm-download

# pnpm release to pin. This MUST equal corelink-server's root package.json
# `packageManager` ("pnpm@10.32.1") and the default of its setup-pnpm composite —
# that composite compares `pnpm --version` to the pinned string EXACTLY and falls
# back to downloading pnpm on any mismatch, which would defeat the whole point.
# PNPM_SHA256 was produced by downloading the 4,534,444-byte tarball over TLS,
# checking its sha512 against the registry's published `dist.integrity` for
# 10.32.1, then recomputing `shasum -a 256` over those same bytes. NEVER
# fabricate it.
ARG PNPM_VERSION=10.32.1
ARG PNPM_SHA256=9b943b94bc8f55efb993aad8e44b538e6b091e60a9e4a944dcde869855f233e3

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL \
      "https://registry.npmjs.org/pnpm/-/pnpm-${PNPM_VERSION}.tgz" \
      -o /tmp/pnpm.tgz && \
    echo "${PNPM_SHA256}  /tmp/pnpm.tgz" | sha256sum -c - && \
    mkdir -p /opt/pnpm && \
    tar -xzf /tmp/pnpm.tgz -C /opt/pnpm --strip-components=1 && \
    rm -f /tmp/pnpm.tgz && \
    test -x /opt/pnpm/bin/pnpm.cjs && test -x /opt/pnpm/bin/pnpx.cjs

# ── Stage 2: final image ─────────────────────────────────────────────────────
# Same PINNED digest as every download stage — all stages MUST use the identical base.
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54

LABEL org.opencontainers.image.title="CoreLink ephemeral GitHub Actions runner" \
      org.opencontainers.image.description="One-shot ephemeral runner for the CoreLink fabric (ADR-0007 Stage A)" \
      org.opencontainers.image.vendor="HuGR / CoreLink" \
      corelink.runner.version="2.335.1" \
      corelink.rust.toolchain="1.96.0,1.91.1" \
      corelink.python3="present" \
      corelink.python3.jsonschema="present" \
      corelink.gh.version="2.97.0" \
      corelink.clw.version="0.1.4" \
      corelink.sccache.version="0.17.0" \
      corelink.node.version="22.23.2" \
      corelink.pnpm.version="10.32.1"

# ── System packages ───────────────────────────────────────────────────────────
# git is required by most GitHub Actions (actions/checkout etc.)
# Additional packages satisfy the runner's libc deps and common build tooling.
SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends \
        git \
        curl \
        ca-certificates \
        libicu-dev \
        libssl-dev \
        liblttng-ust-dev \
        libkrb5-dev \
        zlib1g \
        sudo \
        jq \
        unzip \
        build-essential \
        pkg-config \
        python3 \
        python3-venv \
        python3-yaml \
        python3-requests \
        python3-jsonschema && \
    rm -rf /var/lib/apt/lists/* && \
    apt-get clean
# python3 (stdlib): the corelink-server CI gates are overwhelmingly python3
# scripts (validate_specs / validate_okf / validate_secrets_matrix / every
# validate_*), which exit 127 ("python3: command not found") on a box without
# it. This single package is the dominant unlock for migrating those ~48 hosted
# gates onto `runs-on: corelink`.
#
# python3-venv + python3-yaml + python3-requests: the second increment. Ubuntu
# 24.04 is PEP-668 "externally managed", so a workflow's global `pip install`
# fails ("externally-managed-environment") — the fix is NOT to add global pip
# but to (a) ship `python3-venv` so a gate that needs arbitrary deps can build
# an isolated venv (ensurepip gives it pip), and (b) preinstall, as apt system
# packages, the two libs the currently-armed hosted crons import at runtime:
#   - PyYAML  → okf_nightly (`validate_okf.py`), so it no longer needs a
#     runtime `python3 -m venv && pip install pyyaml`.
#   - requests → billing-reconcile-daily, so it no longer needs a runtime
#     global `pip install requests` (which PEP-668 would reject anyway).
# This keeps the ephemeral runner egress-free for those gates.
#
# python3-jsonschema: the THIRD increment. `scripts/validate_specs.py` (the 463/0
# spec gate, imported transitively by pentest-findings-sync's validate step) does
# a hard `import jsonschema` + sys.exit if it is absent — so without this package
# every validate_specs-based gate exits non-zero on an ephemeral box even though
# PyYAML is already present. apt's python3-jsonschema is used (not a venv) because
# validate_specs runs as a bare `python3 scripts/…` with no venv wrapper.
#
# `gh` (GitHub CLI) is ALSO baked in this third increment — see the verified
# download below. It is the unlock for the workflows that shell out to
# `gh pr comment` / `gh api` (lockfile-diff, api-reference-sync) and the weekly
# compliance-weekly cron.
#
# Container image builds ARE supported on this image — via BuildKit, daemonless,
# NOT via a docker daemon (see the BuildKit block below). This reconciles the
# product capability S1.6.1 ("a job builds a container image") with the substrate:
# a classic docker daemon / DinD (a privileged nested daemon) stays infeasible on
# Firecracker, but BuildKit is not DinD — it is a userspace builder, and the
# per-lease microVM is itself the isolation boundary. Proven end-to-end (build
# FROM ubuntu:24.04 + daemonless push to the CF managed registry) in run
# 31664445243 before this bake.

# ── GitHub CLI (`gh`) — verified pinned download (X4 supply-chain floor) ───────
# Same discipline as the node / pnpm / clw / runner / sccache downloads: a
# VERSIONED URL and a sha256 read from the release's OWN published
# `gh_2.97.0_checksums.txt` (line `…  gh_2.97.0_linux_amd64.tar.gz`) — NEVER
# fabricated. gh is a single static Go binary; drop it on PATH, discard the
# tarball's man/completion trees. Runs as root here, before the USER runner drop.
ARG GH_VERSION=2.97.0
ARG GH_SHA256=a2c9b8497e1f85b1ad0dfcb78b5a622e098801b8e461e459e88e1ee12f018112
RUN curl -fsSL \
      "https://github.com/cli/cli/releases/download/v${GH_VERSION}/gh_${GH_VERSION}_linux_amd64.tar.gz" \
      -o /tmp/gh.tar.gz && \
    echo "${GH_SHA256}  /tmp/gh.tar.gz" | sha256sum -c - && \
    tar -xzf /tmp/gh.tar.gz -C /tmp && \
    install -m 0755 "/tmp/gh_${GH_VERSION}_linux_amd64/bin/gh" /usr/local/bin/gh && \
    rm -rf /tmp/gh.tar.gz "/tmp/gh_${GH_VERSION}_linux_amd64" && \
    gh --version

# ── BuildKit — daemonless container image builds (S1.6.1 F-4.7, X4 floor) ──────
# CoreLink runners build images WITHOUT a docker daemon: `buildkitd` runs per-job
# inside the lease (rootful via the runner's passwordless sudo, or rootless) and
# `buildctl` drives the build + a direct registry push. This is what lets a
# `docker build`-class job work on `runs-on: corelink`, and lets our own prod
# image build off GitHub-hosted runners. BuildKit is NOT DinD (no privileged
# nested daemon); the per-lease Firecracker microVM is the isolation boundary.
#
# Pinned like every other download (X4 floor): a VERSIONED URL + a sha256 computed
# from the real v0.32.2 linux-amd64 release tarball (92,882,168 bytes) — NEVER
# fabricated. We install ONLY buildkitd, buildctl, the CNI plugins (build-step
# networking), and the bundled runc — installed as `runc`, which buildkitd's OCI
# worker looks for on PATH (the tarball ships it as `buildkit-runc`). The
# `buildkit-qemu-*` cross-arch emulators in the tarball are deliberately DROPPED:
# we build native amd64, and keeping them would add ~100 MB for nothing.
ARG BUILDKIT_VERSION=v0.32.2
ARG BUILDKIT_SHA256=2975d0f651ad96ba8b80b9992ae1f9a964f4408569af5b6dc36544165c3926af
RUN curl -fsSL \
      "https://github.com/moby/buildkit/releases/download/${BUILDKIT_VERSION}/buildkit-${BUILDKIT_VERSION}.linux-amd64.tar.gz" \
      -o /tmp/buildkit.tar.gz && \
    echo "${BUILDKIT_SHA256}  /tmp/buildkit.tar.gz" | sha256sum -c - && \
    tar -xzf /tmp/buildkit.tar.gz -C /tmp && \
    install -m 0755 /tmp/bin/buildkitd     /usr/local/bin/buildkitd && \
    install -m 0755 /tmp/bin/buildctl      /usr/local/bin/buildctl && \
    install -m 0755 /tmp/bin/buildkit-runc /usr/local/bin/runc && \
    for p in /tmp/bin/buildkit-cni-*; do install -m 0755 "$p" /usr/local/bin/; done && \
    rm -rf /tmp/buildkit.tar.gz /tmp/bin && \
    buildkitd --version && buildctl --version && runc --version

# ── Non-root runner user (App-layer resource caps — Track-C C2, part 1/2) ──────
# The runner agent must not run as root; the dedicated user also owns the
# Rust toolchain so cargo install works without sudo.
#
# This is the IN-IMAGE equivalent of the on-box DockerEngine hardening (the
# `DockerEngine` `Engine::run` impl in crates/corelink-runner/src/isolation.rs).
# The untrusted job is bounded at the app layer on TWO axes:
#   1. NON-ROOT identity — this USER (the container runs as `runner`, never root;
#      the final ENTRYPOINT stage below drops to `USER runner`). isolation.rs's
#      `--user` note confirms non-root is enforced at the IMAGE layer on the CF
#      substrate rather than re-imposed with a docker `--user` flag.
#   2. ULIMIT pids ceiling — applied in entrypoint.sh just before the untrusted
#      job (the CF-substrate stand-in for --pids-limit / the PIDS_LIMIT const).
#      Memory is NOT app-layer-cappable here (see entrypoint.sh); it is contained
#      by the per-lease Firecracker microVM envelope + in-VM OOM-killer.
RUN useradd -m -s /bin/bash -G sudo runner && \
    echo "runner ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/runner && \
    chmod 0440 /etc/sudoers.d/runner

# ── Copy actions-runner from Stage 1 ─────────────────────────────────────────
COPY --from=runner-download --chown=runner:runner /opt/actions-runner /opt/actions-runner

# ── Copy the digest-pinned clw binary from Stage 1b ──────────────────────────
# root-owned, world-executable on PATH — the runner drives `clw snapshot →
# hydrate → run` (the moat); only used when the fabric injects CLW_* (default-off).
COPY --from=clw-download /usr/local/bin/clw /usr/local/bin/clw

# ── Copy the digest-pinned sccache binary from Stage 1c ──────────────────────
# root-owned, world-executable on PATH (/usr/local/bin is on the default PATH and
# survives the `ENV PATH=/home/runner/.cargo/bin:$PATH` prepend below), so
# `command -v sccache` resolves for the unprivileged `runner` user that actually
# executes the job. Static musl → no runtime deps to install alongside it.
# Inert unless the workflow sets RUSTC_WRAPPER=sccache + the SCCACHE_WEBDAV_*
# env; the binary's mere presence changes nothing (default-off, like clw).
COPY --from=sccache-download /usr/local/bin/sccache /usr/local/bin/sccache

# ── Copy the digest-pinned Node.js + pnpm from Stages 1d/1e ──────────────────
# ⚠️ READ THIS BEFORE CONCLUDING "node is already in the image". The
# actions-runner tree copied above ALREADY ships its own Node under
# `externals/node20` and `externals/node24` — that is why JS-based `uses:` actions
# (actions/checkout, actions/setup-node, …) run on this box today. That Node is
# private to the runner agent and is NOT on PATH. A workflow `run:` step calling
# `node`, `npm` or `pnpm` sees ONLY what the two COPY lines below install. The two
# things are not interchangeable; an `externals/` listing proves nothing about
# `run:` steps.
#
# Placement is load-bearing: these land while still root and BEFORE the
# `USER runner` switch below, root-owned under /usr/local, symlinked onto
# /usr/local/bin. That directory is on the image's default PATH and SURVIVES the
# `ENV PATH=/home/runner/.cargo/bin:$PATH` prepend further down (it prepends, it
# does not replace), so all three resolve for the unprivileged `runner` user that
# actually executes the job — and for entrypoint.sh, which never rewrites PATH.
COPY --from=node-download /opt/node /usr/local/node
COPY --from=pnpm-download /opt/pnpm /usr/local/pnpm

# The final `--version` triple is a BUILD-TIME PROOF that each name resolves on
# PATH in this image (a broken symlink or a missing interpreter fails the build
# closed, here, rather than silently on a customer's box).
RUN ln -sf /usr/local/node/bin/node      /usr/local/bin/node && \
    ln -sf /usr/local/node/bin/npm       /usr/local/bin/npm && \
    ln -sf /usr/local/node/bin/npx       /usr/local/bin/npx && \
    ln -sf /usr/local/node/bin/corepack  /usr/local/bin/corepack && \
    ln -sf /usr/local/pnpm/bin/pnpm.cjs  /usr/local/bin/pnpm && \
    ln -sf /usr/local/pnpm/bin/pnpx.cjs  /usr/local/bin/pnpx && \
    node --version && npm --version && pnpm --version

# Install runner dependencies (requires running as root, inside the container).
RUN /opt/actions-runner/bin/installdependencies.sh

# ── Rust toolchain (1.96.0, matching rust-toolchain.toml) ────────────────────
USER runner
WORKDIR /home/runner

ENV CARGO_HOME=/home/runner/.cargo
ENV RUSTUP_HOME=/home/runner/.rustup
ENV PATH=/home/runner/.cargo/bin:$PATH

# Install the Rust toolchain via rustup.
# Track-C C2b (X4 supply-chain floor MET): the rustup-init bootstrap is now
# fetched from a VERSIONED URL and `sha256sum -c`-verified BEFORE execution —
# mirroring the runner tarball + clw. The SHA below was human-verified by the clw
# go-live coordinator (downloaded the 20,838,840-byte binary, recomputed
# `shasum -a 256`, matched Rust's published `${URL}.sha256` sidecar over TLS;
# `docs/handoff/2026-07-02-REPLY-from-clw-coordinator-...VERIFIED-rustup-pin.md`).
# Both ARGs are operator-overridable to bump the pinned version+SHA (re-verify
# the SHA the same way — NEVER fabricate it). This is the rustup-init BOOTSTRAP
# version (1.29.0), independent of the Rust TOOLCHAIN it installs (1.96.0).
ARG RUSTUP_INIT_VERSION=1.29.0
ARG RUSTUP_INIT_SHA256=4acc9acc76d5079515b46346a485974457b5a79893cfb01112423c89aeb5aa10
RUN curl -fsSL "https://static.rust-lang.org/rustup/archive/${RUSTUP_INIT_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
        -o /tmp/rustup-init && \
    echo "${RUSTUP_INIT_SHA256}  /tmp/rustup-init" | sha256sum -c - && \
    chmod +x /tmp/rustup-init && \
    /tmp/rustup-init -y \
        --default-toolchain 1.96.0 \
        --profile minimal \
        --component rustfmt \
        --component clippy \
        --no-modify-path && \
    rm -f /tmp/rustup-init && \
    rustup show && \
    rustup toolchain install 1.91.1 \
        --profile minimal \
        --component rustfmt \
        --component clippy \
        --component rust-src && \
    rustup target add --toolchain 1.91.1 \
        wasm32-unknown-unknown \
        x86_64-unknown-linux-musl
# Two toolchains ON PURPOSE: 1.96.0 stays the DEFAULT (this repo's own CI /
# rust-toolchain.toml wants it), and 1.91.1 is baked ALONGSIDE it because the
# corelink-server workspace pins 1.91.1 (ADR-0015, rust-toolchain.toml). The
# rustup shim auto-honours a checkout's rust-toolchain.toml, so `cargo` inside a
# corelink-server checkout resolves to the baked 1.91.1 with ZERO per-spawn
# download (the per-run tax runner-probe.yml's rust-detail job measures). It
# lands at /home/runner/.rustup/toolchains/1.91.1-x86_64-unknown-linux-gnu,
# which also satisfies scripts/ci-use-host-toolchain.sh when invoked with
# HOST_TRIPLE=x86_64-unknown-linux-gnu. rust-src + the wasm32/musl targets match
# what rust-toolchain.toml requires so target-add is a no-op at CI time.
# Do NOT swap the default to 1.91.1 — that would make this repo's lanes
# auto-download 1.96.0 on every spawn.

# cargo-deny and cargo-audit — versions match .github/workflows/ci.yml
# (taiki-e/install-action pins: cargo-audit@0.22.2, cargo-deny@0.19.8)
RUN cargo install cargo-deny --version 0.19.8 --locked && \
    cargo install cargo-audit --version 0.22.2 --locked && \
    cargo cache --version 2>/dev/null || true

# Pre-warm: fetch the registry index so the first `cargo build` in CI is warm.
# This is a best-effort step; failure is non-fatal.
RUN cargo search --limit 1 tokio 2>/dev/null || true

# ── Entrypoint ────────────────────────────────────────────────────────────────
USER root
COPY --chown=runner:runner entrypoint.sh /opt/actions-runner/entrypoint.sh
RUN chmod +x /opt/actions-runner/entrypoint.sh

USER runner
WORKDIR /opt/actions-runner

ENTRYPOINT ["/opt/actions-runner/entrypoint.sh"]
