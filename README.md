# CoreLink Runners

**Ephemeral, cache-warm CI/build compute — billed by concurrency, not minutes.**

CoreLink expansion campaign #1. The compute substrate beneath CoreLink Cache and
beneath hugit's memoized-CI forge:

```
HuGR (the company / brand)
 └─ CoreLink (the platform)
     ├─ Cache        — content-addressed CAS + Action Cache   (live)
     ├─ Runners      — ephemeral compute on the cache         (THIS REPO)
     └─ Workspaces   — workspace-as-object                    (campaign #2)
   hugit (the forge for agent fleets)                         (BUILT)
```

## What it is

GitHub Actions charges per-minute and divides your quota by how many jobs run in
parallel. CoreLink Runners inverts that model: **you buy N parallel runners, flat,
and minutes are unlimited**. Jobs boot on ephemeral microVMs with CoreLink's
CAS/Action-Cache pre-warmed — inputs are local before the job starts. A re-run
whose result is already memoized returns from cache in milliseconds rather than
re-executing; you are never billed as if it ran again.

The ICP is teams running wide, warm, and often: agent fleets, heavy CI,
monorepos. Per-minute pricing punishes exactly this workload; flat concurrency
serves it.

## Pricing

**Flat by concurrency, never per-minute. Minutes unlimited.**

| Tier | $/mo | Concurrency | Hard ceiling (vCPU-h/mo) | Max COGS |
|---|---|---|---|---|
| Starter | $16 | 20 | 100 | $10 |
| Pro | $40 | 40 | 240 | $24 |
| Team | $100 | 80 | 600 | $60 |
| Scale | $200 | 160 | 1,200 | $120 |
| Max | $400 | 320 | 2,400 | $240 |

*Ratified 2026-06-16 at the real $0.10/vCPU-h COGS basis — the canonical ladder
(`docs/product/pricing.md` §2). The pre-amendment $8/$20/$50/… table was ~6×
underwater and is superseded.*

No free tier. 5-day trial at Team-level capability (card on file; converts or
downgrades at end). Above Max: Enterprise (custom, governance, BYOC).

Each tier has two limits: a **concurrency cap** (always on — bounds peak burn
rate) and a **vCPU-hour ceiling**. Crossing the ceiling is **deliberately not a
gate**: the customer's job keeps running, and usage above the tier's included
`max_vcpu_h` is billed as overage at 3x COGS (`deploy/cloudflare/src/lib.ts:712,722-723`)
— it ships no queue and no block, only warning thresholds at 80%/100% of the
ceiling. With both limits, the maximum COGS a single tenant can incur is strictly
below the tier price (Max COGS column above) — so **within the tier limits it is
structurally impossible to lose money on a tenant**, and overage past the tier
limits is billed, not absorbed. ⚠️ The vCPU-h ceiling is enforced in code but
**default-off**: it is armed by setting `FABRIC_RUNNER_VCPU > 0` (a deployment
step, not a code change); until armed, the concurrency cap is the only wall. The
ceiling is generous enough that a real workflow never approaches it. Full model:
[`docs/product/pricing.md`](docs/product/pricing.md).

## Architecture

Eight crates in one Cargo workspace (`crates/`):

| Crate | Role |
|---|---|
| `corelink-runner` | Execution core: lease lifecycle, isolation, teardown, boot, concurrency/expiry/recovery, Actions-YAML shim, fence enforcement (`materialize`/`enforce`), X4 supply-chain oracle, §13 envelope (derivation collector, CaptureHook, JobClose ack). |
| `corelink-fabric` | Control-plane core: `LeaseLedger` trait + in-memory and Postgres (`PgLedger`) implementations, `SlotMeter` bounded billing journal, `FairScheduler` (CP4), plan/tier types, `BoxRegistry`, crash/expiry reaper. |
| `corelink-fabric-server` | HTTP server binary (`corelink-fabricd`): axum router, auth middleware, lease/exec/attestation handlers, tower load-shed, global concurrency limit, admin tenant endpoint, cloud backend wiring. |
| `corelink-fabric-api` | Frozen wire DTOs: `AcquireRequest`, `ExecRequest`, `CloseResponse`, etc. Shared by the server and the CLI — no drift possible. |
| `corelink-cloud-engine` | Northflank Job-run adapter behind the `Engine` seam; `HttpTransport` trait quarantines `ureq`; swap-in for Firecracker when bare-metal arrives. |
| `corelink-cli` | The `corelink` client/ops binary: `smoke` (post-deploy health + fail-closed gate verification) and `verify` (customer-trust primitive: verify `result_binding_sig_v2` against the published ed25519 key). |
| `corelink-runners-contracts` | Frozen wire-contract types transcribed from hugit-contracts (`RunnerLease`, `RunnerState`, `FenceManifest`, `MaterializedEntry`, `IntentMetrics`). Byte-identical conformance vectors under `conformance/`; golden tests verify SHA-256 + membership + tamper rejection. |

**Wire-contract law:** types are transcribed on each side; `hugit-contracts` is
frozen and never imported. `deny.toml` enforces crates.io-only external deps (no
`git`/`path` dep in either direction). The conformance vectors are the drift
tripwire — either side's golden tests go red on any type divergence.

## Status

