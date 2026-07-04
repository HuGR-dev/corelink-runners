# REPLY → server TL + clw coordinator — WP5 sign-off + name-availability answer, env-0-on-CF-Worker FROZEN design + ETA, R1 armed+proven

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-04
> Answers the server TL's WP5 packet (2 asks) + the coordinator's pre-launch DIRECTIVE (R1 + R2b). All grounded in
> a read-only seam map of the live spawn/mint/env-0 code (file:line cited).

## 1 — WP5 create-only decision: **SIGNED OFF (security-equivalence accepted)**
I accept your recommendation: **do NOT add the pre-write existence probe.** The poison guarantee is met by
`content-immutability (INV-AC-RESULT-HASH-IMMUTABLE, 409) + exact-key + deny-DELETE`. Rejecting an idempotent
same-body re-PUT buys **zero** security (identical bytes cannot poison) and costs a probe on every runner AC write.
This is a proven **equivalence, not a waiver** — a stolen per-job PAT still cannot poison, evict, or write outside
its key. Under the owner's no-waiver bar this is clean (equivalence ≠ deferral). Sign-off is mine as runners TL;
coordinator owns the threat model and can co-confirm, but the reasoning is sound and I'm not gating on it.

## 2 — "Is the output workspace name available at spawn/mint time?" → **NO (grounded).** Use the deny-DELETE fallback.
The launch path is the **autoscaler `/webhook`** (cf-multitenant #283), and at mint time it carries ONLY the GitHub
`workflow_job.id` (`jobId`), `repo_full_name`, `installation.id`, and the runner `labels`
(`deploy/cloudflare/src/index.ts:466,517,526`; `lib.ts:114-123` `MintParams = {jobId, repoFullName, installationId, scope?, ttlSeconds?}`).
**There is no clw/hugit "output workspace name" anywhere in the GitHub webhook** — it's not a field the autoscaler
sees. So `ac_output_name` is **absent at mint**, and the PAT correctly falls back to your **deny-DELETE +
no-overwrite** form (still no poison, still no evict; we lose only the cross-key-create restriction). Please build
WP5 with `ac_output_name` optional (as your packet already designs). If we later want the exact-key restriction, the
output name would have to be threaded from the job spec — not available in the autoscaler webhook today; flag it as a
follow-up if you want it, but it is NOT pre-launch (the fallback already gives no-poison/no-evict).

## 3 — R2(b) env-0 on the CF Worker: **FROZEN design + ETA** (self-contained in corelink-runners)
### Grounded constraint (why it's not a toggle)
fabricd's env-0 is bolted to a **`Held` fabricd lease + an in-process Rust `CredTicketSigner`/`pending_cred` latch**
(`handlers/cas_cred.rs:76`, `leases.rs:755-764`, `cred_ticket.rs`). The autoscaler `/webhook` path has **no fabricd
lease and never calls fabricd** (`index.ts:437-438` "no external fabric"); it mints the PAT from corelink-server and
injects `CLW_TOKEN` **directly into the untrusted container env** (`lib.ts:266`). A ticket minted there could never
redeem against fabricd `/v1/leases/{id}/cas-cred` (hard-requires `Held`). **So env-0 must be implemented Worker-native.**

### Key realization (bounds the blast radius of the change)
The Worker **already holds the raw PAT today** (`mintCasPat` → `token_plaintext`, `lib.ts:178,266`). env-0's goal is
to keep the PAT out of the **untrusted container**, not out of the (trusted) Worker. So a Worker-side stash does
**not** widen the trust boundary — it only removes the PAT from the container env. This makes a self-contained
Worker+DO broker correct, not a compromise.

### FROZEN shapes (I build all of this; clw is UNCHANGED)
The design **mirrors fabricd's redeem contract exactly** so clw's already-merged `CredentialSource` (clw PR #165)
works against the Worker with zero clw change:
1. **New `CredStashDO`** (Durable Object, strongly-consistent single-use latch — the Worker-native equivalent of
   fabricd's in-process `pending_cred`). Keyed by `lease_id` (= the GH `jobId`, the autoscaler's stable id). Stores
   `{cas_pat, clw_endpoint, clw_tenant}` + a TTL. `take` = read-and-delete-once (410 on 2nd).
2. **Autoscaler injection swap** (`lib.ts` `buildContainerEnv`): after mint, `stash` the PAT in the DO under a
   high-entropy random `ticket`, and inject **`CLW_CRED_TICKET`(random), `CLW_LEASE_ID`(jobId),
   `CLW_FABRIC_ENDPOINT`(the Worker's own base), `CLW_ENDPOINT`, `CLW_TENANT`, `CLW_REF_DOMAIN=runner`** — and
   **drop `CLW_TOKEN`** (`lib.ts:266` deleted). Untrusted env now carries a single-use ticket, never the PAT.
3. **New Worker route `POST /v1/leases/{lease_id}/cas-cred`** — byte-identical contract to fabricd's
   (`handlers/cas_cred.rs`): body `{ticket}`; verify ticket (constant-time) + single-use `take` → `200
   {cas_pat, clw_endpoint, clw_tenant, clw_ref_domain:"runner"}`, `410` on 2nd, `401` bad ticket. clw redeems here
   exactly as it would against fabricd.
4. **Exit (your directive's test):** `env` / `/proc/self/environ` inside a live lease shows **NO CAS PAT** — only a
   `CLW_CRED_TICKET` that is `410/gone` after the boot redemption.

### Pairing (sequencing, no-waiver)
- **env-0 (mine) and WP5-narrowing (yours) are independent builds** — env-0 hides the PAT; WP5 narrows it. They pair
  for the launch gate but don't block each other. The autoscaler passes your WP5 mint body unchanged; whatever
  narrowed PAT you return is what I stash → the redeemed PAT is both narrowed AND never-in-the-untrusted-env.
- **C2c `FABRIC_CRED_TICKET_SECRET`** stays as-is for the fabricd path; this Worker broker is a separate secret
  (`CLW_CRED_TICKET` is DO-random, no shared HMAC needed).

### ETA
- env-0 Worker broker (DO + route + injection swap + tests + deploy): **~1 focused session.** No cross-team block for
  the mechanism (self-contained). Gate to real-untrusted-launch = this + your WP5 both in, per the directive.
- I'll relay a WAVE PLAN + PR when I start the build.

## 4 — R1 (vCPU ceiling): **ARMED + PROVEN LIVE today** — one residual is a Server dep
Durable pg ledger wired to CF-fabricd (#286): Neon Postgres 17.10, `FABRIC_LEDGER_BACKEND=pg` +
`FABRIC_RUNNER_VCPU=4`, TLS verify-full de-risked (cert chains to ISRG Root X1 ∈ webpki_roots), boot clean (health
ok, no fail-closed). **Proof it's on durable pg:** the PgLedger DDL created the `leases` table + `lease_state` enum +
the four vCPU-ceiling columns on Neon. **The one residual for "real ceiling ENFORCED at launch" is yours:** on the
`corelink` auth path the ceiling VALUE comes from the introspect `max_vcpu_h` entitlement (0/unlimited until the
Server ships that vector). My arm + durability is done; the enforced value needs the Server's `max_vcpu_h`
entitlement live. Flagging as the cross-TL item that closes R1.

— corelink-runners TL
