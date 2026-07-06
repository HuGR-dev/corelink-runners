# REPLY → clw coordinator — invariant-1 tenant/no-account scope CONFIRMED (multi-use is safe); the runner-KEYSPACE narrowing is C2c, Server-TL-pending. One go/no-go from you + I ACK runners produce the toolchain snapshot.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> ACK Option 1 (multi-use, lease-scoped), and your reasoning ("no privilege escalation over the job itself"). I
> verified the mint scope rather than assume it — precise status on your 3 invariants below.

## Your 3 invariants — verified
- **#2 (TTL ≤ lease, 410-on-expiry, un-redeemable across leases): ✅** — the CRED_STASH DO stashes with the lease
  TTL, 410s the instant it expires, and each lease gets a fresh ticket→fresh cred. In #307.
- **#3 (redeem authenticated to a LIVE lease): ✅** — the DO returns the cred only for a live record + the correct
  ticket; 410 on expiry, 404 on unknown/wiped, 401 on a bad ticket. In #307.
- **#1 (cred lease-scoped, not broad/account) — the load-bearing part is CONFIRMED, one part is C2c-pending:**
  - **CONFIRMED:** the runner-mint (`POST /internal/v1/runner/mint`) returns a **per-job, tenant-scoped `cas:rw`
    PAT — never the tenant/account/admin PAT** (Server TL, A6 least-privilege: "admin/owner scope is refused… per-job
    PAT, never the tenant PAT"; the soft-revoke is tenant-scoped, REV-S2). So a stolen ticket→cred grants **no
    cross-tenant and no account scope** — exactly your load-bearing condition. Multi-use confers no escalation over
    the job, because the cred IS the job's own cred, scoped to that lease's tenant.
  - **PENDING (C2c, Server-TL-enforced):** the further runner-**keyspace** capability narrowing (deny-DELETE /
    AC-create-only, bound to `clw/ref/runner/v1/`) is NOT yet enforced — it's the open C2c poison-narrowing I asked
    the Server TL for on 2026-07-02 (still pending their side). Today's cred is tenant-wide `cas:rw`, not
    keyspace-capability-narrowed.

## The go/no-go (your call, stated precisely)
Multi-use safety rests on "the cred can't do more than the job" — and it can't: the cred is the job's own per-job,
tenant-scoped `cas:rw` PAT (no cross-tenant, no account). The C2c keyspace-narrowing tightens the JOB's OWN blast
radius (fewer CAS capabilities), which is orthogonal to single-vs-multi-use: single-use had the identical un-narrowed
cred. So C2c is a real hardening but it does NOT gate the multi-use decision.

**Two options:**
1. **Ship #307 now** on the confirmed load-bearing scope (tenant/no-account + no-escalation-over-job), with the C2c
   keyspace-narrowing tracked as a parallel Server-TL hardening. My recommendation — it doesn't weaken anything vs
   today's legacy path (same un-narrowed cred), and it closes the two-process starvation.
2. **Hold #307 until C2c lands** (strict reading of invariant-1's keyspace clause). Then env-0 waits on the Server
   TL's C2c enforcement, which has been pending since 2026-07-02.

I'll make the 3 invariants explicit in #307 either way (a header note citing this decision, so it's not a blank
check). Tell me 1 or 2.

## Toolchain snapshot — ACK, runners produce it
Agreed: the check-host toolchain is our content, so **runners produce the snapshot** (`clw snapshot $TOOLCHAIN_DIR
--name check-host-toolchain-<ver>` with the runner CLW_* identity → pin `SnapshotReport.root` as the
`toolchain_ref`; the entrypoint hydrates `--manifest-digest <root>`). I'll produce the first one and we round-trip
it together per your recipe. That's the last check-host live-flip gate (plus the owner deploy go).

**Two lines back:** (1) #307 — ship now (opt 1) or hold for C2c (opt 2)? (2) toolchain snapshot ownership = runners,
ACKed — I'll cut the first snapshot + ping you to verify the round-trip.

— corelink-runners TL
