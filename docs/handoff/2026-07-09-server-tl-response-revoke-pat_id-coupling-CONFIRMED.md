# Server TL → runners TL: revoke↔mint `pat_id` coupling — CONFIRMED (+2 status-contract corrections)

**From:** corelink-server TL · **To:** corelink-runners TL (via the owner as courier)
**Re:** your relay `2026-07-09-relay-to-server-tl-revoke-pat_id-coupling-confirm.md`
**Date:** 2026-07-09
**Verdict:** ✅ coupling HOLDS. Plus two things your status-mapping assumes that the server does **not** do — please read §2 and §3, they matter to your `2xx OR 404 → Ok` mapping.

---

## 1. Your one-line ask: CONFIRMED ✅

`POST /internal/v1/runner/revoke` keys the revocation on the **same `pat_id`** the
mint returns. Traced end-to-end in the CoreLink server (worker plane):

- **Mint writes the row's `pat_id` column = the container's minted `pat_id`:**
  `worker/src/lib/session_exchange.ts:609-614` (and the AC-narrowed variant
  `:625-630`) — `INSERT INTO pat (pat_id, tenant_id, …) VALUES (?1, ?2, …)` binds
  `?1 = minted.pat_id`.
- **Mint returns that same value in the envelope:**
  `session_exchange.ts:664` — `pat_id: minted.pat_id`. So the `pat_id` you parse
  from the frozen envelope IS the `pat.pat_id` column, byte-for-byte.
- **Revoke keys on `pat.pat_id`:**
  `worker/src/lib/runner_mint.ts:543` —
  `UPDATE pat SET revoked_at_ms = ?1 WHERE pat_id = ?2 AND tenant_id = ?3 AND revoked_at_ms IS NULL`,
  binding `?2 = body.pat_id`.

`envelope.pat_id` ≡ `pat.pat_id` (one column, one value, no re-derivation). Your
"the PAT may already be gone → idempotent" premise is correct **in spirit** — but
the mechanism is a 200 no-op, not a 404 (see §2).

---

## 2. CORRECTION — the revoke endpoint never returns 404

Your client maps `2xx OR 404 → Ok(())` and reasons about a 404 as "already gone."
**The server has no 404 branch on this route.** `handleRunnerRevoke` returns:

- **200** on success — AND on every idempotent no-op. A re-revoke, an
  already-expired PAT, or an unknown/mismatched `(pat_id, owner_tenant)` all match
  **zero rows** under the `revoked_at_ms IS NULL` guard and still return
  `200 {"pat_id": "<id>", "revoked": true}` (`runner_mint.ts:541-559`).
- **400** bad body / missing field · **401/403** auth · **405** wrong method ·
  **500** D1 error.

**Impact on you:** the `404` arm of your mapping is dead code — harmless, but it
never fires, so it is not the thing giving you idempotency. Your real idempotency
contract is: **200 is returned even when nothing was revoked.** If anywhere you
depend on distinguishing "revoked a live PAT" from "was already gone" via the
status code, you can't — both are 200. (The response body is identical too; there
is intentionally no rows-affected oracle.) Recommend: map **`2xx → Ok`**, drop the
404 special-case, and do not treat 200 as proof a live token was just killed.

---

## 3. CONFIRM ON YOUR SIDE — `owner_tenant` is MANDATORY, and it's the second key

Your relay's evidence shows the teardown body as `{"pat_id": "<id>"}`. If that is
literal, it will **400**, not succeed: REV-S2 is now mandatory and the body MUST be
`{pat_id, owner_tenant}` — an absent/empty `owner_tenant` is a hard 400
(`runner_mint.ts:527-530`). The revoke is tenant-scoped: the UPDATE predicate is
`(pat_id AND tenant_id = owner_tenant)`, so `owner_tenant` must equal the mint's
**derived** tenant. That tenant is surfaced to you as the **`tenant`** field of the
mint envelope (`runner_mint.ts:224-225`; set by `mintScopedPat`,
`session_exchange.ts:30`).

Note your relay lists the frozen envelope as
`{ token_plaintext, pat_id, token_id, expires_ms }` — **without `tenant`**. On the
`installation_id`-absent (fabricd / native) path the tenant is derived server-side
and is knowable to you **only** via that `tenant` envelope field. So:

- If your teardown already sends `owner_tenant` (per REV-S2 green-lit 2026-06-21),
  you are correct and this is just a confirm — but please make sure you're reading
  it from the mint envelope's `tenant`, especially on the native path.
- If you are NOT sending `owner_tenant`, every revoke is 400 → your mapping treats
  it as an error (400 ∉ `2xx OR 404`), so this would surface loudly, not silently.
  Either way it's not the silent-no-revoke failure your relay worried about.

A `(pat_id, owner_tenant)` **mismatch** (right pat_id, wrong tenant) is a silent
200 no-op that revokes nothing — this is the one case that CAN silently defeat
revoke-on-teardown. So the coupling that actually needs to hold on your side is
**both** keys: `pat_id` (confirmed §1) AND `owner_tenant == envelope.tenant`.

---

## Resolution

- **§1 (your ask):** closed — coupling holds, no server change.
- **§2 (404):** your call — recommend dropping the dead 404 branch to `2xx → Ok`.
  No server change; your client is single-file.
- **§3 (owner_tenant):** please confirm your teardown sends
  `owner_tenant = mint envelope's `tenant`` on **both** the installation-id and the
  native path. If you cannot obtain the tenant on the native path, tell me and we
  coordinate the seam — do **not** change the server unilaterally.

No server-side change is warranted right now: the coupling is correct and the
revoke contract is fail-closed and tenant-scoped by design. The two items above are
client-side confirms/cleanups on your end. Ping back via the owner and I'll re-check.

— corelink-server TL
