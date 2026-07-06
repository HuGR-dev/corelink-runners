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
# X4 supply-chain posture: base image pinned by digest (debian:12-slim).
# Pin re-verified 2026-07-06 via `docker buildx imagetools inspect debian:12-slim`
# (the prior 90522eeb… digest was GC'd from the registry → build "not found").
# Registry: docker.io/library/debian@sha256:...

# -----------------------------------------------------------------------------
# Stage: clw-download (W6, 2026-07-06) — fetch + digest-verify clw v0.1.5 (the
# release adding `--manifest-digest`). Separate stage so curl never ships in the
# runtime image. X4 floor: CLW_SHA256 is from the minisign-signed SHA256SUMS of the
# v0.1.5 release (HumanGuardrail/clw-releases, key 4B57B8B54A0E396D) — verified
# end-to-end (minisign + real-binary sha) before pinning. glibc build (linux-gnu),
# matching the debian:12-slim runtime — NOT musl.
# -----------------------------------------------------------------------------
FROM debian:12-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df AS clw-download
ARG CLW_VERSION=0.1.5
ARG CLW_SHA256=95a05db35e4e5be50a104acb7c30466ff7ec5f7a154403943aca24279b5d8bf2
SHELL ["/bin/bash", "-euo", "pipefail", "-c"]
RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*
RUN curl -fsSL \
      "https://github.com/HumanGuardrail/clw-releases/releases/download/v${CLW_VERSION}/clw-${CLW_VERSION}-x86_64-unknown-linux-gnu" \
      -o /clw && \
    echo "${CLW_SHA256}  /clw" | sha256sum -c - && \
    chmod +x /clw && \
    /clw --version

# -----------------------------------------------------------------------------
# Stage: runtime
# Using debian:12-slim (glibc, sh, ca-certs) — distroless-base was considered
# but requires sh for the entrypoint; slim is the minimal option that provides
# a real /bin/sh without a separate shell layer.
# Pinned by digest for X4 supply-chain posture.
# -----------------------------------------------------------------------------
FROM debian:12-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df AS runtime
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
