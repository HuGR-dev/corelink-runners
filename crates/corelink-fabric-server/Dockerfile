# Multi-stage build for corelink-fabricd.
#
# X4 supply-chain floor: every FROM is digest-pinned (mirrors deploy/runner/Dockerfile).
# A bare tag is mutable — pinning the manifest-list digest makes the base image
# reproducible and tamper-evident. To bump, resolve the new manifest-LIST digest
# (multi-arch) and replace it here:
#   docker buildx imagetools inspect rust:1.96            # → Digest:
#   docker buildx imagetools inspect debian:bookworm-slim # → Digest:
#
# Stage 1: build the release binary.
FROM rust:1.96@sha256:6df234c1eb92b0545468fab8c18fc5f9adfb994e7d4f67d81d45fe2fcabf5657 AS builder

WORKDIR /build

# Copy the entire workspace so cargo can resolve path deps.
COPY . .

RUN cargo build --release --bin corelink-fabricd --locked

# Stage 2: minimal runtime image.
FROM debian:bookworm-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df AS runtime

# Install CA certificates (needed for outbound TLS to the CoreLink/CF/Northflank APIs).
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/corelink-fabricd /usr/local/bin/corelink-fabricd

EXPOSE 8080

ENTRYPOINT ["corelink-fabricd"]
