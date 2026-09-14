# Runbook — Dogfood go-live (first real CI on CoreLink Runners)

> **Living runbook** (not a dated handoff). Run this the moment the Northflank
> ephemeral-storage allowance is raised — it takes the runner from "wired but
> 503ing" to "a real job ran on a CoreLink ephemeral box," then to "my repos'
> CI runs here." Owner-facing; steps you (or the runner TL) execute on the box /
> in the GitHub UI.

## 0. What this proves and what it does NOT need

- **Proves:** ADR-0007 Stage-A — a queued GitHub Actions job triggers
  `acquire(runner)` → the fabric provisions a Northflank ephemeral box → it
  registers as an ephemeral GH Actions runner (label `corelink-dogfood`) → runs
  the job → self-deregisters.
- **Does NOT need the moat.** This is the COLD path (no cache-warm / no CAS
  memoization). North-star: cache absent ⇒ slow, never broken. The moat
  (D-9 mint deploy + memo-key freeze + `max_vcpu_h`) layers on later; your CI
  runs without it first. Mint wiring (WP-8a) is present but **default-off**
  (`CORELINK_PAT_MINT_*` unset ⇒ cold), so it does not gate this.

## 1. Pre-flight — confirm the blocker is actually gone

The only hard blocker today is the Northflank **ephemeral-storage allowance**
(the 503 root cause: a runner box requests ~6144 MB; the young-account cap was
2048 MB → "Configured runtime ephemeral storage exceeds your project resource
allowance"). Before running anything:

1. Northflank console → project `corelink-runners` → **Quotas**: confirm
   **ephemeral storage per instance ≥ 4096 MB** (the in-code floor
   `RUNNER_EPHEMERAL_STORAGE_FLOOR_MB`; 6144 is what we request via
   `NORTHFLANK_RUNNER_EPHEMERAL_STORAGE_MB`). If it still reads 2048 → the
   ticket has not landed; do not proceed (it will 503).
2. Confirm the fabric deployment picked up any allowance change (a redeploy/
   restart of the `corelink-runners` service if Northflank applied it lazily).

## 2. Step 1 — dispatch the smoke

`dogfood-smoke.yml` exists for exactly this (`workflow_dispatch`, `runs-on:
corelink-dogfood`). In the `HuGR-Labs/corelink-runners` repo:

- Actions → **dogfood-smoke** → Run workflow (on `main`).

The job will sit **queued** until a `corelink-dogfood` runner appears — that is
the autoscaler's job (next step).

## 3. Step 2 — watch the acquire + registration

The path: GitHub App `workflow_job: queued` webhook → HMAC verify → label-subset
gate (job labels ⊆ `FABRIC_AUTOSCALER_LABELS`) + repo allowlist → `acquire`
lease as `FABRIC_AUTOSCALER_PAT` → Northflank provision (`net_policy:
egress-runner`) → JIT config registers the runner.

Check, in order:
1. **Fabric does NOT 503 on acquire.** Northflank → service logs: the acquire
   should succeed (no `FailClosed`/`capacity_exhausted` 503). If you want to
   probe independently, a single `POST /v1/leases` as the autoscaler PAT should
   return `201`, not `503`.
2. **Runner registers:** repo → Settings → Actions → Runners → a transient
   runner advertising **`corelink-dogfood`** (NOT `self-hosted`, NOT
   `corelink-builder` — those are the persistent builders) appears.
3. **Job starts** on that runner; the box is cache-warm (the smoke asserts
   `cargo --version` / `rustc --version` work from the image).
4. **Job ends + runner deregisters;** the smoke prints
   `✅ This job ran on a CoreLink Runners ephemeral box on the cloud.`

## 4. Diagnosis — if it doesn't go green first try

This path has **never run green E2E on the live box** (it always 503'd before),
so budget for a first-run shakeout. Most likely failure modes:

| Symptom | Likely cause | Fix |
|---|---|---|
| Acquire still **503** (`exceeds allowance`) | Northflank cap not actually raised, or service didn't pick it up | Re-check Quotas (§1); redeploy the service |
| Acquire **503 fail-closed** ("plan source unreachable" / cap-absent) | entitlement lookup down, or `FABRIC_AUTH_BACKEND` not `corelink` | confirm the dogfood `runners_entitlement` row (tenant `ee30f7ba`, 80/600) is LIVE + the fabric runs `FABRIC_AUTH_BACKEND=corelink` |
| Job stays **queued forever**, no runner | label gate / allowlist | job labels must be a **subset** of `FABRIC_AUTOSCALER_LABELS` (set it to include `corelink-dogfood`); repo must pass `FABRIC_AUTOSCALER_REPO_ALLOWLIST` (or leave it unset) |
| Runner provisions but **never registers** | bad/expired `FABRIC_AUTOSCALER_PAT`, wrong `FABRIC_AUTOSCALER_RUNNER_IMAGE` digest, or JIT-config injection | check service logs for the registration call; verify the image digest is pinned + pullable |
| Job runs, **`actions/checkout` 401/403** | per-job token / network policy | the runner uses an ephemeral per-job token (no stored secret); confirm `egress-runner` net policy allows github.com |

## 5. Step 3 — point your real repos at Runners

Once the smoke is green:
1. In each target repo's workflow: `runs-on: corelink-dogfood` (or whatever
   managed label you standardize on — it must be in `FABRIC_AUTOSCALER_LABELS`).
2. The repo must be reachable by the same GitHub App (HMAC-authenticated) and,
   if `FABRIC_AUTOSCALER_REPO_ALLOWLIST` is set, listed there.
3. **Concurrency:** the dogfood tenant cap is **80** parallel runners
   (`max_concurrency`, Team). Past that, acquires queue/reject by design — not a
   failure. `max_vcpu_h=600` is the compute ceiling (only enforced once
   `FABRIC_RUNNER_VCPU` + the ceiling are armed; off by default).
4. Set `FABRIC_AUTOSCALER_EXPIRY_MS` just above your longest CI job (default
   45 min) — it is the orphan-leak backstop, not a per-job timeout.

## 6. Config reference (autoscaler env, on the deployed fabric)

`FABRIC_AUTOSCALER_*` (see `handlers/webhook.rs::env`): `WEBHOOK_SECRET`
(required — enables the route), `PAT` (tenant PAT to acquire as), `RUNNER_IMAGE`
(digest-pinned), `LABELS` (CSV, default `corelink`; must include
`corelink-dogfood`), `REPO_ALLOWLIST` (optional CSV), `EXPIRY_MS`, `TMP_ROOT`,
`MAX_TRACKED_JOBS`. Mint (default-off, WP-8a): `CORELINK_RUNNER_MINT_AUTH_KEY` +
`CORELINK_PAT_MINT_URL` — leave UNSET for the cold dogfood; set them only when
D-9 is deployed (flipping the moat on). `CLW_ENDPOINT` likewise (clw drive).

## 7. After this — the moat layer (separate, later)

Cold CI working ≠ the differentiated product. The moat (cache-warm + CAS
memoization, the "recompute ~0" wedge vs per-minute competitors) needs: (c) D-9
mint prod-Worker deploy, (d) memo-key/action-digest freeze (→ production
`AcPreLeaseHook` + the typed `_public` plan-builder, WP-8b), (e) `max_vcpu_h` on
introspect. All owner/cross-TL, sequenced AFTER cold CI is on its feet. Tracking:
the WP-8 flip-live task + `docs/handoff/2026-06-17-cache-moat-initiative-state.md`.
