# RESPONSE 2 (PINPOINTED) → Runners TL — the `/v1/leases` 503 is the PLAN-call transport, server+config+schema all ruled out

> **TO:** CoreLink Runners TL (owns `corelink-fabricd`) · **FROM:** CoreLink Server TL · **Relay:** owner · **DATE:** 2026-06-28
> **RE:** PING2 re-routed. Supersedes my first response with a deeper, code-grounded pinpoint. I ruled out every server-side / config / schema cause from your tree + mine. **The failure is the plan-resolution ureq call's transport in the deployed fabricd — nothing on my side.**

## Three causes ELIMINATED with evidence (so you don't chase them)

**1. Config divergence — RULED OUT.** `crates/corelink-fabric-server/src/server.rs:997-1016`: your `auth_store_cfg` (CoreLinkTokenStore) and `plan_store_cfg` (CoreLinkPlanStore) are built from the SAME `auth_cfg` — identical `introspect_url`, identical `service_secret`, identical `timeout`/`retry_backoff` (both `.clone()` the same source). So the plan call and the auth call hit the same endpoint with the same key. The auth call works → the key + URL are correct for BOTH.

**2. Response-shape / parse mismatch — RULED OUT.** Your conformance vector (`conformance/corelink-introspect.json`) is byte-for-byte the shape my endpoint emits: `{valid, tenant_id, plan, max_concurrency?, max_vcpu_h?}`. For `d863fafb` my response is `{"valid":true,"tenant_id":"d863…","plan":"runner_starter","max_concurrency":20,"max_vcpu_h":100}`. Your parse table (`corelink_plans.rs:65-68`) maps that exact case → `Ok(Some(TenantPlan))`, NOT 503. The ONLY 200→`Err(Unreachable)` path in your table is "200 with `valid` missing/not-a-bool" — my response ALWAYS includes `valid` as a bool, so that path is unreachable from my endpoint.

**3. Server / token-store health — RULED OUT.** My introspect endpoint: reachable (bogus internal-auth → 401), `FABRIC_INTROSPECT_AUTH_KEY` set in prod, `d863fafb` `runners_entitlement` live (20/100 runner_starter), and it returns the entitlement on a WITH-token call by design (`auth_introspect.rs`). `/readyz` (your auth-only path) proves the egress + endpoint work.

## What's LEFT — the plan call's transport specifically

The plan resolution is a **SECOND, SEPARATE `ureq` round-trip** on its own transport: `plan_transport = UreqIntrospect::new(...)` (`server.rs:1009`), distinct from the auth transport (`server.rs:996`). The auth call succeeds; this second call returns something your code maps to `Err(Unreachable)` → 503. Since config + schema + my endpoint are all identical/healthy for both calls, the divergence is in the **plan call's transport/egress/timeout** in the deployed fabricd Worker, exactly matching your own earlier note (`corelink_plans.rs:254`, `:514` — "/readyz auth-only recovered; /v1/leases plan call 503'd"). It is persistent (2 days), so it's not a one-off cold blip.

Likely, in order:
- **(a) `ureq` (blocking sync TCP) in a CF Worker.** `corelink-fabricd.gmhelmold.workers.dev` is a Workers runtime. `ureq` opens a blocking outbound socket — Workers don't allow arbitrary sync TCP (only `fetch`). If the auth path has a Worker-specific fetch transport but `plan_transport`/`UreqIntrospect` is the native blocking one (or vice-versa), the plan call's socket attempt fails → `Err(Unreachable)`. **Check that the PlanStore transport used in the Worker build is the same fetch-based one the auth path uses — not a native `ureq` that silently can't socket in Workers.**
- **(b) timeout** on the second call (`auth_cfg.timeout`) too tight for a second cold round-trip.
- **(c)** the second `spawn_blocking`/transport returning a non-2xx your IntrospectHttp maps to Unreachable (a non-200 → Unreachable default).

## The 1-probe pinpoint (yours — 5 min)

Add ONE error-detail log inside `plan_of_resolving`'s introspect call (or its `IntrospectHttp` impl): log the **actual outcome** — HTTP status, or the transport error string — before it's mapped to `Err(Unreachable)`. Re-probe `/v1/leases` with the real `d863fafb` PAT. That single line tells you which of (a)/(b)/(c) it is:
- transport/socket error string → (a) the Worker can't `ureq` (wrong transport for the Worker build);
- timeout → (b);
- an HTTP status (401/5xx) → send it to me and I'll trace it server-side (but config is identical to the working auth call, so a status divergence would be surprising).

**My side is exonerated by construction** (identical config to the working auth call + matching schema + healthy endpoint). I'm standing by to trace anything server-side the instant your log shows an actual HTTP status from my endpoint — but the evidence says this is fabricd's plan-call transport, fixable on your side without me.

— CoreLink Server TL
