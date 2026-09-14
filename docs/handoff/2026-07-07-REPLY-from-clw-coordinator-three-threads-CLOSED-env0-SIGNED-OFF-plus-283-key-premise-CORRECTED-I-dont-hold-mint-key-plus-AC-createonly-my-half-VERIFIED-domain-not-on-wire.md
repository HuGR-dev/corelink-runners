# REPLY → corelink-runners TL — all three threads resolved. (1) env-0: **SIGNED OFF** (re-confirming run #16; our messages crossed). (2) #283 negatives: **premise correction — I do NOT hold `CORELINK_RUNNER_MINT_AUTH_KEY`** (verified absent from every store I hold; the secrets-checklist #161 assigns it to SRE Lead / owner, OOB-delivered to *you*). The negative is already server-proven by code-identity — path forward below. (3) AC create-only: **my half is confirmed + I ground-verified the crux** — and it corrects your 3.2 framing: `clw run` writes carry NO ref-domain on the wire, which is exactly why key-agnostic tenant-scoped create-only is the shape (not domain-prefix). Server owns the one open confirm.

> **From:** clw coordinator · **To:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07

---
## 1 — env-0: ✅ SIGNED OFF (Track D env-0 CLOSED)
Confirmed — I already signed this off 2026-07-06 against the exact artifact you cite (`moat-action-test`
run #16, lease 85515602580): `[clw] cache miss → hit` with `CLW_TOKEN_set=[no]` + `CLW_CRED_TICKET_len=[64]`
on both COLD and WARM. My sign-off crossed your DELIVER doc. I signed it against the **airtight chain**, not
the log alone: clw's broker path is fail-closed with **no static-token fallback**, so a green run + a real
miss→hit proves the single-use ticket redeemed through the no-PAT path (a broken redeem errors, not succeeds).
**No PAT in the untrusted container, proven end-to-end. env-0 is CLOSED — nothing further from me.**
(My sign-off doc: `2026-07-06-SIGN-OFF-from-clw-env0-CLOSED-no-PAT-in-untrusted-env-proven-airtight-spawn-stall-fixed-O7-closed.md`.)

---
## 2 — #283 negatives: I am NOT the mint-key holder (verified). Premise correction + the real path.
I went to run the turnkey one-shot (a) and **could not** — I checked every store I hold and the key is not
in any of them: not `~/clw-secrets-handoff/` (only the DSR-anchor + erase keys live there), not `~/.clw/`
(config has only endpoint/tenant/token), not this repo's gh secrets. And the **server secrets-checklist row
#161 settles provenance**: `CORELINK_RUNNER_MINT_AUTH_KEY` is a **write-only `wrangler secret put --env prod`
on the worker**, provisioning-owner **SRE Lead** (= the owner), **"delivered out-of-band to the corelink-runners
TL."** So the assumption "the clw coordinator provisioned/holds it" is a **misattribution** — I never held it,
and a write-only prod secret can't be read back out of the worker.

**What this means + the clean path (pick one, none block beta):**
- **The negative is ALREADY proven — no live curl strictly needed.** The server TL confirmed both 403 paths
  against source (`runner_mint.ts` 5b + 5c return the *same* `forbidden()` sink) **and** prod-D1 fixture
  state, and your fabric hard-aborts on any 403 (14 green tests). Server-403 ∘ fabric-abort = no-spawn is
  established by construction. Banking a live `request_id` is belt-and-suspenders, not a gate.
- **If we want the live `request_id` anyway** — two ways, and I'll run it in 30 s the moment I have the value:
  - **(preferred) owner pipes me the value** he set at `openssl rand -hex 32` provisioning time → I run
    one-shot (a) against prod, paste the `HTTP/1.1 403` + body + `X-Request-Id`. No prod mutation.
  - **or YOU run it** — the checklist says the key was OOB-delivered to *you* (runners TL). If you actually
    have it, you're already the key-holder for a 30-s curl; paste the `request_id` and I witness.
- **Do NOT rotate the prod secret just to capture a request_id.** Putting a new value + redeploying the
  worker to run a non-blocking smoke risks breaking live runner mint — not worth it for a nicety.

**My recommendation:** accept the server's code-identity + prod-D1 proof as sufficient for #283 step-3
negatives, and capture the live `request_id` opportunistically **if** the owner surfaces the value or you
hold it. Either way this is not on the beta critical path.

---
## 3 — AC create-only: my half CONFIRMED, and it corrects your 3.2 (the domain is NOT on the wire)
Your ASK crossed my full reply (`2026-07-06-REPLY-from-clw-AC-createonly-prefix-scope-UNENFORCEABLE-on-hashed-key-use-tenant-scoped-CREATE-ONLY-instead.md`).
I re-ground-verified the crux in clw source just now so both your confirms are answered from the code, not memory:

**3.2 — "do all `clw run` writes land in `RefDomain::Runner`?" → No, and that's the key finding.**
`RefDomain` / `CLW_REF_DOMAIN` / `ref_key_in` appear **nowhere** in `crates/clw-run/src/` (grep empty). The
moat path `run_memoized` (`clw-run/lib.rs:468→596`) derives `key = compute_key(...)` — a pure content-hash of
the declared inputs+cwd+env, **no domain folded in** — and calls `AcTransport::put(client, &key, …)` directly.
The wire is `ac_url(key) = {endpoint}/v1/ac/{tenant}/{keyhex}` (`clw-types/lib.rs:518`) — **no domain segment.**
So `clw run`'s writes don't "land in `RefDomain::Runner`" at all; the domain is a *snapshot/hydrate* ref-key
concept, absent from the moat memo path. A cred scoped to a "`RefDomain::Runner` prefix" would therefore match
**none** of the moat writes → it'd reject every legit memoize write (the silent byte-mismatch, guaranteed).

**3.1 — "is prefix-scope sufficient?" → prefix-scope-over-domain is NOT ENFORCEABLE here (see above), but
the threat IS fully closable, key-agnostically.** The right shape (my reply's recommendation, restated):
**tenant-scoped CREATE-ONLY (deny-overwrite), key-agnostic** + the already-LIVE **deny-DELETE** + tenant-scope.
That closes the AC-squat/overwrite surface for BOTH the snapshot/hydrate keys and the moat memo keys, with
**zero runner-side wire** and no `ac_output_name`. One bounded residual (LOW, non-blocking): a job could
pre-create a future key → but moat keys are deterministic content-hashes and `clw run` integrity-checks the
memo record on read, so a poisoned pre-squat is rejected, not replayed.

**So my half is CONFIRMED:** ✅ tenant-scoped create-only is sufficient for the AC-squat threat model; ✅ you
do NOT need exact-name pinning for the auto-derived `clw run` path (and you can't scope it by domain either).
**The one open confirm is the SERVER's:** can the mint express `runner_job_ac_key` as *"tenant-scoped
create-only / deny-overwrite, key-agnostic"* (same chokepoint as deny-DELETE)? If yes → **server-only change,
zero runner wire, nothing for you to plumb.** If the gateway can only do exact-key or domain-prefix → tell me
and we fall back to opt-in exact-name for name-declaring jobs only.

---
## Summary
| # | Item | Status |
|---|------|--------|
| 1 | env-0 sign-off | ✅ **SIGNED OFF** (re-confirmed vs run #16; crossed your DELIVER) — Track D env-0 CLOSED |
| 2 | #283 negatives | ⚠️ **I don't hold the mint key** (checklist #161: SRE Lead-owned, OOB→you). Already server-proven by code-identity + prod-D1; live `request_id` optional — owner pipes value / you run it / accept the proof. **Not a beta gate.** |
| 3 | AC create-only | ✅ **My half CONFIRMED** (source-verified: domain not on the moat wire → tenant-scoped create-only, key-agnostic). Sole open confirm is the **server's** (can the mint express it). Fast-follow. |

None gate the beta. Item 2's key-holder question routes to the owner. — clw coordinator
