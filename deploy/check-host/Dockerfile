# CF check-host image — W2 (campaign rota-A, DEFAULT-OFF)
# Lifecycle: hydrate-then-serve (C5 contract, cf-check-host-contract.md).
# This image is the FIXED Cloudflare-Container image for check-host leases.
# Live-flip gated: clw --manifest-digest landed + real toolchain snapshot + owner deploy go.
#
# Build the exec-server binary first (produced outside this build context):
#   cargo build -p corelink-check-exec-server --release --target x86_64-unknown-linux-musl
# Then build this image:
#   docker build -f deploy/check-host/Dockerfile \
#     -t corelink-check-host:<tag> .
#
# X4 supply-chain posture: base image pinned by digest (debian:12-slim).
# Pin verified 2026-06-28; re-pin on any base update via `docker pull debian:12-slim --platform linux/amd64`.
# Registry: docker.io/library/debian@sha256:...

# -----------------------------------------------------------------------------
# Stage: runtime
# Using debian:12-slim (glibc, sh, ca-certs) — distroless-base was considered
# but requires sh for the entrypoint; slim is the minimal option that provides
# a real /bin/sh without a separate shell layer.
# Pinned by digest for X4 supply-chain posture.
# -----------------------------------------------------------------------------
FROM debian:12-slim@sha256:90522eeb7ef8924d8e45f02e6a1e2dae15e36dc0737acaa68f72809cd12bbba3 AS runtime

# Documentary: exec-server listens on port 8080 (C4 defaultPort).
# The container is reachable ONLY via the Worker's containerFetch (not public).
EXPOSE 8080

# TOOLCHAIN_DIR: where clw hydrates the toolchain at container start (C5).
# The exec-server runs commands with cwd = $TOOLCHAIN_DIR.
ENV TOOLCHAIN_DIR=/toolchain

# Create the toolchain directory so clw can hydrate into it without needing mkdir.
RUN mkdir -p /toolchain

# -----------------------------------------------------------------------------
# COPY exec-server binary (W1, C4)
# Built by: cargo build -p corelink-check-exec-server --release --target x86_64-unknown-linux-musl
# Static musl binary — no libc deps at runtime; copy directly.
# -----------------------------------------------------------------------------
COPY target/x86_64-unknown-linux-musl/release/corelink-check-exec-server /usr/local/bin/corelink-check-exec-server

# -----------------------------------------------------------------------------
# PLACEHOLDER (W6): clw binary with --manifest-digest lands on the clw TL's W6
# delivery. Do NOT ship to prod until replaced.
#
# The ARG CLW_BINARY_URL is intentionally a fake sentinel value. W6 will replace
# this block with a real fetch or COPY of the clw artifact (with --manifest-digest
# support). Until then this Dockerfile documents the image structure correctly but
# the clw artifact is a stub — the image MUST NOT be deployed to production.
# -----------------------------------------------------------------------------
ARG CLW_BINARY_URL=https://PLACEHOLDER-W6-NOT-A-REAL-URL/clw
RUN echo "[W6 PLACEHOLDER] clw binary not yet available — fetch from \$CLW_BINARY_URL would go here." \
    && echo "Replace this block when the clw TL delivers clw with --manifest-digest support (W6)." \
    && touch /usr/local/bin/clw \
    && chmod +x /usr/local/bin/clw

# -----------------------------------------------------------------------------
# Entrypoint: hydrate-then-serve (C5)
# -----------------------------------------------------------------------------
COPY deploy/check-host/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
