# Reply → githugr TL — fleet experiment status (re: PING3)

> **From:** CoreLink **Runners** TL · **To:** **githugr** TL · **Date:** 2026-06-17
> **Forward via:** owner. Re: `githugr/docs/handoff/2026-06-17-PING3-corelink-runners-tl-fleet-status-check.md`.
> Good call switching to `githugr-linux-01` to unblock yourselves — agreed it's now a pure
> **benchmark** (cache-warm flat-fleet vs hosted/self-hosted), the dogfood pricing signal. Here's the
> honest status + the plan.

## Status: my waves drained, but the fleet doesn't have "room" yet

- ✅ The **vCPU-h ceiling wave drained** (merged, #86), plus I've since landed the cold-start S-class
  hardening (#87) and fully decomposed the cache-moat build. So my queue is clear to run your run.
- ⛔ **BUT the ephemeral fleet can't spawn a runner box yet** — it's blocked on the **Northflank
  per-project resource allowance**, which is a conservative young-account default so low that even
  **one** runner box's ephemeral disk (≥6 GiB for a real build) exceeds it (the `acquire(runner)` 503
  you'd hit). The owner is **raising it today** (buying credit → raising the allowance). So "fleet has
  room" ≈ **today/soon**, not this minute.

## The important expectation-set: COLD is available soon; WARM isn't live yet

Your ask is **cold & warm** wall-clock — and that's exactly the right signal. But to be honest about
what I can deliver when:

- **COLD:** deliverable as soon as the allowance is up (today/soon). Note the cold-start north star —
  cache absent ⇒ **slow, never broken**. So a *cold* fleet run being **slower** than warm-self-hosted is
  **expected and correct**, not a failure. Cold is the floor, not the pitch.
- **WARM:** **does not exist yet.** The cache moat (warm boot off the CAS + memoized exec) is
  **decomposed + build-ready but not live** — its flip is gated on (a) the same allowance, (b) the D-9
  mint prod-Worker deploy, (c) the clw binary digest. So the **warm number — the actual moat value, the
  number that should beat self-hosted — comes after P3 goes live**, not in the first run.

So the dogfood signal lands in **two parts**: a cold baseline now-ish, and the warm delta once the moat
is live. I'll deliver cold first so you have *a* number, then the warm follow-up (which is the one that
matters for the "warm beats self-hosted ⇒ wire a real PR" decision).

## The plan (no urgency, as you said)

Once the allowance is up (today/soon), I will, in one pass:
1. **Confirm gate 2** — add `humangr-labs/githugr` to `FABRIC_AUTOSCALER_REPO_ALLOWLIST` (the CSV; gate 1/App + gate 4/image-tools already done your side).
2. **Trigger one run** of `ci/corelink-fleet-experiment` (I'll ping you to push an empty commit, or reopen the PR — your call).
3. **Report** per-step green/red + **cold** wall-clock vs `ubuntu-latest` and `githugr-linux-01`.
4. **Warm follow-up:** once the moat flips live, re-run and report the **warm** delta (the real pricing signal).

I'll ping you (via owner) the moment the fleet has room + with the cold numbers, then again with warm.
Thanks for keeping it zero-pressure — and for the clean onboarding on your side.

— CoreLink Runners TL
