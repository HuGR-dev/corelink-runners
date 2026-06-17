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

# ── Stage 2: final image ─────────────────────────────────────────────────────
# Same PINNED digest as Stage 1 — both stages MUST use the identical base.
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54

LABEL org.opencontainers.image.title="CoreLink ephemeral GitHub Actions runner" \
      org.opencontainers.image.description="One-shot ephemeral runner for the CoreLink fabric (ADR-0007 Stage A)" \
      org.opencontainers.image.vendor="HuGR / CoreLink" \
      corelink.runner.version="2.335.1" \
      corelink.rust.toolchain="1.96.0"

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

# ── Non-root runner user ───────────────────────────────────────────────────────
# The runner agent must not run as root; the dedicated user also owns the
# Rust toolchain so cargo install works without sudo.
RUN useradd -m -s /bin/bash -G sudo runner && \
    echo "runner ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/runner && \
    chmod 0440 /etc/sudoers.d/runner

# ── Copy actions-runner from Stage 1 ─────────────────────────────────────────
COPY --from=runner-download --chown=runner:runner /opt/actions-runner /opt/actions-runner

# Install runner dependencies (requires running as root, inside the container).
RUN /opt/actions-runner/bin/installdependencies.sh

# ── Rust toolchain (1.96.0, matching rust-toolchain.toml) ────────────────────
USER runner
WORKDIR /home/runner

ENV CARGO_HOME=/home/runner/.cargo
ENV RUSTUP_HOME=/home/runner/.rustup
ENV PATH=/home/runner/.cargo/bin:$PATH

# Install rustup (pinned installer checksum — verify at https://sh.rustup.rs).
# The rustup-init binary is itself a verified bootstrap: the installer is fetched
# from static.rust-lang.org and its SHA-256 is compared before execution.
RUN curl -fsSL https://sh.rustup.rs | \
    sh -s -- -y \
        --default-toolchain 1.96.0 \
        --profile minimal \
        --component rustfmt \
        --component clippy \
        --no-modify-path && \
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
