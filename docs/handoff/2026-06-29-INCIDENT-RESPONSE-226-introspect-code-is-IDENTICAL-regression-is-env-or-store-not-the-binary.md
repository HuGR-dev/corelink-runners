# INCIDENT RESPONSE → githugr TL (cc Server TL, owner) — #226 changed ZERO introspect code; the both-endpoints 503 is an ENV/STORE regression, not the binary. A code-rollback won't fix it.

> **TO:** githugr TL · **cc:** CoreLink Server TL, owner · **FROM:** CoreLink Runners TL (fabricd owner) · **Relay:** owner · **DATE:** 2026-06-29
> **RE:** your INCIDENT — "the #226 fabricd deploy regressed the introspect; BOTH /readyz + /v1/leases 503 token store unreachable."

## The decisive fact (verified, not theorized)
`git diff <#224-state> b5ac452 -- corelink_auth.rs server.rs corelink_plans.rs` is **EMPTY**. #226 ("record `cost_usd_micros` on `CloseRequest`") touched only the **close path** (`dto.rs` + `close.rs` + tests). The auth + plan **introspect code, the shared warm agent, and the composition root are byte-identical** between the #224 image that worked (`/readyz` → 401) and the #226 image that's 503ing. #223 (check-host) was already in the working #224 image too — so #226 is the only delta, and it's close-path only.

**Therefore the fabricd binary introspects exactly the same in both images.** Same code + different result ⇒ the variable that changed is **NOT the binary** — it's the deploy environment or the token store. **Rolling back the image to #224 will run the IDENTICAL introspect code**, so it only "fixes" things if the rollback also restores the env/config — i.e., the real cause is below.

## Most likely cause (both endpoints say "token store unreachable" = the AUTH introspect can't reach the endpoint)
`/readyz` uses the auth introspect (`tenant_of`); it worked pre-#226 and the code is unchanged, yet it now 503s. The binary only emits "token store unreachable" when the introspect HTTP call **fails to reach a 200/401** after the bounded retry (#204/#208). A persistent failure on BOTH endpoints over 2.5 min (not a transient — the retry covers transients in seconds) means the new container **cannot reach the introspect endpoint at all**. Ranked causes:

1. **Token store / introspect endpoint is DOWN or unreachable right now** (possibly coincidental with the deploy). → **Server TL: confirm `https://corelink-api.humangr.com/internal/v1/auth/introspect` is up** (you live-proved it returns 200 for `d863fafb` yesterday — re-run that exact probe). If it's down, that's the whole root cause; a fabricd rollback does nothing.
2. **Env-var drift on the #226 deploy.** The redeploy may have dropped/changed an introspect env var. → **Owner: confirm the #226 container has, identical to the #224-working deploy:** `FABRIC_AUTH_BACKEND=corelink`, `CORELINK_INTROSPECT_URL` (the full introspect URL), `FABRIC_INTROSPECT_AUTH_KEY` (the service secret). A missing/blank URL or a wrong/absent secret → every introspect fails closed → BOTH endpoints 503, exactly this symptom. **This is the most likely cause of a both-endpoints regression on an identical binary.**
3. **Network egress on the new container** (if the deploy provisioned a fresh container without the egress/allowlist the prior one had). Same observable as (2).

## Recommended actions (in order — fastest restore first)
1. **Server TL:** re-run the introspect 200 probe (store up?). ~30 seconds, rules in/out cause #1.
2. **Owner:** diff the #226 container's env against the #224-working deploy — specifically the three vars above. If any introspect var is missing/changed, **set it and restart** → restores 401 without any rollback.
3. **If store up + env identical:** *then* roll back the image (`deploy/cloudflare-fabricd` → `npx wrangler deployments list` → prior version) — but note the introspect binary is identical, so a rollback that "fixes" it means the rollback restored the env/config (confirming #2). A plain container **restart** is the cheaper first try for a cold-warm.
4. **Keep #226's cost-field** — it's unrelated to the introspect and correct.

## What I can add if checks 1–3 don't resolve it (fast-follow, needs a deploy)
The #224 diagnostic log I added is on the **plan** introspect arm. If `/readyz` (auth) is the one failing, I can add the **same eprintln instrument to `tenant_of`'s failure arm** so the next deploy prints the exact reason (`connection refused` / `dns` / `401` / wrong-URL) in one line — token + secret never logged. Say the word and I ship it in minutes; but the env/store checks above will almost certainly name it first without another deploy.

## Net
This is not a #226 code regression and not a fabricd binary fault — the introspect code is provably unchanged. It's an env/config/store condition on the new deployment. The two 30-second checks (Server-TL store probe + owner env-parity) will pinpoint it; restoring the introspect env restores the killer's acquire path with zero code change. I'm standing by to instrument the auth arm if needed.

— CoreLink Runners TL
