# Moat benchmark — first MEASURED warm signal (clw run memoization) 🎯

> **Author:** CoreLink **Runners** TL · **Date:** 2026-06-21 · **Type:** measured result (#12 pricing signal).
> **TL;DR: the cache-moat earns its keep — a memoized re-run is ~2.7× faster (6.8s saved on an 11s check),
> with the cache HIT confirmed by clw's own verdict, reproducible in a single dispatch.**

## Result (self-contained run, `moat-benchmark.yml`, N=6000 generated fns)
```
 COLD  (cache MISS, ran):      10849 ms     [clw] cache miss
 WARM1 (cache HIT, memoized):   4074 ms     [clw] cache hit
 WARM2 (cache HIT, stability):  3495 ms     [clw] cache hit
 speedup: 2.7x      saved: ~6.8 s / re-run
```
Run on a CoreLink ephemeral runner (`cf-runner-*`, Firecracker), `moat=WARM`, against the live in-network
CoreLink CAS. Earlier separate runs corroborate: once the CAS is warm, EVERY repeat of the same workload is
a `cache hit` at ~3.5–3.9 s (stable across 3+ runs).

## What this proves (and the honest bounds)
- **Memoization works end-to-end on the runner:** `clw run --input <src> -- <cmd>` → MISS runs+caches,
  HIT returns the memoized output **without re-running**. clw's own `cache miss`/`cache hit` verdict confirms
  it (not inferred from timing).
- **The delta is real and the moat's core economic claim ("re-runs ≈ 0") holds directionally:** a re-run
  costs ~1/3 of the cold run here.
- **WARM is not literally zero (~3.7 s):** a HIT restores the memoized OUTPUT (build artifacts) from the CAS
  — you skip the compile, pay the CAS restore. For a verdict-only check (clippy/test pass-fail) the restore
  is smaller; for an artifact-producing build it's the artifact download. The speedup grows with the cold
  cost: bigger/longer builds → larger absolute saving (this 11 s workload is deliberately modest).
- **Scope:** this measures the `clw run` *capability* on the runner. The default GH-Actions dogfood job does
  NOT yet auto-wrap its checks in `clw run` (the entrypoint hydrate-only path is wired-not-effective — see
  `2026-06-21-finding-warm-wired-not-yet-effective…`). Productizing = auto-memoize runner checks via
  `clw run` (the next increment). The capability is proven; the default wiring is the remaining work.

## Feeds #12 + githugr F7
- **#12 (pricing signal):** first real warm number — a memoized check is ~2.7× cheaper in wall-clock; with
  concurrency pricing (flat) the customer pockets that as throughput. Re-run the benchmark (each dispatch is
  self-contained via a run-unique cache key) for more data points / larger N.
- **githugr F7 `cache_saved`:** now sourced from a MEASURED saving (6.8 s/run on this workload), not
  illustration — once the default runner path memoizes via `clw run`, the per-check `saved` is real.

## Repro
`gh workflow run moat-benchmark.yml` (optional input `fns=<N>` to scale the cold cost). The workflow bakes
`github.run_id` into the source so COLD is always a fresh miss; WARM1/2 are hits; it prints clw's verdict.

— CoreLink Runners TL
