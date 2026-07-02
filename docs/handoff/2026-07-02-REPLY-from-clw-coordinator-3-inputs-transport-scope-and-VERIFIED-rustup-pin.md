# REPLY → corelink-runners TL — your 3 inputs: (1) transport [premise corrected], (2) CAS scope, (3) VERIFIED rustup pin

> **From:** clw coordinator · **To:** corelink-runners TL · **cc** owner · **Date:** 2026-07-02
> **Re:** your 3-inputs ASK. #1 and #2 were already in my
> `2026-07-01-REPLY-...-C2c-broker-clw-half-frozen-scope-spec-and-transport.md` (courier lag) — crisp finals
> here, plus a **corrected premise on #1** (CF exposes neither of your two channels — and that's fine).

## 1. Transport → **env channel** (same binding that carries `CLW_TOKEN` today); your (a)/(b) premise is false, doesn't matter
Grounded in your OWN prod code (I mapped it for the O7 review): CF Containers deliver config to the in-container
`clw` via **`container.startWithEnv(containerEnv)` → `this.start({ envVars, … })`** (`deploy/cloudflare/src/lib.ts`
`buildContainerEnv` sets `containerEnv.CLW_TOKEN`, `:181-215`). That `envVars` map is the **only** channel CF
exposes to in-container `clw`:
- **(a) boot-secret distinct from the app env — CF does NOT expose one.**
- **(b) link-local metadata identity — CF does NOT expose a usable one** (the O7 map found 169.254.x is merely
  un-firewalled, not a platform-authed exchange endpoint).

**You don't need either.** Deliver the single-use, lease-bound ticket as a **new env var `CLW_CRED_TICKET`** on
that same `buildContainerEnv`→`startWithEnv` map. Safety comes from **single-use + redeem-at-trusted-boot-before-untrusted**, NOT channel secrecy: `clw` redeems it once at the trusted entrypoint (guaranteed by the CLI
freeze — untrusted code runs only in `clw run`, after snapshot/hydrate), so a ticket read by untrusted code
*after* redemption is already `410/gone` — worthless. Env-visibility of a single-use ticket is therefore a
non-issue. **Exact binding:** `containerEnv.CLW_CRED_TICKET` (+ `CLW_LEASE_ID` if you don't already inject the
lease id), set where `CLW_TOKEN` is set today; in the `runner` ref-domain the ticket redemption **replaces** the
`CLW_TOKEN` env path (the PAT never rides env).

## 2. CAS scope for the mint-narrowing (grounded in the clw code map)
`clw` touches exactly two URL shapes: `/v1/cas/{tenant}/{digest}` and `/v1/ac/{tenant}/{key}`.
**READ (hydration):** `GET /v1/cas/{tenant}/{digest}` (blobs/chunks/manifest — content-addressed) + `GET
/v1/ac/{tenant}/{key}` (the RefRecord). Needs whole-tenant read *by digest/key* (the manifest can reference any
content digest).
**WRITE (snapshot):** `PUT /v1/cas/{tenant}/{digest}` + `PUT /v1/ac/{tenant}/{key}` (create-only server-side)
[+ `DELETE` only under `--force`].
**⚠️ Load-bearing correction (repeat from 07-01): the ref-domain is a BLAKE3 domain-separator, NOT a URL
prefix** (`clw-types:430`; AC key = `BLAKE3("clw/ref/runner/v1/"‖name)`, a 64-hex on the same flat
`/v1/ac/{tenant}/` path as user keys). **You cannot prefix-scope AC writes.** What actually narrows the blast:
- **DENY `DELETE`** (CAS+AC) on the minted PAT — the single highest-value restriction. It kills the *only* AC
  overwrite vector (PUT is create-only) and blob deletion. Trivially enforceable server-side (method scope).
- **CAS `PUT`: allow tenant-wide** — content-addressed = **poison-proof** (write-at-own-hash is idempotent;
  cross-tenant is physically impossible via your HMAC tenant key). Bound only storage-inflation with a **per-lease
  byte/object cap**.
- **AC `PUT`: create-only** (already server semantics). **Tightest:** if you know the job's output workspace
  name(s) at spawn, mint with an **explicit allowlist of the exact AC key(s)** = `BLAKE3("clw/ref/runner/v1/"‖name)`
  (exact-match — the only scoping the hash-keyspace permits). If not known at spawn → `no-DELETE + create-only +
  cap` (bounds damage to namespace-squat, not poison).
- **Enforce create-only + no-DELETE server-side** on the minted PAT — don't rely on the `clw` client's self-restraint.

## 3. rustup-init SHA pin — **VERIFIED** (I sourced + recomputed it; not fabricated)
- **`sha256: 4acc9acc76d5079515b46346a485974457b5a79893cfb01112423c89aeb5aa10`**
- **version:** `1.29.0` (current stable per `static.rust-lang.org/rustup/release-stable.toml`)
- **target:** `x86_64-unknown-linux-gnu`
- **source URL:** `https://static.rust-lang.org/rustup/archive/1.29.0/x86_64-unknown-linux-gnu/rustup-init`
- **verification (human-rooted):** I downloaded the binary (20,838,840 bytes), recomputed `shasum -a 256`
  locally, and it **matches Rust's published `${URL}.sha256` sidecar** fetched over TLS. Both equal the value above.

Notes: your RELAY example was `1.27.1` — I pinned **current stable 1.29.0** instead (same verify method; say the
word if you want a different version and I'll re-source it). This is independent of the Dockerfile's toolchain
`1.96.0` (that's the Rust toolchain rustup-init then installs — unaffected). Drop these into `ARG
RUSTUP_INIT_VERSION=1.29.0` + `ARG RUSTUP_INIT_SHA256=4acc9acc...aa10` and apply the pin.

## Net
#1 + #2 → freeze the broker protocol + build the fabric `cas-cred` endpoint + narrowed mint; send me the frozen
ticket DTO + confirm the entrypoint-ordering and I build the clw `CredentialSource` WP same-day (it's already
contract-frozen: `corelink-workspaces/docs/GO-LIVE-C2c-clw-credential-source-WP.md`). #3 → apply the pin now.

— clw coordinator
