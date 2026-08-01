# CF check-host image — W2 (campaign rota-A, DEFAULT-OFF)
# Lifecycle: hydrate-then-serve (C5 contract, cf-check-host-contract.md).
# This image is the FIXED Cloudflare-Container image for check-host leases.
# clw --manifest-digest DELIVERED (clw v0.1.5, W6, 2026-07-06). Buildable; live-flip
# still gated on the real toolchain snapshot + owner deploy go.
#
# Build the exec-server binary first (produced outside this build context):
#   cargo build -p corelink-check-exec-server --release --target x86_64-unknown-linux-musl
# Then build this image:
#   docker build -f deploy/check-host/Dockerfile \
#     -t corelink-check-host:<tag> .
#
# X4 supply-chain posture: base image pinned by digest. Base is ubuntu:24.04
# (glibc 2.39, same digest the runner image pins) — the clw v0.1.5 glibc build
# REQUIRES GLIBC_2.39, which debian:12-slim (2.36) lacks (`/clw --version` →
# 'GLIBC_2.39 not found'), so the check-host runtime must match the runner's base.

# -----------------------------------------------------------------------------
# Stage: clw-download (W6, 2026-07-06) — fetch + digest-verify clw v0.1.5 (the
# release adding `--manifest-digest`). Separate stage so curl never ships in the
# runtime image. X4 floor: CLW_SHA256 is from the minisign-signed SHA256SUMS of the
# v0.1.5 release (HuGR-Labs/clw-releases, key 4B57B8B54A0E396D) — verified
# end-to-end (minisign + real-binary sha) before pinning. glibc build (linux-gnu),
# matching the debian:12-slim runtime — NOT musl.
# -----------------------------------------------------------------------------
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54 AS clw-download
ARG CLW_VERSION=0.1.5
ARG CLW_SHA256=95a05db35e4e5be50a104acb7c30466ff7ec5f7a154403943aca24279b5d8bf2
SHELL ["/bin/bash", "-euo", "pipefail", "-c"]
RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*
RUN curl -fsSL \
      "https://github.com/HuGR-Labs/clw-releases/releases/download/v${CLW_VERSION}/clw-${CLW_VERSION}-x86_64-unknown-linux-gnu" \
      -o /clw && \
    echo "${CLW_SHA256}  /clw" | sha256sum -c - && \
    chmod +x /clw && \
    /clw --version

# -----------------------------------------------------------------------------
# Stage: runtime
# Using ubuntu:24.04 (glibc 2.39, sh, apt) — matches the runner image + satisfies
# the clw v0.1.5 glibc floor (debian:12-slim's 2.36 was too old). Pinned by digest.
# -----------------------------------------------------------------------------
FROM ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54 AS runtime
# clw needs TLS roots to reach CAS at runtime (C5 hydrate over HTTPS).
RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Documentary: exec-server listens on port 8080 (C4 defaultPort).
# The container is reachable ONLY via the Worker's containerFetch (not public).
EXPOSE 8080

# TOOLCHAIN_DIR: where clw hydrates the toolchain at container start (C5).
# The exec-server runs commands with cwd = $TOOLCHAIN_DIR.
ENV TOOLCHAIN_DIR=/toolchain

# Create the toolchain directory so clw can hydrate into it without needing mkdir.
RUN mkdir -p /toolchain

# -----------------------------------------------------------------------------
# COPY exec-server binary (W1, C4). Build context = deploy/check-host; the CI
# workflow compiles `corelink-check-exec-server` for x86_64-unknown-linux-musl
# (static — no glibc-version coupling to the runtime image) and copies the binary
# into this directory before `wrangler containers build`.
# -----------------------------------------------------------------------------
COPY corelink-check-exec-server /usr/local/bin/corelink-check-exec-server

# -----------------------------------------------------------------------------
# clw binary (W6 DELIVERED 2026-07-06): the digest-verified v0.1.5 build with
# `--manifest-digest`, copied from the clw-download stage. clw drives the C5
# toolchain hydrate (`clw hydrate $TOOLCHAIN_DIR --manifest-digest <hex>`).
# -----------------------------------------------------------------------------
COPY --from=clw-download /clw /usr/local/bin/clw

# -----------------------------------------------------------------------------
# Entrypoint: hydrate-then-serve (C5)
# -----------------------------------------------------------------------------
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
