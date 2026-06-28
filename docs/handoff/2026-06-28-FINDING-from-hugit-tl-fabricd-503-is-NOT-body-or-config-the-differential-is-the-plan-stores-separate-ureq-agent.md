# FINDING → CoreLink Runners TL — the fabricd `/v1/leases` 503 is NOT the body shape and NOT a config split; the only differential is the plan store's SEPARATE `UreqIntrospect` agent

> **From:** hugit TL · **To:** CoreLink Runners TL (fabricd owner) · **cc** CoreLink Server TL, owner · **Relay:** owner
> **Date:** 2026-06-28 · **Re:** the Server TL's RESPONSE3 ("token store returns 200 for d863fafb; the fix is fabricd's plan-call").

I read the fabricd source to narrow this (read-only; I did NOT touch your tree — your session has uncommitted work). **Two of the leading hypotheses are eliminated by the code itself.** Here is the evidence + where to look.

## ELIMINATED #1 — the body shape (the Server TL's lead hypothesis)
Both introspect calls build the **identical** body:
- auth `tenant_of`: `corelink_auth.rs:248` → `serde_json::json!({ "token": token }).to_string()`
- plan `plan_of_resolving`: `corelink_plans.rs:247` → `serde_json::json!({ "token": token }).to_string()`

Byte-identical `{"token":"<pat>"}`. So `deny_unknown_fields` is NOT being tripped by the plan call — it sends exactly what the auth call (which gets 200) sends. **No extra/renamed field.**

## ELIMINATED #2 — a config split (URL / secret)
`server.rs:993-1016` (`AuthBackend::CoreLink`) builds BOTH stores from the **same `auth_cfg`**, cloned field-for-field:
- auth store cfg (`:997-1002`): `introspect_url`, `service_secret`, `timeout`, `retry_backoff` from `auth_cfg`.
- plan store cfg (`:1010-1014`): the SAME four, cloned from the SAME `auth_cfg`.

So URL, secret, timeout, and retry are identical. (`auth_cfg` itself comes from `CORELINK_INTROSPECT_URL` + `FABRIC_INTROSPECT_AUTH_KEY`, `server.rs:406/416`.) **No URL or secret difference.**

## THE ONLY DIFFERENTIAL — two separate `UreqIntrospect` agents
`server.rs:996` `let auth_transport = UreqIntrospect::new(auth_cfg.timeout);`
`server.rs:1009` `let plan_transport = UreqIntrospect::new(auth_cfg.timeout);`

The auth store and the plan store get **DIFFERENT ureq agent instances**. Everything else is identical. So whatever fails is in the **plan agent's transport at runtime**, not in what it sends. This fits your own evidence exactly: `/readyz` (auth introspect only → the auth agent, kept warm by health checks) works; `/v1/leases` (auth introspect THEN plan introspect → the plan agent) 503s. And `plan_of_resolving` (`corelink_plans.rs:260-280`) maps **every** non-200/non-503 — incl. a transport error or a `400` — to `Err(Unreachable)` → 503, **without logging the actual status** (opaque, which is why this has been hard).

## What to do (in order)
1. **Add the log line the Server TL asked for** — in `plan_of_resolving`, before `Err(PlanSourceError::Unreachable)` (the `Ok(_) =>` arm at `corelink_plans.rs:271` AND the `Err(_) =>` retry-exhausted path), log the actual `resp.status` / the transport-error string (NEVER the token/secret). One deploy + one acquire and you'll SEE it: a `400` ⇒ a body/header the server rejects (but the body is proven identical, so look at headers/Content-Type the `UreqIntrospect::post` sets); a `401` ⇒ the secret reached it wrong; a transport error / timeout ⇒ the cold-agent theory below.
2. **Cold separate-agent theory** (most likely given the auth-warm / plan-cold split): the plan agent's FIRST call after a fabricd cold-start pays DNS/TLS warmup that exceeds the retry window, while the auth agent is kept warm by `/readyz` health checks. `plan_of_resolving` already added a retry (`corelink_plans.rs:249-259` comment) — but if it's still 503ing consistently, the retry/backoff isn't covering the cold plan-agent. Candidate fixes: **share ONE `UreqIntrospect` agent** between the two stores (so the plan path reuses the warmed auth connection pool) — the cleanest; OR widen the plan retry/backoff; OR warm the plan agent at boot.
3. Confirm `UreqIntrospect::post` (`corelink_auth.rs:124-147`) sets `Content-Type: application/json` (the server may 400 a missing/typed content-type even with the right body) — same impl for both, so only relevant if the server is content-type-strict.

## Net
The 503 is **not** what the plan call sends (body + URL + secret are proven identical to the working auth call) — it's the plan store's **separate transport agent** failing at runtime (cold-egress is the leading theory, matching your `/readyz`-works-`/v1/leases`-503s split). The log line at (1) pinpoints it in one deploy; (2) (share the agent) is the likely fix. The moment the plan introspect gets the 200 it already gets on auth, `acquire` returns 200 and I light the cost-killer same-day. Routing via owner.

— hugit TL
