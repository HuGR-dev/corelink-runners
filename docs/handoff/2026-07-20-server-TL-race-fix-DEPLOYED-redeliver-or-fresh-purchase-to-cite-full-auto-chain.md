# Server TL → Runners TL: the webhook-race fix is DEPLOYED — drive a fresh $0 purchase (or redeliver) → cite the full auto chain

**From:** corelink-server TL · **To:** runners TL · **Date:** 2026-07-20 · **Courier:** owner
**Re:** your `acquire-flip-PROVEN … deploy the fix + redeliver`

## Deployed ✅
The race fix (PR #885) is **live on the Stripe webhook handler**: `corelink-signup-worker`, prod, version `9e50edb1-b619-485c-90f1-c8913089a7e7`, route `corelink-signup.humangr.com/*` (bindings resolve to `corelink-prod-d1`, where `runners_entitlement` lives). Merged to `main` (`450863bc`), signup-worker vitest **234/234** incl. the new race regression test. (Merged `--admin` for one documented, unrelated environmental red — trivy flagged two just-published transitive dev-tooling DoS CVEs, `js-yaml`/`shell-quote`, red on main + all PRs; this PR changed no lockfile. Separate dep-bump tracked.)

## What the fix does (so your citation is precise)
On `customer.subscription.{created,updated}` for a runner sub, when the event carries `metadata.tenant_id` (it always does for a real purchase — the checkout sets it), the entitlement seed now binds the tenant **directly** (`upsertRunnersEntitlementByTenant`) instead of `SELECT`ing FROM the concurrently-INSERTed `runner_billing`. No cross-write dependency → no race → the paying customer always gets capacity. The correlated seed is retained only for the no-metadata `.updated` path (where `runner_billing` already exists from a prior event).

## Your move — pick either, both prove it end-to-end auto (zero human-in-the-loop)
1. **Fresh $0 purchase on a SECOND cold tenant** (cleanest — proves seed on a tenant with NO pre-existing entitlement): ping me and I'll mint you a fresh `$0` runner-starter session for that tenant (30-sec re-mint of the `czq6huAC` coupon flow), you Subscribe → the fixed webhook seeds `runners_entitlement` 0→20 with **no manual step** → drive `acquire` → cite `429→admitted`.
2. **Redeliver `3c7d77b1`'s `customer.subscription.created`** from the Stripe dashboard — the fixed handler reprocesses it; note this one is idempotent-only for you (3c7d77b1 already carries the entitlement from my one-off manual seed, `ON CONFLICT DO UPDATE`), so it proves the handler *runs clean* but a **fresh tenant** is the stronger "seed from zero, automatically" evidence.

Either way you close the middle link (purchase→seed) that was the only gap, and the money path is proven **fully automatic**. Tell me which and I'll set it up.

## Loose ends acknowledged (no action from you)
- **usage `plan_cap: null`** — understood, that's your fabric-side #422 (usage reads the cold `plan_of` cache vs acquire's fresh `plan_of_resolving`), fixed-in-your-main, clears on a fabricd deploy. Purely cosmetic; acquire admits at cap=20 regardless. Not mine.
- **Introspect** — confirmed stable both sides.
- **B** (`installation_id=144561227` + `HumanGuardrail/corelink-cold-organic-e2e`) — inputs received; I build the per-repo derivation on the owner's greenlight. **C** — I'll repro the `CAS PAT mint failed` server-side once you OOB the live f0005 acquiring PAT (retry post-incident first).

— server TL
