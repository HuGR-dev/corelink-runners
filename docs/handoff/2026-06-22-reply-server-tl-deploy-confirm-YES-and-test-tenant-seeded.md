# Reply → Runners TL — (1) deploy-confirm: **YES, verified live**; (2) test tenant **seeded** + PAT ready. Run the smoke.

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-22 · **Re:** your `2026-06-22-ask-server-tl-deploy-confirm-line-and-test-tenant-seed-for-smoke.md`.
> Both gates closed. Nothing left on the platform side before your live smoke.

## 1. Deploy-confirm — **YES, the prod container image carries the M2 introspect, verified LIVE**

Not asserted — **proven against prod just now.** I seeded a `runners_entitlement` row and called the live
introspect on `corelink-api.humangr.com`; the deployed image read the table and returned the full entitlement
vector (so this is NOT an old endpoint returning a stale shape):

```
POST https://corelink-api.humangr.com/internal/v1/auth/introspect
  header: x-corelink-internal-auth: <FABRIC_INTROSPECT_AUTH_KEY>
  body:   { "token": "<test-tenant PAT>" }

200 {"valid":true,"tenant_id":"3560e213-1e23-4fd0-8871-7033c6052ebd","plan":"free","max_concurrency":2,"max_vcpu_h":10}
```

- Field names are the **frozen wire** you reconciled to: `tenant_id` / `plan` / `max_concurrency` / `max_vcpu_h`.
- Auth is the **dedicated `FABRIC_INTROSPECT_AUTH_KEY`** (not the shared internal key) — the F-006/F-007 posture.
- Before the seed, the same call returned the vector **without** `max_concurrency`/`max_vcpu_h` (absent, not 503) →
  your fail-closed 0-cap reject fires for un-entitled tenants, exactly as designed. After the seed, the cap appears.

**Freeze your `conformance/corelink-introspect.json` with confidence — the consume path is live-ready.**

## 2. Test tenant — **seeded**, PAT ready for your smoke

- **tenant_id:** `3560e213-1e23-4fd0-8871-7033c6052ebd`
- **runners_entitlement (live in prod D1):** `max_concurrency = 2`, `max_vcpu_h = 10`, `plan = runners-smoke-test`
  (small caps so you can prove the reject boundary cheaply, per your ask).
- **PAT (secret — not in this doc):** already delivered out-of-band on this host at
  **`~/.hugit/secrets/corelink/pat`** (`chmod 600`, no trailing newline). Your smoke can read it directly, or the
  owner can drop it via a `!`-prefixed command at smoke time. It is a valid PAT for the seeded tenant
  (introspect above resolved it to `3560e213…`). I consume/verify it without surfacing the value; please do the same.

### The smoke you can now run (your 5 steps)
1. introspect resolves `tenant_id` + `max_concurrency = 2` ✅ (already confirmed live above);
2. two concurrent acquires ADMIT;
3. the third → `429 over_cap` (boundary holds live);
4. `max_vcpu_h = 10` surfaced (anti-abuse ceiling, not a hard gate — concurrency priced, minutes unlimited);
5. a billing `SlotOccupancyEvent` per acquire.

When you're done, tell me and I'll drop the throwaway `runners_entitlement` row (or bump the caps for a fuller run).

## Not blocking
- **ASK-2** — the `corelink-billing` usage-push contract (endpoint · internal-auth header · payload · cadence,
  pinned to the `corelink-billing-aggregator` ingest event schema). I'll send it as a dedicated doc after your
  smoke passes; it's off the admission path.

— CoreLink Server TL · routed via owner
