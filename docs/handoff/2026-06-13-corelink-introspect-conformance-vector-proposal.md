# → corelink-server: introspect conformance vector — candidate shape for ratification

**De:** corelink-runners techlead · **Para:** corelink-server techlead (via owner) ·
**Data:** 2026-06-13 · **Status:** PROPOSAL — ratify against your handler, then we commit
byte-identical in BOTH repos (closes the introspect-field drift tripwire). ·
**Em resposta a:** `corelink-server/docs/handoff/2026-06-13-corelink-runners-m2-LIVE.md` (ação 2).

---

## TL;DR

M2 live + shape-locked confirmed. Per your ask #2, here is the **candidate conformance
vector** for `POST /internal/v1/auth/introspect` — the four response cases our
`CoreLinkTokenStore` + `CoreLinkPlanStore` parse. Ratify each line against your handler's
actual serialization; once you confirm byte-exact, we commit it as
`conformance/corelink-introspect.json` **byte-identical in both repos** + a golden test each
side (the drift tripwire: either side's serialization diverging breaks a golden test).

## Candidate vector — the 4 cases

```jsonc
// CASE 1 — valid + Runners entitlement → cap applied
{ "valid": true, "tenant_id": "11111111-1111-4111-8111-111111111111", "plan": "pro", "max_concurrency": 40 }

// CASE 2 — valid + cache-only / no entitlement → max_concurrency ABSENT (the current prod state)
{ "valid": true, "tenant_id": "22222222-2222-4222-8222-222222222222", "plan": "solo" }

// CASE 3 — valid + Enterprise (custom/BYOC) → max_concurrency ABSENT (custom, off-ladder)
{ "valid": true, "tenant_id": "33333333-3333-4333-8333-333333333333", "plan": "enterprise" }

// CASE 4 — invalid / unknown / revoked → NEVER a tenant_id
{ "valid": false }
```

## What each case must pin (our consumer behavior)

| Case | `CoreLinkTokenStore` (auth) | `CoreLinkPlanStore` (cap) |
|---|---|---|
| 1 | `Ok(Some(tenant 1111…))` → 200 | `Ok(Some(cap=40))` → admit ≤40 |
| 2 | `Ok(Some(tenant 2222…))` → 200 | `Ok(None)` → over-cap reject (authenticated, uncapped) |
| 3 | `Ok(Some(tenant 3333…))` → 200 | `Ok(None)` → over-cap reject (Enterprise cap is provisioned out-of-band, not via this field) |
| 4 | `Ok(None)` → 401 | `Ok(None)` → reject |
| 503 / transport / malformed-200 | `Err(Unreachable)` → 503 | `Err(Unreachable)` → 503 |

## Questions to ratify (so the bytes match exactly)

1. **Field order + exact keys on the wire:** is it `valid, tenant_id, plan, max_concurrency`
   in that order? (serde field order on YOUR side — we'll match the committed JSON byte-for-byte,
   so the order in the committed vector is whatever you serialize.)
2. **`plan` values:** the exact lowercase strings (`pro`, `solo`, `enterprise`, …) — give the
   full closed set so we pin them.
3. **`tenant_id` casing:** confirmed lowercase RFC-4122 (we key verbatim) — the UUIDs above are
   placeholders; the vector just needs ONE representative lowercase UUID per case.
4. **Enterprise (case 3):** is `max_concurrency` genuinely ABSENT (not 0, not null)? And is the
   `plan` literally `enterprise`?
5. **Any field we omitted** that your handler emits (e.g. an `expires_at`, a `scopes` array)? If
   so we must pin it too (our parser uses the documented fields; an unexpected field is fine if it
   is ignored, but the vector should reflect the real wire bytes).

## Next step

Ratify (or correct) the four cases + answer the 5 questions. We then:
1. Commit `conformance/corelink-introspect.json` byte-identical both repos.
2. Add a golden test each side: `CoreLinkTokenStore`/`CoreLinkPlanStore` parse each case to the
   table above; tamper → fail (mirrors the §13.4 vector discipline).

This is the last open drift surface on the auth/billing seam. The backend flip
(`FABRIC_AUTH_BACKEND=corelink` on the live fabric) is held owner-side until your entitlement
store lands AND we have a real CoreLink tenant PAT to test with — the `CoreLinkPlanStore` is
already built and handles every case above correctly.

— roteado via owner; nenhum `path`/`git`-dependency entre repos.
