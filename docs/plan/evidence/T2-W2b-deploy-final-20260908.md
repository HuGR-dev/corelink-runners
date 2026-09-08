# T2-W2b final promotion — PASS — 2026-09-08

The clean B2 tip `fa13e8a305b13f1dcbfa4cf0ebd15f83f1f1da52` was promoted after a no-force `0/0/0` fleet gate and a dry-run confirming all bindings and immutable container refs. Intake and redrive remained paused at `1/1`.

The Worker deployment completed at 100% on version `1418af47-d71a-488f-89a6-cbb9173402bd` (deployment `215138ea-1201-4e3d-ba27-ed20837aaa13`). DevEnv is `ready`, UUID `a037c709-9f21-493c-8dcb-2414a588a1cd`, pinned to the `d105e11f...` digest. RunnerContainer and CheckHost refs are unchanged; Fabricd was not deployed.

Immediately after deployment, `/v1/health` and `/v1/attestation/key` woke fabricd. The detailed provider poll then observed the target instance `12e222c7...` as `running`, version 26, with the expected `2e7bcea9...` image digest. The attestation response carried key id `faa5b7726ccd2c52`. The authenticated fleet endpoint returned HTTP 200 with `busy=0`, `checked=0`, `unverifiable=0`; the post-gate passed with `force=false`.

The local CI substitution is version-bound in the preceding canonical artifact: 90 test files / 1119 tests and typecheck passed at `9273080e...`; the current tip contains evidence-only changes after that green product source. No rollback was required.
