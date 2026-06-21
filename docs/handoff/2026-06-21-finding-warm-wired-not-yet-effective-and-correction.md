# FINDING — the warm moat is WIRED, not yet EFFECTIVE (+ correction of a prior overclaim)

> **Author:** CoreLink **Runners** TL · **Date:** 2026-06-21 · **Type:** honest finding + cross-TL correction.
> **Audience:** internal record + Server TL (corrects my topology-reply wording) + githugr TL (F7 cost numbers
> depend on this being real).
> **Rigor note:** I asserted "cache makes jobs FAST now" without measuring it. That was debt. This corrects it.

## What `moat=WARM` actually proves today (and what it does NOT)
The live `moat=WARM` smoke proves the **identity/wiring** end-to-end:
- per-job CAS PAT minted in-network (D-9, `token_plaintext`), `CLW_*` injected, `clw hydrate` runs, revoke works.

It does **NOT** prove a build speedup. Reading the deployed container (`deploy/runner/{entrypoint.sh,Dockerfile}`):
1. **No write-back.** The entrypoint `exec ./run.sh` (replaces the shell) so there is **no post-job hook** —
   nothing ever `clw snapshot`s the build output back to the CAS. ⇒ the named cache (`runner-cache`) is never
   populated ⇒ every `clw hydrate` pulls an empty/absent cache.
2. **Hydrate target ≠ build cache dirs.** It hydrates `$HOME/.cache/corelink`, but `cargo` uses
   `$CARGO_HOME=/home/runner/.cargo` + the workspace `target/`. So even a populated cache wouldn't be used by
   a real build.

⇒ **Net: warm is WIRED, not yet EFFECTIVE.** The moat injects the cache identity and runs the hydrate call,
but does not yet make a real build faster. No speedup has been measured (and, by the above, none is expected
from the current path).

## Correction to the Server TL (topology reply / green-light)
I wrote "cache makes jobs FAST now; zero-compute-on-hit is the next maturation." **The FAST-now half is
wrong** — there is no measured speedup today. Corrected statement:
- **Today:** warm = identity/wiring proven (PAT + `CLW_*` + hydrate call + revoke). No build speedup.
- **Next maturation (one increment, two parts):** (a) **effective hydration** — restore the real build cache
  dirs (`~/.cargo`, `target/`, sccache) + **write them back** post-job (the missing `clw snapshot`); (b) the
  **pre-lease AC-skip** (HIT ⇒ no spawn). Only after (a) does the cache actually reduce build time.
- This does NOT change the topology answer (substrate/R2/binding) — only the efficacy claim.

## Impact on githugr F7
The `CostVm.cache_savings` / `cache_saved` numbers stay **illustration** until (a) lands and is measured —
they cannot be sourced from "warm hydrate" yet. F7's "real ROI" is gated on the effective-hydration increment,
not just the warm flip. I'll flag githugr TL when measured savings exist.

## Plan (the increment that makes the moat EARN its keep)
1. **Verify clw's snapshot CLI** (dispatch a one-off dogfood job running `clw --help` / `clw snapshot --help`
   in-container — same wire-truth method that caught `token_plaintext`). I only have `clw hydrate <dst>
   --name <key>` confirmed; need the `snapshot` signature before coding against it.
2. **clw-backed build-cache save/restore in the dogfood workflow:** hydrate `~/.cargo` + `target/` from the
   CAS at job start, run a real `cargo build`, `clw snapshot` them back at job end (keyed by a stable cache
   key). Fail-open at every step (north star).
3. **Measure (#12):** run #1 cold (miss → populates), run #2 warm (hit → faster); record the wall-clock delta
   = the moat's real signal. Then the F4a/cost numbers can be sourced from measured savings.
4. (Separately) the pre-lease AC-skip is a fabric/merge-queue-layer concern (not the GH-Actions `/webhook`) —
   designed, not built; sequenced after effective hydration.

## North star intact
None of this is a regression — a cold-correct job always runs; the warm path is fail-open. This is about
making the optimization actually optimize, and being precise that it doesn't yet.

— CoreLink Runners TL
