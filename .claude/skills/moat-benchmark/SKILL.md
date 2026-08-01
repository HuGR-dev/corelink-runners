---
name: moat-benchmark
version: 0.1.0
description: How to run a CoreLink cache-moat benchmark on a real `runs-on: corelink` box and read honest, citable numbers. Covers both a COLD→WARM whole-build benchmark (clw run / corelink-memoize) and a "rebuild-only-changed" incremental test. Includes the exact dispatch mechanics (push a workflow to HuGR-Labs/corelink-cold-organic-e2e, `gh workflow run`, poll, read `[clw]` verdicts + GH step timestamps), the tenant-attribution proof (Option-C mint on the tail), and the pitfalls that produce fake numbers (non-identical COLD/WARM commands, missing run-unique key, clw skipping the command on a hit, compile errors). Invoke whenever asked to benchmark/measure the moat, prove a speedup, or demonstrate rebuild-only-changed on a box.
---

# moat-benchmark — measure the moat honestly on a real box

Runs on `HuGR-Labs/corelink-cold-organic-e2e` (Option-C maps it to cold tenant `3c7d77b1`; the
vendored `./actions/corelink-memoize` is already in the repo). Requires the App-installed org install
(150584374) for JIT + the spawn-worker deployed with `REPO_TENANT_PAT_MAP` + `COLD_ORGANIC_TENANT_PAT`.

## Dispatch mechanics
1. Write the workflow locally; base64 via node (avoids sandbox `$(...)` issues), PUT it:
   `gh api -X PUT repos/HuGR-Labs/corelink-cold-organic-e2e/contents/.github/workflows/<name>.yml -f message=… -f "content=$(cat /tmp/x.b64)" [-f "sha=$SHA" to update]`
   (gh/git hit a sandbox "failed to change group ID" error → run these Bash calls with
   `dangerouslyDisableSandbox: true`; node `fetch` works fine in-sandbox).
2. `gh workflow run <name>.yml --repo HuGR-Labs/corelink-cold-organic-e2e --ref main`, then poll
   `gh run view <id> --json status,conclusion` in a background loop until `completed`.
3. Read verdicts + timing:
   - `[clw] cache miss` / `[clw] cache hit` — grep the job `--log`. These are the ground truth.
   - Wall-clock: read GH **step timestamps** (`--json jobs -q '.jobs[0].steps[]'`, second-granularity) or
     the sub-second timestamp on the `[clw]` verdict line. On a HIT clw SKIPS the wrapped command, so
     in-command `date` timers DON'T fire on WARM — always use step time for WARM.
   - `du -sh <dir>/target` = what the moat stored/restored.
4. Tenant proof (that it ran under the cold tenant, not dogfood): `wrangler tail corelink-spawn-worker`
   shows `mint_option_c_pat_dispatch` + `POST /v1/leases/<job>/cas-cred - Ok`; the box log shows
   `CLW_TOKEN_set=[no]` (env-0, no raw PAT) + `CLW_CRED_TICKET_len=[64]`.

## COLD→WARM whole-build benchmark (Layer 1)
Two steps, **byte-identical** `run:` command (clw folds the command into the key — any difference makes
WARM a false MISS). Add a `// run-unique: ${{ github.run_id }}-${{ github.run_attempt }}` marker to a
source file so COLD is a genuine fresh miss each run. `.clwignore` must list `target/` (it's the OUTPUT,
not a key input). `inputs:` = the source paths only. Expect COLD `[clw] cache miss`, WARM `[clw] cache hit`.
Real numbers on this box: ripgrep 32.6s→1.9s (17×), dep-tree ~250 crates 42.2s→1.7s (25×). WARM≈2s.

## "Rebuild only what changed" — DON'T use clw run
clw run is whole-build: a 1-file change → full rebuild (proven: run 29850459289, v2 = 53s full rebuild
after changing one function). To demonstrate partial/incremental reuse you MUST use the fine-grained
REAPI layer — **sccache** (cargo/Rust) or **Bazel/Buck2** (REAPI-native) pointed at
`https://corelink-api.humangr.com/bazel/v2/<tenant>` with `Authorization: Bearer <cas:rw PAT>`. See the
`corelink-moat` skill for endpoints. Demo shape: build COLD (all actions cached) → change ~25% of
crates/targets → rebuild → ~75% ActionCache hits, ~25% recompile = the mixed number.

## Pitfalls that produce FAKE numbers (all hit at least once, 2026-07-21)
- Non-identical COLD/WARM commands → WARM false-miss (looked like a 0.29s no-op). Make them identical.
- No run-unique marker → a later run's COLD hits a prior run's cache (not a fresh miss).
- In-command timers on WARM don't fire (clw skips the command on a hit) → use step timestamps.
- A compile error in the bench crate fails COLD and skips WARM — validate the crate compiles.
- `[clw] cache hit` in the WARM STEP ≠ proven for the tenant — confirm `mint_option_c_pat_dispatch` on
  the tail (else it may have resolved to dogfood). test-green ≠ live-proven.

## Honest framing (owner mandate)
Report the real numbers + caveats. A fast box makes COLD small (understating absolute savings); the WARM
constant (~2s) is restore-bound so the ratio grows with build size. Don't sell a 3-digit number off a
fast toy build. The BIGGER moat is Layer 2 (rebuild-only-changed) — benchmark that with sccache/Bazel.
