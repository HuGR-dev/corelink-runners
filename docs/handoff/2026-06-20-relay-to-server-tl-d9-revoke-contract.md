# Relay → Server TL — D-9 **revoke** endpoint contract (per-job CAS-PAT teardown)

> **From:** CoreLink Runners TL · **To:** CoreLink Server TL · **Date:** 2026-06-20
> **Type:** contract proposal to FREEZE (transcribed each side, like the mint contract).
> **Status:** the runner/Worker side is **already built + tested + shipped** against this
> shape (fail-open: a missing/!2xx revoke is swallowed — the PAT's TTL is the backstop), so
> there is **no rush and no risk** on your side. Confirm or amend the shape; nothing breaks
> until you deploy the endpoint.

## Why (context)

The all-Cloudflare autoscaler mints a **per-job CAS PAT** via D-9 on `workflow_job:queued`
(A6: per-job, never the tenant PAT; `cas:rw`; 5400s TTL). The PAT is currently reclaimed by
**TTL expiry only**. Per owner mandate ("hardening is never optional"), we now also **revoke
on completion** to shrink the post-job validity window.

**Key simplification we adopted so YOU need no `job→pat` map:** the Worker now mints the PAT
under `job_id = <GitHub workflow_job.id>` (previously a random UUID). That id is **stable across
the `queued` and `completed` events for the same job**, so completion can revoke by
`(owner_tenant, job_id)` — the server resolves it to the issued PAT. No new storage on either side.

## Proposed contract (mirror of the mint contract)

```
POST {CORELINK_MINT_URL}/internal/v1/runner/revoke
Headers:
  x-corelink-internal-auth: <CORELINK_PAT_MINT_AUTH_KEY>      # SAME key as mint
  content-type: application/json
Body:
  { "owner_tenant": "<tenant-uuid>", "job_id": "<github workflow_job.id, as string>" }
Response:
  2xx (any) on success — body ignored. (204 No Content is fine.)
  non-2xx ⇒ the Worker logs + swallows (PAT TTL-expires anyway).
```

Semantics requested:
- **Idempotent**: a repeat revoke for the same `job_id`, or a revoke for an unknown/already-
  expired `job_id`, should return 2xx (not 4xx). Completion webhooks can be redelivered.
- **Scoped**: revoke only the PAT(s) minted for `(owner_tenant, job_id)` — never broader.
- **Auth**: identical `x-corelink-internal-auth` shared key as `/mint` (one secret to manage).

## What's already live on the runner side (no action needed from you)

- `deploy/cloudflare/src/lib.ts`: `maybeRevokeCasPat(env, jobId)` — no-op unless the mint is
  configured; POSTs the body above; **fail-open** (any error swallowed).
- `deploy/cloudflare/src/index.ts`: the `/webhook` route now handles `workflow_job:completed`
  (already delivered — the hook subscribes to `workflow_job`) → calls `maybeRevokeCasPat`.
- Mint now keys on the GH `workflow_job.id` (the correlation id you'll receive on revoke).
- 20 unit tests green (5 cover revoke: not-configured no-op, 2xx→true, !2xx→swallowed, throw→swallowed, correct body/header).

## The ask (one decision)

1. **Confirm** `POST /internal/v1/runner/revoke` with the body/auth/idempotency above — or amend it.
2. If amended, reply with the exact shape; we transcribe + re-test (cheap, isolated to the Worker).

No deadline: TTL expiry already bounds the PAT, so this is pure window-shrinking hardening. When
your endpoint is live, revoke begins working automatically (same `CORELINK_PAT_MINT_AUTH_KEY`,
no Worker redeploy needed beyond what's already shipped).

— CoreLink Runners TL
