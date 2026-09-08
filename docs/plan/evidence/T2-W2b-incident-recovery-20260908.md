# T2-W2b incident recovery — 2026-09-08

Status: **RED** (service recovered; historical fabricd provider version 26 not restored).

The canonical `--containers-rollout=none` deployment completed the Worker deployment but skipped container application creation. The subsequent immediate rollout recreated `corelink-fabricd-fabricdcontainer` with UUID `a03e645c-9589-401c-84a0-e023ff2b6749`, provider version `1`, and the pinned image digest. The instance is running and `/v1/health` and `/v1/attestation/key` returned HTTP 200. The attestation key id prefix observed was `faa5b7726ccd2c52`.

The spawn Worker was temporarily rolled back to `cb704a9d-f6e0-45d7-9e68-89a263a77993` by deployment `200ea7f0-cd1b-47b2-8986-66ad6344612e` at 100%, then restored to final known version `1418af47-d71a-488f-89a6-cbb9173402bd` by deployment `794345b4-3404-465a-a256-53a652a8f9dc` at 100%. The DevEnv app remains ready on digest `d105e11f92718d610b390a71c37acb9bfb668da278b2f80c1f617f7cd068768c`. The authenticated fleet gate was `busy=0`, `checked=0`, `unverifiable=0`, `force=false`. Intake and redrive remain paused (`1/1`).

The previously expected fabricd provider version 26 could not be retained or restored after the app disappeared; this is the blocking RED criterion. No delete, unpause, secret disclosure, or test was performed.
