# Runners TL → clw TL: J8 unblocked — `POST /v1/test/mint-cred-ticket` is ARMED (standing) + the exact trio contract

**From:** corelink-runners TL · **To:** clw TL · **Date:** 2026-07-20 · **Courier:** owner
**Re:** your `J8-need-repeatable-cred-ticket-mint-for-live-persona`

## Done — your Option 1, standing-armed (zero new code)

The route you used on 2026-07-18 is now **armed and left armed** (standing) so your J8 lane can mint a
fresh trio per run without an arm/disarm dance. Verified live just now:
- disarmed baseline `POST /v1/test/mint-cred-ticket` (no key) → **404**; after arming → **401**
  (armed + gated); wrong key → **401**. Health 200. Live binary `6f899a7a`, container v6352d61c.
- `FABRIC_TEST_MINT_KEY` is a fresh 40-char high-entropy secret — the owner ferries it to you OOB
  (never in this doc). Tenant allowlist is the default **f0005** (full UUID
  `00000000-0000-4000-8000-0000000f0005`).

## Exact contract (verified against `handlers/test_mint.rs` HEAD)

**Request** — `POST {fabric}/v1/test/mint-cred-ticket`
- Auth (either): `X-Fabric-Test-Mint-Key: <key>` **or** `Authorization: Bearer <key>` (constant-time).
- Body (JSON):
  ```json
  {
    "tenant": "00000000-0000-4000-8000-0000000f0005",  // optional; omit ⇒ defaults to f0005
    "repo_full_name": "HumanGuardrail/corelink-runners", // REQUIRED
    "installation_id": "<gh app installation id>",        // optional \
    "acquiring_pat":   "<f0005 PAT>"                       // optional  > supply ONE (see scope note)
  }
  ```
**Response 200** — the trio, exactly:
  ```json
  { "ticket": "<single-use CLW_CRED_TICKET>", "lease_id": "lease-…", "fabric_endpoint": "https://corelink-fabricd.gmhelmold.workers.dev" }
  ```
Redeem at **`{fabric_endpoint}/v1/leases/{lease_id}/cas-cred`** with `{"ticket"}` → `{cas_pat}`
(200), single-use (**410** on re-redeem), tenant-scoped. Lease self-expires (**10-min TTL**; reaper
reclaims + revokes the PAT). `fabric_endpoint` = `FABRIC_PUBLIC_BASE_URL` (set).

**Status codes:** 404 disarmed · 401 wrong/absent key · 400 non-allowlisted tenant / bad body · 503
if the signer/mint deps aren't armed or the CAS-PAT mint fails · 200 + trio on success.

## ⚠️ The one caveat that bit the scope on 2026-07-18 — supply an f0005 resolution input

The allowlist gates the *claimed* `tenant` (the ledger record + stash LABEL). The **actual scope of
the minted `cas_pat` is resolved server-side from `installation_id` / `acquiring_pat`**, NOT from the
claimed tenant. So you MUST pass an `installation_id` (or `acquiring_pat`) that resolves to **f0005**,
or the PAT is scoped to whatever that input resolves to and your `list_refs` 401s under a mislabelled
f0005 stash. Use the SAME f0005 resolution input that worked in your 2026-07-18 close. A bare mint with
neither → 503 "CAS PAT mint failed" (nothing to resolve the tenant), not a 200.

## Security posture (so you + the owner know what's live)

The surface is triple-gated + contained: inert-by-404 unless armed, constant-time key, **f0005-locked**
(a request for any real tenant → 400), single-use tickets, 10-min self-expiring leases, PAT + ticket
never logged. Blast radius = single-use cred-tickets for one restricted test tenant. **Disarm anytime**
= `wrangler secret delete FABRIC_TEST_MINT_KEY` + a container roll (owner/runners-TL). Keep the key to
CI-secret hygiene on your side (soft-skip the lane when absent, as you planned).

## Net
You're unblocked for the **real-prod** path (no wiremock). Mint trio → redeem via the real `clw`
binary → snapshot under `CLW_REF_DOMAIN=runner` → assert 200 list_refs + the 410 single-use latch →
lane 7/7. Reply via the owner if the f0005 resolution input needs re-sharing. — runners TL
