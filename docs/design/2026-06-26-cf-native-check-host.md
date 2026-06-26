# Design — CF-native check-host (the Cloudflare-first check execution path)

> Status: **PLANNED** (planning round 2026-06-26, 6 read-only seam studies). NOT yet building —
> gated on (G1) the hugit A/B answer, (G2) the toolchain-resolver cross-TL seam, (G3) owner go on scope.
> Supersedes the "rota A is multi-week, undefined" framing in ADR-0008 with a concrete, mostly-reusable design.

## Goal
Run a check's `CheckDef.command` in its **real toolchain** on a **Cloudflare Container** (R2-co-located,
zero-egress hydration — the moat), capturing stdout/stderr/exit + signing the attestation — so the killer's
check path is Cloudflare-first (Northflank stays fallback-only, owner directive).

## Why it's NOT a from-scratch multi-week build (the studies' verdict)
Three machineries already exist, test-hardened, and are reusable wholesale:

| Need | Existing machinery (REUSE) | Anchor |
|---|---|---|
| Hydrate toolchain content from CAS into the box | `BootCas`/`HttpBootCas`/`hydrate`/`cold_hydrate` + `HydrationPlan{toolchain_layers: Vec<ToolchainLayer>}` + `materialize_sparse` + `BoxHydrate`/`clw_drive` | `corelink-runner/src/boot/mod.rs`, `cas_http.rs`, `materialize/`, `clw_drive.rs` |
| Per-job CAS credential (mint → inject → fetch → revoke) | D-9 `CasPatMint` + `inject_clw_env` (CLW_ENDPOINT/TENANT/TOKEN/REF_DOMAIN) + `CasHttpClient` + `revoke_pat_for` on all terminals | `runner_cas_mint.rs`, `runner_inject.rs:89`, `app.rs:950` |
| Run a command in a CF container + capture output | Model-1 exec-server (rota-A study): a fixed CF image runs an in-container HTTP server; the Worker `containerFetch`es it (CF Containers have no exec API, only `containerFetch`) | rota-A studies; `@cloudflare/containers` v0.3.7 |

`HydrationPlan` was **already designed for toolchain layers**. The CAS cred flow is platform-neutral. So the
check-host is ~70% composition of existing parts.

## Architecture
A new **fixed CF base image** (`deploy/check-host/`) containing: the `clw` binary + a small in-container
**exec-server** (HTTP on a port) + a hydration entrypoint. Lifecycle:

1. **acquire (check lease):** the fabric mints a D-9 per-job CAS PAT + injects CLW_* (EXISTING path).
2. **container start:** the entrypoint runs `clw hydrate <toolchain-layers>` to materialize the real
   toolchain from the R2 CAS into the container (EXISTING machinery), then starts the exec-server.
3. **exec:** `CloudflareEngine::exec_captured` POSTs `{argv}` to a new spawn-Worker `/v1/exec` route →
   `containerFetch` the exec-server → it runs `sh -lc <CheckDef.command>` in the hydrated toolchain,
   captures stdout/stderr/exit → returns `{exit_code, stdout, stderr}` → `CmdOutput` (NEW wire, Model-1).
4. **attest + close:** the fabric signs the result (EXISTING `exec_handler` path — engine-agnostic) and
   revokes the CAS PAT on teardown (EXISTING).

## The ONE cross-TL gap (G2) — the toolchain resolver
`CheckDef.toolchain_ref` is a **pass-through string** (e.g. `"rust@1.96.0"`) used verbatim as the 3rd
memo-key axis. **There is NO resolver** that maps it to CAS content-keys (confirmed:
`exec.rs:18-20` "until a resolver maps refs to content digests";
`docs/handoff/2026-06-13-githugr-fabric-integration-answers.md:244`). The check-host needs, for a given
`toolchain_ref`, the **ordered list of CAS content-keys** (`ToolchainLayer`s) to hydrate. That requires:
1. A **toolchain content model** — how a toolchain (Rust 1.96 + cargo-deny + …) is represented as CAS blobs.
2. A **resolver seam** — `toolchain_ref → Vec<ToolchainLayer>` (content-keys), ideally R2-co-located.
This is the **Cache/clw TL ask** (relay: `docs/handoff/2026-06-26-ASK-cache-clw-tl-toolchain-resolver-seam.md`).

## ⚠️ Latent correctness finding (pre-existing, flag now)
Today the toolchain comes from `AcquireRequest.image_digest`, NOT `toolchain_ref` — and **they can diverge**:
a check declared `toolchain_ref:"rust@1.96.0"` but run in an image with rust 1.97 produces a **false cache
hit** (same memo_key, wrong toolchain). This exists on the current Docker/NF path. The resolver (G2) closes
it by making the materialized toolchain provably match `toolchain_ref`. Worth a tracking note regardless of
the check-host. (Not introduced by this design; surfaced by the study.)

## WP decomposition (buildable SOLO against a STUB resolver; disjoint)
| WP | Scope | Owner-files | Dep |
|---|---|---|---|
| W1 | The exec-server (in-container HTTP: `POST /exec {argv} → {exit,stdout,stderr}`, runs `sh -lc`, byte-faithful capture) | `deploy/check-host/server/**` (greenfield) | contract |
| W2 | The check-host image (`Dockerfile`: base + `clw` + exec-server + hydration entrypoint) | `deploy/check-host/Dockerfile` + entrypoint (greenfield) | W1, contract |
| W3 | Spawn-Worker `/v1/exec` route + a `CheckHostContainer` DO (`containerFetch` the exec-server) | `deploy/cloudflare/src/index.ts` (+lib.ts) | contract |
| W4 | `CloudflareEngine::exec_captured` impl (POST `/v1/exec` → `CmdOutput`) + relax the runner-only floor for the check-host path | `crates/corelink-cloud-engine/src/cloudflare.rs` | contract |
| W5 | Toolchain-hydration composition: build a `HydrationPlan` from the resolver (STUB until G2) + wire it into the check-host provision | `crates/corelink-fabric-server/src/` (cloud_exec/leases) | W4, resolver-stub |
| W6 | Resolver seam (trait + STUB impl now; real impl when G2 lands) | new module + contract | G2 for real impl |

All disjoint files; contract-bound (the `/v1/exec` wire + the in-container exec contract + the resolver
trait are the frozen anchors). Conflict-free fanout once contracts are frozen.

## Gates (why we PLAN now but don't BUILD yet)
- **G1 — hugit A/B:** if hugit's agent executes + feeds §13 (answer A), the check-host is NOT NEEDED. Build
  only on a "B" answer (`docs/handoff/2026-06-26-ASK-hugit-tl-DECISIVE-...`).
- **G2 — toolchain resolver:** the cross-TL seam (Cache/clw). The mechanism builds against a stub; the real
  resolver is required for correctness/value.
- **G3 — owner go** on the scope (it's a real subsystem, even if ~70% reuse).

When G1=B + G3=go, W1–W4 + W6-stub are a clean parallel wave (solo, no further gate); W5/W6-real land when
G2 resolves. Northflank remains the inert fallback throughout.
