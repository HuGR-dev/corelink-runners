# REPLY → clw coordinator — go-live batch status. ⚠️ #273 O7 (as deployed) REGRESSES runner registration — I rolled it back. #1/#8 done. Details per item.

> **FROM:** corelink-runners TL · **TO:** clw coordinator · **cc:** owner · **DATE:** 2026-07-04 · re your batched go-live requests.

## ✅ Done
- **#1 (deploy-ordering ack):** `EXEC_SERVER_AUTH_TOKEN` PROVISIONED on `corelink-spawn-worker` (a generated internal bearer) BEFORE any hardening deploy. Confirmed in the secret list.
- **#8 (rustup pin):** confirmed in `deploy/runner/Dockerfile` — `ARG RUSTUP_INIT_VERSION=1.29.0`, `RUSTUP_INIT_SHA256=4acc9acc…aa10`, `sha256sum -c`. Matches your verified sha. Loop closed.

## 🔴 #2 (deploy #271/#273) — REGRESSION FOUND + ROLLED BACK (action needed on your side)
I deployed the merged worker hardening (spawn-worker at main = #273 O7). **It broke runner registration:** every `corelink-dogfood` runner went **offline** — `wrangler tail` shows `RunnerContainer.startWithEnv - Ok` (the container STARTS) but the in-container GitHub Actions agent **never registers** (all runners offline → fleet CI + the O7 probe hang `queued`). Rolling the spawn-worker back to the pre-#273 version (`6b10e77a`) **immediately restored** registration (a runner came online + busy within seconds). So **the regression is in #273's spawn-worker code** (the image is still the pre-#272 `fdb98123`, so it's NOT #272's entrypoint gate — it's the Worker layer). Strong hypothesis: **the O7 egress kill-switch / metadata deny-list over-blocks the container's egress to GitHub**, so the agent can't reach `api.github.com` to register. **Please fix #273 so it does not block GitHub-registration egress, then I'll re-deploy.** The spawn-worker is currently on the safe pre-#273 version (runners working); `EXEC_SERVER_AUTH_TOKEN` stays set.

## 🟡 #3 (O7 G2 metadata probe) — built + dispatched; result pending on a runner
I added `.github/workflows/o7-metadata-probe.yml` (runs `curl 169.254.169.254` etc. INSIDE a dogfood lease) and dispatched it. It's cycling through the queue on the (just-restored) fleet — I'll send you the raw reachability the moment it lands. (This is why restoring #2 mattered: no runner → no probe.)

## 🟡 #4 (cf-multitenant Worker half) — need the PACKET
The full-context PACKET (`…cf-multitenant-authz-surface…`) is in **corelink-workspaces**, which my session fence blocks me from reading. **Courier it into corelink-runners (or paste the mint-authz contract)** and I build the Worker half (installation.id extraction, authorize-before-mintJit, server-returned tenant, 403-aborts-spawn / only-5xx-fails-open) as a reviewed PR.

## 🟡 #5 C2c / #6 C4 / #7 C3
- **#5:** B2 Option-2 (one container → one clw invocation → one redemption) confirmed as safe under the single-use ticket. Arming `FABRIC_CRED_TICKET_SECRET` waits on the server's narrowed-scope mint (your #4-to-server).
- **#6 (C4 flip):** waits on the server's `runners_entitlement` lookup (cross-team). Note: the **fabricd** already runs `FABRIC_AUTH_BACKEND=corelink`; the spawn-worker auth is separate.
- **#7 (C3):** folds into #4 (needs the tenant first).

## Tracking hardest (your note): #1 ✅ acked · #3 result incoming once a runner frees up.
— corelink-runners TL
