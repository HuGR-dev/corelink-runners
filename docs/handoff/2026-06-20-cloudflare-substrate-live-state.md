# State — CoreLink Runners on Cloudflare: LIVE + autoscaling (2026-06-20)

> Consolidated state after the 2026-06-20 warp-speed Cloudflare go-live. Resume point for any session.
> Headline: **a real CI job autoscales onto a Cloudflare Firecracker microVM with zero manual steps.**

## What is PROVEN live (on `gmhelmold@gmail.com`'s CF account — the R2-CAS-hosting account)

- **Substrate:** Cloudflare Containers = AWS Firecracker microVM per instance (KVM hardware isolation;
  confirmed via primary CF docs). Isolation assessment: CONDITIONAL PASS (`docs/review/2026-06-20-cloudflare-containers-isolation-assessment.md`).
- **spawn-Worker** (`deploy/cloudflare/`, `corelink-spawn-worker.gmhelmold.workers.dev`): `/v1/spawn`,
  `/v1/status/{handle}`, `/v1/teardown` (bearer, constant-time), `/webhook` (GitHub HMAC autoscaler).
  Container = the real runner image (`deploy/runner/Dockerfile`: GH agent + clw + Rust), standard-4.
- **CloudflareEngine** (`corelink-cloud-engine`, Rust) behind the `Engine` seam; composition-root
  selection CF→Northflank→off (ADR-0008). All default-off in the fabric.
- **Automatic autoscaler (ALL-CLOUDFLARE, no external fabric):** GitHub `workflow_job:queued` → Worker
  `/webhook` (HMAC verify) → mint one-shot JIT (`generate-jitconfig` via `GITHUB_MINT_TOKEN`) → spawn.
  PROVEN: `dogfood-smoke` dispatch → webhook `queued→201` → job SUCCESS on `cf-runner-<uuid>`
  (`cloudflare-firecracker` kernel) → self-deregister. Repo webhook id 644667520.
- **Cache-warm entrypoint hook** (`deploy/runner/entrypoint.sh`): hydrates from the CAS via clw when
  CLW_* injected; **fail-OPEN to cold** (north star). Verified: cold path still green (moat dormant).

PRs #102–#115 (ADR-0008, CloudflareEngine, contract, Worker, selection, isolation assessment, Stage A/B,
auth hardening, R2 finding, autoscaler, cache-warm entrypoint).

## What gates the WARM moat (recompute ≈ 0) — the one real external lever

The runner-side moat wiring is DONE. A functional WARM run needs the runner to authenticate to the CAS
with a **per-job CAS PAT** — which is minted by **D-9 (server-side, corelink-server)**. A6 forbids using
the tenant PAT on the box, so there is NO shortcut. **⇒ The warm moat is gated on the D-9 mint deploy.**
Once D-9 is live: inject `CLW_ENDPOINT` (corelink-server's in-network CAS API — see
`2026-06-20-r2-colocation-finding.md`: in-network HTTP, NOT direct R2) + `CLW_TENANT` + `CLW_TOKEN`
(per-job) + `CLW_REF_DOMAIN=runner` into the spawn → the entrypoint hook hydrates → warm.

## Remaining (by owner / gate)

- **Set `CORELINK_PAT_MINT_AUTH_KEY` on the spawn-Worker → unblocks the warm moat. THE lever.**
  The D-9 mint endpoint is LIVE (server-side); the only missing piece is this Worker secret. Its
  value is delivered OOB by the Server TL (not in the repo/account — confirmed absent from
  `wrangler secret list` 2026-06-20). Once set, the `/webhook` mints the per-job PAT + injects
  `CLW_*` → warm, no redeploy.
- **Prod secrets rotation — PARTIALLY DONE 2026-06-20/21:**
  - ✅ `CLOUDFLARE_SPAWN_AUTH_TOKEN` — rotated to a fresh random (off the `dogfood-smoke-…` throwaway).
  - ✅ `GITHUB_WEBHOOK_SECRET` — rotated on BOTH sides (GitHub hook 644667520 + Worker), verified by a
    ping delivery returning HTTP 200 (HMAC matches; autoscaler intact). NOTE: hook edits must use the
    **canonical** repo path `HumanGuardrail/corelink-runners` — `humangr-labs/…` 307-redirects and
    `gh api -X PATCH` does NOT follow it (silent no-op).
  - ⏳ `GITHUB_MINT_TOKEN` — still the dogfood `gh auth token`. Swapping for a dedicated fine-grained
    repo-admin PAT is **UI-only** (GitHub forbids PAT creation via API) → owner action, only needed
    before a non-dogfood tenant.
- **§6 isolation — ALL CLEARED 2026-06-20** (PR #121 + the assessment doc): rate-limit on `/webhook`
  (`WEBHOOK_LIMITER`, 30/60s), egress per ADR-0003, side-channel resolved-by-design, cache-seam tenant
  isolation confirmed by the Server TL. Assessment is now PASS (was CONDITIONAL).
- **Polish — DONE:** Worker unit tests (14, auth/HMAC/fail-open), docs reconciled, dead code removed.

### Per-job CAS-PAT lifecycle posture (TTL backstop + explicit revoke — BUILT)

The per-job CAS PAT is **TTL-bounded** (D-9 mints with a 5400s TTL) AND now **explicitly revoked on
completion** (hardening is not optional). The Worker mints the PAT under `job_id = GitHub workflow_job.id`
(stable across queued→completed), so on `workflow_job:completed` it calls
`POST {CORELINK_MINT_URL}/internal/v1/runner/revoke` `{owner_tenant, job_id}` — **no `job→pat` KV/DO map
needed**. Built + unit-tested (`maybeRevokeCasPat`, 5 tests) and shipped; it is **fail-open** (a missing/
non-2xx revoke is swallowed — TTL expiry remains the backstop), so it never breaks a job. The one
remaining piece is **server-side**: the `/internal/v1/runner/revoke` endpoint itself, whose contract was
relayed for freeze: `docs/handoff/2026-06-20-relay-to-server-tl-d9-revoke-contract.md`. Until that endpoint
is live, revoke is a harmless no-op and the PAT TTL-expires as before; once live, revoke activates with no
Worker redeploy beyond what's already shipped.

## Architecture note (reversal, flagged)

ADR-0008 selection policy said the autoscaler would be the existing Rust fabric. In practice the
autoscaler was built **all-Cloudflare** (the `/webhook` route on the spawn-Worker) because it's the path
fully ownable without an external Rust-fabric deploy — it drops Northflank from the runner path entirely.
The Rust fabric autoscaler (`webhook.rs`) still exists and works with `CloudflareEngine`; the all-CF path
is the deployed reality. Update ADR-0008's selection note to reflect this when convenient.
