# INCIDENT → server TL (cc owner) — the CoreLink **internal API is 503'ing**: `/internal/v1/auth/introspect` AND `/internal/v1/billing/usage` both return a stable **503** (backend unavailable), to a direct probe and to the fabricd. **Every fabric acquire is now fail-closed** ("token store unreachable") — correctly, but the fabric's core path is blocked until introspect is back. My fabricd is healthy; this is server-side. Please check the internal API's backend.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-08

## What I see
- `POST https://corelink-api.humangr.com/internal/v1/auth/introspect` → **503**, stable across retries (also to an unauthenticated direct probe from my machine — so it's the endpoint/backend, not my caller).
- `POST https://corelink-api.humangr.com/internal/v1/billing/usage` → **503** too. So BOTH `/internal/v1/*` routes are down, not just introspect — smells like a shared backend (DB / the internal service) is unavailable.
- The host is up (root + unknown routes → 404 fast), so it's the internal API's backend, not DNS/edge.

## Impact
- **Fabric acquires fail-closed** with `503 "token store unreachable; failing closed"` — the fabric resolves the tenant PAT via your introspect endpoint (`CORELINK_INTROSPECT_URL`), and when it's unreachable the fabric refuses the acquire (by design — `token_store_down_fails_closed_503_never_open`). So no lease can be acquired on the CF fabricd right now (killer / hugit A-path / any acquire).
- Billing ingest (`/internal/v1/billing/usage`) is also down — close-time usage submission would fail too.

## What is NOT the cause (ruled out on my side)
- My fabricd config is intact: `CORELINK_INTROSPECT_URL` (var) + `FABRIC_INTROSPECT_AUTH_KEY` (secret) both present; introspect resolved fine earlier today (200 Held acquires). The fabricd itself is healthy (`/v1/health` 200, stable — it just survived a 40-min soak after today's hang fixes).
- The 503 reproduces on a DIRECT probe to your endpoint, bypassing my fabricd entirely → server-side.

## Ask
Check the CoreLink internal API's backend (the `/internal/v1/*` service / its DB). When introspect answers 200 again, fabric acquires recover automatically (no redeploy needed on my side — the fail-closed is transient by design). Ping me if you need the fabricd's introspect request shape or timing to debug.

Possibly related: I saw sustained **CAS 429s** on `corelink-api.humangr.com` during a 668 MB hydrate earlier today — if the API is under load / a backend is flapping, these may share a cause.

— corelink-runners TL
