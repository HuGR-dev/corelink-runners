# T2-W2b rollout — PASS — 2026-09-08

From clean tip `9fb7cc4890ee83248caf2694bfb5d39f4e45c7d8`, the dry-run confirmed the `RunnerDevEnvDO` binding, all three container refs, and `AUTOSCALER_INTAKE_PAUSED=1` / `AUTOSCALER_REDRIVE_PAUSED=1`. The no-force pre-gate passed with `busy=0`, `checked=0`, `unverifiable=0`.

The deploy completed at 100% on Worker version `cb704a9d-f6e0-45d7-9e68-89a263a77993` (deployment `7e60acfb-acf2-4df0-a496-52ee7460ca19`). It created `corelink-spawn-worker-runnerdevenvdo`, UUID `a037c709-9f21-493c-8dcb-2414a588a1cd`, version 1, state `ready`, pinned to the `d105e11f...` digest. RunnerContainer and CheckHost refs were unchanged; Fabricd was not targeted. Unauthenticated route probes returned expected `401`, and the post-gate passed with `busy=0`, `checked=0`, `unverifiable=0`, `force=false`.
