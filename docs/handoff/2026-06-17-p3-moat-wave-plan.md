# P3 — Moat wave plan (test-first decomposition)

> **Owner:** Runners TL · **Date:** 2026-06-17 · **Status:** decomposition (pre-build).
> The cross-TL seams are DECIDED (Cache TL → Option B + native CAS/AC HTTP + D-9 + AC pre-lease;
> clw TL → CLW_* names + drive + exit-transparent). Nothing is `⟨FILL⟩`. This plan is the red→green
> target + the conflict-disjoint slicing the fleet executes.
> **Cold-start north star (binds every WP):** cache absent ⇒ **slow, never broken**. A cold first run
> on an empty CAS MUST succeed (miss → cold build → write-back). `miss ≠ unreachable`.

---

## 0. The done-definition — acceptance suite (write these FAILING first)

Each scenario is built + proven against a **fake HTTP transport** (the existing `acceptance_c3`/`c9`
fake-CAS pattern + the Northflank `StubTransport` pattern) — so the whole suite is green **without the
Northflank allowance or a prod deploy**. The live flip (§1) is the only allowance-gated bit.

| # | Scenario | Asserts |
|---|---|---|
| A1 | **Cold first run, empty CAS** | `clw hydrate` → 404 misses → cold build runs → results written back (`PUT` CAS + `PUT /v1/ac`). Run SUCCEEDS (slow). |
| A2 | **Warm boot** | hydrate → 200 hits → working set local before first instruction; no cold fetch of hit layers. |
| A3 | **AC hit ⇒ no box** | pre-lease `GET /v1/ac/<tenant>/<digest>` 200 ⇒ return stored ActionResult, **acquire is never called, no slot reserved, no box spawned**. |
| A4 | **AC miss ⇒ run + store** | 404 ⇒ acquire + run + `PUT /v1/ac/...` (store-after-miss). |
| A5 | **miss ≠ unreachable (the guard)** | 404 = cold path (proceed). 401/403 + 5xx/timeout/DNS = **explicit fail-closed error**, never a silent cold-result-dressed-as-warm. Guard keyed on **status class**, not "got bytes" (`interop.md:26`). |
| A6 | **CLW_* injection** | per-job env carries `CLW_ENDPOINT`/`CLW_TENANT`/`CLW_TOKEN`/`CLW_REF_DOMAIN=runner`; `CLW_TOKEN` = the minted per-job PAT, **never** the tenant PAT (mirror the `box_env_carries_scoped_token_never_the_tenant_pat` invariant). |
| A7 | **D-9 mint + revoke** | acquire mints a per-job PAT (`POST /internal/v1/runner/mint`, scope `read-write`, TTL ≤ lease deadline); teardown `POST .../revoke {pat_id}`. Mint failure fails closed (no config-less box), mirroring the JIT-mint rollback. |
| A8 | **clw drive + exit-transparency** | runner invokes `clw snapshot → hydrate → run`; `clw run` child exit code passes through; non-zero **not cached**; `exit 2` = clw-internal (distinct from a child's 2). |
| A9 | **auth posture** | every CAS/AC/mint call sends `Bearer <per-job PAT>`, tenant in URL path, **never** `x-corelink-tenant-id`; native CAS digests are **BLAKE3** (not SHA-256/REAPI). |

**Cold-critic gate (techlead-decompose):** before slicing is dispatched, an INDEPENDENT cold reviewer
confirms A1–A9 cover the demand (whitepaper §10/§11 + `interop.md` + the Cache TL contract). *(Pending —
run before the build wave launches.)*

---

## 1. Mock-buildable NOW vs live-gated

- **NOW (no external dep):** A1–A9 against fakes; all of §2 WP-1..WP-7 (impl + unit/acceptance).
- **Live flip (gated, NOT blocking the build):** needs (a) the **Northflank allowance** raised (owner —
  buying $50 credit today, then raise ephemeral+compute allowance); (b) the **D-9 mint prod Worker
  redeploy** (Cache TL / owner — #307 merged, not deployed); (c) the **clw binary digest** (Workspaces
  TL — interim `rc/dev` pin usable meanwhile). WP-8 is the flip; it waits on (a)+(b)+(c).

---

## 2. Work packages (atomic, conflict-disjoint)

| WP | Responsibility | Owner-files (disjoint) | Depends on | Mock-testable now |
|---|---|---|---|---|
| **WP-1** | Acceptance suite A1–A9 (RED) | `crates/corelink-fabric-server/tests/acceptance_moat.rs` (new) + fakes | — | ✅ (it's the target) |
| **WP-2** | Network `BootCas` over native CAS/AC HTTP (BLAKE3, Bearer-PAT, tenant-in-path) **+ the miss≠unreachable status-class guard** | new `crates/corelink-runner/src/cas_http.rs` impl of the `BootCas` trait | trait (exists) | ✅ (fake transport) |
| **WP-3** | Runner-side D-9 mint+revoke client | new `crates/corelink-fabric-server/src/runner_cas_mint.rs` (mirror `runner_broker`) | — | ✅ |
| **WP-4** | `CLW_*` per-job injection seam | `…/runner_inject.rs` (sibling of `inject_runner_jitconfig`) | WP-3 (the minted PAT) | ✅ |
| **WP-5** | `clw` baked + digest-pinned in image (interim `rc/dev` pin, flagged) | `deploy/runner/Dockerfile` (+ the X4 guard already validates) | clw rc/dev artifact | ✅ (wiring) |
| **WP-6** | clw drive: `snapshot`/`run` + write-back call-sites (extend `BoxHydrate`, which only builds `hydrate` argv today) | `crates/corelink-runner/src/boot/mod.rs` + provision/exec hook | WP-2, WP-4 | ✅ |
| **WP-7** | AC pre-lease lookup (memoized-exec short-circuit: hit⇒skip box, miss⇒run+store) | acquire path in `…/handlers/leases.rs` (a pre-admission step, like the S2/broker guards) + WP-2's AC client | WP-2 | ✅ |
| **WP-8** | Flip-live (config flag) + prove the guard prod-reachable (cold-degrades-slow; unreachable⇒explicit error) | composition root (`…/server.rs`, `cloud_exec.rs`) + flag | WP-2..7 + §1(a,b,c) | ⚠️ live-gated |
| **WP-9** | Graceful infra-capacity degrade (fleet-ceiling → queue, task #10) | `…/admission.rs` (extend `AdmissionMode::Queue`) | — (independent) | ✅ |

---

## 3. Conflict map + merge DAG

- **Disjoint by file:** WP-2 (`cas_http.rs`), WP-3 (`runner_cas_mint.rs`), WP-5 (`Dockerfile`),
  WP-9 (`admission.rs`) — zero overlap, fully parallel.
- **`leases.rs` is the one shared site** (WP-7's pre-lease guard). It also hosts the S2 guard +
  acquire. **Eliminate the conflict:** WP-7 adds its pre-lease AC step as a *new* guarded block (like
  the `0b` box-backend guard), and is the ONLY WP touching `leases.rs` in this wave → no collision.
- **`boot/mod.rs`** is WP-6 only.
- **Merge DAG (merge order, not dispatch order):**
  `WP-1` → { `WP-2`, `WP-3`, `WP-5`, `WP-9` parallel } → { `WP-4`(after WP-3), `WP-6`(after WP-2+WP-4), `WP-7`(after WP-2) } → `WP-8`(last, live).

---

## 4. Contract anchors to FREEZE before dispatch (código-âncora)

Freeze these signatures so dependent WPs transcribe, not design (exact current sigs pulled at pack-time):
- **`BootCas` trait** (`boot/mod.rs:155-179`) — WP-2 implements it over HTTP; do NOT change the trait.
- **CAS/AC client surface** — `get_cas(blake3) / put_cas(blake3,bytes) / get_ac(digest) / put_ac(digest,ActionResult)`, each returning a status-class-typed result (`Hit | Miss | FailClosed(reason)`).
- **Mint client** — `mint(owner_tenant, job_id) -> {token, pat_id, expires_ms}` + `revoke(pat_id)`.
- **Inject seam** — mirror `inject_runner_jitconfig(&mut spec, …)` → `inject_clw_env(&mut spec, mint, endpoint, tenant)`.
- **AC key** — the action digest computed runner-side (same as clw); freeze the derivation.

---

## 5. How the fleet executes

Rolling wave (dispatch-on-ready, merge-on-green per the DAG). WP-1 first (the RED target). Then the
disjoint impl WPs in parallel (worktree-isolated only if they'd collide — here they're file-disjoint, so
plain branches are fine). Each WP: failing-test→impl→green→change-scoped gate→cold-review→merge. WP-8
(flip-live) lands LAST and only after §1(a,b,c). #9 (queue-degrade) can land anytime.

**This whole wave is buildable + mergeable behind the seam WITHOUT the Northflank allowance** — the
allowance only gates turning the flag on (WP-8). So the credit/allowance resolving later today is NOT
on the critical path of building the moat.
