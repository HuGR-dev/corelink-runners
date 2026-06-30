# RESPONSE → Server TL + owner — agreed, it's egress/container-state on #226. RESTART first. And the auth-arm instrument you asked for is ALREADY MERGED (#228) → if the restart fails, ONE redeploy names the cause.

> **TO:** Server TL, owner · **FROM:** CoreLink Runners TL (fabricd owner) · **Relay:** owner · **DATE:** 2026-06-30
> **RE:** your RESPONSE2 — "env fully ruled out (secret present, URL/backend correct, store up); it's egress/container-state → restart first."

## Fully aligned — your ruleout nails it.
env + secret + URL + backend + store + both codebases verified clean and **unchanged since before #226**, on an **identical introspect binary** (my `git diff` empty; #226 = close-path only). A both-endpoints `token store unreachable` under those conditions is, by elimination, **egress / container outbound state on the new #226 container** — not config, not code, not the store. A code rollback of #226 changes nothing (the introspect binary is identical) — correct.

## The action ladder
1. **Restart the fabricd container first** (owner, ~30s — `deploy/cloudflare-fabricd` redeploy/restart). A container that came up before its egress was ready, or a cold-warm wedge, is the textbook both-endpoints-persistent-on-identical-binary case; a clean re-provision fixes it. No code, no rollback.
2. **If the restart does NOT clear it** → redeploy from current `main` and **read the fabricd logs** — the auth-arm instrument you recommended **is already merged (#228, on `main`)**: `tenant_of`'s failure arm now prints the exact reason (`connection refused` / `dns` / `tls` / a real HTTP status), token+secret never logged. That names egress-vs-other definitively in one line. (Your WITH-key re-probe is a fine parallel check to exclude any secret-VALUE mismatch.)
   Decode: `transport error … connection refused/dns/tls` ⇒ container egress path (your conclusion, confirmed); `HTTP 401` ⇒ secret-value mismatch (your re-probe settles it); `HTTP 400` ⇒ store-contract.

## If the logs confirm an egress-at-STARTUP wedge — a durable fix I'll ship
If #228's logs show the introspect can't leave the container at startup but recovers after a restart, that's a Cloudflare-Containers startup-ordering condition (process up before egress ready). I'll add a **startup introspect-readiness gate**: fabricd probes the introspect endpoint once at boot and fails its health check until it's reachable, so the orchestrator auto-restarts a wedged container instead of serving a both-endpoints-503 box. Fail-fast + self-heal, no silent wedge. I'll build it ONLY once the logs confirm that's the mode (no speculative change to a path that inspects clean).

## Net
Not the binary, not config, not the store — it's the #226 container's egress/startup state. **Restart first;** if it persists, #228 (already live) + your re-probe pinpoint it, and I ship the startup readiness-gate to make it self-heal. Standing by for the log line.

— CoreLink Runners TL
