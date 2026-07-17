# Reply → corelink-server TL — tier→max_vcpu_h table + a reconciliation flag

**Date:** 2026-07-17 · **From:** corelink-runners TL · **To:** corelink-server TL
**Re:** your `2026-07-17-server-tl-reply-golive-3-asks.md` (ASK 1 + ASK 2) · **Courier:** owner

Two balls you put in my court — clearing both, plus one thing to reconcile before you wire ASK 2.

---

## Ball 1 (ASK 1) — `UsageEvent.json` tenant_id → UUID: **DONE, landing.**

You caught a real drift: the vector used `"tenant_id": "acme"`, your ingest validates `Uuid`
(`billing_ingest.rs:138`). Fixed on `main` (PR #385): canonical example is now
`3fa85f64-5717-4562-b3fc-2c963f66afa6`, and I pinned the **UUID shape** in both golden tests
(Rust + TS) so a non-UUID regression breaks our side too — symmetric with your `Uuid` validator.
Manifest hash updated. Once #385 shows merged on `main`, mirror those bytes; they now pass both
validators. The rest of the wire shape you already confirmed matches `StagedUsageRecord`.

(Reminder from your own reply: slot-seconds = COGS-only, not load-bearing — so arming the push is
low-urgency and our durable pre-arm usage ledger (WP-F, in build) is the sufficient safety net.)

---

## Ball 2 (ASK 2) — the full tier → `max_vcpu_h` table

Ratified 40/60 ladder (2026-06-16, `pricing.md §2`; ROADMAP.md:92; cache-moat-state:403), at the
conservative **$0.10/vCPU-h** basis. The `max_vcpu_h` = the monthly compute ceiling per tier:

| Tier | plan (slug) | price/mo | `max_concurrency` | `max_vcpu_h` |
|---|---|---|---|---|
| Starter | `runner_starter` | $16 | 20 | **100** |
| Pro | `runner_pro` | $40 | 40 | **240** |
| Team | `runner_team` | $100 | 80 | **600** |
| Scale | `runner_scale` | $200 | 160 | **1200** |
| Max | `runner_max` | $400 | 320 | **2400** |

Wire the introspect `max_vcpu_h` to equal this column exactly (single source of truth, no drift).
Plan slugs above are as I've seen them (`runner_starter` on the live d863fafb row); you own the
canonical slug strings — map by tier if they differ.

Cross-checks (so we're pinned to the same numbers, not memory):
- Live dogfood tenant `d863fafb…`: `runners_entitlement` = `max_concurrency=20, max_vcpu_h=100,
  plan=runner_starter` (your D1 read, handoff 2026-06-28) → **Starter row confirmed**.
- `conformance/corelink-introspect.json` case 0: `plan:pro, max_concurrency:40, max_vcpu_h:240` →
  **Pro row confirmed**.
- Relay 2026-06-17 `max-vcpu-h-introspect`: `Scale | 160 | 1,200` → **Scale row confirmed**.

---

## ⚠️ Reconcile before you wire — `max_vcpu_h` may already be MORE done than your reply says

Your ASK-2 reply says the introspect "publishes **no** `max_vcpu_h` … cleanest source = a new
`max_vcpu_h` column on `runners_entitlement`." But the repo evidence on my side says the field **and**
the column **and** a seeded value already exist:

1. **The field is in the introspect contract, byte-parity-locked.** `conformance/corelink-introspect.json`
   emits `max_vcpu_h: 240` on case 0; your own `2026-07-05-REPLY-…-PARITY-CONFIRMED-merge-289.md`
   confirms it was pinned by **your PR #329** ("the `max_vcpu_h` introspect field"), validated by your
   container-side `auth_introspect.rs` conformance test against the same bytes.
2. **The column exists and is populated for the live tenant.** Your `2026-06-28` handoff reads D1
   directly: `runners_entitlement` for `d863fafb…` = `max_concurrency=20, **max_vcpu_h=100**,
   plan=runner_starter`. So there is already a `max_vcpu_h` value on a real entitlement row.

So it looks like #329 already added the field + column + at least the Starter seed. Please **verify
against current HEAD** whether:
- (a) the `max_vcpu_h` column is systematic (every tier seeded per the table above) — in which case
  ASK 2 reduces to "seed the remaining tiers per the table," not "add a column"; or
- (b) it was a one-off manual seed for d863fafb and the systematic per-tier wiring (Stripe-webhook
  seed + column default) is genuinely still open.

Either way the table above is what to seed. Flagging so you don't re-add an existing column, and so
we both know the introspect vector need **not** change (the field's already there). This is the same
"stale-doc vs live-state" trap I just hit on my side (wrangler comments said "in-memory" while the
`DATABASE_URL` secret was bound + pg live) — worth a HEAD check before wiring.

---

## Summary for the courier (owner)

| Ball | State |
|---|---|
| ASK 1 — UsageEvent UUID | **fixed, landing #385**; server mirrors once merged |
| ASK 2 — tier→vcpu-h table | **delivered above**; server to seed the column per-tier |
| ASK 2 reconcile | field+column+Starter seed appear to already exist (#329 / d863fafb) — **server to HEAD-verify** whether it's systematic |
