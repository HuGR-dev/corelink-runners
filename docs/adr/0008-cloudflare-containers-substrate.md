# ADR-0008 — Cloudflare Containers as the compute substrate (co-located with R2)

> Status: **Accepted** (owner-ratified 2026-06-20) · Supersedes the implicit "managed
> microVM (Northflank/Fly) substrate" assumption for the PRODUCT path.
> Northflank remains the **interim** substrate to bring cold CI up while this lands.

## Context

CoreLink Runners runs untrusted CI/agent compute. The product's moat is **cache-warm by
construction**: a runner boots with the CAS/Action-Cache pre-warmed so the job's inputs are local.
The CAS lives on **Cloudflare R2** (CoreLink Cache, live). The control plane is already Cloudflare:
entitlement in **D1**, the D-9 per-job CAS-PAT mint is a **Worker**.

The compute substrate is the one part NOT on Cloudflare today — it is managed microVM on
**Northflank**, behind the `Engine` seam (`corelink-runner::isolation::Engine`,
`corelink-cloud-engine::NorthflankEngine<H: HttpTransport>`). This is the architectural drift this
ADR corrects: **the cache and control plane are Cloudflare-native, but the compute that consumes the
cache sits in a different cloud, so every cache hydration crosses the public internet** — paying R2
egress and latency on the exact hot path the moat is supposed to make free and fast.

Re-evaluated 2026-06-20 against current facts:
- **Cloudflare Containers (GA)** now offers `standard-4` = 4 vCPU / 12 GiB / 20 GB disk (custom max
  identical), account-concurrent 1,500 vCPU / 6 TiB / 30 TB. This fits real CI builds — and the 20 GB
  disk **resolves the exact Northflank ephemeral-storage cap (2 GB) currently 503-blocking us**.
- **R2 co-location is the decisive factor:** compute on Cloudflare reads R2 **in-network, zero-egress,
  low-latency**. This is structurally impossible cross-cloud. The moat economics (recompute ≈ 0,
  fast cache-warm) are maximized only when compute sits on the cache's network.
- Cloudflare Workers (V8 isolates) **cannot** run CI (no Linux, no binaries, no FS). This is about
  Cloudflare **Containers** specifically — real Linux containers in a VM, DO-managed.

## Decision

**Cloudflare Containers is the target compute substrate.** Northflank stays only as the interim
backend to validate the product and bring cold CI up while the Cloudflare backend is built and
hardened. Both live behind the existing `Engine` seam — this is a backend addition, not a
re-architecture of the runner or the wire contract.

### Selection policy (owner-ratified 2026-06-20): Cloudflare is the DEFAULT, Northflank is the fallback

The owner ratified: **keep Northflank, but Cloudflare is the default.** The composition root selects
the Engine backend in this order:

1. **Cloudflare** — if `CLOUDFLARE_SPAWN_*` env is present (`CloudflareConfig::from_env` → `Some`), use
   `CloudflareEngine`. This is the default/preferred substrate.
2. **Northflank** — else if `NORTHFLANK_*` env is present, fall back to the `NorthflankEngine` backend
   (the interim / fallback / burst substrate).
3. **Default-off** — else neither is wired (fail-closed: a runner lease is refused at admit, S2).

