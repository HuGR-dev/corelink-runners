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

## The ONE cross-TL gap (G2) — RESOLVED 2026-06-26 (clw TL: option b)
`CheckDef.toolchain_ref` is a pass-through string with no in-repo resolver. **clw TL answered (option b,
verified against the frozen clw contract):** a toolchain in the CAS **IS a `clw snapshot`**, and
`toolchain_ref` **= the snapshot's manifest `root` digest**. Consequences:
- **No resolver service to build.** The "resolver" = `CAS GET(toolchain_ref)` → manifest → flatten each
  File entry's `chunks` → `Vec<ToolchainLayer{content_key=chunk.digest, size_bytes=chunk.size}>` = exactly
  `HydrationPlan.toolchain_layers`. (`clw-snapshot/src/lib.rs:68`, `clw-types/src/lib.rs:114-160`.)
- **Materialize via `clw hydrate` — do NOT re-implement manifest→tree** (paths/modes/symlinks/chunk grouping
  are clw's frozen format; reassembly is clw's). clw delivers **one small additive seam:
  `clw hydrate --manifest-digest <D> <dest>`** (digest-direct, skips the AC name lookup, reuses the existing
  materialize path). It plugs into **W6**; clw lands it **on our W6 timeline** (no API ahead of a consumer).
- **Closes the latent false-cache-hit bug below** (ref IS content → hydrated bytes provably match the memo axis).
- **Remaining owners:** (i) **producer (githugr/hugit)** must set `CheckDef.toolchain_ref = the snapshot root
  digest` — the ONE producer behavior change (relay when G1=B); (ii) **Cache TL** confirms the toolchain
  manifest+chunks live on the **R2-backed CAS** (zero-egress for the CF check-host).
- Reply: `corelink-workspaces/docs/REPLY-clw-TL-toolchain-resolver-seam-2026-06-26.md` (archived pointer:
  `docs/handoff/2026-06-26-clw-tl-RESPONSE-toolchain-resolver-option-b.md`).

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
| W5 | Toolchain-hydration composition: `CAS GET(toolchain_ref)` → manifest → flatten chunks → `HydrationPlan`; wire into the check-host provision | `crates/corelink-fabric-server/src/` (cloud_exec/leases) | W4 |
| W6 | Consume `clw hydrate --manifest-digest <toolchain_ref> <dest>` in the check-host entrypoint (clw delivers the flag on this WP's timeline) | `deploy/check-host/` entrypoint + the manifest-read helper | clw flag |

All disjoint files; contract-bound (the `/v1/exec` wire + the in-container exec contract + the clw
`--manifest-digest` flag are the frozen anchors). Conflict-free fanout once contracts are frozen. **G2 is
resolved (clw option b), so W5/W6 are now concrete — no stub resolver needed; W5 reads the manifest from CAS
directly, W6 hydrates via the clw flag.**

## Gates (why we PLAN now but don't BUILD yet)
- **G1 — hugit A/B:** if hugit's agent executes + feeds §13 (answer A), the check-host is NOT NEEDED. Build
  only on a "B" answer (`docs/handoff/2026-06-26-ASK-hugit-tl-DECISIVE-...`).
- **G2 — toolchain resolver:** ✅ **RESOLVED** (clw option b — `toolchain_ref` = clw manifest digest; clw
  delivers `--manifest-digest`). Sub-items: producer sets `toolchain_ref = snapshot digest` (relay on G1=B);
  Cache TL confirms R2 placement. No resolver service to build.
- **G3 — owner go** on the scope (it's a real subsystem, even if ~70% reuse + G2 resolved).

When **G1=B + G3=go**, W1–W6 are a clean parallel wave (solo; clw lands `--manifest-digest` into W6 on our
timeline). The producer `toolchain_ref=digest` change + Cache R2 confirm are the only remaining cross-TL
items, both small. Northflank remains the inert fallback throughout.
