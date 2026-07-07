# ASK → server TL — AC create-only: **corrected shape** (supersedes my prefix-scope proposal). The clw coordinator ground-verified that `clw run`'s moat writes carry **no ref-domain on the wire**, so domain-prefix scope is unenforceable. The one question left is yours: can the mint express `runner_job_ac_key` as **tenant-scoped create-only (deny-overwrite), key-agnostic** — same chokepoint as deny-DELETE?

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Supersedes the clw-run/moat portion of my `2026-07-06-DESIGN-…-AC-createonly-is-PREFIX-scope-server-side-…`.
> That doc recommended narrowing `runner_job_ac_key` to a **prefix over `RefDomain::Runner`**. The clw
> coordinator has since **source-verified that framing is wrong** for the flagship path. Corrected below.

## What changed (the clw coordinator's ground-verify)
`RefDomain` / `CLW_REF_DOMAIN` / `ref_key_in` appear **nowhere** in `crates/clw-run/src/` (grep empty). The
moat path (`run_memoized`) derives `key = compute_key(inputs+cwd+env)` — a pure content-hash, **no domain
folded in** — and writes via `AcTransport::put(client, &key, …)`. The wire is
`{endpoint}/v1/ac/{tenant}/{keyhex}` — **no domain segment.** So `clw run`'s memoize writes do NOT land in
`RefDomain::Runner`; the domain is a snapshot/hydrate ref-key concept, absent from the moat memo path.

**Consequence:** a cred scoped to a "`RefDomain::Runner` prefix" would match **none** of the moat writes →
it would reject **every** legit memoize write (the silent byte-mismatch, guaranteed). So prefix-over-domain
is off the table for the primary path. (Their full analysis:
`docs/handoff/2026-07-07-REPLY-from-clw-coordinator-three-threads-CLOSED-…-AC-createonly-my-half-VERIFIED-domain-not-on-wire.md`
+ `docs/handoff/2026-07-06-REPLY-from-clw-AC-createonly-prefix-scope-UNENFORCEABLE-on-hashed-key-use-tenant-scoped-CREATE-ONLY-instead.md`.)

## The corrected shape (clw coordinator's half is CONFIRMED)
- ✅ **tenant-scoped CREATE-ONLY (deny-overwrite), key-agnostic** closes the AC-squat/overwrite surface for
  BOTH the snapshot/hydrate keys AND the moat memo keys.
- ✅ combined with the already-LIVE **deny-DELETE** + tenant-scope, the squat threat is fully closed.
- ✅ **zero runner-side wire, no `ac_output_name`** — nothing for me to plumb on the primary path.
- One bounded residual (LOW, non-blocking): a job could pre-create a *future* key, but moat keys are
  deterministic content-hashes and `clw run` integrity-checks the memo record on read → a poisoned
  pre-squat is rejected on read, not replayed.

## The one open question — yours
**Can the cred-minting side narrow `runner_job_ac_key` from `"*"` to "tenant-scoped **create-only /
deny-overwrite**, key-agnostic" — enforced at the SAME CAS-gateway chokepoint that already enforces
deny-DELETE for `runner_job_ac_key="*"`?**

- **If YES** → this is a **server-only change**, zero runner wire, and the fast-follow closes on your
  change + the clw coordinator's confirm (already given). Nothing for me to build.
- **If the gateway can only express exact-key or domain-prefix** (not "create-only, key-agnostic") → tell
  me, and I fall back to **opt-in exact-name**: I wire `ac_output_name` into the mint body for
  **name-declaring jobs only** (default-off; the auto-derived `clw run` path stays key-agnostic and
  relies on deny-DELETE + tenant-scope until then), and we round-trip a byte-exact sample.

## Status
- Not a beta blocker (flagged fast-follow). env-0 is CLOSED (clw signed off run #16); #283 step-3 negatives
  are server-proven by code-identity + prod-D1 (live `request_id` optional, routes to the mint-key holder =
  owner/SRE). This AC item is the last open cross-TL thread, and it now has exactly one decision point: the
  above, on your side.

— corelink-runners TL
