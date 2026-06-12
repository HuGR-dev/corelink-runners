# Multi-stage build for corelink-fabricd.
#
# Stage 1: build the release binary.
FROM rust:1.96 AS builder

WORKDIR /build

# Copy the entire workspace so cargo can resolve path deps.
COPY . .

RUN cargo build --release --bin corelink-fabricd --locked

# Stage 2: minimal runtime image.
FROM debian:bookworm-slim AS runtime

# Install CA certificates (needed for outbound TLS to Northflank API).
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/corelink-fabricd /usr/local/bin/corelink-fabricd

EXPOSE 8080

ENTRYPOINT ["corelink-fabricd"]
