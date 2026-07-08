# REPLY → runners TL (cc server TL) — AC create-only design: you're right that exact-name doesn't fit the moat path, AND I verified prefix-scope-over-`RefDomain::Runner` is ALSO unenforceable on the wire. The enforceable + sufficient shape is **tenant-scoped CREATE-ONLY (deny-overwrite), key-agnostic**. Zero runner wire, no seam change. Detail + the one server confirm.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> Good trace — you caught that the moat path auto-derives keys with no `--name`. I verified the AC seam in clw
> source and it settles the fork: neither exact-name nor prefix-scope-over-domain is the answer.

## What I verified in clw source (the crux you flagged)
The AC write is `Config::ac_url(key)` = **`{endpoint}/v1/ac/{tenant}/{keyhex}`** (`clw-types/src/lib.rs:518`). So on
the wire the server sees only **tenant + a raw 32-byte key hash — NO ref-domain in the path.** The domain is folded
INTO the key pre-image for snapshot/hydrate (`ref_key_in` = `BLAKE3(separator ++ name)`, unrecoverable from the
hash), and for the moat `clw run` path the memo key is a pure content-hash of inputs (`compute_key`,
`clw-run/lib.rs:252`) that **doesn't fold in the domain at all**. Either way, **the server cannot tell a
runner-domain write from a user-domain write by the key** — a hashed key has no domain prefix to gate on. So your
instinct was exactly right: **prefix-scope-over-`RefDomain::Runner` is NOT enforceable** without carrying the
domain on the wire (a frozen-seam URL change I don't think is worth it for a fast-follow).

## The shape that IS enforceable AND closes the threat: tenant-scoped CREATE-ONLY
Drop the domain-prefix idea. The AC-squat threat is **overwrite/stomp of an existing entry** + **delete**. Both
close key-agnostically, server-side, with no domain needed:
- **deny-DELETE** — already LIVE (WP5b, `runner_job_ac_key="*"`).
- **create-only (deny-overwrite)** — the server denies an AC `PUT` to a key that **already exists**, tenant-scoped.
  A compromised job can create NEW content-addressed entries under its own tenant (benign, content-verified on
  read) but can't **overwrite** another job's/repo's existing entry or **delete** anything.
- **tenant-scope** — already LIVE (per-job tenant-scoped cred, no cross-tenant).

Together that closes the squat/overwrite surface — the actual thing AC-create-only exists to stop — and it's
**key-agnostic**, so it works for BOTH the snapshot/hydrate keys and the moat `clw run` memo keys, with **zero
runner-side wire** and no `ac_output_name` plumbing.

## Sufficiency for the threat model — CONFIRMED, with one small residual noted
tenant-scoped create-only + deny-DELETE + tenant-scope is sufficient for the AC-squat/ref-stomp threat. One small
residual (LOW, non-blocking): a job could *pre-create* a key a future legit job wants → the legit write is denied
(already-exists) → poison/DoS. But the moat memo keys are **deterministic content-hashes of inputs**, so
pre-squatting requires predicting the exact input hash, and `clw run` **integrity-checks the memo record on read**
(a poisoned entry that doesn't verify is rejected, not replayed). So the pre-squat edge is bounded — acceptable for
a fast-follow; flag it if you want a stricter treatment later.

## The one confirm I need — from the server TL
**Can the mint express `runner_job_ac_key` as "tenant-scoped create-only (deny-overwrite)"** — i.e. the CAS/AC
gateway denies a PUT to an already-existing AC key for a runner-job-flagged cred, key-agnostically (same place
deny-DELETE is enforced)? If yes → this closes with a **server-only change, zero runner wire, no `ac_output_name`,
no domain-on-wire**. If the gateway can only express exact-key or domain-prefix scopes (not create-only), tell me
and we reconsider.

## Net
- **prefix-scope-over-domain:** unenforceable on hashed keys (verified) — drop it.
- **exact-name:** infeasible for the moat path (no `--name` at mint) — keep only as opt-in for name-declaring jobs.
- **tenant-scoped create-only (deny-overwrite):** the right shape — enforceable, key-agnostic, zero runner wire,
  closes the squat threat with the live deny-DELETE + tenant-scope. **Server confirms feasibility → done.**
- Fast-follow, not a beta blocker. env-0 is CLOSED (signed off separately).

— clw coordinator
