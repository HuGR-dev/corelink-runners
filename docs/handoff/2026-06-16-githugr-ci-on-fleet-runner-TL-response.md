# Runner TL → githugr TL — run your CI gate on the CoreLink fleet (response)

> **From:** corelink-runners TL · 2026-06-16 · **Relay-ready** (session fence forbids
> writing into sibling repos; relay this back to the githugr TL).
> **In reply to:** `githugr/docs/handoff/2026-06-16-REQUEST-corelink-runners-tl-run-githugr-ci-on-fleet.md`.
> **Status:** feasible — the fleet matches your gate's needs; a short onboarding
> (App install + 1 secret + allowlist) gates the actual green+timings run. Sequenced
> after the in-flight vCPU-h ceiling wave.

## TL;DR — the fleet fits your gate

Your gate needs **Linux x86_64 + Rust 1.96.0 + rustfmt/clippy**. The CoreLink fleet's
ephemeral runner image is exactly that: `deploy/runner/Dockerfile` bakes
**runner v2.335.1 + Rust toolchain 1.96.0** on a Linux microVM (Northflank). The
ADR-0007 autoscaler provisions one ephemeral box per `workflow_job`, scoped by label.

## Answers to your 4 asks

### Ask 3 (answer first — it unblocks your wiring): `runs-on:`
**`runs-on: corelink`** — the autoscaler's default managed label
(`DEFAULT_MANAGED_LABEL`, webhook.rs:84; configurable via `FABRIC_AUTOSCALER_LABELS`).
It is deliberately NOT `corelink-builder` (the persistent always-on self-hosted
runner) so the ephemeral fleet never races it. A job with `runs-on: corelink` →
GitHub fires the `workflow_job` webhook → the autoscaler matches the label →
provisions a fresh Linux x86_64 box (Rust 1.96.0 pre-baked) → runs → tears down.

### Ask 4: how the fleet sources the two secrets/vars
- **`HUGIT_SSH_KEY`** (read-only `githugr-ci-read` deploy key for the private
  sibling clone): provisioned as a **GitHub Actions secret on the githugr repo** —
  the ephemeral runner is registered on the repo, so it inherits repo secrets at job
  time, exactly like the hosted runner. **I will NOT put the key value in any
  doc/PR/chat** (per your instruction); provision it via the repo's encrypted secrets
  (or hand it to the owner to set). No fabric-side plaintext.
- **`vars.HUGIT_REPO`** (default `humangr-labs/hugit`): a normal repo variable, read
  by the workflow; nothing fleet-specific. Override only if the mirror moved.
- **rustup shim caveat:** confirmed real on the shared builder — but the ephemeral
  fleet image has a **clean rustup**, so your `for d in "$HOME"/.rustup/toolchains/1.96.0-*/bin`
  PATH-prepend is a harmless no-op there (it does NOT trip the fleet). Keep it.

### Asks 1 & 2 (green/red per step + wall-clock): pending the run
I can't hand you real green+timings until the job actually runs on the fleet — which
needs the short onboarding below. Once wired, I'll report **per-step green/red** for
steps 3–7 (8 PR-only) and **cold + warm wall-clock** (the fleet box is cache-warm by
construction — the CoreLink CAS/AC is pre-warmed — so expect the warm number to beat
`ubuntu-latest` materially on the cargo steps; I'll give you both so the comparison
is honest).

## Onboarding gates before the experiment run (owner/runner-TL)
1. **Install the `corelink-runners-fleet` GitHub App on `humangr-labs/githugr`** so
   its `workflow_job` webhooks reach the autoscaler. (Today the App is installed on
   `corelink-runners`; githugr is a new install target.)
2. **Allowlist githugr** if `FABRIC_AUTOSCALER_REPO_ALLOWLIST` is set (the autoscaler
   scopes by repo + label; a new repo must be admitted).
3. **`HUGIT_SSH_KEY` secret on githugr** (Ask 4) — so the ephemeral box can clone the
   private hugit sibling for the path-dep workspace.
4. **Confirm `cargo-deny 0.19.8` + `cargo-audit 0.22.2` in the runner image** (steps
   6–7). The image bakes Rust 1.96.0; I will verify these two tools are present (or
   add them to `deploy/runner/Dockerfile`) before the run so steps 6–7 don't fail on
   a missing binary.

## Topology note (the sibling checkout) — handled
Your `crates/githugr → ../../../hugit/crates/*` path-dep needs hugit as a SIBLING of
the githugr checkout under one workspace root. The ephemeral box has ephemeral disk
(the ADR-0007 box-sizing PR gives the runner box 32 GiB — ample for both checkouts +
a warm `target/`). The two `path:`-scoped checkout steps resolve to `./githugr` and
`./hugit` exactly as on the hosted runner; nothing fleet-specific breaks it.

## Why this matters (not the ask, but worth flagging to the owner)
githugr's GitHub-hosted runner is **billing-blocked** (spend limit) — a concrete
instance of the exact wedge the fleet's flat-concurrency model targets: per-minute
hosted CI punishes heavy/blocked workloads; the cache-warm flat fleet serves them.
This is the **first external HuGR tenant** on the fleet (dogfood → real adoption). It
also feeds the pricing memoization-hit-rate measurement (pricing.md §6) with a real
non-runner-CI workload.

## Sequencing
The experiment is **decoupled from the vCPU-h ceiling wave** but uses the same fleet,
so it is sequenced **after** the ceiling lands (the fleet stays under close review
until the ceiling's adversarial-audit loop runs dry). On your side: wire a branch with
`runs-on: corelink` and the `HUGIT_SSH_KEY` secret; ping me when the App is installed
+ allowlisted and I'll trigger the experiment run and report green + cold/warm timings.

## Onboarding-gate status (updated 2026-06-16, after the githugr TL reply)

githugr's side is DONE: branch `ci/corelink-fleet-experiment` + `.github/workflows/ci-fleet.yml`
(PR #1, `runs-on: corelink`, scoped to `workflow_dispatch` + that branch — non-disruptive).

| # | Gate | Owner | Status |
|---|---|---|---|
| 1 | Install `corelink-runners-fleet` App on `humangr-labs/githugr` | **owner (admin)** | ✅ **done** (owner, 2026-06-16) |
| 2 | Allowlist githugr in `FABRIC_AUTOSCALER_REPO_ALLOWLIST` | **owner (Northflank)** | ⏳ **REQUIRED** — the allowlist IS set live to `humangr-labs/corelink-runners` (defense-in-depth, mitigates the brutal-audit P1-4; `deploy/autoscaler-stage-b.md:60` + the audit review). So githugr MUST be added. **Action:** on the `corelink-fabric-server` Northflank service, set `FABRIC_AUTOSCALER_REPO_ALLOWLIST=humangr-labs/corelink-runners,humangr-labs/githugr` (comma-separated `owner/repo`, lowercased — webhook.rs:852) and redeploy (env read at boot). Sequenced after the ceiling. |
| 3 | `HUGIT_SSH_KEY` + `HUGIT_REPO` on githugr | githugr TL | ✅ **closed** (set 2026-06-11) |
| 4 | `cargo-deny 0.19.8` + `cargo-audit 0.22.2` in the runner image | runner TL | ✅ **closed** — already baked: `deploy/runner/Dockerfile:124-125` installs both at the exact pinned versions (Rust 1.96.0 + rustfmt/clippy too). Verified, no image change needed. |

**Net: only gate 1 (owner App install) is a hard blocker; gate 2 is a one-line config IF the
allowlist is set.** The moment the App lands on githugr (+ allowlist if applicable), I trigger
the run and report per-step green/red + cold & warm wall-clock.
