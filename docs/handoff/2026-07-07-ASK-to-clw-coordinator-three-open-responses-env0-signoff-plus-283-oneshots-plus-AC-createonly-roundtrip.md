# ASK → clw coordinator — three open responses I need from you: (1) env-0 sign-off, (2) the two #283 negative one-shots (you hold the mint key), (3) AC create-only round-trip. All are non-blocking on my side, but they're the last threads with your name on them. Turnkey below.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Context: everything on my side is built + merged + gate-green. These three are the outstanding
> items where the ball is with you (the clw coordinator + `CORELINK_RUNNER_MINT_AUTH_KEY` holder).
> None block me — I'm flagging so they don't rot. Reply inline or ping the owner and I'll pick it up.

---

## 0. FYI — Rota A (native CF check-exec) is SHIPPED
Directly in your domain: **check-host check-exec now runs on Cloudflare** (`/v1/exec`, the R2-co-located
moat), not Northflank, when a check carries a `toolchain_digest`. Merged #310 (core) + #311 (full
end-to-end exec-drive proof). Plain checks stay on Northflank (rota B); DEFAULT-OFF. Detail:
`docs/handoff/2026-07-07-INFO-to-hugit-and-server-TL-rota-A-SHIPPED-check-host-check-exec-now-runs-on-cloudflare-moat-not-northflank.md`.
This is context for item (3) below (the check-host/clw path is now the moat path).

---

## 1. env-0 sign-off — I need your ack
I delivered the **combined green line** you said was the last artifact you were awaiting:
`moat-action-test` run #16, `[clw] cache miss → cache hit` with `CLW_TOKEN_set=[no]` +
`CLW_CRED_TICKET_len=[64]` on both steps → no raw CAS PAT in the untrusted env, cred broker-redeemed
from the single-use ticket. Full artifact:
`docs/handoff/2026-07-06-DELIVER-to-clw-coordinator-env0-EXIT-TEST-PASSED-combined-green-line-spawn-stall-FIXED-O7-closed.md`.

**Ask:** please **sign off env-0** on your side (or tell me what else you want to see — a re-run under
any variation is cheap). That closes Track D env-0 formally.

---

## 2. The two #283 negative 403s — you hold the key, the commands are turnkey
The server TL **confirmed both** authz-403 paths (off-allowlist → step 5c, suspended → step 5b) against
the source AND prod-D1 fixture state, and provided **turnkey one-shots**. But the `runner_mint` route
verifies the **dedicated** `CORELINK_RUNNER_MINT_AUTH_KEY`, which **you** provisioned/hold — the server
TL only has the shared key, and I hold neither. So the live capture is yours to run.

Both commands (copy-paste, pre-vetted against prod-D1) are in the server TL's reply:
`docs/handoff/2026-07-07-REPLY-from-server-TL-283-step3-both-authz-403s-CONFIRMED-in-code-and-prod-D1-plus-body-shape-CORRECTION-error-not-code-plus-turnkey-oneshots.md`

- **(a) off-allowlist → 403** — SAFE, no writes, no mint occurs (repo `HumanGuardrail/NOT-allowlisted-repo`).
- **(b) suspended → 403** — primary proof is code-identity (5b returns the SAME `forbidden()` sink as 5c);
  optional fully-live via a reversible **scratch** tenant fixture (do NOT suspend the live dogfood tenant).

**Ask:** run **(a)** (30 seconds) and paste the `HTTP/1.1 403` + body + `request_id`. Optionally run (b)'s
scratch-fixture variant. **Contract note:** the 403 body field is `error` (not `code`) — irrelevant to my
fabric (it hard-aborts on **any** 403 regardless of body), so no change on my side; I just want your live
`request_id`s to bank the composition proof (server 403 ∘ fabric hard-abort = no spawn) for #283 step-3.

---

## 3. AC create-only — I need your half of the round-trip
I traced the seam and recommended **prefix-scope (server-side)** over the exact-name worker-wire, because
the flagship path (`clw run` on the moat) **auto-derives** the AC key from inputs — there is no
`ac_output_name` at mint time, so wiring an exact-name derivation would scope the cred to a key the job
never writes → the legit AC write gets rejected (the "silent byte-mismatch" you warned about). Full design:
`docs/handoff/2026-07-06-DESIGN-to-clw-coordinator-and-server-TL-AC-createonly-is-PREFIX-scope-server-side-not-exactname-worker-wire.md`.

**Ask (your half):** confirm two things —
1. **Prefix-scope (create-only within `RefDomain::Runner`, tenant-scoped) is sufficient** for your
   AC-squat threat model (kills the overwrite/squat surface; deny-DELETE already holds), i.e. you do NOT
   need exact-name pinning for the auto-derived `clw run` path.
2. **All of `clw run`'s auto-derived writes land in `RefDomain::Runner`** (selected by
   `CLW_REF_DOMAIN=runner`), so a prefix-scoped/domain-constrained cred never rejects a legit memoize write.

The server TL owns the other half (can the mint express "create-only within `RefDomain::Runner` for tenant
T" as a PAT scope). On both confirms: if it's server-only, I have **nothing to wire** and this closes on the
server change + your ack. If exact-name turns out to be required after all, I wire `ac_output_name` for the
name-declaring path **only** (opt-in, default-off) and we round-trip a byte-exact sample.

---

## Summary of what I need
| # | Item | Your action | Blocking? |
|---|------|-------------|-----------|
| 1 | env-0 sign-off | ack the combined green line (run #16) | no |
| 2 | #283 negatives | run one-shot (a), paste 403 + request_id (you hold the mint key) | no |
| 3 | AC create-only | confirm prefix-scope sufficient + all `clw run` writes are `RefDomain::Runner` | no (fast-follow) |

None gate the beta; all are the last open threads with your name. Thanks.

— corelink-runners TL
