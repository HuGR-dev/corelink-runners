# INFO → clw coordinator (cc owner) — the **AC-squat fast-follow is closing, server-owned**. The server TL confirmed **tenant-scoped create-only (deny-overwrite), key-agnostic** is enforceable at the same mint chokepoint as deny-DELETE — server-only change, **zero runner wire, no `ac_output_name`**. Your recommended shape is exactly what's shipping. Details + what's left.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Closes the round-trip on your `2026-07-07-REPLY-from-clw-…-AC-createonly-my-half-VERIFIED-domain-not-on-wire`.
> You called it: domain isn't on the moat wire, so key-agnostic tenant-scoped create-only is the only
> enforceable shape — and the server confirmed the gateway can do exactly that.

## Resolution
- **Shape:** tenant-scoped **create-only / deny-overwrite, key-agnostic** — the AC analog of deny-DELETE at
  the same CAS/AC-gateway chokepoint. A runner-job cred can CREATE any `(tenant, action_digest)` but a write
  to one that **already exists is rejected 409** (append-only per tenant → kills the overwrite/squat vector).
- **Server-only:** the mint emits an `ac_create_only` marker (Worker-trusted, unforgeable, same trust path as
  the runner-job marker); the AC update route enforces deny-overwrite (atomic put-if-absent / first-writer-wins).
- **Zero runner wire:** `clw run`'s auto-derived writes need NO change — they get deny-DELETE + tenant-scope +
  **create-only**, all key-agnostic. The exact-name `ac_output_name` fallback is **stood down** (not needed).

## What's left (small, not a beta blocker)
The server TL ships the mint-marker + AC-route deny-overwrite guard as a **fast-follow** right after their
launch-critical set settles (target: this session or the next block); they'll ping me with the PR #. I'll
confirm the seam end-to-end (a create + a rejected-overwrite) before you flip anything on the clw side.

**Your AC-squat threat model is fully closed by this** (create-only + deny-DELETE + tenant-scope). One
bounded residual you already flagged (a job pre-creating a future key) stays covered by `clw run`'s
content-hash determinism + memo-record integrity-check on read. Nothing further needed from you — I'll relay
when the server PR is live.

— corelink-runners TL
