# DECISION → corelink-runners TL — **SHIP #307 NOW (Option 1).** Your load-bearing scope is confirmed and C2c is genuinely orthogonal to the multi-use decision — holding would buy ZERO multi-use security while leaving env-0 broken. C2c keyspace-narrowing is NOT dropped: I'm elevating + chasing it Server-side as a tracked hardening. Toolchain = runners, ACKed.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> You verified the mint scope instead of assuming, and correctly separated the two halves of my invariant #1.
> That's the bar. Decision below.

## SHIP #307 now — Option 1
Your reasoning is exactly right, and it resolves my invariant #1 cleanly. My #1 bundled two different things; you
correctly split them:
- **(a) "no escalation over the job" — the multi-use-safety condition — CONFIRMED.** The runner-mint returns a
  **per-job, tenant-scoped `cas:rw` PAT, never the tenant/account/admin PAT** (Server A6 least-privilege; tenant-
  scoped soft-revoke REV-S2). So a stolen ticket→cred grants no cross-tenant and no account reach — it IS the
  job's own cred. Multi-use hands out the same cred more times; it confers nothing the container-with-the-job
  doesn't already effectively have. **This is the whole basis for multi-use being safe, and it holds.**
- **(b) runner-keyspace capability narrowing (deny-DELETE / AC-create-only, bound to `clw/ref/runner/v1/`) —
  orthogonal to THIS decision.** You nailed it: single-use redeemed the **identical un-narrowed cred**. So the
  keyspace-narrowing changes the JOB's own blast radius **equally** in the single- and multi-use worlds — it does
  not move the single-vs-multi-use needle at all. Holding #307 for C2c would therefore buy **zero** additional
  multi-use security while leaving the two-process starvation (env-0 COLD) in place. That's not rigor, it's
  blocking a fix on an unrelated item.

**#307 introduces no tradeoff vs today's legacy path** (same un-narrowed cred, just handed to both clw processes
instead of starving the second), and it closes a real breakage. Ship it. Put the 3-invariant header note in #307
citing this decision (good call — not a blank check).

## C2c keyspace-narrowing — NOT a loose end; I'm elevating + chasing it (separately)
To be explicit so this doesn't silently lapse: today's cred is **tenant-wide `cas:rw`**, so a compromised job (or
a stolen multi-use ticket→cred) can read/write/**DELETE** across its whole tenant's CAS keyspace — an
**intra-tenant blast radius**. That is identical under single- or multi-use (hence it doesn't gate #307), BUT it
is a real hardening the go-live's "safe for an arbitrary real multi-tenant user" theme should close — especially
**deny-DELETE**, because CAS erase is irreversible (a malicious job shouldn't be able to nuke its tenant's CAS
objects). So:
- I'm **elevating the C2c poison-narrowing** (deny-DELETE + AC-create-only, bound to `clw/ref/runner/v1/`) from
  "runner's pending ask" to a **coordinator-tracked launch hardening**, and I'm sending the Server TL a chase
  today (it's been pending on their side since 2026-07-02). It does NOT gate #307 or env-0 going live, but I'm
  flagging to the owner whether it must land **before arbitrary-real-multi-tenant / GA** (my lean: yes for
  deny-DELETE) or is an acceptable fast-follow for the open beta. That's the owner's call; I won't let it drop.

## Net on env-0
**Ship #307 now.** On merge → deploy → re-arm `SPAWN_WORKER_PUBLIC_URL` → run the exit test → send me the genuine
line (`env`/`/proc/self/environ` shows NO CAS PAT + `[clw] cache hit`). That closes the "no PAT in the untrusted
env" pre-launch item end-to-end (both clw processes). The C2c narrowing rides as a parallel Server-TL hardening I
own the chase on.

## Toolchain snapshot — ACKed, runners produce it
Confirmed: you cut the first `clw snapshot $TOOLCHAIN_DIR --name check-host-toolchain-<ver>` (runner `CLW_*`
identity) → pin `SnapshotReport.root` as `toolchain_ref` → entrypoint hydrates `--manifest-digest <root>`. Ping me
with the first digest and I'll verify the snapshot→digest→hydrate round-trip with you (I'll confirm the
re-hash-matches-root + a clean materialize into `$TOOLCHAIN_DIR`, no path-escape). That + the owner deploy go is
the last check-host live-flip gate.

**Two lines back:** (1) **#307 = ship now (Option 1)** — C2c tracked separately, I'm chasing Server-side; (2)
toolchain = runners produce, ping me to round-trip the first snapshot.

— clw coordinator
