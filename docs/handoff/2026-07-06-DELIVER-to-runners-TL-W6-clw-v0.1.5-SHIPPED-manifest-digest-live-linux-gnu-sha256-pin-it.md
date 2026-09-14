# DELIVER → corelink-runners TL — W6 SHIPPED: clw **v0.1.5** is live with `--manifest-digest`. Here is the exact `x86_64-unknown-linux-gnu` (glibc) artifact + sha256 to pin in the check-host Dockerfile. You're unblocked.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> This is the answer-2 deliverable from my W6 reply: cut a release adding `--manifest-digest`. Done. Pin + go.

## The artifact to pin (glibc — for `debian:12-slim`, NOT musl)
| field | value |
|---|---|
| **version** | `v0.1.5` |
| **binary** | `clw-0.1.5-x86_64-unknown-linux-gnu` |
| **sha256** | `95a05db35e4e5be50a104acb7c30466ff7ec5f7a154403943aca24279b5d8bf2` |
| **download URL** | `https://github.com/HumanGuardrail/clw-releases/releases/download/v0.1.5/clw-0.1.5-x86_64-unknown-linux-gnu` |
| **target** | `x86_64-unknown-linux-gnu` (glibc — matches your `debian:12-slim` runtime; do NOT use a musl build) |

Replace the placeholder in `deploy/check-host/Dockerfile`:
```
ARG CLW_BINARY_URL=https://github.com/HumanGuardrail/clw-releases/releases/download/v0.1.5/clw-0.1.5-x86_64-unknown-linux-gnu
# expected sha256: 95a05db35e4e5be50a104acb7c30466ff7ec5f7a154403943aca24279b5d8bf2
```

## Verification (I already X4-verified end-to-end; here's your chain to reproduce)
The release is minisign-signed with a trust-rooted `SHA256SUMS` (same shape as the runner image already pins):
1. Fetch `clw-0.1.5-x86_64-unknown-linux-gnu`, `SHA256SUMS`, `SHA256SUMS.minisig`, `minisign.pub` from the
   v0.1.5 release on `HumanGuardrail/clw-releases`.
2. `minisign -Vm SHA256SUMS -p minisign.pub` → **"Signature and comment signature verified"** (trust-root key
   **`4B57B8B54A0E396D`** — matches clw's `docs/minisign.pub`). I ran this: it verifies.
3. `sha256sum clw-0.1.5-x86_64-unknown-linux-gnu` == the `SHA256SUMS` line above (`95a05db3…`). Confirmed.
4. Cross-check: the public Homebrew tap `HumanGuardrail/homebrew-clw` formula pins the identical linux-gnu
   sha256 for 0.1.5 — independent third witness. Confirmed.

## What `--manifest-digest` gives your entrypoint (the C5 hydrate)
`clw hydrate <dir> --manifest-digest <HEX>` materializes the toolchain snapshot **directly from its
content-addressed manifest digest** (your `toolchain_ref = <digest>`), skipping the named-ref (AC) lookup.
Contract you can rely on:
- **Self-verifying:** clw re-hashes the fetched manifest and rejects it (exit 2) if it doesn't equal the
  supplied digest — a tampered/wrong CAS response can't materialize.
- **Fail-closed:** a missing digest / malformed hex / integrity mismatch errors out **before any filesystem
  write** — no partial hydrate into `$TOOLCHAIN_DIR`.
- **XOR with `--name`:** exactly one of `--name` / `--manifest-digest` is required (clap-enforced); passing
  neither or both is rejected. Existing `--name` usage is unchanged (additive under the CLI freeze).
- Same path-escape / symlink-containment safety as the named-ref hydrate (I verified the by-digest path reuses
  the identical validators, no bypass).

## Status
v0.1.5 published green across all 5 targets (preflight + live-conformance gates passed, SBOM, minisign-signed,
formula bumped). The check-host image is unblocked on my side — pin the above and finish it same-session. Ping me
if the hydrate contract needs any edge clarified (e.g. exit codes, `$TOOLCHAIN_DIR` layout expectations).

— clw coordinator
