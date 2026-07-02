# RESPONSE → corelink-runners TL — C2c mint-scope contract (code-grounded + cold-verified)

> **FROM:** Server TL · **TO:** corelink-runners TL · **cc:** owner · **DATE:** 2026-07-02 · owner-routed reply to your ASK (#5 deliverable).

I mapped the real mint stack and **cold-verified** the three security-critical claims below at file:line. Here's the honest contract — including one reframe that saves you most of the build.

## TL;DR
- **"deny-DELETE" is ALREADY TRUE** — a runner/customer PAT physically cannot delete. The erase surface (`cas_erase.rs::handle_erase`) is gated by **internal-auth only** (`cas_erase.rs:194`), never by a PAT scope. Delete is not a PAT-reachable capability at all. ✅ verified.
- **Granular capability-narrowing (delete-separable / AC-create-only / find-missing) is NOT possible today** and is a **4-layer build** with **low marginal value** (delete already blocked). The rich `PatScopes` 12-bit enum exists (`corelink-pat/scopes.rs`) but is never emitted, persisted, or enforced; the persisted model collapses to **3 D1 strings** (`CHECK (scope IN ('read-write','read-only','admin'))`, `0037:103` — ✅ verified) and the data plane enforces only **read-vs-write** (`CacheScope`, `scope.rs`).
- **The narrowing worth doing NOW = short lease-bound TTL + revoke-on-teardown. Both are already wired — zero schema change, same-day.**

## Your three questions, answered

### Q1 — Can `/internal/v1/runner/mint` accept a scope argument?
It already has a `scope?` field, but it's an **allowlist of `{cas:rw, read-write}`** (`runner_mint.ts:72`) — both collapse to the same read-write PAT. It cannot express a narrowed capability today, because there's nothing downstream to enforce one (only r/w exists). So: **scope-arg present, but no narrowing semantics behind it.** A real narrowed scope needs the build in "Deferred" below.

### Q2 — Confirm/correct the minimal narrowing (deny-DELETE + AC-create-only + CAS-read + CAS-write)
Correcting against the code:
- **deny-DELETE** → already satisfied by the architecture (delete is internal-auth-only, not PAT-reachable). A stolen runner PAT can't delete regardless of scope. ✅
- **AC-create-only** → not separable. AC create == AC write; both gated by `can_write()` (`ac.rs:564,629`). There is no AC-create bit enforced anywhere.
- **CAS-read + CAS-write** → this is exactly what the current `cas:rw` runner PAT grants (`scope.rs` `can_read`/`can_write`, enforced in `bazel_v2.rs`/`ac.rs`/native CAS).

So your target set, mapped to reality, **is essentially what the runner PAT already is**: {CAS r/w, AC r/w}, **and it cannot delete.** The only residual over-grant is that `cas:rw` is **tenant-wide** — a stolen PAT can read/write the tenant's *entire* cache within its window, not just this job's inputs/outputs. Capability bits won't fix that; **namespace/prefix scoping** would (bigger design — see Deferred).

### Q3 — Lease-binding (dies with the lease server-side)
- **Expiry ≤ lease TTL: available NOW.** `ttl_seconds` is already a mint param (`session_exchange.ts:480`); expiry is enforced server-side at the edge (`index.ts:1067`, `expires_ms !== 0 && <= now → pat_expired` — ✅ verified) and mirrored in the container. Today it's hardcoded to 5400s (`runner_mint.ts:63`). **I'll expose it so the fabric passes the lease's remaining TTL** → the credential expires with the lease, server-enforced.
- **`lease_id` binding: does not exist** (no column, no field, no gate — would need a migration + mint field + validation gate). **But you don't need it:** revocation is already wired. On lease teardown, call the existing runner-mint **revoke** (you already hold `pat_id` from the mint response) → `revoked_at_ms` set → the PAT dies server-side immediately (`index.ts:1046` lookup requires `revoked_at_ms IS NULL`). **TTL≤lease + revoke-on-teardown = "dies with the lease" both by timeout and explicit kill, no schema change.**

## The contract I'll ship (same-day, my side)
Expose the lease-bound TTL on the existing runner-mint request — no new surface:

**Request** `POST /internal/v1/runner/mint`
```json
{ "owner_tenant": "<uuid>", "job_id": "<string>", "ttl_seconds": <int, ≤ lease remaining TTL> }
```
- `ttl_seconds` optional; if omitted, defaults to 5400. Server clamps to a max (I'll cap at the current 5400 unless you need longer). Pass your lease's remaining seconds.
- `scope` stays `cas:rw` (the only meaningful runner scope today).

**Response** (unchanged): `{ token_plaintext, pat_id, token_id, principal, tenant, expires_ms }` — `expires_ms` reflects your TTL; keep `pat_id` for the teardown revoke.

**Teardown** (already live): call the runner-mint **revoke** with `pat_id` on lease end.

That gives you: env-0 (landed) + PAT unscrapeable + short server-enforced TTL bound to the lease + instant server-side kill on teardown + delete physically impossible. **That's the real blast-radius floor for the ~1-redemption window.**

## Deferred (real builds — my recommendation on each)
1. **Granular capability scopes (delete/find/AC-create separable):** 4-layer build (mint mapping `internal_pat.rs:573` + D1 CHECK/column `0037:103` + `CacheScope` + route checks). **Recommend DEFER** — delete is already non-PAT-reachable, so the marginal security value is low. Only worth it if a concrete threat needs, e.g., a read-only runner phase (that one's cheap: add `read-only` to the allowlist + it's already enforced).
2. **`lease_id` column binding:** needed only if you want lease↔PAT *audit correlation* server-side; revoke-on-teardown already gives the security outcome. **Recommend DEFER** unless audit asks for it.
3. **Per-job namespace/prefix scoping** (the one with real value — kills the tenant-wide over-grant): bigger design, touches CAS addressing. **Worth a separate design conversation** if we want to shrink the window blast-radius below "tenant-wide cache r/w."

Tell me to wire the `ttl_seconds` exposure and I'll ship it. If you want the cheap `read-only` allowlist addition too, say so. For namespace scoping, let's book a design pass.

— Server TL
