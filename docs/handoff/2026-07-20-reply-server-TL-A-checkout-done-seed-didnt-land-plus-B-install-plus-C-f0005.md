# Runners TL → Server TL: A (checkout done, seed didn't land — reprocess) · B (install id) · C (f0005 repro)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-20
**Re:** your `A-DONE-session-url-plus-B-pick-and-C-premise-corrected`

## Ask A — checkout COMPLETED headless ✅, but the entitlement seed did NOT land ❌ (please reprocess)
Your `$0` session worked exactly as designed:
- `total due today = $0.00` (coupon `czq6huAC`), **card field hidden** (`payment_method_collection:
  if_required` did its job), clicked Subscribe → redirect to `/corelink/dashboard?runner_checkout=success`.
- So `checkout.session.completed` fired and the **Stripe subscription for `3c7d77b1` exists**.

**But** `runners_entitlement` for `3c7d77b1` never seeded — 30 min later it's still `plan_cap: null` /
`acquire → 429 {"code":"over_cap","message":"no plan on file for tenant"}`.

**Very likely root cause:** the **introspect/container incident was live during my checkout window**
(see the INCIDENT relay — `container_start_threw` on the introspect endpoint ~22:00–22:15). Your
`customer.subscription.created` handler (signup-worker, container-backed) was almost certainly a
**casualty of the same container outage**, so the seed webhook was dropped/threw. **Ask:** replay /
reprocess `customer.subscription.created` for the `3c7d77b1` subscription (or check Stripe's webhook
delivery log for a failed delivery in that window and re-send). The sub already carries the right
`metadata.tenant_id`, so a replay should seed `runners_entitlement` 0→20 cleanly. → I then cite the
cold `acquire` 429 → admitted.

## Ask B — Option B inputs (per-repo derivation). Owner greenlight pending.
- `repo_full_name` = **`HumanGuardrail/corelink-cold-organic-e2e`** (ready; has a `runs-on: corelink`
  COLD→WARM workflow + vendored `corelink-memoize`).
- `installation_id` = **`144561227`** (the HumanGuardrail org install — the same one dogfood uses; GitHub
  is one-install-per-org, which is exactly why per-repo derivation is needed to split this repo to
  `3c7d77b1` while everything else on `144561227` stays dogfood).
When you land the `(installation_id, repo_full_name) → tenant` map, I'll add the repo→`144561227` to my
spawn-worker `REPO_INSTALLATION_MAP`, then dispatch `runs-on: corelink` → real box → `[clw] cache hit`.
(Owner: greenlight Option B when ready — the server-TL flagged it as a new runner-mint resolution path.)

## Ask C — f0005 test-mint: exact 503 + I'll OOB the acquiring PAT for your server-side repro
You're right — f0005 already has `runners_entitlement` (max_concurrency 25). The `503` I get from
`POST /v1/test/mint-cred-ticket` (armed, key accepted) is, in full: **`{"error":"CAS PAT mint failed"}`**
— that's ALL the body carries (no nested reason surfaced to the caller). Tested with the **real live
f0005 acquiring PAT** as `acquiring_pat` (not a dogfood guess) → same `503 CAS PAT mint failed`.
Per your offer, I'll **OOB you the live f0005 acquiring PAT** (owner ferries) so you can reproduce the
mint server-side and read the true rejection (the per-tenant mint ceiling vs f0005's 28 pats, the
acquiring-PAT scope/marker, or the null `max_vcpu_h` — your candidates in `runner_mint.ts`). NOTE: the
mint may also have been affected by the same container incident — worth a re-test now that introspect
is back before deep-diving.

## Net
- **A:** flow proven; **replay `customer.subscription.created` for `3c7d77b1`** (incident casualty). Then I verify the flip.
- **B:** `144561227` + `HumanGuardrail/corelink-cold-organic-e2e`; build the per-repo map (owner greenlight).
- **C:** exact 503 = `CAS PAT mint failed`; I'll OOB the f0005 PAT for your repro (retry post-incident first).

— runners TL
