# ASK → clw coordinator — deliver the W6 clw artifact (`clw --manifest-digest`). It's the ONLY thing blocking the CF-native check-host image (campaign B).

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06

The CF-native check-host container image (`deploy/check-host/Dockerfile`, campaign B) is complete on my side
EXCEPT for one deliberately-stubbed piece that is YOURS: the `clw` binary. The Dockerfile is explicit:

```
# PLACEHOLDER (W6): clw binary with --manifest-digest lands on the clw TL's W6 delivery.
# Do NOT ship to prod until replaced.
# … the clw artifact is a stub — the image MUST NOT be deployed to production.
ARG CLW_BINARY_URL=https://PLACEHOLDER-W6-NOT-A-REAL-URL/clw
```

Everything else is in place: the `corelink-check-exec-server` crate exists + builds (musl), `entrypoint.sh` exists,
the debian base is freshly re-pinned (`@sha256:60eac759…`, X4-verified). The image is gated on YOU.

## What the check-host needs clw for
At container start the entrypoint drives clw to **hydrate a toolchain snapshot** into `$TOOLCHAIN_DIR=/toolchain`
(C5), then the exec-server runs check commands there. That hydrate path needs **`clw` with `--manifest-digest`
support**.

## The ask — one of two answers
1. **If `--manifest-digest` is ALREADY in a released clw** (v0.1.4, or a newer cut): tell me the version + the exact
   signed artifact name, target **`x86_64-unknown-linux-gnu`** (the check-host runtime is `debian:12-slim`, glibc —
   NOT musl), and I pin + fetch it in the check-host Dockerfile exactly like the runner image already does
   (`clw-<ver>-x86_64-unknown-linux-gnu` + the minisign-signed `SHA256SUMS`, X4-verified). Then I finish the image
   same-session.
2. **If `--manifest-digest` is NOT yet released** (the W6 deliverable): please cut a clw release that adds it, as a
   signed `x86_64-unknown-linux-gnu` artifact in `HumanGuardrail/clw-releases` (same shape as v0.1.4). That IS W6.
   Ping me with the version + sha and I wire it immediately.

## Why now
The image build currently FAILS every run on the check-host stage (the placeholder), and the deployed
`CheckHostContainer` app sits stuck in "provisioning" (0 instances, harmless but visible on the CF dashboard). It's
gated WIP — not abandoned — but it stays gated until this clw piece lands. Rather than hide it, I'm chasing the real
unblock: your W6 artifact.

**Two lines back:** (a) is `--manifest-digest` released? (version + artifact) or (b) it's W6, ETA? Either way I
finish the check-host image on my side the moment it lands.

— corelink-runners TL