**`v0.1.0-seed` shipped 2026-06-12.** ⚠️ **Substrate flip (2026-07, ADR-0008):**
the live deploy is now **Cloudflare-first** — fabricd runs as a CF Container +
proxy Worker (`deploy/cloudflare-fabricd/`), boxes spawn on the CF
`corelink-spawn-worker` (`deploy/cloudflare/`), and the moat (native check-host
exec + per-job CAS-PAT mint + attested cost) is live on Cloudflare. Per
`deploy/cloudflare-fabricd/wrangler.jsonc`, the current CF deploy is a
**singleton** (`FABRIC_NUM_SHARDS=1`) with `DATABASE_URL` **declared bound** (a
wrangler secret — its value can't be read back from this repo, so this is what
the config states, not an independently-verified live probe) → the pg-durable
ledger is the declared active backend; N>1 is kept at 1 by deliberate volume
choice, not a technical block. See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the
full, current item list — it is the source of truth over this section.

<details>
<summary><strong>Historical status (as of 2026-06-14, Northflank substrate)</strong> — preserved for the record, superseded by the flip above</summary>

The fabric was live on Northflank, proven end-to-end (acquire → real microVM →
exec → signed attestation → teardown):

- Multi-instance on a persistent Postgres ledger; cross-instance cap-safety
  proven live (2 containers, advisory-lock serialized admission, no over-admit).
- Exhaustively audited (28 confirmed findings closed, including a P0
  attestation-forgery fixed as `result_binding_sig_v2`). Zero open P0/P1.
- §13 envelope seam fully wired on the fabric side: per-lease `CaptureHook`,
  per-turn ingest endpoint, durable checkpoint, 3-tier abnormal flush.
- Control plane: runtime tenant onboarding, durable billing exporter
  (`PgBillingSink`), `FairScheduler` (CP4), crash-probe sweep, orphan GC.
- `corelink` CLI shipped.

Northflank remains the ADR-0008 fallback substrate (not the live one) — see
`docs/deploy/northflank-postgres-runbook.md` and
`docs/deploy/corelink-flip-runbook.md` (both now marked superseded-substrate).

</details>

What remains before paying customers: the CoreLink auth+billing flip
(`FABRIC_AUTH_BACKEND=corelink`, pending corelink-server `runners_entitlement`),
hugit adopting `result_binding_sig_v2`, and M2 self-serve onboarding.

## Quickstart — local single-tenant fabric

```sh
# 1. Build
cargo build --workspace --locked

# 2. Run the fabric (dev key; mock exec — no cloud backend needed)
FABRIC_DEV_UNSAFE=1 \
FABRIC_PAT=dev-pat \
FABRIC_TENANT=dev \
FABRIC_TENANT_MAX_CONCURRENCY=4 \
FABRIC_MOCK_EXEC=1 \
  ./target/debug/corelink-fabricd
# binds localhost:8080; refuses to start on a non-loopback address with FABRIC_DEV_UNSAFE=1

# 3. Smoke it
CORELINK_PAT=dev-pat corelink smoke --url http://localhost:8080
# add --full to do a real acquire → cancel round-trip
```

`FABRIC_DEV_UNSAFE=1` boots with the insecure well-known dev signing key — local
use only; attestations are forgeable. `FABRIC_MOCK_EXEC=1` routes all exec calls
through `MockLeasedExec` (no Northflank credentials needed). The full env-var
reference is in [`docs/deploy/fabric-server.md`](docs/deploy/fabric-server.md);
the CLI reference is in [`docs/cli.md`](docs/cli.md).

To run against the live Northflank backend, add `FABRIC_SIGNING_KEY` (32-byte
random seed, base64) and the `NORTHFLANK_*` vars (see
[`docs/deploy/northflank-postgres-runbook.md`](docs/deploy/northflank-postgres-runbook.md)).

## Gate (CI)

```
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check
cargo audit --deny warnings
```

All five pass on CI (`runs-on: corelink` — the self-hosted ephemeral Firecracker
fleet, per `.github/workflows/ci.yml`) before merge. Never
`gh pr merge --auto` — the CI-green-before-merge rule is manual discipline (GitHub
free plan + private repo, no branch protection).

## Key documents

| Document | What it is |
|---|---|
| [`docs/whitepaper/corelink-runners-v1.md`](docs/whitepaper/corelink-runners-v1.md) | Canonical product vision — source of truth on why and what it must be |
| [`docs/product/product.md`](docs/product/product.md) | Product: vision, market wedge, user stories, positioning, roadmap |
| [`docs/product/pricing.md`](docs/product/pricing.md) | Pricing model: full rationale, loss-impossible guarantee, competitive position |
| [`docs/cli.md`](docs/cli.md) | `corelink` CLI reference (`smoke`, `verify`) |
| [`docs/api/v1-reference.md`](docs/api/v1-reference.md) | Full `/v1` HTTP API reference — every endpoint, DTO, auth, status code |
| [`docs/spec/hugit-integration-contract.md`](docs/spec/hugit-integration-contract.md) | fabric wire + envelope contract v1.4.0 (historical hugit framing — hugit discontinued; the §13/attestation mechanisms it specs are the fabric's own + live) |
| [`docs/spec/corelink-fabric-stub.md`](docs/spec/corelink-fabric-stub.md) | CoreLink-side fabric/scheduler/billing stub |
| [`docs/deploy/fabric-server.md`](docs/deploy/fabric-server.md) | `corelink-fabricd` env vars and Docker deploy |
| [`docs/deploy/northflank-postgres-runbook.md`](docs/deploy/northflank-postgres-runbook.md) | Northflank + Postgres production deploy runbook |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Full item-by-item roadmap: closed, in-flight, remaining |
| [`CLAUDE.md`](CLAUDE.md) | Context and house rules for AI agents working in this repo |
