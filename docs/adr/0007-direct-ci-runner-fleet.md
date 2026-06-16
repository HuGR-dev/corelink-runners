# ADR-0007 — Direct-CI on-ramp: an ephemeral GitHub Actions runner fleet

- **Status:** Accepted (owner decision, 2026-06-15)
- **Date:** 2026-06-15
- **Context source:** a drift review (2026-06-15, 3 read-only studies) found that
  the "direct adoption surface" shipped earlier this session (`corelink run` CLI +
  the GitHub Action wrapping it) is actually the **hugit memoized-check path**
  (campaign #3) in Action clothing — not the direct-CI on-ramp the canonical
  vision (whitepaper §9, product.md ICP-B) calls for. This ADR fixes the model.

One line: the direct-CI front door is an **ephemeral, JIT-registered GitHub
Actions self-hosted runner fleet** — the customer targets `runs-on: corelink[-<size>]`
and their **unmodified** workflow (checkout, matrix, every step) runs on a
cache-warm CoreLink microVM, one ephemeral runner per job, billed by concurrency.

## Context

**The drift (evidence, from our own code):**
- `corelink run` zero-fills `tree_hash` and the comment admits it: *"a command
  runner, not a memoized check pipeline"* (`crates/corelink-cli/src/run.rs`). It is
  the hugit `CheckDef`/`CheckResult` path with two of three memo axes nulled.
- `result_binding_sig_v2` binds **`memo_key`** (`binding.rs`) — hugit's frozen
  three-axis memoization key. A normal CI customer does not ed25519-verify a
  signature over their build's exit code; that is a memoized-fleet trust primitive
  (campaign #3), not ICP-B's need.
- The shipped GitHub Action runs `runs-on: ubuntu-latest`, does `actions/checkout`
  on GitHub's **own** runner, and wraps **one** `corelink run --check '<cmd>'` step.
  It does not provide `runs-on: corelink`.

**What ICP-B actually asks for** (product.md:56-61): *"8 parallel runners, fixed
price, unlimited minutes, already warm"* — a **runner fleet** their whole existing
pipeline lands on, not a single attested-command step.

**Why private-repo CI is NOT a blocker** (the question that triggered this ADR):
GitHub injects a per-job `GITHUB_TOKEN` at **runtime**, auto-expiring at job end —
`actions/checkout` uses it to clone a private repo. It is never a persistent stored
secret. ADR-0003 / §5 (C5b) ban only **persistent platform secrets on the box**;
a short-lived, job-scoped capability is an explicit, bounded exception — and the
fabric **already injects exactly such a capability today** (the per-lease ingest
token, `envelope_inject.rs`, ADR-0006). A GitHub *runner registration token* is the
same bounded class. So private-repo CI on the fabric is native, not precluded.

## Decision

1. **The direct-CI on-ramp is an ephemeral GitHub Actions runner fleet.** A
   customer installs the **CoreLink GitHub App** (granting the fabric permission to
   register runners on their repo/org) and writes `runs-on: corelink[-<size>]`.
   The fabric provisions a cache-warm ephemeral microVM per queued job, registers it
   as a JIT `--ephemeral` runner, GitHub assigns the job, the runner runs the
   **unmodified** workflow, then it deregisters and the box is torn down. One runner
   lease = one billable concurrency slot; pricing is unchanged (flat by concurrency).

2. **The `corelink run` CLI + GitHub Action + `result_binding_sig_v2` + verify SDKs
   are re-scoped** to what they are: the **hugit / campaign-#3 memoized-check path**
   plus a power-user "run one attested check" primitive. They are correct and
   load-bearing there — but they are **not** the direct adoption surface and must
   stop being labelled as it. (No code is wrong; the positioning is corrected — see
   the whitepaper §9 + ROADMAP edits accompanying this ADR.)

## Architecture

A new **runner-lease mode** alongside the existing check/exec-lease mode. Both ride
the same lease / isolate / concurrency-cap / teardown spine; they differ in what the
box runs and how the lifecycle is driven.

| | Check lease (today, hugit/power-user) | Runner lease (this ADR, direct-CI) |
|---|---|---|
| Box command | fabric sets `sh -lc <check.command>` per `/exec` | provision-time: the GH runner agent (`config + run --ephemeral --jitconfig`) |
| Drive | fabric-driven, per-`/exec` REST, attested | GitHub-driven: GitHub assigns the job; fabric provisions-then-waits-for-exit |
| Source onto box | CAS / cache-warm materialization | native `actions/checkout` (GitHub's per-job token) |
| Credential injected | per-lease ingest token | per-lease **JIT runner registration config** (ephemeral) |
| Result | `CheckResult` + `result_binding_sig_v2` | the customer's own GitHub check status |

**Lifecycle (runner lease):** acquire → broker mints a JIT ephemeral runner config →
provision box (`image` = runner image, command = agent run, `env` = {JIT config,
labels}) → box registers + GitHub assigns one job → run → agent exits (`--ephemeral`)
→ teardown. The fabric's per-`/exec` attestation path is **bypassed** in this mode.

**The one genuinely new subsystem — the registration-token broker.** A GitHub App
(the fabric holds the App private key, **never** on the box) that, per runner-lease,
mints a short-lived, repo/org-scoped JIT `--ephemeral` runner registration config.
Architecturally identical to the existing `IngestSigner` (ADR-0006), just exchanging
GitHub App credentials instead of signing an ingest token. Only the ephemeral config
reaches the box.

## Frozen contracts (build against these)

- **C1 — `RunnerRegistrationBroker` trait** (`corelink-fabric-server`):
  `async fn mint_jit_config(&self, scope: &RunnerScope) -> Result<JitRunnerConfig, BrokerError>`
  where `RunnerScope` identifies the customer repo/org + requested labels, and
  `JitRunnerConfig` carries the opaque, short-lived registration payload the agent
  consumes. A `MockBroker` (deterministic, no network) backs tests; the real
  `GitHubAppBroker` calls the GitHub App runner-registration API. `BrokerError`
  fail-closed: any failure ⇒ the runner lease is **not** provisioned (no silent
  half-state).
- **C2 — runner net_policy.** A new isolation value `"egress"` (egress-allowed) the
  isolation gates admit **only** for runner-mode leases; check leases keep the
  no-egress floor. ADR-0003 already accepts outbound egress on the managed tier, so
  this is admitting at the gate what the provider already permits — scoped to runner
  leases so the hugit/check path's posture is unchanged.
- **C3 — runner image.** A digest-pinned image (X4 floor) containing the GitHub
  Actions runner agent + the cache-warm toolchain base; its command launches
  `config.sh --ephemeral --jitconfig <…> && run.sh`. The registration config is read
  from env (the existing provision-time env channel), never baked into the image.
- **C4 — runner lease ⇆ GitHub job.** One runner lease backs at most one GitHub job
  (`--ephemeral`); the lease holds one concurrency slot for the job's lifetime,
  enforced by the same ledger `try_admit`. Billing is unchanged (slots, never
  minutes).

## Staged build

- **Stage A (MVP / dogfood-able):** C2 net_policy + C3 image + C1 broker + the
  runner-lease provision→wait→teardown lifecycle. Enough to point **our own** repos'
  CI at `runs-on: corelink` and offload the builder Mac (the immediate need).
- **Stage B (autoscaler) — BUILT (2026-06-15):** a `workflow_job` webhook receiver
  (`POST /webhooks/github`, `crate::handlers::webhook`) → provision one runner lease
  per queued job, cancel it on completion (the ARC-style autoscaling pattern).
  HMAC-authenticated (the App webhook secret), default-off, and **no new authority** —
  it drives the same audited `leases::acquire`/`cancel` path as the `/v1` surface, as a
  configured tenant PAT (no admission bypass). Label-scoped (+ optional repo allowlist).
  Runbook: `deploy/autoscaler-stage-b.md`. This is what makes auto-provisioned CI real.
- **Stage C (GA):** sizes/labels, the App-install UX, the customer console
  (consumes `/v1/usage`), per-job billing reconciliation, SLOs.

## Consequences

- Whitepaper §9 and ROADMAP are updated: the direct front door is a runner fleet,
  not a "shim" step. The hugit/via-hugit front door is unaffected (it keeps the
  memoized attested check-exec model).
- New trust surface: the GitHub App private key, held by the fabric (same class as
  the Northflank token + the ingest signing key — never on the box). Per-job tokens
  are bounded ephemeral capabilities, ADR-0003-consistent.
- The earlier "adoption surface" framing is corrected in-place; the code (`corelink
  run` / Action / attestation / SDKs) is retained for the hugit path + power users.
- Supersedes the discarded ADR-0007 (Firecracker isolation seam, removed 2026-06-14)
  — that number is reused here.
