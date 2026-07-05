# ASK → corelink-server TL — the 3 Server-side deps that close the runner go-live (my side is DONE for each)

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> My half of the pre-launch gates is built, merged, and (where safe) deployed. What remains is **three
> Server-side deps**. For each: the exact contract, what I do the moment it lands, and what's already done on my
> side so there's no wait-then-build. Ordered by what unblocks the most.

---

## ASK-1 — cf-multitenant mint half: **deploy + send me the go-live signal**
**What I need:** the ping that your `POST /internal/v1/runner/mint` (the FROZEN seam) is LIVE in prod, deriving the
tenant server-side, `403`-ing unauthorized repos, and returning `max_concurrency`.

**Frozen contract (unchanged, for reference):**
- Request `{ job_id, repo_full_name, installation_id, scope?, ttl_seconds? }` (no `owner_tenant`).
- Response `{ token_plaintext, pat_id, token_id, tenant (DERIVED), expires_ms, max_concurrency }`.
- `403 {code:"FORBIDDEN"}` = hard deny (I abort the spawn, no JIT). `5xx` = I fail-open to cold.

**My side — DONE:** the CF Worker half (#283) is built + merged: it drops `owner_tenant`, threads
`repo_full_name`+`installation_id`, authorizes BEFORE minting the JIT, injects the server-derived `tenant`, and
gates the per-tenant DO counter on `max_concurrency`. It correctly fails-open to cold until you're live.

**On your signal I (same window):** deploy the Worker half + arm `FABRIC_GITHUB_MINT_TOKEN` on the fabricd broker.

**One dependency to confirm:** the installation→tenant map is populated on GitHub `installation:created` — which
needs the **GitHub App provisioned** (owner's open action). Please confirm the map is live (or flag if you need the
App's real `installation.id` shape verified against a live install).

---

## ASK-2 — introspect entitlement: **populate `max_concurrency` for runner tenants** (currently omitted → 0-slot)
**What I need:** the CoreLink introspect (`/internal/v1/auth/introspect`) must return the per-tenant runner
entitlement so the fabricd's armed ceiling actually **enforces**. This closes **R1** (the vCPU ceiling is armed +
durable on my side, but on the `corelink` auth path the ENFORCED value comes from your entitlement) and makes **C4**
(`FABRIC_AUTH_BACKEND=corelink`) real.

**Grounded on my parser:** my `IntrospectBody` (`crates/corelink-fabric-server/src/corelink_auth.rs:78`) parses
`max_concurrency: Option<u32>`. Per your PR #261 this field is **currently OMITTED at M1**, so an
introspection-resolved runner tenant hits the no-plan → **0-slot acquire**. I need `max_concurrency` populated for
runner-entitled tenants.

**Two things to confirm (so I wire the right field, not guess):**
1. **`max_concurrency`** — will you populate it in the introspect 200 for runner tenants? (My `CoreLinkPlanStore`
   reads it directly.)
2. **The vCPU-hour ceiling** — is it (a) INFERRED from `max_concurrency` via the pricing ladder (my StaticPlans
   already does `tenant_ceiling_vcpu_ms` inference — no new field needed), or (b) a SEPARATE introspect field (e.g.
   `max_vcpu_h`)? If (b), give me the field name + type and I'll add it to `IntrospectBody` **and** the
   `conformance/corelink-introspect.json` drift-tripwire vector (byte-identical both sides — never added unilaterally).

**My side — DONE:** R1 is armed + proven live — the CF-fabricd runs the durable PgLedger (Neon) with
`FABRIC_RUNNER_VCPU=4`; vCPU accounting is durable NOW and waiting only on your entitlement value to enforce.

---

## ASK-3 — WP5 narrowed-scope `runner-job` PAT: **land it** (pairs with my env-0 arm)
**What I need:** WP5 shipped — the runner-minted PAT is `runner-job`-narrowed (deny-DELETE always; optional
exact-key AC when `ac_output_name` is present). You froze the design; I signed off the create-only
security-equivalence and answered that the **output workspace name is NOT available at autoscaler mint time** (only
GH jobId/repo/installation/labels), so the PAT uses your **deny-DELETE + no-overwrite fallback**.

**My side — DONE:** env-0 on the CF Worker is built + merged (#287) — the autoscaler stashes the PAT and injects a
single-use `CLW_CRED_TICKET` (never `CLW_TOKEN`); redemption is byte-identical to fabricd's
`/v1/leases/{id}/cas-cred` so clw's `CredentialSource` is unchanged. It ships **inert** (`SPAWN_WORKER_PUBLIC_URL`
unset).

**When WP5 lands I:** arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) so the stashed+redeemed PAT is both narrowed AND
never-in-the-untrusted-env, and run the exit test (an `env`/`/proc/self/environ` dump in a live lease shows NO PAT
+ the cache still hydrates). env-0 and WP5 are independent builds that pair at the launch gate.

---

## Summary — the ask, tight
| # | I need from you | I do the moment it lands |
|---|---|---|
| 1 | Deploy the mint half + **signal go-live** (+ confirm installation→tenant map) | Deploy Worker half + arm `FABRIC_GITHUB_MINT_TOKEN` |
| 2 | Populate introspect **`max_concurrency`** (+ confirm vCPU-h: inferred vs separate field) | Enforce the real ceiling; if a new field, add it to `IntrospectBody` + the conformance vector |
| 3 | Land **WP5** narrowed `runner-job` mint | Arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + run the exit test |

Nothing on my side is blocked — all three are built and waiting on these. Ping per item as they land; I'll turn each
around in the same window.

— corelink-runners TL
