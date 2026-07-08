# ADR-0007 Stage B — the autoscaler (go-live runbook)

The Stage-B autoscaler turns a queued GitHub Actions job into a freshly
provisioned ephemeral CoreLink runner **with no human in the loop**. It is the
piece that lets this repo's own CI (or any installed repo's) run on the cloud
fleet — and retire the builder Mac.

```
GitHub: job queued ─(workflow_job webhook)─▶ POST /webhooks/github
   verify HMAC → match managed label → leases::acquire(runner=Repo{owner,repo})
   → ephemeral box provisioned → JIT runner registers → job runs
GitHub: job completed ─(webhook)─▶ cancel(lease)  (frees the slot immediately)
```

It is **default-off** and adds **no new authority**: the webhook is
HMAC-authenticated (the GitHub App webhook secret) and provisions through the
exact same audited `POST /v1/leases` admission path, as a configured tenant PAT.
See `crates/corelink-fabric-server/src/handlers/webhook.rs` (module docs) and
`docs/adr/0007-direct-ci-runner-fleet.md` (Stage B).

---

## Prerequisites

- **Stage A live.** The fabric is deployed and the runner-registration broker is
  wired (`FABRIC_GITHUB_APP_*`) — proven by the manual dogfood-smoke run
  (2026-06-16). The autoscaler depends on the broker: without it `acquire(runner)`
  fails closed (400) and the autoscaler just logs the deferral.
- The runner image digest currently pinned in the fleet (see
  `memory/corelink-runner-fleet.md` for the live digest).

---

## 1. Configure the GitHub App webhook

In the **CoreLink GitHub App** settings (App id 4061919):

1. **Webhook URL:** `https://p01--corelink-runners--pmk6nf8xbcjb.code.run/webhooks/github`
2. **Webhook secret:** generate a strong random secret (e.g.
   `openssl rand -hex 32`). Save it — it goes into the fabric env (step 2) as
   `FABRIC_AUTOSCALER_WEBHOOK_SECRET`. The App and the fabric must hold the SAME
   value (the HMAC is symmetric).
3. **Subscribe to events:** check **Workflow job** (`workflow_job`). Nothing else
   is required.
4. Content type: `application/json`.

GitHub sends a `ping` on save — the fabric answers `200 pong` (once deployed with
the secret set).

## 2. Set the fabric env (Northflank `corelink-runners` service)

All default-off; absent ⇒ the route is not mounted (404 by absence).

| Var | Required | Value (dogfood) |
|---|---|---|
| `FABRIC_AUTOSCALER_WEBHOOK_SECRET` | ✅ | the secret from step 1 |
| `FABRIC_AUTOSCALER_PAT` | ✅ | the same value as `FABRIC_PAT` (acquire as the bootstrap tenant) |
| `FABRIC_AUTOSCALER_RUNNER_IMAGE` | ✅ | the pinned `ghcr.io/humangr-labs/corelink-runner@sha256:…` digest |
| `FABRIC_AUTOSCALER_LABELS` | ▫️ | `corelink-dogfood` (default: `corelink`) |
| `FABRIC_AUTOSCALER_REPO_ALLOWLIST` | ▫️ | `HumanGuardrail/corelink-runners` (defense-in-depth) |
| `FABRIC_AUTOSCALER_EXPIRY_MS` | ▫️ | default `3600000` (1h) |
| `FABRIC_AUTOSCALER_TMP_ROOT` | ▫️ | default `/tmp/runner` |
| `FABRIC_AUTOSCALER_MAX_TRACKED_JOBS` | ▫️ | default `4096` |

Notes:
- **The dogfood tenant cap** (`FABRIC_TENANT_MAX_CONCURRENCY`) must be ≥ the CI
  matrix width, or extra jobs queue with no runner. `ci.yml` is a single `gates`
  job today, so cap ≥ 2 comfortably covers a PR + a push at once.
- `FABRIC_AUTOSCALER_LABELS` must **not** include `corelink-builder` (the
  persistent self-hosted runner's label) or the autoscaler would race the
  always-on builder. Use a distinct ephemeral label (`corelink-dogfood`).

CD redeploys from `main` (~10–15 min); the new env takes effect on the new build.

## 3. Prove it end-to-end (no `ci.yml` change yet)

Dispatching the existing `dogfood-smoke` workflow emits a `workflow_job.queued`
that the autoscaler picks up automatically — proving auto-provisioning without
touching the real CI:

```sh
gh workflow run dogfood-smoke.yml -R HumanGuardrail/corelink-runners
```

Expected, with no manual `acquire`:
1. The fabric log shows `autoscaler: provisioning runner for queued job …`.
2. An ephemeral box boots, the JIT runner goes online (label `corelink-dogfood`),
   the `smoke` job runs and succeeds.
3. On completion: `autoscaler: job … completed → cancelling lease …` (slot freed,
   box torn down immediately — not waiting for the deadline).

Tail the fabric log in the Northflank UI (or via the API) to watch the three
`autoscaler:` lines.

## 4. Flip the real CI (after the proof)

Once step 3 is green, point this repo's CI at the fleet. In `.github/workflows/ci.yml`
change `runs-on: [self-hosted, corelink-builder]` to the ephemeral label
(`runs-on: corelink-dogfood`) — and the builder Mac stops running CI. (The image
must carry the full gate toolchain; the dogfood image already does — see
`deploy/runner/README.md`.) Do this as its own PR so the cutover is reviewable
and revertible.

---

## Failure modes (all fail-closed / acked)

| Symptom | Cause | Behaviour |
|---|---|---|
| 404 on the webhook | secret not set | route not mounted (default-off) |
| 401 on the webhook | wrong/missing signature | rejected before any side effect |
| `acquire(runner) returned 400` | broker not wired / runner mode off | logged, webhook acked (no redelivery storm) |
| `acquire(runner) returned 429` | dogfood tenant over cap | logged, job stays queued; raise the cap |
| job stays queued, no `autoscaler:` log | label not in `FABRIC_AUTOSCALER_LABELS`, or App not subscribed to `workflow_job` | check the App event subscription + the label set |

The autoscaler never holds a lease past the lease deadline: even if a `completed`
webhook is missed, the always-on reaper reclaims the slot at expiry.
