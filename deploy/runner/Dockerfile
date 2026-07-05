# -----------------------------------------------------------------------------
# CoreLink ephemeral GitHub Actions runner image — ADR-0007 Stage A
#
# X4 supply-chain floor: every FROM is digest-pinned.
#
# BASE DIGEST (PINNED — X4 supply-chain floor):
#   Both FROM lines below are digest-pinned to a specific ubuntu:24.04 manifest
#   (resolved with `docker buildx imagetools inspect`; pinned in PR #75). This is
#   NOT a placeholder — a digest pin is INTENTIONALLY frozen: the X4 floor
#   verifies it before spawn, so the base is reproducible and tamper-evident.
#
#   To REFRESH the pin (e.g. to pick up a base-image security update), re-resolve
#   the digest and replace it in BOTH FROM lines (they must stay identical):
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
#   (HumanGuardrail/clw-releases, minisign key id 4B57B8B54A0E396D), so pinning
#   the digest transitively trusts the signed manifest. To refresh: bump
#   CLW_VERSION + CLW_SHA256 from the signed SHA256SUMS of the new release.
#   The runner drives `clw` per the frozen CLI/exit contract (125 = clw-internal).
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
# SHA256SUMS at https://github.com/HumanGuardrail/clw-releases/releases.
ARG CLW_VERSION=0.1.4
ARG CLW_SHA256=9ec443d173c6088978cb73caf7b15983ac7161879b3c5663059fea7a4fdf5d8c

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL \
      "https://github.com/HumanGuardrail/clw-releases/releases/download/v${CLW_VERSION}/clw-${CLW_VERSION}-x86_64-unknown-linux-gnu" \
      -o /tmp/clw && \
    echo "${CLW_SHA256}  /tmp/clw" | sha256sum -c - && \
    install -m 0755 /tmp/clw /usr/local/bin/clw && \
    rm /tmp/clw && \
    /usr/local/bin/clw --version

# ── Stage 2: final image ─────────────────────────────────────────────────────
# Same PINNED digest as Stage 1 — both stages MUST use the identical base.
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54

LABEL org.opencontainers.image.title="CoreLink ephemeral GitHub Actions runner" \
      org.opencontainers.image.description="One-shot ephemeral runner for the CoreLink fabric (ADR-0007 Stage A)" \
      org.opencontainers.image.vendor="HuGR / CoreLink" \
      corelink.runner.version="2.335.1" \
      corelink.rust.toolchain="1.96.0" \
      corelink.clw.version="0.1.4"

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
        pkg-config && \
    rm -rf /var/lib/apt/lists/* && \
    apt-get clean

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
    rustup show

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
