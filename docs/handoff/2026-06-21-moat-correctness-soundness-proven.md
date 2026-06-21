# Moat correctness — memoization is SOUND (no stale hits) ✅

> **Author:** CoreLink **Runners** TL · **Date:** 2026-06-21 · **Type:** soundness proof (SOTA rigor).
> **TL;DR: clw run memoization is correctly input-addressed — a changed input busts the key (MISS), a
> repeat hits with the CORRECT replayed output, and distinct inputs coexist. Speed (2.7×) is only useful
> if it's sound; it is.**

## Why this matters
A fast cache that can serve a STALE result is worse than no cache — it would silently return a wrong CI
verdict. Before trusting memoized checks, we must prove the cache is keyed on the inputs and invalidates on
change. The benchmark proved HIT=fast; this proves HIT=CORRECT.

## Result (`moat-correctness.yml`, live on a CoreLink runner, job GREEN)
The wrapped command's stdout IS the payload, so a wrong replay is directly detectable. Run-unique payloads
so step 1 is a guaranteed first miss. Hard assertions fail the job on any violation.
```
 [1: A first]            verdict=cache miss   out=A-<run>    ✅ first run → MISS
 [2: A again]            verdict=cache hit    out=A-<run>    ✅ repeat → HIT, correct output replayed
 [3: B changed]          verdict=cache miss   out=B-<run>    ✅ changed input → key BUSTS (no stale "A")
 [4: A again]            verdict=cache hit    out=A-<run>    ✅ distinct entry intact (no cross-contamination)
 → moat correctness PASSED
```

## What this establishes
- **Input-addressed:** the cache key folds in the input content; changing it produces a MISS, never a stale
  hit. (`clw run --input <paths> -- <cmd>`, key = hash(inputs ‖ env ‖ cmd).)
- **Correct replay:** a HIT returns the command's actual cached **stdout** (step 2 returned "A" without
  re-running) — so `clw run` memoizes verdict+output, not just exit code. Confirms the "memoized check"
  model.
- **No cross-contamination:** A and B coexist as distinct entries; revisiting A still hits A.

## Soundness contract (the rigor boundary — why opt-in is correct)
Memoization is correct ONLY for commands that are **pure + deterministic + fully input-addressed**: same
inputs ⇒ same output, no hidden dependencies (wall-clock, network nondeterminism, untracked files, ambient
env). This is exactly why `corelink-memoize` is **opt-in per step** (the author asserts the step is
memoizable via the `inputs`/`env` they declare) — blindly memoizing arbitrary steps would be UNSOUND.
Guidance: keep `inputs` to the true source; fold the toolchain/compiler version into `--env` (or an input)
so a toolchain bump correctly busts the key.

## Where the moat stands now (2026-06-21)
- WARM live (per-job CAS PAT minted + `CLW_*` injected; revoke-on-completion). 
- Measured: `clw run` memoization **2.7×**, ~6.8 s saved/re-run (`…-moat-benchmark-result…`).
- Productized: `actions/corelink-memoize` (one-line `uses:`, fail-open), proven E2E.
- **Sound: this proof** — input-addressed, no stale hits.
Reusable gates: `moat-benchmark.yml` (speed), `moat-action-test.yml` (action E2E), `moat-correctness.yml`
(soundness). `gh workflow run <name>`.

— CoreLink Runners TL
