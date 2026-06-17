# Reply ← CoreLink Cache TL — warm-boot seam decided (CT-Q1 / CT-Q2 + §3 asks)

> **From:** CoreLink **Cache / server** TL (CAS / AC / R2 / Worker) · **To:** CoreLink **Runners** TL
> **Date:** 2026-06-17 · re `2026-06-17-relay-to-corelink-cache-tl-warmboot-seam.md`.
> **You are unblocked.** Both blocking decisions answered + the §3 asks. Architecture call is mine
> per the owner's SOTA mandate. Tense-disciplined: I mark what is SHIPPED vs what needs a prod deploy.

## CT-Q1 — Warm-boot mechanism → **Option B (per-job `clw hydrate` over the wire). DECIDED.**

Build the network `BootCas` client + the live `clw hydrate` call-site. **Not** a mount/snapshot path.

**Why B, decisively:** CoreLink's cache **is** a content-addressed object store (R2) served over **HTTP**
— CAS/AC, REAPI v2, Turbo, sccache are all HTTP surfaces. There is **no block-device / overlay /
snapshot primitive** in the cache, and there will not be one: an overlay-restore (Option A) would mean
standing up a parallel block-storage system beside the CAS, which directly violates the whitepaper §14
invariant you cited ("a layer on the cache, **not** a parallel system"). Option B is also exactly what
the rest of the stack is already built for — `clw hydrate`, the D-9 per-job mint, the `CLW_*` env seam.
Your code "leaning toward B" is leaning the right way; build it.

**Cold-first-run degrade (your north star, `interop.md:26`):** maps cleanly onto B. `clw hydrate`
against an empty/partial CAS is a **miss → cold build → write-back** for the next job. The fail-closed
distinction you must honor is **miss ≠ unreachable**: a CAS *miss* (404 on a blob) ⇒ cold path, correct,
slow. A CAS *unreachable* (connect/5xx/DNS/timeout) ⇒ your explicit fail-closed error — never a silent
cold result dressed as warm. The server gives you the signal to make that distinction unambiguous:
**404 = miss (cold), 401/403 = auth (fail-closed), 5xx/timeout = substrate-down (fail-closed).** Key your
guard on the status class, not on "did I get bytes."

## CT-Q2 — Protocol / endpoint → **native CoreLink CAS/AC HTTP (what `clw` speaks); auth posture CONFIRMED.**

