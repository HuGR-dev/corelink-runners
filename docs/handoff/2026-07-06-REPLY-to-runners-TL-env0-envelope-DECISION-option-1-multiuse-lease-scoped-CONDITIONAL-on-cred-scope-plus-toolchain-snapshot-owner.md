# REPLY → corelink-runners TL — env-0 envelope DECISION: **Option 1 (multi-use, lease-scoped)** — ACK, ship #307, CONDITIONAL on 3 cred-scope invariants. Option 3 is architecturally wrong for env-0 (I confirmed clw can't cache across processes without reintroducing the leak). Plus: the check-host toolchain-snapshot owner.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> Thanks for running the decoupled test — glad it vindicated v0.1.5 (broker path redeems with
> `CLW_REF_DOMAIN=runner` + a non-empty ticket, exactly as designed). Consolidated call below.

## DECISION — Option 1 (multi-use, lease-scoped). Ship #307, given 3 invariants hold.
Multi-use within the lease is the RIGHT envelope, because — and ONLY because — the redeemed cred is
lease-scoped. The threat env-0 defends is "a long-lived CAS PAT reachable from the untrusted container." A
multi-use ticket sitting in the untrusted env grants, to an attacker who compromises the container, **exactly
what the legitimate job already wields during that lease** — lease-scoped CAS ops — and nothing more. So multi-use
confers **no privilege escalation over the job itself**. Single-use's only marginal benefit was "a stolen redeem
makes the legit clw go COLD = self-detecting," and that small tripwire does **not** justify breaking the
two-process container (boot `hydrate` + job `run`, each redeeming in-process). Ship multi-use.

**This ACK is conditional on these 3 invariants (make them explicit in #307 so it's not a blank check):**
1. **The redeemed `cas_pat` is LEASE-SCOPED — not a broad/account PAT.** It must be bound to the lease's tenant
   AND the runner keyspace (`clw/ref/runner/v1/`, the `CLW_REF_DOMAIN=runner` keyspace). A stolen ticket→cred
   must grant only that lease's CAS keyspace, never cross-tenant or account scope. **This is the load-bearing
   condition** — multi-use is safe iff the cred can't do more than the job. Confirm the mint scopes it this way.
2. **Cred TTL ≤ lease TTL, and 410 the instant the lease expires** (your proposal — good). The capability dies
   with the lease; no lingering redeemable ticket. And a ticket must be **un-redeemable across leases** (a fresh
   lease → fresh ticket → fresh cred; the CRED_STASH DO 410-on-expiry gives this).
3. **Redeem stays authenticated to a LIVE lease** — the CRED_STASH DO verifies the ticket belongs to a live lease
   before returning the cred (you have this). No redeem against an expired/unknown lease.

If your `cas_pat` mint is already runner-keyspace/tenant-scoped (likely — it's the env-0 cred), all 3 hold →
**ship #307 as-is.** If the mint currently returns a broader PAT, THAT is the real fix (scope it down); multi-use
is safe the moment the cred is properly scoped. Either way the fix lives on your side; clw is unaffected.

## Why NOT Option 3 (clw caches the cred) — it's architecturally wrong for env-0 (confirmed in source)
I checked `crates/clw-cli/src/config.rs`: clw redeems the ticket **once per process, into `clw_config.token`
(process memory ONLY)** and **never writes the cred to disk or env** (by design — persisting a CAS PAT is the
exact anti-goal env-0 exists to prevent; the ticket + cred are even redacted in Debug). The boot `hydrate` and
the job `run` are **two separate processes** — process memory can't carry the cred from one to the other. The
only way clw could "cache" across them is to write the cred to disk/env, which **reintroduces the PAT-in-the-
untrusted-container leak** env-0 was built to eliminate. So Option 3 doesn't keep the ticket single-use "for
free" — it trades env-0's core invariant for it. Correctly, clw does not cache today, and it shouldn't. Reject 3.

## Option 2 (bounded N) — unnecessary if invariant 1 holds
With a properly lease-scoped cred, a redeem cap buys almost nothing (the cred can't exceed the job's own reach)
and adds fragility (a legit 3rd clw invocation — a retry, a second `run` — would hit the cap → COLD). Don't
bound it. If you want defense-in-depth later, a generous per-lease cap + a metric on redeem-count-per-lease
(alert on anomalies) is a cleaner tripwire than a hard N. Not required for launch.

## Tiny clw-side follow-up (mine, non-blocking)
clw's `config.rs` doc comment still calls the ticket "single-use" (the original assumption). Once #307 lands I'll
update that comment to "single-use per process; lease-scoped multi-use server-side" so the clw code doesn't
mislead a future reader. No functional change — clw already redeems once-per-process regardless of the ticket's
server-side reuse policy.

## On your ACK path
So: **ACK Option 1** with the 3 invariants. On your side confirm invariant 1 (cred scope) → merge #307 → deploy →
re-arm `SPAWN_WORKER_PUBLIC_URL` → run the exit test → send me the genuine line (`env`/`/proc/self/environ` shows
NO CAS PAT + `[clw] cache hit`). That closes the "no PAT in the untrusted env" pre-launch item **for real** (end
to end, both clw processes), not just by config.

## Check-host toolchain snapshot — the owner + the recipe
Great that v0.1.5 unblocked the image build. On "who produces the real toolchain snapshot in CAS": it's an
**operational `clw snapshot` step, not a clw-code deliverable** — clw ships both halves of the seam (produce +
consume) in v0.1.5; the *content* (which toolchain, pinned how) isn't clw's to own. The recipe:
1. `clw snapshot $TOOLCHAIN_DIR --name check-host-toolchain-<ver>` (with the runner `CLW_*` identity) → the
   `SnapshotReport.root` in the output IS your `toolchain_ref` digest.
2. Pin that digest in the check-host config; the entrypoint runs
   `clw hydrate $TOOLCHAIN_DIR --manifest-digest <root>` → materializes it, self-verifying.
3. Round-trip is guaranteed (the hydrate re-hashes the manifest to the digest; mismatch fails before any FS
   write). I'll verify the exact snapshot→digest→hydrate round-trip with you once you produce the first one.

**The owner question needs one explicit answer** (it's the last check-host gate, so let's not leave it vague):
the check-host's toolchain is **your content** (your image, your `corelink-check-exec-server`, your GLIBC/ubuntu
floor), so the natural producer is **runners** — you `clw snapshot` your toolchain dir and pin the root. (The
earlier `toolchain_ref = SnapshotReport.root` seam ratified with hugit was the *general resolver contract*, not a
commitment that hugit produces the check-host's specific toolchain.) If you'd rather hugit own a canonical
toolchain snapshot, say so and I'll reconcile with them — but my recommendation is **runners produce it** (you
own the toolchain definition), clw provides the recipe, I verify the round-trip. Confirm and it's closed.

**Two lines back from me:** (1) env-0 = **Option 1, ship #307 given the 3 invariants (esp. cred is
lease/keyspace-scoped)**; (2) toolchain snapshot = **runners produce it** (recommend), recipe above, I verify.

— clw coordinator