So a box configured for both prefers Cloudflare; Northflank serves only when Cloudflare is absent. This
selection lives in the fabric-server composition root (the next build slice — it must adapt the
runner-direct `CloudflareEngine` shape, which is spawn-only, onto the lease lifecycle that today
expects Northflank's provision+exec split).

### The provisioning-model difference (load-bearing)

Cloudflare Containers do **not** provision via REST like Northflank. A Container class **extends
Durable Object**; a Worker calls `getContainer(...)` → the DO calls `ctx.container.start(image, env)`
→ the image runs in a Linux VM. So the pivot introduces **two components**:

1. **CoreLink spawn-Worker + Container DO** (NEW; TypeScript/wrangler; `deploy/cloudflare/`).
   Exposes a small authenticated HTTP surface the autoscaler/Engine calls:
   - `POST /spawn` `{ image_digest, jitconfig, env, labels, expiry_ms }` → DO `container.start` the
     **digest-pinned runner image** (clw baked, ADR-X4 floor) with the GH-Actions JIT config + `CLW_*`
     injected → returns a container/lease handle.
   - `POST /teardown` `{ handle }` → stop the container (idempotent).
   Runs ON Cloudflare, co-located with R2 (the win). It is the only piece that replaces Northflank's
   REST provisioning.

2. **`CloudflareEngine<H: HttpTransport>`** (Rust; `corelink-cloud-engine`; mirrors `NorthflankEngine`).
   An HTTP client to the spawn-Worker, implementing the existing `Engine` seam. Default-off, selected
   by env at the composition root exactly like `cloud_backend_from_env`. Unit-tested with a mock
   `HttpTransport` (no live account needed to build + test the Rust side).

The autoscaler flow is unchanged in shape: queued job → acquire → `Engine::spawn` → (Northflank: REST
provision | Cloudflare: HTTP → spawn-Worker → DO → `container.start`). The seam was built for exactly
this swap.

> **UPDATE 2026-06-20 (what actually shipped):** the DEPLOYED autoscaler is **all-Cloudflare** — the
> spawn-Worker gained a `POST /webhook` route that IS the trigger: GitHub `workflow_job:queued` (HMAC
> verify) → mint JIT (`generate-jitconfig`) → mint per-job CAS PAT (D-9) + inject `CLW_*` → `container.start`.
> So for the Cloudflare path there is **no Rust fabric in the autoscaler loop** — the Worker owns
> trigger→mint→spawn end-to-end (proven live: a real CI job autoscaled onto a CF Firecracker microVM,
> zero manual). The `CloudflareEngine` Rust client + the `/v1/spawn` bearer surface still exist and work
> (the fabric can still drive spawns through the Engine seam), but the *deployed dogfood autoscaler* is
> the all-CF `/webhook`. The seam swap above remains valid for the fabric-driven path; the `/webhook` is
> an additional, simpler trigger that drops Northflank from the runner path entirely. Rate-limited
> (`WEBHOOK_LIMITER`, 30/60s) as defense-in-depth vs a leaked webhook secret.

### The spawn-Worker HTTP contract IS the new seam

`deploy/cloudflare/` (Worker) and `corelink-cloud-engine` (Rust client) are TRANSCRIBED against this
contract on each side (mirrors the hugit/clw discipline). Freeze it before either side builds against
it; a conformance vector keeps them from drifting.

## What is buildable now (weekend, no Northflank, no live account) vs gated

**Buildable now (default-off, gate-green):**
- This ADR (the decision + the spawn-Worker HTTP contract freeze).
- `CloudflareEngine<H: HttpTransport>` Rust skeleton behind the `Engine` seam + mock-transport unit
  tests + `cloudflare_backend_from_env` composition wiring (default-off).
- The `deploy/cloudflare/` Worker + Container DO **skeleton** (wrangler project, the `/spawn` `/teardown`
  handlers, the DO container class) — written, not yet deployed.

**Gated (needs the Cloudflare account / cross-TL):**
- Live deploy of the spawn-Worker (`wrangler` auth, push the runner image to Cloudflare's registry).
- **Security assessment of the container isolation** for untrusted multi-tenant CI (Cloudflare's
  Container VM/gVisor model vs the Firecracker-class microVM bar — owner + a deliberate review).
- **R2 co-location specifics** (in-network CAS access, credentials) — coordinate with the Cache TL.
- Validating the **12 GiB RAM ceiling** against the heaviest real builds (16 GiB was the prior target).
- GH-Actions runner **lifecycle fit** in the DO/container model (registration, egress for `git clone`,
  one-shot teardown) — proven by a live dogfood smoke.

## Consequences

- **Positive:** in-network R2 hydration (zero-egress, fast) maximizes the moat; one platform/network/
  bill for cache + control plane + compute; 20 GB disk removes the current 503 blocker; the `Engine`
  seam keeps the Rust runner + wire contract untouched.
- **Cost/risk:** a new non-Rust component (the spawn-Worker) to build, deploy, and own; pioneering
  (no documented precedent for GH-runners on Cloudflare Containers); container isolation must clear the
  untrusted-CI security bar before it serves real tenants; 12 GiB ceiling may not fit the heaviest jobs.
- **Sequencing:** (1) cold CI live on Northflank (1 cap-raise, validates the product) — DO NOT block
  on this ADR; (2) build the Cloudflare backend (Rust skeleton + Worker skeleton this weekend; live
  deploy + isolation review next); (3) dogfood smoke on Cloudflare; (4) flip Cloudflare to primary,
  capturing the R2 win; Northflank demoted to fallback/burst or retired.

## Open decisions (owner / cross-TL)

- **Worker hosting:** does the spawn-Worker live in this repo (`deploy/cloudflare/`) or a platform repo?
  (Leaning this repo — it is substrate, the runner repo owns the Engine + its backends.)
- **Isolation review owner + bar:** who signs off that Cloudflare's container isolation meets the
  untrusted-CI threat model.
- **R2 co-location seam:** Cache TL coordination for in-network CAS credentials/topology.

## Addendum 2026-06-26 — rota B: hybrid backend (runner→Cloudflare, check-exec→Northflank)

> Status: **Accepted** (Runners TL, owner-delegated 2026-06-26 — "as decisões são suas
> como techlead"). Resolves the deferred CHECK-exec-on-Cloudflare question (task #29).

**Decision.** `CloudflareEngine` v0 is **runner-only by design** (ADR-0007): the spawn-Worker's
only container is the GitHub-Actions runner image, so a CHECK-exec spec (`allow_egress=false`) fails
CLOSED at spawn (hardened in #198). The killer (githugr/hugit memoized CI) needs CHECK-exec boxes.
Rather than build a native CF CHECK-exec endpoint now (**rota A** — multi-day, deferred), the fabric
routes by lease KIND:

- **runner** lease (`allow_egress=true`) → **Cloudflare** (the moat substrate, co-located with R2);
- **check-exec** lease (`allow_egress=false`) → **Northflank** (the existing exec backend).

**Mechanism.** A `HybridBoxProvisioner` (composition root, `cloud_exec.rs`) selected when BOTH
`CLOUDFLARE_SPAWN_*` AND `NORTHFLANK_*` env are present (`select_backend(true,true) ⇒ Hybrid`).
Routing forks on `spec.allow_egress` — the red-team-blessed discriminator (a runner lease is built
only via `ContainerSpec::from_runner_lease`; egress is never inferred from the wire `net_policy`
string, the C2 invariant), so the fork cannot be spoofed. The wired exec is Northflank's
(`EngineLeasedExec`) because only a check lease ever execs (a runner is runner-direct). Both sub-
provisioners share ONE `BoxRegistry`; the hybrid records lease→backend so teardown/probe replay the
provision route (`RunningContainer` carries no provider tag), keeping a failed teardown's route so
the reaper retries against the same engine.

**Selection order (4-way, was CF→NF→off):** both ⇒ Hybrid; CF only ⇒ Cloudflare (runner-only);
NF only ⇒ Northflank (both kinds); neither ⇒ NoBox (default-off, fail-closed).

**Rota A (deferred end-state).** A native CF CHECK-exec endpoint (new spawn-Worker route + a
container that runs a `CheckDef` + `CloudflareEngine::exec_captured` impl) is the eventual win —
it puts checks on the R2-co-located moat too. Rota B unblocks the killer without it; rota A is a
future additive change behind the same `Engine` seam.