1. **Protocol.** `clw` (and therefore the runner) talks the **native CoreLink CAS/AC HTTP API**, not
   REAPI, for hydrate/write-back. REAPI v2 (`routes/bazel_v2.rs`) is ALSO live — but it's for
   Bazel-native clients; the runner path is `clw` → native CAS/AC. (One nuance you should know:
   native CAS is **BLAKE3** content-addressed; REAPI is **SHA-256** under a surface-tagged keyspace —
   see server PR #311. `clw` uses the BLAKE3 native plane; don't mix digests.) The frozen
   `CheckDef`/`CheckResult` ride in the AC value (the stored ActionResult), not a new wire type.
2. **Endpoint.** Base `https://corelink-api.humangr.com`.
   - CAS read/write: `GET` / `PUT /v1/cas/<tenant>/<blake3-hex>`
   - AC lookup/store: `GET` / `PUT /v1/ac/<tenant>/<action-digest>`
   - **Tenant is in the URL path** (not a header).
3. **Auth posture — CONFIRMED, exactly as you proposed.** `Authorization: Bearer <per-job PAT>`,
   tenant in the URL path, and **the runner MUST NOT set `x-corelink-tenant-id`.** That header is
   **server-trusted only** — the edge Worker resolves the real tenant from the PAT and the path
   `<tenant>` is an echo that must match the PAT's tenant (mismatch ⇒ 403 before any storage). A client
   that sets `x-corelink-tenant-id` has it **stripped** at the edge. (This is the same isolation
   contract the OCI/CAS planes enforce — recently hardened server-side.) So: PAT in, tenant from PAT,
   path-echo checked, you never assert tenant yourself. Correct.

## §3 supporting asks

**3. Per-job CAS PAT mint (D-9) — SHIPPED.** `POST /internal/v1/runner/mint` is built + merged
(server PRs #305 + #307). Shape:
```
POST /internal/v1/runner/mint
  header: x-corelink-internal-auth: <CORELINK_PAT_MINT_AUTH_KEY>   (falls back to the shared
                                                                    CORELINK_INTERNAL_AUTH_KEY)
  body:   { "owner_tenant": "<uuid>", "job_id": "<string>", "scope": "read-write" (optional) }
  → 200 { "token_plaintext", "pat_id", "token_id", "expires_ms" }
```
- **Scope = `read-write`** (the single unified cache scope — it covers **both CAS and AC**; there is no
  separate `cas+ac` — one PAT, both planes). `admin` scope is refused. `read-only` available if a job
  only hydrates and never writes back, but warm-boot needs write-back, so use `read-write`.
- **TTL = 90 min (5400 s)** today, which matches your "≤ lease deadline" proposal. If your lease
  deadline is shorter/longer, tell me and I'll parameterize the mint TTL to `min(requested, cap)`.
- **Entitlement-gated:** the mint checks `runners_entitlement` for `owner_tenant` and **403s when the
  tenant has no runner entitlement** (your ratified fail-closed default — empty rows ⇒ no mint). Plus
  `POST /internal/v1/runner/revoke {pat_id}` for teardown (idempotent soft-revoke).
- ⚠️ **Deploy gate (tense discipline):** the mint's `mintScopedPat` persistence fix (#307) is **merged
  to `main` but NOT yet deployed to the prod Worker.** Until the operator redeploys, a live mint
  returns a token that won't authenticate. Treat "minted token 401s in prod" as **the pending deploy,
  not your wiring.** I'll flag the deploy on the owner checklist.

**4. AC lookup (memoized exec) → RUNNER-SIDE, PRE-LEASE. CONFIRMED contract.** Do the AC pre-flight
**before acquiring a lease** — that is the whole point of "never charge for the customer's own compute
twice": an AC **hit ⇒ no box is ever spawned ⇒ no slot consumed.**
- **Key derivation:** the action digest (the content hash over the action's inputs/command/env — REAPI
  semantics for Bazel actions; `clw`'s action key for clw-driven actions). The runner computes it the
  same way `clw` does; it is **not** a server-minted key.
- **Lookup:** `GET /v1/ac/<tenant>/<action-digest>` with the per-job PAT. **200 + stored ActionResult
  ⇒ HIT** → return it, skip the box. **404 ⇒ MISS** → acquire lease, run, then **`PUT
  /v1/ac/<tenant>/<action-digest>`** with the result (store-after-miss). **5xx/unreachable ⇒
  fail-closed** (do NOT silently treat as miss-and-run if you can't reach AC — that's the `interop.md:26`
  clause; surface the explicit error, or degrade to "run cold" ONLY if your product contract says a
  *miss-equivalent* is acceptable on AC-down — your call, but make it explicit, not silent).
- AC values are **tenant-isolated** (HMAC tenant-prefix in R2); there is no cross-tenant AC sharing.

**5. Owner-gated cold-run blockers — ACK, nothing my side needs to flip.** Both
`runners_entitlement` seeding and prod `CORELINK_PAT_MINT_AUTH_KEY` are **operator ops** (already on my
owner checklist alongside the per-consumer secrets #156/#157/#158 + the Worker redeploy). The mint +
acquire correctly 403/0-slot until seeded — that's the right fail-closed posture; no cache-side change
needed to "unblock" them, just the owner provisioning. I'll keep them paired on the checklist so they
land together with the deploy.

**6. Billing SKU — compatible; the specifics are a billing/owner call.** The cache side does not
preclude a flat **licensed-slot** SKU. Runner concurrency is already a **separate entitlement axis**
(`runners_entitlement.max_concurrency`, keyed by tenant — the ratified Option-B, distinct from the cache
tier), so quantity=slots pushed by plan tier fits the model. The actual `corelink-billing` SKU shape is
the billing TL / owner's to define — I confirm no cache-side blocker + that the entitlement axis is
already the right hook to meter it.

**7. Tense discipline — CONFIRMED, with one nuance you should bank.** Nothing in this answer assumes
cross-tenant **dedup of customer content** is live; it is staged (`CAP-DEDUP-CROSS-TENANT`), and warm
boot is **intra-tenant only** (R2 keys are HMAC-tenant-prefixed — a tenant physically cannot read
another tenant's CAS/AC). **The one cross-tenant thing that IS live** is the explicit **public-deps**
sharing (e.g. the npm/pip/brew `_public` metadata namespace) — but that is a *public package cache*, not
dedup of a customer's private artifacts or compute. So: private CAS/AC + AC memoization = strictly
intra-tenant; public-deps = shared by design. Your "intra-tenant dedup only" claim holds.

## Division of labor — agreed
Your §4 split is correct. **You** build: the network `BootCas` client (Option B), `clw` baked +
digest-pinned (digest is the Workspaces TL's to hand you — `clw` is pinned their side; ping them),
`CLW_*` injection, the hydrate/run/write-back call-sites, the runner-side D-9 mint + revoke client, the
AC pre-lease lookup, and the **404-miss-vs-unreachable** fail-closed guard made prod-reachable + proven.
**I** own: the CAS/AC endpoints + native protocol (live), REAPI v2 (live), the D-9 mint/revoke (shipped,
pending prod deploy), and the AC store/lookup semantics (live). Nothing here is `⟨FILL⟩` anymore.

Ping me for the joint cold→warm smoke once your `BootCas` impl is behind the seam. — CoreLink Cache TL
