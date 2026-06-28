# RELAY → owner / build-infra — pin the rustup-init bootstrap (X4 supply-chain floor; SHA must be human-verified)

> **From:** autonomous audit loop (round 9 ops/supply-chain) · **To:** owner / build-infra · **Date:** 2026-06-28
> **Why a handoff, not a full patch:** the fix needs a **human-verified SHA-256** for a specific `rustup-init` version. I corrected the misleading comment in code now, but I will NOT fabricate a checksum — a made-up "pin" is fake security worse than an honest gap.

## Finding (medium, build-time) — audit r9 #checkhost-image
`deploy/runner/Dockerfile` installs Rust via `curl -fsSL https://sh.rustup.rs | sh` — **unpinned, no checksum** — while the actions-runner tarball and the `clw` binary in the same file ARE `sha256sum -c`-verified. The previous comment falsely claimed "its SHA-256 is compared before execution" (now corrected to state the truth). Build-time only (not the untrusted runtime container) + over TLS + rustup-init verifies the toolchain components it then fetches — but the bootstrap trust-root is unpinned, so the **X4 supply-chain floor** (verify every fetched executable) is not met for this one fetch.

## Recommended fix (mirror the `clw` pattern)
1. Add build-args: `ARG RUSTUP_INIT_VERSION` (e.g. `1.27.1`) + `ARG RUSTUP_INIT_SHA256` (the operator/CI supplies the **verified** value — exactly as `CLW_SHA256`/`CLW_VERSION` are supplied).
2. Replace the `curl | sh` with the verified pattern already used for `clw`:
   ```dockerfile
   RUN curl -fsSL "https://static.rust-lang.org/rustup/archive/${RUSTUP_INIT_VERSION}/x86_64-unknown-linux-gnu/rustup-init" -o /tmp/rustup-init && \
       echo "${RUSTUP_INIT_SHA256}  /tmp/rustup-init" | sha256sum -c - && \
       chmod +x /tmp/rustup-init && \
       /tmp/rustup-init -y --default-toolchain 1.96.0 --profile minimal --component rustfmt --component clippy --no-modify-path && \
       rm /tmp/rustup-init && rustup show
   ```
3. Pass the args from `.github/workflows/build-runner-image.yml` (alongside the existing `CLW_*` args). **The operator verifies the SHA** against the official rustup release checksums before committing it.

This closes the X4 gap with a human-rooted trust anchor. Tracked in `2026-06-28-audit-loop-round-9-ops.md` (#checkhost-image). The misleading comment is already fixed in code.
