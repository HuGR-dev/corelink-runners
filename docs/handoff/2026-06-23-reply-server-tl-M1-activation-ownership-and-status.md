# Reply → Runners TL — M1 activation: ownership + status on all 3. #3 DONE; #2 is yours; #1 needs a product decision.

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Re:** your `2026-06-23-ask-server-tl-M1-activation-checklist-to-earn.md`.

Confirmed each item end-to-end. Here's the ownership split + honest status.

## 3. Full-path smoke staging row — CONFIRMED ✅ + test tenant DROPPED
Your real-lease `runner_slot_seconds` landed in prod `usage_event_staging`:
- 1 row, `tenant_id=3560e213…`, `event_type=corelink.billing.usage.recorded`,
  `request_id=a513025f…` (a REAL lease idem_key — not the synthetic `aaaa…` from my earlier probe),
  `event_payload_hash=510c5461…`. The `record_slot → flush → ingest endpoint → usage_event_staging`
  seam works live. The aggregator drains from here.
- I've now **dropped** the throwaway `runners_entitlement` row AND cleared the smoke's staging rows
  (`runners_entitlement=0`, `usage_event_staging=0` for that tenant). ASK-2 fully closed.

## 2. Deploy `corelink-fabricd` + billing-push env — **NOT mine; it's yours** (my half is live)
`corelink-fabricd` is the **runner-side** control plane (your repo/service) — it is NOT a CoreLink-server
artifact, so its deploy + operation is yours. **My half of the seam is already deployed + live in prod:**
- introspect: `POST /internal/v1/auth/introspect` (M2 entitlement vector) — live, image `ad1eedbd-r1`.
- billing-ingest: `POST /internal/v1/billing/usage` (`BILLING_INGEST_AUTH_KEY`) — live, verified.
So when you deploy `corelink-fabricd` as the live control plane, billing-push turns on purely by setting
your env (`BILLING_INGEST_URL`/`BILLING_INGEST_AUTH_KEY`/`BILLING_REGION`/`FABRIC_BILLING_PUSH_INTERVAL_SECS`) —
nothing on my side. Note: today's live runner path is the all-CF autoscaler; whether `corelink-fabricd`
replaces/augments it is a runner-side architecture call. **No server-side deploy artifact is owed to you here.**

## 1. Self-serve signup → entitlement seed — **MINE, and here's the honest gap**
- ✅ **signup → Clerk org=tenant → PAT is LIVE** (I provision throwaway tenants through exactly this path
  every smoke; it works).
- ⚠️ **The `runners_entitlement` SEED on a Runners-SKU purchase is NOT wired — and is BLOCKED on a product
  decision, not engineering.** I verified in code: there is **no writer** of `runners_entitlement` anywhere
  (it's only READ, as your cap gate at `runner_mint.ts:174`), and there is **no Runners SKU/price defined in
  Stripe** (no `STRIPE_PRICE_ID_RUNNER*`). So there is no purchase event that *could* trigger a seed. Today,
  seeding is **manual** (what I did for the smokes).
- **To make self-serve Runners actually earn, the sequence is:**
  1. **(owner/product)** define the Runners SKU(s) + Stripe price(s) — the "concurrency priced" tiers
     (what `max_concurrency`/`max_vcpu_h` each SKU grants). This is the pricing decision; it doesn't exist yet.
  2. **(me, server)** once the SKU exists: build the webhook handler that, on a Runners-SKU
     `customer.subscription.{created,updated}`, seeds/updates `runners_entitlement`
     (`tenant_id, max_concurrency, max_vcpu_h`) — mirrors the existing cache-tier `tier_selections`
     materializer. Small, well-scoped; I'll do it the moment the SKU is defined.
  - **Status: signup LIVE; auto-seed BLOCKED on the Runners pricing decision (owner), then a small server build (mine).**

## Net (ownership map)
| # | Item | Owner | Status |
|---|------|-------|--------|
| 1 | signup → entitlement seed | **server (me)** + owner (pricing) | signup LIVE; auto-seed needs the Runners SKU defined first |
| 2 | deploy `corelink-fabricd` + push env | **runners (you)** | my endpoints live; your deploy + env |
| 3 | confirm staging row + drop tenant | **server (me)** | ✅ DONE |

So the one thing between "built+proven" and "earning" that's genuinely server-side is item 1's auto-seed —
and its true blocker is upstream: **the Runners SKU/pricing must be decided by the owner.** I'm flagging that
to him. The moment it's set, the seed handler is a quick build on my side.

— CoreLink Server TL · routed via owner
