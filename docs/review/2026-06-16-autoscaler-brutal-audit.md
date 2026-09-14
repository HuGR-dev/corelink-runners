# Brutal adversarial audit — ADR-0007 Stage B autoscaler (2026-06-16)

A multi-angle adversarial security audit (14 attack lenses, every finding
triple-refuted by independent skeptics; 10 lenses were rate-limited out and are
being re-run). Verdict on the as-audited surface: **DO-NOT-DEPLOY as configured —
3 P0, 6 P1**. This doc records the findings and their disposition; the fixes
landed in PR `fix/autoscaler-audit-hardening`.

The crypto/admission design was found **sound**: HMAC verify is constant-time
over the raw body, the egress grant is structurally gated to the runner
constructor (a forged `net_policy` on a check lease is rejected), `tmp_root` is
injection-hardened, and acquire reuses the audited cap/rate path with no bypass.
The defects were all in the autoscaler layer bolted on top.

## Two factual corrections to the audit (it read a stale committed template)

The audit read `deploy/northflank-service.json` (a stale template). The **live**
Northflank service (verified via the runtime-environment API) has `DATABASE_URL`
+ `FABRIC_LEDGER_BACKEND` set (ledger is **Postgres**, not in-memory) and
`FABRIC_AUTOSCALER_REPO_ALLOWLIST=humangr-labs/corelink-runners` set. So:
- **P0-1** worst-case ("ledger in-memory too → leaks forever") does not hold; the
  pg ledger + deadline reaper reclaim an orphaned lease at its TTL.
- **P1-4** is mitigated live (allowlist set), though the code default is serve-any.

## Disposition

| # | Sev | Finding | Status |
|---|-----|---------|--------|
| P0-2 | P0 | **Label hijack** — `job.labels` forwarded verbatim into the JIT mint; an intersection gate let `runs-on: [corelink-dogfood, corelink-builder]` mint a runner advertising the privileged `corelink-builder`. | **FIXED** — subset gate: serve only if ALL labels are managed; labels are then provably safe to forward. |
| P0-3 | P0 | **Fork-PR egress box** — `ci.yml` triggers on `pull_request:`; the allowlist checks the base repo, so a fork PR provisions an egress box billed to base. | **DOWNGRADED → GA-gated.** The repo is **private** → no external forks can open PRs, so not exploitable now. Real for public/GA: documented as a GA blocker; the GitHub fork-PR approval setting is load-bearing (runbook). `workflow_job` payloads lack head-repo, so a fabric-side fork gate needs a different signal — tracked for GA. |
| P0-1 | P0 | **In-memory job→lease map** → a redeploy resets it; later `completed`s find no binding → orphaned boxes pin slots until the lease TTL. | **MITIGATED** — default lease TTL 1h→45min (bounds the leak window); live ledger is pg so the deadline reaper reclaims. **Follow-up:** durable job→lease binding (ledger column) + provider reconciliation sweep — tracked below. |
| P1-1 | P1 | **`completed` races provision** → placeholder dropped, `record_lease` no-ops → leaked Held box + lost dedup → double-provision. | **FIXED** — `JobState` state machine: `completed` during provisioning → `CancelRequested`; `record_lease` then cancels the fresh lease immediately. |
| P1-2 | P1 | **`completed` flushes dedup** → requeue/reorder double-provisions. | **FIXED** — `completed` tombstones (`Done`) instead of deleting; a later `queued` for a finished job is deduped. |
| P1-3 | P1 | **No replay protection** — a captured delivery replays forever (cancel a victim's box / burn slots). | **FIXED** — bounded `SeenDeliveries` over `X-GitHub-Delivery`; a replayed GUID is dropped before any side effect. |
| P1-4 | P1 | **No mandatory repo auth** — default-off allowlist ⇒ serve any installed repo. | **MITIGATED** — allowlist set live; code now logs a LOUD boot warning in serve-any mode. Follow-up: derive the authorized set from the App installation. |
| P1-5 | P1 | **Minted runner registrations never deregistered** → phantom offline runners accumulate on failed provisions. | **TRACKED (owner decision)** — lowest-urgency P1 (private repo, low volume, failure-path-only, unique-name suffix avoids 409s); the fix touches the audited lease lifecycle (broker DELETE leg + runner-id side table + terminal-path calls). Implement-now vs. next-PR is an explicit owner call. |
| P2-1 | P2 | **Cancel teardown failure discarded** → a transient provider 5xx leaves a live box to its deadline, silently. | **FIXED** — teardown failure on cancel is now LOUD (reaper-style log). |
| P2-2 | P2 | **FIFO eviction orphans a live binding.** | **FIXED** — eviction reclaims terminal tombstones first; a live binding is evicted only with a loud warning. |
| P2-3 | P2 | **`ContainerSpec` derives `Debug`** with the injected JIT/ingest credential in `env`. | **FIXED** — hand-written redacting `Debug` (env keys only); regression test. |
| INFO-1 | INFO | **Northflank error echoes the raw provider body** (which carried the JIT config in the create request) into a `bail!` reaching the 503/log. | **FIXED** — provider body bounded to 200 chars in errors (`bounded_provider_body`). |

## Tracked follow-ups (not silently shipped)

1. **Durable job→lease binding + provider reconciliation** (closes P0-1 fully and
   the multi-instance variant): persist `workflow_job.id` on the lease; a startup
   sweep lists real Northflank boxes and reclaims any with no live ledger lease;
   make `claim` atomic over the durable store for `instances > 1`. Until then the
   fabric stays single-instance (the ledger already hard-pins this) and the tight
   TTL bounds the leak.
2. **P1-5 runner deregistration** (owner-gated implement-now decision).
3. **P0-3 fork gate for GA** (public repos): a fabric-side trust signal + the
   documented approval setting; per-repo concurrency sub-caps.

## Operational note (separate from the audit)

The `ci.yml` flip to the fleet (PR #82) ran fmt+clippy green on an ephemeral box
but the runner **lost communication during `cargo test`** (the box was too small
for the full `cargo test --workspace` compile). The Mac does not retire until the
runner box is sized up (a bigger Northflank plan for runner leases) or the gate is
split. #82 is held unmerged.
