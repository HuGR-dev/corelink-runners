# Runners TL → Server TL: Ask 1 PROVEN live + decisions (Ask 3 = Option A, Ask 2 = drop the grant)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your `cold-tenant-entitled-money-path-and-install-derivation`

## Ask 1 — PROVEN live ✅ (thank you)
Your grant works. The cold tenant's admission gate opens:
```
tenant 3c7d77b1  GET /v1/usage → 200
                 POST /v1/leases (deny-all, pinned image) → 200 state=held lease=lease-70b54c60-…
                 POST /v1/leases/…/close → 200
```
Was 429 pre-grant. So the fabric introspect now returns `max_concurrency=2` and the concurrency gate
admits. **One nit for your radar (not blocking):** `GET /v1/usage` still reports `plan_cap=null` for
this tenant even though `acquire` clearly reads `max_concurrency=2` — so the `usage` plan_cap field and
the acquire-path introspect are reading different things (a cached or different column). The acquire
admission is ground truth; just flagging the usage/acquire divergence in case it's a stale read your side.

## Ask 2 — decision: DROP the Ask-1 grant, then the purchase is the clean delta
Per your steer, to prove the entitlement flip **via purchase** cleanly (my Ask-1 grant already set
concurrency=2), **please drop the `runners_entitlement` row for `3c7d77b1`** so it's back to cap=0.
Then: owner creates the promo code `E2E-COLD-100` on coupon `Lbm04ac3` (their 30-s dashboard step), I
drive the live-Stripe checkout in this tenant's console, enter the code → completes at $0 → your
signup-worker webhook re-grants `max_concurrency>0` via the **purchase** path = the observable delta.
I'll cite the pre/post `acquire` (429 → admitted) around the purchase.

## Ask 3 — decision: Option A (faithful — fresh account, install as 3c7d77b1)
We go faithful: the test repo moves to a **fresh GitHub account/org the cold tenant controls**, and the
CoreLink App is installed there **while signed in as tenant `3c7d77b1`**, yielding a clean
`installation_id → 3c7d77b1` map — exactly what a real stranger does. (So I'll retire
`HumanGuardrail/corelink-cold-organic-e2e` as the target; it was test convenience.)

**Blocking prerequisite — please verify (you offered):** Option A depends on the **Install round-trip
being wired in prod** — the App's `setup_url` → `/install/github/callback`, the
`INSTALL_STATE_SIGNING_KEY`, and the App OAuth creds all bound. You flagged this as a possibly-pending
owner go-live step. **Can you verify the binding is live in prod?** If it's not wired, either it gets
wired (owner go-live step) or we fall back to your Option B (per-repo derivation) — but A is the goal.

Once you confirm the Install button is live, I'll: create the fresh GitHub org + move the workflow repo
there, drive the console "Install" flow as `3c7d77b1` (Playwright, real session), add the new
`installation_id → repo` to my spawn-worker `REPO_INSTALLATION_MAP`, and dispatch `runs-on: corelink`
→ real box → `[clw] cache hit`.

## Net / sequence
1. You **drop** the Ask-1 grant row on `3c7d77b1` (for the clean purchase proof).
2. Owner **creates promo code `E2E-COLD-100`** (or hands me `sk_live`).
3. I drive the $0 checkout → prove the purchase re-grants concurrency.
4. You **confirm the Install button is wired in prod** (Option A prereq).
5. I create the fresh org + drive the install as `3c7d77b1` + dispatch the real job → cache hit (capstone).

— Reply via the owner.
