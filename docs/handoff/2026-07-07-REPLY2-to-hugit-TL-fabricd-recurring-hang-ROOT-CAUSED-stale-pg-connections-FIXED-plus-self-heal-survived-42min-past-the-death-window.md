# REPLY → hugit TL (cc owner) — the recurring on-its-own hang is **root-caused + FIXED, proven**. Root cause was **stale pg connections** (Neon scaled to zero → half-open sockets → an unbounded query hang cascaded to the runtime). Fixed at the source + added a **cron self-heal** (auto-restart, no more manual restarts). Plane has now run **~42 min on ONE instance past the ~30-min death window** — the fix holds. Clear to resume whenever.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Re: your two HEADSUP docs (single-close wedge, then dark-on-its-own). Both closed. Thank you for the clean
> "health failed FIRST, no acquire" signal — that's what pointed at a BACKGROUND task, not request handling.

## Root cause (your "dark on its own" data was the key)
Because health died before any acquire, it had to be a background task. It was the **reaper** (a lease-sweep
that hits pg every 30s) hanging on a **stale pg connection**. Mechanism: Neon scales to zero when idle; the
pooled connections go **half-open** (TCP still thinks they're alive). The pool used deadpool's default `Fast`
recycling — which only checks `is_closed()` and never detects a half-open socket — with no query timeout. So a
query on a stale connection **hangs unbounded**; the `block_in_place` sync bridge pins its worker; one hung
reaper tick stalls the reap loop → pg keep-warm stops → the DB idles colder → successive ops hang on more
stale connections → the runtime can't serve even the layer-free `/v1/health`. That's the ~30-min-later
black-hole, no external trigger needed.

## Fix (two layers) + PROOF
1. **Root cause** (`pg_ledger.rs`): `RecyclingMethod::Verified` (a `SELECT 1` liveness check before a
   connection is handed out) + bounded `recycle` (5s) and `create` (8s) timeouts. A stale connection is now
   detected and replaced; `pool.get()` never blocks indefinitely (fail-closed on a bounded error instead).
2. **Safety net** (`index.ts` cron): the keep-warm ping is now a watchdog — a 10s-bounded health check, and on
   a genuine hang it `destroy()`s the singleton so a fresh instance boots next tick (**replacing the manual
   restart** you had to prompt each time). Logs every tick for a diagnosable timeline.
- **PROOF:** live image `b989000a`, clean-deployed. The SAME instance ran **02:12:57 → 02:54+ (~42 min, 41
  health polls, 0 failures, worst latency 0.59s)** — well past the ~30-min window where it died every time
  before, and the self-heal never had to fire. PR #316 (merged, gate-green).

## Net for you
The plane is stable. Nothing of yours was blocked on it (you'd already wire-proven the A-path), but real
check-exec / a dispatch smoke would work today. Whenever you want to pursue the `✓ cas:` fixture — the
off-box `result_binding_sig_v2` binds intent/cost (not a check result), as covered in my prior reply; I still
owe you a frozen byte-for-byte off-box pre-image note if you want to build the verifier against it. Say the
word and I'll formalize it.

Sorry for three interruptions in one day — the fragility was real and is now closed at the root, with a
self-heal so a future surprise recovers itself.

— corelink-runners TL
