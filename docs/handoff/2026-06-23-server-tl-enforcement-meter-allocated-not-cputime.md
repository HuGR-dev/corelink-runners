# Heads-up → Runners TL — meter the vCPU-h ceiling on ALLOCATED time, not just cpuTimeSec (real CF data inside)

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-23 · **Priority:** P2 — margin-integrity nuance; not blocking. CF substrate is ratified.

The owner ratified **Cloudflare as the runner substrate** (COGS parity with Northflank — ~$0.40/runner-h
= $0.10/vCPU-h on both; CF wins on cache-proximity speed + the 20 GB disk + ops unification, at cost parity,
not at margin cost). The ratified loss-proof ladder (`docs/product/pricing.md §2`) stands on CF. One
enforcement detail surfaced from **real CF billing data** that's worth pinning before scale.

## The finding (from live CF `containersUsageAdaptiveGroups`)
I pulled the real consumption of `corelink-spawn-worker-runnercontainer` (4 vCPU / 12 GiB / 20 GB, confirmed
live). For the actual smoke runs:
- **`cpuTimeSec` = 170.75** (CPU actually burned)
- **allocated memory/disk** ≈ **490 seconds** of full-instance allocation (back-computed from the byte-second
  counters: mem 6.3e12 B·s ÷ 12 GiB ≈ 489 s; disk 9.9e12 B·s ÷ 20 GB ≈ 495 s)

So the instance was **allocated ~490 s while CPU burned only ~170 s** (~8.7% CPU util — normal for bursty CI).

**The point:** Cloudflare bills **memory + disk by ALLOCATION (wall-clock the instance is up)**, only CPU by
usage. The memory floor (12 GiB × $0.009/GiB-h ≈ **$0.11/runner-h**) accrues for the whole allocated window
regardless of CPU.

## The ask — meter the ceiling on allocated vCPU-h, not cpuTimeSec
If the `max_vcpu_h` hard ceiling is computed from **CPU time** (`cpuTimeSec`-equivalent), then an
**idle-long job** (little CPU, long wall-clock — e.g. a build waiting on network/IO, or a hung step) burns
the memory+disk allocation floor that the ceiling does **not** capture → a margin leak the loss-proof wall
was designed to prevent.

- **Meter the ceiling on `allocated wall-clock × vCPU`** (i.e. instance-up time × the instance vCPU count),
  not just consumed CPU-seconds. That's what CF actually bills, so it's what the loss-impossible guarantee
  must bound.
- Equivalently: charge the lease against the ceiling for the **lease's held duration**, not its CPU burn.

This keeps the $0.10/vCPU-h basis honest on CF (where allocation ≈ the cost driver), not just on a
pure-CPU model. No server-side change — it's the runner fabric's lease-accounting; flagging so the wall
is bound to the real cost axis before real tenants run idle-heavy jobs.

## Not blocking
CF substrate + pricing are ratified; this is a metering-axis refinement. Confirm how the ceiling currently
meters (CPU vs allocation) and we're aligned.

— CoreLink Server TL · routed via owner
