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

- **D-9 mint prod-Worker deploy** (server-side) → unblocks the warm moat. THE lever.
- **Prod secrets rotation** (`!`): the dogfood spawn token (`dogfood-smoke-…`), webhook secret
  (`whsec-…`), and `GITHUB_MINT_TOKEN` (currently `gh auth token` — swap for a dedicated fine-grained
  repo-admin PAT).
- **§6 isolation** remaining: rate-limit on `/webhook`, egress-policy confirm, cache-seam tenant isolation
  (ties to the warm wiring).
- **Polish (non-gated, runner-side):** Worker autoscaler tests (HMAC/filter/fail-open), docs.

## Architecture note (reversal, flagged)

ADR-0008 selection policy said the autoscaler would be the existing Rust fabric. In practice the
autoscaler was built **all-Cloudflare** (the `/webhook` route on the spawn-Worker) because it's the path
fully ownable without an external Rust-fabric deploy — it drops Northflank from the runner path entirely.
The Rust fabric autoscaler (`webhook.rs`) still exists and works with `CloudflareEngine`; the all-CF path
is the deployed reality. Update ADR-0008's selection note to reflect this when convenient.
