# CoreLink Runners — Features & Functionality

**Purpose.** The exhaustive, evidence-grounded inventory of every feature and capability in
CoreLink Runners — the product's canonical capability map, and the baseline the validation
campaign's "every feature validated" is measured against.

**Last updated:** 2026-07-17 · **Status:** `v0.1.0-seed` shipped; moat proven LIVE (2026-07-09);
control plane a CF singleton. **Round:** R4 (final dual-critic — completeness + craft) of a 4–5x loop; both
critics DRY against code. See the Change log and Coverage summary.

## Legend

**Evidence badge** (one per feature card, in the H3 heading — the evidence grade):

| Badge | Meaning |
|---|---|
| 🟢 | **LIVE-proven** — armed and proven on the live path (dogfood/first-party), or a green test proving the exact behavior. |
| 🟡 | **built-not-proven** — code-complete + gate-green, but not yet proven on the live path (often DEFAULT-OFF, fail-closed until armed). |
| 🔵 | **owner-gated** — built, blocked on an owner action / product decision / cross-repo move / real volume. |
| ⚪ | **X4-external / oracle** — proof depends on an external actor (hugit (discontinued)/equivalent external dispatch, a live CF account) OR the code is verification-only machinery (red-team, conformance, X4 oracle), not a production code path. |
| ⚫ | **INERT / planned** — built-but-unwired into the live composition, or not built (planned / v0-simulated / stub). |

**Arm-state qualifier** (orthogonal; appears verbatim in each card's **Status** line):

| Tag | Meaning |
|---|---|
| **LIVE** | Armed and serving on the live path (dogfood/first-party). |
| **DEFAULT-OFF** | Present + fail-closed; inert until its env/secret is armed. |
| **OWNER-GATED** | Gated on an owner action, product decision, cross-repo move, or real volume (e.g. RAISE-N). |
| **INERT** | Built + tested but not wired into the live composition. |
| **PLANNED / v0-SIM** | Not built, or present as a documented simulation / stub / upgrade path. |
| **ORACLE / TEST** | Verification-only machinery (red-team, conformance, X4 oracle) — not a production code path. |

**Sources of truth (precedence order).** Code + config `file:line` (ultimate authority) · the frozen
wire contracts (`docs/spec/`, `conformance/`) · the ADRs (`docs/adr/`) · the canonical vision
(`docs/whitepaper/corelink-runners-v1.md`) · the deploy/ops docs. Where a doc and the code disagree,
the code wins and the doc is flagged. Compiled 2026-07-17.

**Live-deploy reality (read first).** The LIVE substrate is **Cloudflare-first** (ADR-0008): `fabricd`
runs as a CF Container + proxy Worker (`deploy/cloudflare-fabricd/`), runner/check boxes spawn on
`corelink-spawn-worker` (`deploy/cloudflare/`), and the moat (native check-host exec + per-job CAS-PAT
mint + attested cost) is **proven live** (2026-07-09). The current CF deploy is a **singleton**
(`FABRIC_NUM_SHARDS=1`, no `DATABASE_URL` ⇒ in-memory ledger). Northflank is the ADR-0008 **fallback**,
not the live substrate.

---

## Table of contents

- [Legend](#legend)
- [Summary matrix](#summary-matrix)
- [1. Core value proposition & pricing model](#1-core-value-proposition--pricing-model) — F-1.1 … F-1.6
- [2. Two front doors, one fabric](#2-two-front-doors-one-fabric) — F-2.1 … F-2.5
- [3. The frozen wire contract & conformance](#3-the-frozen-wire-contract--conformance) — F-3.1 … F-3.3
- [4. Execution core — `crates/corelink-runner`](#4-execution-core--cratescorelink-runner) — F-4.1 … F-4.10
- [5. Control plane — fabricd + `corelink-fabric`](#5-control-plane--fabricd--corelink-fabric) — F-5.1 … F-5.11
- [6. Compute substrate — the `Engine` seam](#6-compute-substrate--the-engine-seam) — F-6.1 … F-6.5
- [7. Cloudflare deploy surface](#7-cloudflare-deploy-surface--deploycloudflare) — F-7.1 … F-7.3
- [8. Identity, onboarding & the external-customer path](#8-identity-onboarding--the-external-customer-path) — F-8.1 … F-8.2
- [9. Client tools, front-door integrations & SDKs](#9-client-tools-front-door-integrations--sdks) — F-9.1 … F-9.5
- [10. Observability & ops](#10-observability--ops) — F-10.1 … F-10.5
- [11. Integration seams (family)](#11-integration-seams-family)
- [12. Roadmap status snapshot](#12-roadmap-status-snapshot)
- [13. Config-knob index](#13-config-knob-index)
- [14. Deliberate exclusions & ambiguities](#14-deliberate-exclusions--ambiguities)
- [Appendix A — Glossary](#appendix-a--glossary)
- [Appendix B — Reserved / unwired constants (not features)](#appendix-b--reserved--unwired-constants-not-features)
- [Appendix C — Feature ↔ story cross-reference matrix](#appendix-c--feature--story-cross-reference-matrix)
- [Appendix D — Change log](#appendix-d--change-log)
- [Appendix E — Coverage summary](#appendix-e--coverage-summary)

---

## Summary matrix

The whole doc at a glance: feature area → headline evidence → where the code is.

| # | Feature area | Headline | Where |
|---|---|---|---|
| 1 | Value proposition & pricing model | 🟢 principle LIVE; ceiling-enforcement 🟡 | `plans.rs`, `compute_meter.rs`, `pricing.md` |
| 2 | Two front doors (direct / hugit (discont.) / power-user / workspaces / agent-exec) | 🟢 dogfood LIVE; external 🔵; workspaces ⚫ | `dto.rs`, `interop.md`, ADR-0007 |
| 3 | Frozen wire contract & conformance | 🟢 LIVE (Rust tripwire); TS gap ⚪ | `corelink-runners-contracts/`, `conformance/` |
| 4 | Execution core (lease/isolate/boot/fence/X4/§13/attest) | 🟢 LIVE (182+ tests) | `crates/corelink-runner/` |
| 5 | Control plane (API/admission/ledger/attest/reaper/moat) | 🟢 LIVE singleton; pg + N>1 🔵; queue ⚫ | `corelink-fabric-server/`, `corelink-fabric/` |
| 6 | Compute substrate (`Engine` seam) | 🟢 Cloudflare LIVE; Northflank 🟡 fallback | `corelink-cloud-engine/`, `cloud_exec.rs` |
| 7 | Cloudflare deploy surface | 🟢 spawn + fabricd LIVE; canary 🟡 | `deploy/cloudflare*/` |
| 8 | Identity & external-customer path | 🟢 server-side; last step 🔵 | ADR-0002, ADR-0007 |
| 9 | Client tools / SDKs / integrations | 🟢 LIVE (binary-gated) | `corelink-cli/`, `sdk/`, `integrations/` |
| 10 | Observability & ops | 🟢 LIVE (obs-key gated) | `observability.rs`, `metrics.ts` |

---

## 1. Core value proposition & pricing model

CoreLink Runners is **ephemeral, cache-warm CI/build compute, billed by concurrency not by the minute.**
CoreLink expansion campaign #1 — the compute layer that turns CoreLink's content-addressed cache (the
moat) into compute revenue. **Margin thesis (context, not a code feature):** three levers all downstream
of the cache — warm ⇒ shorter jobs, memoized ⇒ jobs that never run, flat-for-concurrency ⇒ idle is margin
(whitepaper §6; `pricing.md §0`). Typical margin 85–95%, worst-case floor ~37–40%; real hit-rate is
**unmeasured** (`pricing.md §6`).

### F-1.1 — Concurrency pricing, never per-minute  🟢

**What** The customer buys N parallel runners flat/month; minutes are unlimited — the deliberate inversion
of GitHub Actions' per-minute model.
**Where** `docs/product/pricing.md §1`; whitepaper §3,§7.
**Status** 🟢 LIVE (pricing principle; the billing meter itself is DEFAULT-OFF — see F-5.6).
**Details** Load-bearing decided principle (CLAUDE.md). The billing SKU is concurrency; minutes never enter the meter.
**Exercised by** S6.1, S6.3, S1.3.1.
**Validated by** GAP — a pricing principle, not a code path; the concurrency cap it implies is validated at F-5.2.

### F-1.2 — Never charge for the customer's own compute twice  🟢

**What** Cache-warm boot + memoization mean a re-run of an already-computed state costs ~0 and is billed ~0.
**Where** whitepaper §2,§7; `docs/product/product.md §5`; mechanisms: cache-warm boot (F-4.3) + memoization (below).
**Status** 🟢 LIVE (principle; the concrete boot/memo mechanisms are LIVE at F-4.3 and here).
**Details** Two mechanisms carry it: (a) **Cache-warm by construction** — a runner boots with CAS/AC
pre-warmed, the job's inputs local before the first instruction (`boot/mod.rs`, F-4.3); (b) **Memoized
execution** — result content-addressed by `H(inputs ‖ command ‖ toolchain)`; if the AC has the key the
result is returned and the job never runs (`exec.rs::compute_memo_key`; the memo key was hugit's (discontinued), the
fabric serves misses).
**Exercised by** S1.2.1, S2.1.1, S1.2.5.
**Validated by** F-4.3 boot suite; memo-key integrity at F-5.4 (`conformance/CheckResult` formula).

### F-1.3 — The 5-tier flat concurrency ladder (SKUs)  🟢

**What** Starter $16 / Pro $40 / Team $100 / Scale $200 / Max $400 — concurrency caps 20/40/80/160/320,
vCPU-h ceilings 100/240/600/1,200/2,400. No free tier; 5-day trial.
**Where** `pricing.md §2`; `crates/corelink-fabric/src/plans.rs:76` (`PlanTier`, `plan_for`, `ceiling_for`).
**Status** 🟢 LIVE (the ladder is in code); prices are OWNER-tunable.
**Details** `plan_for` maps a tier to (concurrency cap, vCPU-h ceiling); `ceiling_for` returns the ceiling
in vCPU-h. Prices are config, not law.
**Exercised by** S1.3.1, S6.1, S6.3.
**Validated by** unit tests in `plans.rs` (tier → cap/ceiling); `corelink_plans` (entitlement resolve).

### F-1.4 — Loss-impossible hard-ceiling guarantee  🟡

**What** Two hard limits per tier — a concurrency cap **and** a hard active-compute ceiling (vCPU-h/mo) —
so the max COGS a user can incur (`ceiling × $0.10/vCPU-h`) is structurally below the price.
**Where** `pricing.md §3`; `compute_meter.rs` (`ceiling_vcpu_ms`, `MS_PER_VCPU_HOUR`).
**Status** 🟡 built-not-proven — ceiling **enforcement** is DEFAULT-OFF; armed by `FABRIC_RUNNER_VCPU>0`
+ tenant `max_vcpu_h`, requires pg (F-5.2, F-5.3).
**Details** The meter math (vCPU·ms → vCPU-h) is live; the *enforcing* gate (`ComputeGate`) is off until
armed. Failure mode when unarmed: no ceiling reservation, concurrency cap still binds.
**Exercised by** S6.2, S1.3.2, S5.3.2.
**Validated by** `compute_meter.rs` unit tests; enforcement path at F-5.2 (`ComputeGate`) — GAP: no live-armed proof.

### F-1.5 — Entitlement model (Runners axis)  🟡

**What** Per-tenant concurrency cap derived from a **separate Runners entitlement** (not the Cache tier)
via CoreLink introspect (`max_concurrency`). Ratified §B Option-B.
**Where** `corelink_plans.rs` (`CoreLinkPlanStore`); `conformance/corelink-introspect.json`.
**Status** 🟡 DEFAULT-OFF (corelink auth mode; static is default). Dogfood entitlement = 20.
**Details** See F-5.11 for the resolve logic (no-TTL caches, fail-closed `Unreachable`). `valid:true`
without a u32 cap ⇒ authenticated-but-uncapped ⇒ over-cap reject (not a 503).
**Exercised by** S1.1.1, S1.1.2, S6.1.
**Validated by** `corelink_plans`, `corelink_introspect_vector`, `corelink_admission_arms`.

### F-1.6 — Anti-abuse rails (sustained-pin + acquire-rate)  🟡

**What** Two within-ceiling abuse rails: (a) sustained-pin / mining detection on `CapGate` + slot metering;
(b) an acquire-*request* rate rail `rate_ceiling_per_min = max_concurrency × 10`.
**Where** `pricing.md §5`; `caps.rs` (`CapGate`, `RateWindow`); ROADMAP "Rate-ceiling tier formula".
**Status** 🟡 rate rail LIVE; the mining-detection *heuristic* is PLANNED.
**Details** The rate rail is a 60s sliding window, never binding in honest use. Mining detection (catching
slot-pinning that burns the ceiling on junk) is not yet a heuristic — the rate rail is the built floor.
**Exercised by** S5.3.3, S7.7.
**Validated by** `caps.rs` unit tests (rate window); GAP — no mining-heuristic (not built).

---

## 2. Two front doors, one fabric

Same lease/isolate/cap/teardown spine; distinct buyers and execution models (whitepaper §9; `interop.md §4`;
ADR-0007). Cards F-2.1..F-2.4 are the four front doors; F-2.5 is the agent-exec seam that hugit's (discontinued) real-cost
gate rides on.

### F-2.1 — Direct front door (ephemeral GitHub-Actions fleet)  🟢

**What** `runs-on: corelink[-<size>]` — the customer's **unmodified** workflow runs on a cache-warm microVM,
one ephemeral runner per job, producing the customer's own GitHub check status.
**Where** the GH runner agent (`config + run --ephemeral --jitconfig`); spawn at F-7.1; broker at F-5.8.
**Status** 🟢 LIVE for dogfood/first-party (App installation 150584374); the **external** customer path is
🔵 OWNER-GATED (F-8.1).
**Details** ICP-B (CI/platform teams). Box command = the GH runner agent; result = the customer's GitHub check.
**Exercised by** S1.1.1–S1.1.4, S1.2.1–S1.2.5, S1.4.1–S1.4.5.
**Validated by** `webhook-route.test.ts`, `github-app.test.ts`, dogfood fleet (App 150584374).

### F-2.2 — hugit (discontinued) front door (memoized, attested check)  🟢

**What** Memoized, attested *check* execution — hugit was the intended consumer (discontinued) and owned the memo
key, the fabric seeing only misses; `CheckDef → CheckResult` + `AttestationChain` + `result_binding_sig(_v2)`.
Under that (discontinued) model a hugit customer would never see a "Runners" line item (Runners as COGS under hugit).
**Where** fabric sets `sh -lc <check.command>` per `/exec` (F-5.1); attestation at F-5.4.
**Status** 🟢 LIVE (moat proven 2026-07-09); the hugit cutover was 🔵 OWNER-GATED (hugit discontinued; §11).
**Details** ICP-C (the anchor tenant). Result = `CheckResult` + attestation + result-binding signature.
**Exercised by** S2.1.1–S2.1.3, S2.2.1–S2.2.2, S2.4.1.
**Validated by** `acceptance_moat`, `cloudflare_flip_e2e`, `conformance_result_binding_v2`.

### F-2.3 — `corelink run` power-user primitive  🟢

**What** One attested command — the check path in CLI clothing (`--check '<cmd>'` → verified verdict).
**Where** `crates/corelink-cli/src/run.rs:114` (F-9.1).
**Status** 🟢 LIVE.
**Details** Acquire→exec→verify→close in one command; unpinned image → exit 2 before box contact, no lease leak.
**Exercised by** S8.1, S8.2.
**Validated by** `e2e_live_fabric.rs`; CLI `run`/`verify` gates.

### F-2.4 — Workspaces front door (M4 adjacency)  ⚫

**What** Agent sandboxes / dev boxes as Workspace SKUs on the same fabric (`clw snapshot/hydrate` state).
**Where** `ws/mod.rs` spine (F-4.8).
**Status** ⚫ PLANNED (campaign #2); the lifecycle spine is built (F-4.8), SKUs are not.
**Details** Box command = workspace manifest; result = a workspace object.
**Exercised by** S3.1, S3.2.
**Validated by** spine tests at F-4.8; GAP — no SKU path.

### F-2.5 — The agent-exec seam  🟡

**What** hugit's (discontinued) real-cost gate was to ride a distinct **agent-exec** seam: arbitrary-command drive on an
egress-allowed, exec-driven lease, never memoized, with async step polling.
**Where** `POST /v1/leases/{id}/agent-exec` + `GET .../{step_id}` (`agent_exec.rs`, F-5.1); egress lease
`from_agent_lease` (F-4.1).
**Status** 🟡 built-not-proven — wired both ends; real e2e when an external consumer dials it (hugit discontinued; ⚪ X4).
**Details** `req.agent` was a dead field until wired (2026-07-09, PR #338); `/exec` refuses agent specs.
**Exercised by** S2.3.1, S2.3.2, S4.1.
**Validated by** `acceptance_agent_exec`, `conformance_agent_exec_dtos`.

---

## 3. The frozen wire contract & conformance

The fabric wire/envelope contract carries the historical hugit framing (hugit discontinued); the fabric now owns
these mechanisms (`docs/spec/hugit-integration-contract.md` v1.4.0).
Types are **transcribed** on each side (hugit-contracts is never imported; `deny.toml` forbids git/path
deps). Drift is caught by shared byte-identical conformance vectors.

### F-3.1 — Wire types (`corelink-runners-contracts`)  🟢

**What** The transcribed §-frozen wire types the seam speaks.
**Where** `crates/corelink-runners-contracts/src/`.
**Status** 🟢 LIVE.
**Details** — each type · what it carries · where:

| Type | Carries | Where |
|---|---|---|
| `RunnerLease` / `RunnerState` | lease_id, principal_chain, path_set, expiry(epoch-ms), net_policy, tmp_root, state (`held`/`expired`/`crashed`/`released`) | `runner_lease.rs:33,16` |
| `FenceManifest` / `MaterializedEntry` | sparse-materialization claim: path_set, `deny_default(must=true)`, materialized[(path,digest)] | `fence_manifest.rs:35,19` |
| `CheckDef` | def_digest, command, inputs, toolchain_ref, env_manifest, glob_set (2nd memo axis) | `check_def.rs:16` |
| `CheckResult` / `Artifact` | frozen memo_key `lower_hex(SHA256(LP(tree)‖LP(def)‖LP(toolchain)))`, exit, artifacts, stdout/stderr_ref | `check_result.rs:28,13` |
| `AttestationChain` | tree/def/runner/model/principal + `sig` (frozen ed25519 pre-image) | `attestation_chain.rs:18` |
| `IntentMetrics` / `TokenCounts` / `ToolCount` | §13.1 per-job spend, `cost_usd_micros` integer micro-USD, schema `"1.2.0"` | `intent_metrics.rs:45,20,36,15` |
| `QueueApi` (`LandableEntry`, `UnionResult`, `MinimalFailingPair`, `BatchSeal`) | §9 landing-queue surface | `queue_api.rs:13,31,45,56,78` |

**Exercised by** S7.3, S2.2.1, S8.2.
**Validated by** `conformance_lease_dtos`, `acceptance_cf0_transcriptions`, `acceptance_s13_contracts`.

### F-3.2 — API DTOs & error vocabulary  🟢

**What** The `/v1` request/response DTOs (all `deny_unknown_fields`) and the fixed HTTP error vocabulary.
**Where** `crates/corelink-fabric-api/src/dto.rs`; `error.rs:15`; path constants in `paths.rs`.
**Status** 🟢 LIVE.
**Details** DTOs: `AcquireRequest`/`Response` (`:25,170`, additive `runner`/`agent`/`toolchain_digest`/
`repo_full_name`/`installation_id`) · `AgentSpec`/`RunnerSpec`/`RunnerTargetDto` (`:94,104,117`) ·
`ExecRequest`/`Response` (`:220,240`, `result_binding_sig`+`_v2`+`fabric_key_id`) · `AgentExecRequest`/`Ack`/
`Result` (`:298,325,341`) · `TriggerRequest`/`Response` (`:371,401`) · `CloseRequest`/`Response` (`:442,476`,
`metrics` REQUIRED, `intent_metrics_sig` additive) · `EnvelopeIngest` (`:144`, redacting Debug) ·
`KeyEntry`/`AttestationKeySetResponse`/`select_attestation_key` (`:551,581,632`, rotation-capable).
**Error vocabulary:** 400 `invalid` / 401 `unauthorized` / 404 `not_found` / 429 `over_cap` / 503
`fail_closed` — **no 403 by design** (cross-tenant = 404, no existence oracle).
**Exercised by** S7.4, S2.3.1, S1.4.5.
**Validated by** `acceptance_cf0_api_vocabulary`, `conformance_agent_exec_dtos`.

### F-3.3 — Conformance vectors (the drift tripwire)  🟢

**What** 17 byte-identical vectors + `manifest.sha256` — the shared drift tripwire across both repos.
**Where** `conformance/`; golden tests `corelink-runners-contracts/src/lib.rs:201,232,266`.
**Status** 🟢 LIVE (byte-checked in CI both repos). ⚪ **Gap:** the TS side of `cloudflare-spawn.json` is
not yet golden-tested (Rust-only tripwire — 2026-07-16 F4).
**Details** Membership derived from the on-disk set (not hardcoded). Golden tests recompute SHA-256, verify
manifest membership, prove tamper-rejection. Vectors: `RunnerLease`, `FenceManifest`, `IntentMetrics`,
`AcquireRequest/Response`, `CloseRequest/Response`, `AgentExecRequest/Ack/Result`, `attestation_key_set`,
`attestation_keyset_selection`, `result_binding_v2`, `intent_metrics_sig`, `cloudflare-spawn`,
`corelink-introspect`, `lease_shard`.
**Exercised by** S7.3, S7.4.
**Validated by** the `conformance/manifest.sha256` golden suite + `spawn-conformance.test.ts` (TS pending).

---

## 4. Execution core — `crates/corelink-runner`

The engine-agnostic execution primitives (lease → spec → spawn → isolate → exec → teardown). Docker-driven in
v0; Firecracker is the frozen upgrade path behind the `Engine` seam (`lib.rs:30-44`). **Lease states**
`Pending → Held → (Released | Expired | Crashed)` (`ledger.rs::LeaseState`); one lease = one isolated job, no
reuse of a dirty box.

### F-4.1 — Lease lifecycle  🟢

**What** The engine-agnostic per-job `ContainerSpec` derived from a frozen `RunnerLease`, and the three
posture-specific constructors that grant exactly the right isolation.
**Where** `crates/corelink-runner/src/lease.rs`.
**Status** 🟢 LIVE (the `SshBox` transport is the interim path M1 replaces).
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `ContainerSpec` | name/image/tmp_root/no_network/allow_egress/run_on_create/path_set/env from a frozen lease | `lease.rs:43-83` |
| Redacting Debug | every `env` value prints `***REDACTED***` (no JITCONFIG/ingest-token leak) | `lease.rs:89-107` |
| `from_lease` (check) | hermetic: `no_network=true`, no egress, no run-on-create | `lease.rs:120-143` |
| `from_runner_lease` | the **only** path setting `allow_egress=true`+`run_on_create=true`; requires `net_policy="egress-runner"` (ADR-0007 C2) | `lease.rs:158-180` |
| `from_agent_lease` | egress-allowed but exec-driven (`net_policy="egress-agent"`) | `lease.rs:198-222` |
| Image/tmp_root validation | X4 pin required; `tmp_root` guard `^/[A-Za-z0-9._/-]+$`, fail-closed | `lease.rs:225-271` |
| `BoxExec` seam | `run(argv)` + `run_with_stdin` (default refuses stdin) | `lease.rs:293-314` |
| `SshBox` transport (interim) | drives `ssh` to `hugit-runner-01`; TOFU host-key pin | `lease.rs:341-469` |

**Note (spoofing):** `allow_egress` is set only by these constructors, never inferred from the wire
`net_policy` string — so the lease-kind IS the isolation posture (the C2 invariant; consumed by F-6.3).
**Exercised by** S1.2.1–S1.2.5, S4.1, S4.2.
**Validated by** `acceptance_c2a`, `acceptance_runner_lease`.

### F-4.2 — Isolation & security  🟢

**What** The fail-closed isolation floor for untrusted compute, its forensic teardown, and the red-team
harness that keeps it honest.
**Where** `isolation.rs`, `teardown.rs`, `redteam.rs`.
**Status** 🟢 LIVE (the red-team harness is ⚪ ORACLE — box-gated, with a hermetic no-box proof).
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `Engine` seam | spawn/probe/exec/exec_captured/is_alive — the frozen substrate interface | `isolation.rs:103-128` |
| Fail-closed floor | `DockerEngine::spawn` refuses `no_network=false`; X4 verify-before-spawn ordering | `isolation.rs:145-205` |
| Untrusted hardening | `--network none`, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pids-limit 4096`, `--memory 12g` (=swap), `--tmpfs 64m` | `isolation.rs:169-197` (consts `:27,69,74`) |
| Isolation probe | tmp-privacy (tmpfs + host-leak) + net-isolation (no non-lo iface + outbound BLOCKED) | `isolation.rs:207-265` |
| Teardown + 4-surface re-scan | `docker rm -f` then scans containers/processes/mounts/network; `is_clean` needs all empty + zero scan-failures | `teardown.rs:106-176,23-53` |
| Fail-closed scan detection | `set -o pipefail`, no `2>/dev/null` swallow; ambiguous grep exit ≥2 = scan-failed | `teardown.rs:68-97,149-153` |
| Red-team escape harness | 6 live escape vectors on a real container (traversal, symlink, out-of-fence write, fork-bomb, disk-fill, fence-materialized-escape) | `redteam.rs:162-610` |
| Guard-the-guard | fence-materialized-escape goes RED under a no-op `classify` — proves the fence is the only control | `redteam.rs:567-610,872-893` |

**Knobs/defaults:** `PIDS_LIMIT=4096` (`:74`), `MEMORY_LIMIT="12g"` + `--memory-swap` pinned equal (`:69`),
`TMPFS_SIZE="64m"` (`:27`). `--read-only`/`--user`/`--cpus` are documented-inert.
**Isolation sign-off (ADR-0009):** CF Containers meet the Firecracker-class bar (each container = its own
microVM, one-per-job, destroyed after). Track-C hardening on the CF path is **PARTIAL**: deployed = image-pin,
revoke-on-complete, egress kill-switch, required exec-server auth, app-layer `ulimit`; **NOT closed** = G2
metadata/link-local egress (`deniedHosts` has no CIDR match + raw-socket bypass — §14). **Egress posture
(ADR-0003):** cross-tenant isolation is the hard guarantee; outbound internet egress is *accepted at launch*
on the managed tier, full lockdown is a BYOC upgrade.
**Exercised by** S4.2, S7.1, S7.2, S7.6.
**Validated by** `acceptance_redteam` (hermetic no-box proof `:809-865`); `acceptance_c5a`.

### F-4.3 — Cache-warm boot  ⚫

**What** The seam + drivers that boot a box with the CAS/AC pre-warmed — zero fetches when warm, force-fetch
when cold, fail-closed when the substrate is down.
**Where** `boot/mod.rs`, `cas_http.rs` (designed mechanism, unwired); the live pre-warm is a shell
one-liner in `deploy/runner/entrypoint.sh:95-98`.
**Status** ⚫ INERT / planned — the `boot/mod.rs` seam (`BootCas`/`BoxHydrate`) is built and unit-tested
but has **zero non-test call sites**: every caller is `crates/corelink-runner/tests/acceptance_c3.rs`
(`:318`, `:341`, `:360`). It is dead code in production. What actually runs live is a *different*,
much cruder mechanism: `deploy/runner/entrypoint.sh:95-98` shells out to the external `clw` binary as
a **best-effort, backgrounded, fail-open** pre-warm — gated on `command -v clw` (`:83`), and if `clw`
is missing it just proceeds fully cold with a stderr line: "cache-warm: clw not found in image —
proceeding COLD." (`:100`). `entrypoint.sh:83-84` itself calls this "a best-effort pre-warm." Namespace
public-routing is a separate, unrelated DEFAULT-OFF knob (`cas_http.rs:465-494`).
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `BootCas` seam | `is_cached`/`fetch_layer`/`write_layer`; 200=Hit / 404=None-miss / 5xx=SubstrateDown | `boot/mod.rs:155-186` |
| Warm `hydrate` | skips cached layers — zero fetches when warm (≤10s target) | `boot/mod.rs:237-275` |
| Cold `cold_hydrate` | force-fetch all layers (≥60s) — the fallback baseline | `boot/mod.rs:302-333` |
| Fail-closed substrate | `SubstrateDown` propagates; zero poisoned writes; 404-miss never written back; `ForcedCold` on AC write-back failure | `boot/mod.rs:56-108,191-219` |
| `BoxHydrate` driver | drives `clw hydrate [--cold] <keys>` over `BoxExec` — **test-only caller**, no production call site | `boot/mod.rs:347-407` (called only from `tests/acceptance_c3.rs:318,341,360`) |
| `CasHttpClient` | CAS/AC over HTTP `/v1/cas\|ac/{tenant}/{blake3}`, Bearer per-job PAT; tenant in path never a header | `cas_http.rs:249-370` |
| `Blake3Key` | canonical content-addressed key (byte-identity); SHA-256 is never a CAS key | `cas_http.rs:54-89` |
| 3-way CAS status guard | 2xx=Hit / 404=Miss(cold) / 401/403/5xx=FailClosed — never a silent miss | `cas_http.rs:99-129,218-236` |
| `HttpBootCas` | BootCas-over-HTTP; write key = BLAKE3(data); 404-on-PUT = fail-closed | `cas_http.rs:411-631` |
| Namespace routing (A13) | `_public:` cross-tenant keyspace routing, inert unless `with_public_routing` opts in | `cas_http.rs:465-494` — **DEFAULT-OFF** |
| **Live pre-warm (actual production path)** | `clw hydrate` backgrounded (`&`), gated on `command -v clw`, fail-open — proceeds COLD with only a stderr line if `clw` is absent | `deploy/runner/entrypoint.sh:83-98` |

**Exercised by** S1.2.1, S1.2.2, S1.2.5.
**Validated by** `acceptance_c3` (boot, test-only — not a production call site); `boot/mod.rs` +
`cas_http.rs` unit tests (3-way status, fail-closed).

### F-4.4 — Fence enforcement  🟢

**What** Sparse materialization IS the fence: only in-fence candidates are hydrated; everything else is
ENOENT. Plus the pure admission verdict and its box-backed proof.
**Where** `materialize/mod.rs`, `enforce/mod.rs`.
**Status** 🟢 LIVE.
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `materialize_sparse` | writes only in-fence candidates under workspace_root; out-of-fence → dropped → ENOENT | `materialize/mod.rs:166-190` |
| `validate_manifest` | requires `deny_default=true`; rejects allow-all / root-cover path_set | `materialize/mod.rs:111-123` |
| Traversal re-guard + base64 | defense-in-depth `path_escapes_root` re-check; SHA-256 `content_digest` (frozen) | `materialize/mod.rs:197-239,92-96` |
| `classify` (pure verdict) | directory-prefix (segment-wise) vs exact-file; absolute / `..` = Outside; covers `srcfoo`-vs-`src/` collision | `enforce/mod.rs:91-121` |
| `is_admitted` / `check_access` | the single fail-closed admission predicate materialize routes through | `enforce/mod.rs:137-160` |
| `probe_outside_enoent` | box-backed ENOENT proof (exit-code probe, locale-independent) | `enforce/mod.rs:194-229` |

**Exercised by** S4.2, S7.1.
**Validated by** `acceptance_c2b`; the red-team fence-materialized-escape vector (F-4.2).

### F-4.5 — Supply chain — verify-before-spawn  🟢

**What** No tenant byte is processed until the image digest is pinned, pulled, and cross-checked.
**Where** `pin.rs`, `x4/`.
**Status** 🟢 LIVE (the X4 oracle is ⚪ ORACLE; live enforcement is in `pin.rs`).
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `PinnedImageRef::parse` | accepts only `repo@sha256:<64-lc-hex>`; rejects tags/bare/uppercase/non-hex/shell-metachar | `pin.rs:103-145` |
| `verify_on_box` | `docker pull` + `RepoDigests` cross-check before spawn; permanent/transient classifier; retry 4 attempts | `pin.rs:195-272,64-80` (`PULL_MAX_ATTEMPTS=4` `:34`) |
| `digest_lock` | per-digest mutex serializes concurrent same-digest pulls | `pin.rs:42-53` |
| `require_pinned` chokepoint | single call every spec-build uses | `pin.rs:282-284` |
| X4 supply-chain oracle | 3 invariants (pinned+verified image, pinned deps, tampered→fail-closed-before-tenant-work); `VerifiedSpawn`/`GuardedSpawn` prove no tenant byte on rejection | `x4/mod.rs:9-26`, `x4/pin.rs:158-230` |

**Exercised by** S7.5.
**Validated by** `acceptance_x4`, `hermetic_supply_chain`.

### F-4.6 — Concurrency / expiry / recovery  🟢

**What** Scheduler fan-out with peak-concurrency sampling; deterministic expiry hard-kill; crash recovery that
never silent-greens a lost job.
**Where** `concurrency/mod.rs`, `expiry/mod.rs`, `recovery/mod.rs`.
**Status** 🟢 LIVE.
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `run_batch` fan-out | spawns N containers, samples peak concurrency (target ≥8), injective naming avoids undercount | `concurrency/mod.rs:169-321,56-86` |
| No-swallow teardown | a failed per-box teardown is recorded (leaked box visible); rest of batch still reclaimed | `concurrency/mod.rs:121-162` |
| Expiry hard-kill | deterministic `is_expired` (epoch-ms; `u64::MAX`=never); `hard_kill`=SIGKILL then forensic teardown | `expiry/mod.rs:42-107` |
| Crash recovery | `probe_liveness` (Alive/Lost); `recover_lost` marks `Crashed`+cleanup+records `LostJob` (requeued/surfaced) | `recovery/mod.rs:88-158` |

**Exercised by** S1.2.3, S1.4.1, S1.4.4.
**Validated by** `acceptance_c9` (concurrency), `acceptance_e4` (expiry/recovery).

### F-4.7 — Actions-YAML shim  🟢 (execution v0-SIM)

**What** A hand-rolled minimal Actions-workflow shim that runs the published subset fail-closed and never
fakes a green.
**Where** `shim/`.
**Status** 🟢 LIVE parser/gates; **execution is v0-SIMULATED** (`executor.rs:321-345`); the live-GH
equivalence lane returns `Partial` (never fake-green), gated on `HUGIT_GH_TEST_REPO`.
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `parse_workflow` | no YAML dep; requires `on:`/`jobs:`; unsupported keys → `OutOfContractReport`, zero silent skips | `shim/parser.rs:105-135,181-477` |
| `ShimExecutor::execute` | per-step outcomes; break on Failure/SecretDenied/OutOfContract | `shim/executor.rs:148-207` |
| Secrets fail-CLOSED | `${{ secrets.X }}` via broker; denied/unavailable → step refused | `shim/executor.rs:248-319` |
| `if:` fail-closed | only `always()`/`true` run; unrecognized condition SKIPS | `shim/executor.rs:217-228` |
| Determinism precondition | flags multi-job, floating action refs, `github.*` context → NotSatisfied | `shim/executor.rs:368-415` |
| Equivalence harness | shim lane runs; live-GH lane returns `Partial` | `shim/executor.rs:444-479` |
| Secrets broker trait | opaque injection tokens only; `NullBroker` default-deny, `StubBroker` test | `shim/broker.rs:91-176` |
| Published subset contract | 15 `SubsetFeature`s + `is_supported`; explicit diagnostics | `shim/subset.rs:101-123`, `report.rs:32-117` |

**Exercised by** S8.3.
**Validated by** `shim/` unit tests (parser/subset/executor); GAP — no live-GH equivalence run (Partial by design).

### F-4.8 — Workspace lifecycle spine (Workspaces, campaign #2)  🟢 spine / ⚫ SKUs

**What** The `ws/mod.rs` lifecycle spine on which Workspace SKUs will run.
**Where** `ws/mod.rs:42-913`.
**Status** 🟢 LIVE (spine); ⚫ Workspaces SKUs PLANNED.
**Details** `spawn_workspace` (<1s warm contract) · `DedupSpawner` (coalesce concurrent identical spawns to
one materialization; lock never held across `docker run`) · identity binding fail-closed (slot keyed by the
lease's container name, cross-lease share refused) · liveness-probe before reuse · bounded slot map + reaping
(teardown OUTSIDE the lock) · `attach`/`resume` (resume cannot widen the fence) · `run_remote`≡`run_local`
identity. *(`ws` is workspace lifecycle, NOT a websocket module.)*
**Exercised by** S3.1, S3.2.
**Validated by** `ws/mod.rs` unit tests (dedup, identity binding, resume fence); GAP — no SKU e2e.

### F-4.9 — §13 envelope — agent-execution metrics  🟢

**What** The contract §13 mechanism: metrics emission + capture-hook surfaces + no-persistence (in-process;
M1 adds transport + PAT).
**Where** `envelope/`.
**Status** 🟢 LIVE.
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `MetricsCollector` | single-writer accumulator; saturating on untrusted usage; derived `tokens.total`; exact-integer `cost_usd_micros`; exactly-once finalize | `envelope/collector.rs:65-231,149-155` |
| Tool-breakdown DoS bound | `MAX_DISTINCT_TOOLS=256`, `MAX_TOOL_NAME_LEN=128`, `<overflow>` bucket; Σ breakdown == tool_calls | `collector.rs:23-29,109-127` |
| Non-destructive `snapshot`/`project` | turn-boundary checkpoint without tripping finalize (ADR-0004 Phase 2b) | `collector.rs:174-231` |
| `CaptureHook` (2 surfaces) | raw-event + per-turn `TurnMeta` bounded in-memory queues; bearer-gated subscribe; per-surface overflow flags; bytes forwarded byte-identical (redaction is forge-side, §13.3) | `envelope/hook.rs:167-381` |
| Drain (in-flight-only) | `next_event`/`next_meta` pop-front, released after forwarding — no durable persistence | `envelope/hook.rs:394-409` |
| JobClose ack state machine | finalize → publish CloseSignal → bearer-gated ack window → fail-closed CloseOutcome; residue/overflow ⇒ `capture_incomplete` | `envelope/close.rs:126-185`; default `ack_timeout`=30s + `buffer_capacity`=256 set at `handlers/leases.rs:1182-1183`, consumed at `close.rs:155` |
| `close_abnormal` (§13.5) | expiry/crash: partial flush, `capture_incomplete=true`, `close_reason` on the wrapper (never inside frozen IntentMetrics) | `envelope/close.rs:203-240,44-78` |

**Exercised by** S4.3, S4.4, S2.3.2.
**Validated by** `acceptance_s13`, `acceptance_envelope_e2e`, `envelope_wire`.

### F-4.10 — Attestation signing  🟢

**What** `FabricSigner` (ed25519, one fabric key/region) signing the frozen attestation pre-image.
**Where** `attest/mod.rs:43-132`.
**Status** 🟢 LIVE.
**Details** `sign_chain` over the FROZEN pre-image `LP(tree)‖LP(def)‖LP(runner)‖LP(model)‖VEC(principal)`;
`verify_chain`/`verify_raw` use `verify_strict` (malleability-rejecting); `key_id` = SHA-256(pubkey)[..8].
**Exercised by** S1.5.1, S7.3, S8.2.
**Validated by** `acceptance_att`; `conformance_attestation_key_set`, `conformance_attestation_keyset_selection`.

---

## 5. Control plane — fabricd + `corelink-fabric`

The multi-tenant server behind the frozen `/v1` API. Composition root `server.rs::build_app_and_state`
(`:999`) selects auth store, ledger, cloud backend, mint, cred-signer, admin, autoscaler — all default-off /
fail-closed.

### F-5.1 — RunnerLease HTTP API  🟢

**What** The `/v1` route surface (`app.rs::build_router`) plus the two conditionally-mounted server routes.
**Where** `app.rs` (routes `:1856-2017`); handlers in `handlers/`.
**Status** 🟢 LIVE (per-route arm-state below).
**Details** — route · handler · where · status:

| Route | Handler | Where | Status |
|---|---|---|---|
| `POST /v1/leases` (acquire) | shard-guard → plan → runner allowlist → rate → atomic `try_admit` → finalize/enqueue | `app.rs:1915`, `leases.rs:257` | 🟢 |
| `GET /v1/leases` (list) | tenant-scoped; scatter-gather at N>1 | `app.rs:1914`, `lease_list.rs` | 🟢 |
| `GET /v1/leases/{id}` | mirrors ledger exactly, no invented states | `app.rs:1916` | 🟢 |
| `POST /v1/leases/{id}/cancel` | release + forensic teardown, idempotent | `app.rs:1917` | 🟢 |
| `POST /v1/leases/{id}/exec` | CheckDef→CheckResult, gate order scope/held/expired/exec/attest | `app.rs:1921`, `exec_handler.rs` | 🟢 |
| `POST /v1/leases/{id}/agent-exec` + `GET .../{step_id}` | egress arbitrary-command drive, never memoized, async step-store | `app.rs:1925,1929`, `agent_exec.rs` | 🟢 |
| `POST /v1/queue/trigger` | §9 landing-queue trigger, attested, at-least-once dedup (cap 4096) | `app.rs:1933`, `queue.rs:64` | 🟢 |
| `POST /v1/leases/{id}/close` | §13 close: teardown-first → ack window → Held→Released → atomic metrics+result | `app.rs:1934`, `close.rs` | 🟢 |
| `GET /v1/leases/{id}/envelope/events\|meta` | §13.2 drain (tenant PAT) | `app.rs:1939,1940` | 🟢 |
| `POST /v1/leases/{id}/envelope/ingest` | §13.2 write, per-lease scoped-token (outside PAT layer) | `app.rs:1891` | 🟢 |
| `POST /v1/leases/{id}/cas-cred` | C2c cred-ticket redeem → per-job CAS PAT (single-use) | `app.rs:1896`, `cas_cred.rs` | DEFAULT-OFF |
| `GET /v1/usage` + `/v1/usage/history` | tenant-facing live usage (cap·active·peak) + history | `app.rs:1910,1913` | 🟢 |
| `GET /v1/metrics/tenant` | §6 per-tenant wait histogram | `app.rs:1911` | 🟢 (count 0 in reject mode) |
| `GET /v1/attestation/key` (unauth) | published keyset (rotation-capable) | `app.rs:1856` | 🟢 |
| `GET /v1/health`, `/`, `/health` (unauth) | liveness, outside the load-shed limiter | `app.rs:2008-2017` | 🟢 |
| `GET /internal/v1/occupancy`, `/internal/v1/status` | ops surfaces, obs-key gated (404 unset) | `app.rs:1864,1867` | DEFAULT-OFF |
| `POST /internal/v1/admin/tenants` (+ `/suspend`\|`/unsuspend`) | onboarding + tenant suspend, admin-key gated | `server.rs:1360`, `app.rs:1872,1876` | DEFAULT-OFF |
| `POST /webhooks/github` | Stage-B autoscaler, HMAC-authed | `server.rs:1418`, `webhook.rs` | DEFAULT-OFF |

**Load-shed layer:** `GlobalConcurrencyLimit` + `LoadShed` → 503 on work routes (health/key excluded);
defaults `MAX_INFLIGHT_REQUESTS=1024` (`app.rs:651`), `CLOSE_ACK_MAX_INFLIGHT=256` (`:633`),
`PROVISION_MAX_INFLIGHT=16` (`:645`); the load-shed cap == the 1024 in-flight ceiling (no separate constant).
**Reserved:** `ADMIN_TENANT_BY_ID` (`paths.rs:128`, `/internal/v1/admin/tenants/{id}`) is defined but
**unwired** — see Appendix B.
**Exercised by** S1.2.1–S1.2.5, S2.1.2, S2.3.1, S5.2.3.
**Validated by** `acceptance_api1`–`acceptance_api4`, `acceptance_runner_lease`, `mock_e2e`, `status_api`, `usage_api`, `occupancy_api`.

### F-5.2 — Admission, caps, fairness, compute ceiling  🟢 reject / 🟡 queue

**What** The admission spine: atomic reject-mode `try_admit`, the opt-in queued fair scheduler, per-tenant
caps, the fleet gate, and the vCPU-h compute ceiling.
**Where** `ledger.rs`, `leases.rs`, `admission.rs`, `scheduler.rs`, `caps.rs`, `global_gate.rs`, `compute_meter.rs`.
**Status** 🟢 reject mode is the LIVE default; 🟡 queue mode DEFAULT-OFF; compute ceiling DEFAULT-OFF; fleet
gate ⚫ INERT.
**Details** — mechanism · what · where · status:

| Mechanism | What | Where | Status |
|---|---|---|---|
| Atomic `try_admit` (reject) | reserve-before-provision under the ledger lock; immediate over-cap 429 | `ledger.rs:178`, `leases.rs:494` | 🟢 |
| Queued fair admission (ADR-0005) | `FABRIC_ADMISSION_MODE=queue`: over-cap enqueue → fair dispatch → authoritative `try_admit` inside `dispatch`; bounded async wait → 503; per-tenant park-cap wedge | `admission.rs:64,421,757`; `scheduler.rs` | 🟡 DEFAULT-OFF |
| Fair-scheduler algorithm | **owed-first cursor-rotation round-robin** (NOT classic deficit-RR): each tick dispatches a budget of `TICK_SLOTS`; an owed-skip set (tenants cap-skipped last tick, sorted) is served first, then the rest cursor-rotated to resume strictly after the last dispatched tenant; a cap-check fail parks the tenant into `owed_skip` and does NOT consume the turn; cursor advances only on a real dispatch; p95 wait = nearest-rank over a bounded ring | `scheduler.rs:223-293,319-350,306` | 🟡 |
| Durable cross-instance queue | pg-backed **deficit-ordered** fair queue: `enqueue` stamps the row's `deficit` from `pg_admission_deficit`; `dequeue_head` = `ORDER BY deficit, enqueued_at_ms FOR UPDATE SKIP LOCKED`; deficit `+1` only on a real admit win | `admission.rs:301`, `pg_queue.rs:203-380,392` | ⚫ INERT (unwired) |
| Per-tenant concurrency + rate caps | `CapGate::check` + `RateWindow` (60s sliding); preventive (before load), fail-closed | `caps.rs` | 🟢 |
| Global fleet gate | `GlobalGate::try_admit_global` — checks per-tenant share first (`TenantShare` reject) then the wall (`GlobalWall`); RAII decrement on drop; poisoned lock fails closed | `global_gate.rs:172-230` | ⚫ **INERT — built-only, zero call sites in the server composition (unit-tested only)** |
| Compute (vCPU-h) ceiling | `ComputeGate` reserves vCPU·ms per lease (`try_admit_with_compute`); ceiling from tier; boot guards require pg + non-zero ceiling | `ledger.rs:194`, `compute_meter.rs`, `server.rs:788-849`, `leases.rs:170` | 🟡 DEFAULT-OFF (`FABRIC_RUNNER_VCPU>0` + pg) |
| TTL clamp | acquire TTL clamped to 60 min (`MAX_EXPIRY_MS=3_600_000`) — bounds slot hold + vCPU·ms reservation | `leases.rs:146` | 🟢 |
| Tenant suspend (Track-C AUP1) | block acquires + kill live leases; durable cross-instance (`fabric_suspended_tenants`) | `enforcement.rs`, `pg_ledger.rs:483-505` | 🟡 DEFAULT-OFF (admin-key) |
| Plan downgrade grace | grace window on a plan downgrade | `downgrade_grace.rs` | 🟢 |

**Tick knobs/defaults:** `TICK_SLOTS=64` (`FABRIC_ADMISSION_TICK_SLOTS`), `TICK_MS=50`
(`FABRIC_ADMISSION_TICK_MS`), `PARK_CAP=8` (`FABRIC_ADMISSION_PARK_CAP`, the P1 cross-tenant load-shed bound)
— `admission.rs:85-117`.
**Exercised by** S1.3.1, S1.3.2, S5.2.3, S2.4.1.
**Validated by** `acceptance_infra_capacity`, `load_shedding`, `corelink_admission_arms`.

### F-5.3 — Ledger & durable state  🟢 InMemory / 🔵 pg

**What** The `LeaseLedger` trait and its three interchangeable implementations, plus the durable reap/suspend
state.
**Where** `corelink-fabric` (`ledger.rs:226`, `pg_ledger.rs`).
**Status** 🟢 InMemory LIVE (default); 🔵 pg BUILT + OWNER-GATED (needs `DATABASE_URL`).
**Details** Three impls behind `FABRIC_LEDGER_BACKEND` (**never a silent fallback**, `server.rs:571`):
**`InMemoryLedger`** (live default) · **`FileLedger`** (fsync + torn-journal tolerance) · **`PgLedger`** —
cross-instance cap-safety via an explicit txn that runs `SELECT pg_advisory_xact_lock(hashtext(tenant))` as
its first statement, then `INSERT ... SELECT ... WHERE (SELECT count(*) ... state IN ('pending','held')) <
cap RETURNING` (racing instances block on the lock, the loser sees the winner's row → inserts nothing → over-cap
`Ok(false)`; PK conflict → fail-closed `Err`) (`pg_ledger.rs:994-1071`). Durable reap state (ADR-0004):
`deadline_ms` (nullable bigint, self-authoritative, never mutated by transitions, `pg_ledger.rs:149,639`) +
`envelope_checkpoint` (text, `set/get_envelope_checkpoint` `:821,846` — the reaper's tier-2 source). Durable
suspension via `fabric_suspended_tenants(tenant_id PK)` (`set_tenant_suspended` `:483-505`). Opt-in pg TLS
(`FABRIC_PG_TLS`).
**Exercised by** S5.2.2.
**Validated by** the ledger conformance suite (InMemory/File/Pg parity); `corelink_admission_arms`.

### F-5.4 — Attestation / result-binding / attested cost  🟢

**What** Every execution result is attested and signed; the close path rejects any result whose memo_key
doesn't recompute; attested §13 cost rides `CloseResponse`.
**Where** `attestation.rs`; DTOs at F-3.2.
**Status** 🟢 LIVE (`intent_metrics_sig` armed on fabricd; external consumption pending (hugit discontinued), ⚪ X4).
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `build_attestation` | frozen `AttestationChain` per execution; a result without attestation is unrepresentable | `attestation.rs` |
| `result_binding_sig` (v1) | detached ed25519 over `LP(memo_key)‖LP(stdout_ref)‖LP(stderr_ref)` | `attestation.rs` |
| `result_binding_sig_v2` | full-outcome binding — adds `i32_be(exit)‖u32_be(artifacts.len)‖∀ LP(path)‖LP(digest)`; closes the forgeable-verdict gap; additive alongside v1 | `attestation.rs`; vector `result_binding_v2.json` |
| memo-key integrity check | close rejects (400) any `CheckResult` whose `memo_key` ≠ SHA-256 of its input axes before attesting | close gate 4 |
| `intent_metrics_sig` (FLIP-B) | signs §13 metrics on `CloseResponse` so a consumer renders ATTESTED cost (hugit was the intended consumer, discontinued); signs honest-zero until a provider `/usage` source | `sign_intent_metrics`, dto `:541`; vector `intent_metrics_sig.json` |
| Keyset rotation | `AttestationKeySetResponse` + fail-closed `select_attestation_key` (UnknownKeyId/Expired); prod key `faa5b7726…` | dto `:581,632`; vector `attestation_keyset_selection.json` |
| Per-lease scoped ingest token (ADR-0006) | HMAC-SHA256(ingest_secret, lease_id) write-only capability injected in place of the PAT — an exfiltrated token authorizes ingest only to that one dying lease | `ingest_token.rs`; `server.rs:1017` |

**Exercised by** S2.2.1, S7.3, S8.2, S8.3.
**Validated by** `acceptance_att`, `conformance_result_binding_v2`, `conformance_intent_metrics_sig`.

### F-5.5 — Reaper & lifecycle  🟢

**What** The always-on reaper: teardown-first reap, stale-pending sweep, opt-in crash surface, and the §13.5
3-tier abnormal envelope flush.
**Where** `reaper.rs`, `lifecycle.rs`.
**Status** 🟢 reaper LIVE; crash-sweep DEFAULT-OFF (armed to `20s` on live fabricd).
**Details** `reap_once` (teardown-first, deadline from ledger — cross-instance backstop) · `sweep_stale_pending`
(reclaim leaked Pending slots, `FABRIC_PENDING_MAX_AGE_SECS` default 300, `reaper.rs:826`) · `surface_crashes`
(opt-in, `FABRIC_CRASH_PROBE_INTERVAL_SECS`, no default) · `BoxRegistry` orphan GC (unbind on expiry) ·
`leases_expired`/`leases_crashed` golden counters. Reap loop interval `FABRIC_REAP_INTERVAL_SECS` default 30s
(`reaper.rs:394`). **The 3-tier `flush_partial_envelope`** (`reaper.rs:191-313`): **Tier 1 (`local-hook`)**
runs the live capture hook's `close_abnormal` through the normal finalize/redaction path (`capture_incomplete=
true`); an already-closed latch returns without falling through. **Tier 2 (`durable-checkpoint`)** — no hook —
reads `get_envelope_checkpoint`, deserializes to `IntentMetrics`, emits a partial forensic envelope; a corrupt
blob falls to tier 3. **Tier 3 (`no-capture`)** — no hook and no usable checkpoint — emits an explicit
`no_capture=true` + `capture_incomplete=true` marker from `zero_intent_metrics()` (never silently dropped).
**Exercised by** S1.2.4, S1.4.1, S1.4.4.
**Validated by** `reaper.rs` unit tests (3-tier flush, stale-pending); `acceptance_e4`.

### F-5.6 — Billing / metering  🟡

**What** Slot-occupancy metering and the durable/push exporters that turn it into billing events.
**Where** `billing.rs`, `billing_sink.rs`, `corelink_billing.rs`.
**Status** 🟡 raw occupancy LIVE; the exporters + CoreLink push are DEFAULT-OFF; GDPR erasure PLANNED.
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `SlotMeter` occupancy | records held-lease occupancy (occupied/peak/journal, bounded `JOURNAL_CAP` with `journal_dropped`); raw occupancy only, no minutes/cost math | `billing.rs:58` |
| Durable billing exporter | `PgBillingSink` + `spawn_export_loop` drain the journal into `billing_events` (exactly-once by PK + `ON CONFLICT DO NOTHING`) | `billing_sink.rs`, `billing_export.rs`; `FABRIC_BILLING_EXPORT_INTERVAL_SECS` (opt-in, no default, needs pg) |
| CoreLink usage push | `runner_slot_seconds` to corelink-billing (`CorelinkBillingTarget`, `flush_now`, push loop default 30s `corelink_billing.rs:349`) | `corelink_billing.rs:58`; `BILLING_INGEST_URL/AUTH_KEY/REGION` |
| GDPR Art.17 erasure | tenant-prefix-bounded `DELETE FROM billing_events WHERE tenant=$1`; PK-bounded, fail-closed, idempotent | `docs/privacy/gdpr-erasure-billing-events.md`; `billing_sink.rs` DDL — **PLANNED** |

**Exercised by** S5.3.1, S5.3.2, S5.3.3, S6.1.
**Validated by** `slot_emission`; GAP — no armed exporter/push proof (both DEFAULT-OFF).

### F-5.7 — Multi-instance / shard routing  🔵

**What** The frozen cross-language shard contract that lets fabricd scale past a singleton.
**Where** `shard.rs`; `deploy/cloudflare-fabricd/src/shard.ts`; vector `lease_shard.json`.
**Status** 🔵 BUILT + INERT at N=1; RAISE-N is OWNER-GATED (needs `DATABASE_URL` + `FABRIC_NUM_SHARDS` +
`max_instances` raised in lockstep on real volume).
**Details** `fnv1a_32` / `shard_of` / `mint_lease_id_for_shard`. `FABRIC_NUM_SHARDS` learned at boot; an N>1
acquire on a non-pg ledger is **REFUSED fail-closed** (cap-guard). All N>1 gaps closed (cap-safety, rate÷N,
durable suspend, Worker shard-routing).
**Exercised by** S5.2.2.
**Validated by** `shard.test.ts`, `n-gt-1-routing.test.ts`, the `lease_shard.json` golden vector.

### F-5.8 — Runner-registration broker & Stage-B autoscaler (ADR-0007)  🟡

**What** The JIT `--ephemeral` runner-registration broker and the webhook autoscaler that drives it.
**Where** `runner_broker.rs`, `webhook.rs`, `runner_inject.rs`.
**Status** 🟡 DEFAULT-OFF per path (mounts only when its secret is present).
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| `RunnerRegistrationBroker` | mints a short-lived JIT `--ephemeral` config per runner-lease; App private key never on the box | `runner_broker.rs:851` |
| `GitHubAppBroker` vs `PatBroker` | App-JWT (App-ID/installation/private-key via `RingRsaJwtSigner`) for customer repos, or static `FABRIC_GITHUB_MINT_TOKEN` for first-party; `MockBroker` for tests | `runner_broker.rs` (`runner_broker_from_env`) |
| Stage-B autoscaler webhook | `POST /webhooks/github` `workflow_job` → 1 runner lease/queued job, cancel on completion; HMAC-authed, drives the audited acquire/cancel path (no admission bypass); bounded job-tracking + delivery-dedup | `webhook.rs:784` (mounts only with `FABRIC_AUTOSCALER_WEBHOOK_SECRET`) |
| Runner repo-allowlist authz (Track-C C1) | bounds `runner:` acquires to allowlisted repos, fail-closed 400 | `leases.rs:440`; `FABRIC_RUNNER_REPO_ALLOWLIST` |
| env-0 injection | `inject_runner_jitconfig`, `inject_clw_env`, `inject_cred_ticket_env`, `inject_ingest_env` | `runner_inject.rs`, `envelope_inject.rs:57` |

**Exercised by** S1.1.1–S1.1.4, S1.4.3, S1.4.5, S5.1.1.
**Validated by** `webhook-route.test.ts`, `github-app.test.ts`; broker unit tests (`MockBroker`).

### F-5.9 — The moat mint & env-0 cred-ticket  🟢

**What** Per-job, tenant-scoped CAS-PAT minting and the PAT-never-on-box cred-ticket redemption.
**Where** `runner_cas_mint.rs`, `cred_ticket.rs`, `cas_cred.rs`.
**Status** 🟢 mint armed FLIP-A + proven live (2026-07-09); cred-ticket DEFAULT-OFF, proven live both sides.
**Details** — mechanism · what · where:

| Mechanism | What | Where |
|---|---|---|
| Per-job CAS-PAT mint (D-9) | `CasPatMint`/`HttpCasPatMint` mints + revokes a per-job `cas:rw` PAT via CoreLink; `MockMint` for tests; weak-secret refusal | `runner_cas_mint.rs:590`; `CORELINK_RUNNER_MINT_AUTH_KEY/URL` |
| C2c cred-ticket (PAT-never-on-box) | a single-use lease-scoped ticket injected instead of the raw PAT; redeemed at trusted boot at `{FABRIC_PUBLIC_BASE_URL}/v1/leases/{id}/cas-cred`; `CredTicketSigner` + `StashedCred` | `cred_ticket.rs`; `cas_cred.rs`; `FABRIC_CRED_TICKET_SECRET` |
| Mint-arm boot guard | `validate_mint_arm` fails boot closed if the mint is armed without cred-signer + `CLW_ENDPOINT` + `FABRIC_PUBLIC_BASE_URL` — a healthy boot proves redemption is wired | `server.rs:965` |

**Exercised by** S1.2.1, S1.2.4, S7.2.
**Validated by** `acceptance_moat`, `cloudflare_flip_e2e`, `corelink_flip_e2e`, `cred-cred-route.test.ts`, `cred-stash-do.test.ts`.

### F-5.10 — Multi-size ladder resolver  ⚫

**What** Derives a box size from `corelink-<size>` labels.
**Where** `size.rs`.
**Status** ⚫ INERT (unwired); OWNER-GATED on the size taxonomy input (rungs + vCPU + CF `instance_type` +
$/slot-second per rung, ratified with server-TL 2026-07-10).
**Details** `SizeSpec`/`SizeRegistry`/`resolve_from_labels`. The single-rung registry resolves every acquire to
the default size (byte-identical to single-size today).
**Exercised by** S1.3.3.
**Validated by** `size.rs` unit tests (label resolve); GAP — no multi-rung path.

### F-5.11 — CoreLink integration seams (auth / plans / clw)  🟡

**What** The seams that consume CoreLink identity, entitlement, and cache-warm.
**Where** `corelink_auth.rs`, `corelink_plans.rs`, `clw_drive.rs`, `ac_pre_lease.rs`.
**Status** 🟡 corelink auth/plan DEFAULT-OFF (static is default; armed live on fabricd); clw/ac-pre-lease ⚫ INERT stubs.
**Details** **`CoreLinkTokenStore`/`UreqIntrospect`** — per-request PAT→tenant via frozen `POST
/internal/v1/auth/introspect` (`X-Corelink-Internal-Auth`; only `200 valid:true` admits, 401/5xx→503, never a
false 401). **`CoreLinkPlanStore`** (`corelink_plans.rs:240-402`) — `plan_of_resolving` POSTs `{"token":pat}`
with bounded retry on TRANSIENT failure only (transport/503 retried; 401 short-circuits `Err(Unreachable)`);
`parse_plan_200` maps `valid:true`+u32 `max_concurrency` → `TenantPlan` (rate = body or `max_concurrency*10`,
empty repo_allowlist), `valid:true` w/o u32 → `None` (over-cap reject, not 503), `valid:false` → `None`+evict,
unparseable-200 → `Err(Unreachable)` (fail-closed, never 0-slot a live tenant). Two per-tenant caches (`plans`,
`ceilings`) with **NO TTL**, read token-free by `plan_of`/`tenant_ceiling_vcpu_ms`. **`clw_drive.rs`** — A8
exit-transparency seam (`CLW_INTERNAL_EXIT_CODE=125` = non-zero-not-cached; INERT stub). **`ac_pre_lease.rs`** —
AC pre-lease cache-hit hook (INERT stub, moat WP-7).
**Exercised by** S1.1.1, S1.1.2, S5.1.1.
**Validated by** `corelink_auth`, `corelink_plans`, `corelink_introspect_vector`, `corelink_admission_arms`.

---

## 6. Compute substrate — the `Engine` seam

Two backends behind the frozen `Engine` trait; the composition root selects 4-way (`server.rs:1204`,
`cloud_exec.rs`).

### F-6.1 — CloudflareEngine (default, moat)  🟢

**What** The HTTP client to the spawn-Worker — R2-co-located, in-network cache hydration (ADR-0008).
**Where** `crates/corelink-cloud-engine/src/cloudflare.rs:330,494,591`.
**Status** 🟢 LIVE (armed `CLOUDFLARE_SPAWN_*`).
**Details** `/v1/spawn`,`/v1/status`,`/v1/teardown`,`/v1/exec`. Runner-lease (egress, runner-direct) +
check-host-lease (check-mode spawn + `exec_captured`). Digest-pinned, fail-closed, redacting Debug.
**Exercised by** S1.2.1, S1.2.2, S5.4.1.
**Validated by** `acceptance_cloud`, `cloud_exec`, `cloud_spawn`, `cloudflare_flip_e2e`.

### F-6.2 — NorthflankEngine (fallback)  🟡

**What** The Northflank job-run backend behind the same seam.
**Where** `northflank.rs:582,403,472`.
**Status** 🟡 DEFAULT-OFF (fallback substrate, not the live path).
**Details** Job-run lifecycle (create→run→poll→capture→delete); typed `ProviderCapacityError` (graceful
degrade); disk-floor boot warn (`RUNNER_EPHEMERAL_STORAGE_FLOOR_MB=4096`, `northflank.rs:146`); fail-closed
run-status classification (exact `SUCCESS` only).
**Exercised by** S1.2.2, S5.4.1.
**Validated by** `acceptance_cloud` (Northflank arm), backend unit tests.

### F-6.3 — HybridBoxProvisioner (rota B)  🟡

**What** When both backends are present, routes each lease to the right substrate by its isolation posture.
**Where** `cloud_exec.rs` (`HybridLeasedExec`, `provision` `:779-785`, `select_backend` `:948`).
**Status** 🟡 DEFAULT-OFF.
**Details** The per-lease router is `HybridBoxProvisioner::provision`, a 3-way match: `spec.allow_egress==true`
→ Cloudflare runner sub; `!allow_egress && is_check_host_spec` (`:643`, `!allow_egress && env has
TOOLCHAIN_DIGEST`) → Cloudflare check-host sub (rota A); else (plain hermetic) → Northflank check sub. It
**can't be spoofed** because `allow_egress` is set only by the lease constructors (F-4.1), never inferred from
the wire `net_policy` — so the routing fork IS the lease-kind fork (C2 invariant, `:668-672`). *(Distinct:
`select_backend` `:948` is a pure oracle choosing Hybrid/CF/NF/Off from `(cf_present, nf_present)` at
composition, not the per-lease router.)* One shared `BoxRegistry`; exec-engine == spawn-engine per lease.
**Exercised by** S5.4.1.
**Validated by** `hybrid_flip_e2e`.

### F-6.4 — NoBox (fail-closed default)  🟢

**What** With neither backend configured, every exec 503s while the lease lifecycle still works.
**Where** `cloud_exec.rs` (`NoBoxExec`).
**Status** 🟢 LIVE default (S2 fail-closed).
**Details** Lease acquire/close/ledger all function; only box exec is refused.
**Exercised by** S5.4.1.
**Validated by** `cloud_exec` (NoBox arm).

### F-6.5 — Spawn-Worker HTTP contract  🟢 Rust / ⚪ TS

**What** The frozen spawn seam transcribed on each side.
**Where** `docs/spec/cloudflare-spawn-worker-contract.md` + `cf-check-host-contract.md`; conformance
`cloudflare-spawn.json`; `UreqTransport` (the only real net impl).
**Status** 🟢 LIVE (Rust side); ⚪ TS golden pending (F-3.3).
**Details** Four routes — `POST /v1/spawn`, `GET /v1/status/{handle}`, `POST /v1/teardown`, `POST /v1/exec` —
transcribed byte-identically each side (the `cloudflare-spawn.json` vector, F-3.3); `UreqTransport` is the only
real net implementation; digest-pinned, fail-closed, redacting Debug (shared posture with F-6.1).
**Exercised by** S1.2.1, S1.2.2.
**Validated by** `cloudflare_conformance`, `spawn-conformance.test.ts` (TS golden pending).

---

## 7. Cloudflare deploy surface — `deploy/cloudflare*`

### F-7.1 — `corelink-spawn-worker` (spawn + autoscaler)  🟢

**What** The prod Worker that spawns runner/check boxes, mints per-job creds, autoscales on webhooks, and
reconciles orphans.
**Where** `deploy/cloudflare/` (`index.ts`, `lib.ts`, `metrics.ts`, `github_app.ts`, `wrangler.jsonc`).
**Status** 🟢 LIVE in prod (per-mechanism arm-state below).
**Details** — mechanism · what · where · status:

| Mechanism | What | Where | Status |
|---|---|---|---|
| Bearer auth gate | constant-time `Bearer <CLOUDFLARE_SPAWN_AUTH_TOKEN>`; empty secret ⇒ deny | `index.ts:431`, `lib.ts:159` | 🟢 |
| `POST /v1/spawn` (runner) | start `RunnerContainer`; assert `image_digest @sha256:` (+ optional `PINNED_IMAGE_DIGEST` match, 409) | `index.ts:1202,1267` | 🟢 (PINNED assertion INERT — accepts any pinned ref) |
| `POST /v1/spawn` (check mode) | `mode:"check"` → `CheckHostContainer`; requires `toolchain_digest` (400) + `EXEC_SERVER_AUTH_TOKEN` (503) | `index.ts:1228-1264` | 🔵 OWNER-GATED |
| `POST /v1/exec` (check-exec, rota A) | relay argv to in-container exec-server (port 8080) via `containerFetch`; non-2xx fail-closed | `index.ts:1290-1341` | 🔵 OWNER-GATED |
| `GET /v1/status/{handle}` / `POST /v1/teardown` | liveness (mode-routed) / idempotent SIGKILL destroy | `index.ts:1344,1364` | 🟢 |
| `POST /v1/egress-cutoff` (O7) | sever outbound egress at runtime (`setDeniedHosts` + `"*"`) without destroy | `index.ts:1404-1427` | 🟢 (operator) |
| `POST /webhook` autoscaler | `workflow_job.queued` → HMAC-verify → label-gate → claim → mint JIT → spawn ephemeral runner; 202 + background | `index.ts:981-1159`, `lib.ts:170` | DEFAULT-OFF (needs webhook+mint secrets) |
| Managed-label family gate | serve bare `corelink` + `corelink-*` minus reserved; subset-gate refuses foreign labels; `self-hosted` passthrough | `lib.ts:661-716` | 🟢 |
| Reserved labels | never mint for `corelink-builder` (persistent builder pool) | `lib.ts:671` | 🟢 |
| Spawn / completion dedup | per-job `spawn:`/`done:` KV claims; redelivery no-op | `lib.ts:88,147`; `index.ts:1078,1142` | 🟢 |
| Per-tenant rate limiting | `WEBHOOK_LIMITER` keyed `spawn:<repo>`, 30/60s; 429 on shed | `index.ts:1100`, `wrangler.jsonc:47` | 🟢 |
| GitHub-App JIT-token mint | App creds + `installationId` ⇒ per-installation token; else static `GITHUB_MINT_TOKEN`; App-token KV-cached `ghtok:<id>` | `index.ts:449-455`, `github_app.ts:87,132` | DEFAULT-OFF (App path inert w/o `GITHUB_APP_*`) |
| Warm-mint / per-job CAS PAT | `mintCasPat` via `/internal/v1/runner/mint`; server-derives tenant; 403 hard-deny, 5xx fail-open cold | `lib.ts:220,414`; `MintForbiddenError` | DEFAULT-OFF (needs `CORELINK_RUNNER_MINT_AUTH_KEY`) |
| env-0 cred-ticket + `CredStashDO` | stash PAT in a per-lease DO, inject `CLW_CRED_TICKET`; multi-use until TTL; wiped at completion; `POST /v1/leases/{id}/cas-cred` redemption (401/410/404) | `index.ts:219-264,1169`; `lib.ts:389` | 🟢 (`SPAWN_WORKER_PUBLIC_URL` set) |
| `ConcurrencySlotsDO` (atomic cap) | singleton DO holds authoritative in-flight slot list — per-tenant entitlement THEN fleet cap, single-threaded (replaces fail-open KV counter); `FLEET_MAX_CONCURRENCY=20`, `COLD_REPO_CAP=8`, `SLOT_TTL_S=2700` | `index.ts:274-297,731`; `lib.ts:527-553` | 🟢 |
| Container-start retry | `SPAWN_MAX_ATTEMPTS=3`, fresh DO handle each, `SPAWN_ATTEMPT_TIMEOUT_MS=8000` + linear backoff | `index.ts:528-565,528,532` | 🟢 |
| Completion actions | revoke per-job PAT by `pat_id` · teardown immediately · wipe cred-stash · optional usage-push | `index.ts:608-701` | 🟢 (billing push DEFAULT-OFF) |
| Re-drive reconciler | cron scans `RECONCILER_REPOS` for queued+labeled+runnerless jobs >90s (`RECONCILE_MIN_AGE_MS=90_000`); clears stale claim, WARM re-drive | `index.ts:1439-1485`, `lib.ts:606,718` | DEFAULT-OFF (needs `RECONCILER_REPOS`) |
| Dead-letter orphan retry | `orphan:<jobId>` records warm-recoverable failures; cron retries WARM, bounded `MAX_ORPHAN_ATTEMPTS=3` / `ORPHAN_TTL_S=1800` (30 min) | `index.ts:855-875,1498-1580`; `lib.ts:782,779` | 🟢 (when armed) |
| `MetricsDO` (golden counters) | singleton DO holds 12 fixed `COUNTER_NAMES` (F-10.2) under one key; bumped at each lifecycle seam; `GET /internal/v1/metrics`, `METRICS_OBSERVABILITY_KEY`-gated (404 unset) | `metrics.ts:24-40,58`; `index.ts:969` | 🟢 (default-off if unbound) |
| `REPO_INSTALLATION_MAP` inject | inject the known installation-id for first-party repos ⇒ WARM mint without an App webhook | `index.ts:1130`, `lib.ts:623` | 🟢 (first-party) |
| Legacy PAT escape hatch | `ALLOW_LEGACY_PAT_ENV=1` injects raw `CLW_TOKEN`; refused in prod (`SPAWN_WORKER_PUBLIC_URL` set) | `lib.ts:462-495` | DEFAULT-OFF / prod-refused |

**wrangler config:** 5 DOs — 2 Container classes (`RunnerContainer`=`RUNNER_CONTAINER`, `standard-4`
= 4 vCPU/12 GiB/20 GB disk `wrangler.jsonc:119`, `max_instances 20` `:140` raised 6→20 on 2026-07-16 to cover a
tenant's default entitlement; `CheckHostContainer`=`CHECK_HOST_CONTAINER`, `standard-4`, `max_instances 4`
`:159`) + 3 plain-storage DOs (`CRED_STASH`, `METRICS`, `CONCURRENCY_SLOTS`); 5 sqlite migrations `v1..v5`.
**Container idle-reap (`sleepAfter`):** each Container subclass sets a `sleepAfter` **idle** backstop as a TS
class property (NOT a wrangler key) — `RunnerContainer` `sleepAfter = "15m"` (`src/index.ts:311`, reduced 45m→15m
2026-07-06) and `CheckHostContainer` `sleepAfter = "45m"` (`src/index.ts:367`). It governs only FAILED/stuck
containers: a completed job is torn down immediately (`teardownCompletedRunner`) and a running job keeps the
container active, so it never cuts a live job; the `max_instances` comment (`wrangler.jsonc:123`) notes idle
instances linger up to this window ("not yet reaped"). KV `RUNNER_JOB_PATS` (multiplexed keyspaces
`spawn:`/`done:`/`jtenant:`/`jhandle:`/`orphan:`/`ghtok:`/bare jobId; job→pat_id map TTL `JOB_PAT_TTL_S=7200`
`index.ts:497` as a self-cleaning revoke backstop); cron `* * * * *`; ratelimit `WEBHOOK_LIMITER` (30/60s
`:47`). Both images pinned by immutable `@sha256` in-config at deploy time (not per-spawn — ADR-0008 wrinkle).
R2 binding intentionally omitted (Cache-TL coordination item). Image builds are Docker-free (CI pushes to the
CF managed registry).
**Exercised by** S1.1.3, S1.2.1–S1.2.4, S1.4.2, S1.4.3, S5.1.2.
**Validated by** `index.test.ts`, `webhook-route.test.ts`, `check-host.test.ts`, `metrics-do.test.ts`, `orphan-retry.test.ts`, `cred-stash-do.test.ts`.

### F-7.2 — `corelink-fabricd` proxy Worker  🟢

**What** The control-plane host: a singleton `FabricdContainer` fronted by a proxy Worker.
**Where** `deploy/cloudflare-fabricd/` (`index.ts`, `shard.ts`, `wrangler.jsonc`).
**Status** 🟢 LIVE singleton (moat proven); N>1 OWNER-GATED (F-5.7).
**Details** `FabricdContainer` (`standard-2` = 2 vCPU `wrangler.jsonc:165`, `max_instances 1` `:168`). It sets
`sleepAfter = "1h"` (`src/index.ts:100`) but **never actually sleeps** — a 1-minute keepalive cron
(`crons: ["* * * * *"]` `wrangler.jsonc:176`) pings `/v1/health` every minute (the "24/7" knob), so the 1h idle
window never elapses; in-memory lease state survives between requests, only a restart/redeploy resets it. Features: FNV-1a shard routing + inert-at-N=1 singleton collapse
(`shard.ts:20,34`; `index.ts:237`); acquire round-robin placement + lease-op hash routing + scatter-gather
list/metrics + §9 trigger body-routing + webhook round-robin (all N>1, inert at N=1); per-request 30s proxy
timeout (long-lived routes exempt) (`index.ts:510-572`); keep-alive cron + watchdog self-heal (3 fails ~30s →
destroy → fresh boot) + boot-grace/reboot-backoff (`index.ts:743-859,587-644`); pg + vCPU-ceiling conditional
arming on `DATABASE_URL`; forwards moat mint / cred-ticket / attested-cost / crash-probe / observability /
autoscaler envVars into the container. Live image `5608d67d` (W1-resilience).
**Exercised by** S5.2.1, S5.2.2, S5.4.1, S5.4.3.
**Validated by** `shard.test.ts`, `resilience.test.ts`, `n-gt-1-routing.test.ts`, `leases-list.test.ts`.

### F-7.3 — `corelink-canary` (golden-counter alerting)  🟡

**What** A scheduled monitor that diffs the golden counters and emails on rule breaches.
**Where** `deploy/cloudflare-canary/` (`rules.ts`, `notify.ts`, `index.ts`).
**Status** 🟡 BUILT; DEPLOY OWNER-GATED (KV id + obs keys + Resend key).
**Details** Every 5 min (`crons ["*/5 * * * *"]`) fetches fabricd `/internal/v1/status` + `/v1/health` + spawn
`/internal/v1/metrics` (via service bindings to dodge same-zone 404), diffs a KV snapshot, runs a pure rule
engine (health-down/surface-unreachable ⇒ CRIT; mint_failures/spawn_failed ⇒ CRIT; capacity-503/revoke_failures
⇒ WARN; 404 ⇒ silent; counter-reset ⇒ INFO; optional staleness), cooldown-dedups (`ALERT_COOLDOWN_MINUTES=30`),
emails via Resend (no-op until `RESEND_API_KEY`). `rules.ts:107-278`, `notify.ts:58`, `index.ts:136`.
**Exercised by** S5.4.3.
**Validated by** `rules.test.ts`.

---

## 8. Identity, onboarding & the external-customer path

### F-8.1 — Direct external `runs-on: corelink` activation (ADR-0007)  🔵

**What** The gap taxonomy between "build complete" and "external customer can activate."
**Where** gap docs `docs/handoff/2026-07-11-*`; code in F-5.8, F-7.1.
**Status** 🔵 build half complete; activation owner/cross-repo gated.
**Details** — item · closed by · status:

| # | Item | Closed by | Status |
|---|---|---|---|
| 1 | GitHub-App installation-token minting (code) | `github_app.ts` (WP-1) | BUILT (`appJwt`/`installationToken`) |
| 2 | Bind `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY` on spawn-worker | owner `wrangler secret put` | 🔵 OWNER-GATED |
| 3 | App `Administration:write` + re-approval | owner GitHub UI | 🔵 OWNER-GATED |
| 4 | `RunnerContainer max_instances` ≥ entitlement | owner (cost decision) | ✅ DONE (raised to 20) |
| 5 | Multi-size taxonomy → resolver activation | owner product input | 🔵 OWNER-GATED |
| 6 | Webhook delivery from external repos → spawn-worker | owner/server-TL routing decision | 🔵 CROSS-REPO |
| 7 | TS↔Rust label split-brain | converge at multi-size activation | ⚫ INERT |

**Exercised by** S1.1.1–S1.1.4, S1.4.5.
**Validated by** F-5.8 / F-7.1 test suites; GAP — no external-activation e2e (owner-gated).

### F-8.2 — Identity (ADR-0002)  🟢 server-side / 🔵 last step

**What** User-facing identity is the **HuGR account** everywhere; underneath is CoreLink machinery behind a
frozen contract. Runners builds **no** identity machinery.
**Where** ADR-0002; `docs/handoff/2026-07-10-OWNER-GO-LIVE-checklist`.
**Status** 🟢 LIVE server-side; last operator step 🔵 OWNER-GATED.
**Details** Clerk sessions, org = tenant, PATs — consumed, not forked. The lease API stays Bearer-PAT. M2 direct
GA onboards via the HuGR account; per-tenant caps/fairness/billing key off org = tenant. Signup chain
(Clerk → tenant, Stripe → `runners_entitlement`, App-install → `installation→tenant` + `repo_allowlist`) is
coded server-side.
**Exercised by** S1.1.1.
**Validated by** GAP — cross-repo (CoreLink identity); consumed via `corelink_auth` (F-5.11).

---

## 9. Client tools, front-door integrations & SDKs

### F-9.1 — `corelink` CLI  🟢

**What** The `run` / `smoke` / `verify` command surface.
**Where** `crates/corelink-cli/src/{run.rs:114,smoke.rs,binding.rs:39}`; `docs/cli.md`.
**Status** 🟢 LIVE.
**Details** `run` (acquire→exec→verify→close; unpinned image → exit 2 before box contact; no lease leak) ·
`smoke` (health/key/fail-closed gates; `--full` = real acquire→cancel) · `verify` (offline v2 binding verify).
**Exercised by** S8.1, S8.2.
**Validated by** `e2e_live_fabric.rs`.

### F-9.2 — GitHub Action + Buildkite plugin  🟢

**What** Two CI front doors wrapping `corelink run --json` with identical gates.
**Where** `integrations/github-actions/action.yml`; `integrations/buildkite/plugin.yml` + `hooks/command`.
**Status** 🟢 LIVE (binary-availability gated).
**Details** "CoreLink Run": exit-2 hard-fail; `verified!=true` hard-fail; PAT never logged. Buildkite mirrors it
(same gates + agent annotate).
**Exercised by** S8.3.
**Validated by** GAP — no e2e workflow run (binary-gated); gates covered by F-9.1.

### F-9.3 — `corelink-memoize` composite action  🟢

**What** Wraps `clw run` for cache memoization, fail-open by design.
**Where** `actions/corelink-memoize/action.yml`.
**Status** 🟢 LIVE (fail-open by design).
**Details** `--input`/`--env`/tool-version fold into the key; moat absent / clw exit 125 ⇒ COLD run; accepts
`CLW_TOKEN` or `CLW_CRED_TICKET`.
**Exercised by** S2.1.1.
**Validated by** GAP — no live memo-hit smoke (⚪ X4).

### F-9.4 — Verify SDKs (Python + TypeScript)  🟢

**What** In-language v2 result-binding verifiers, vector-locked.
**Where** `sdk/python/corelink_verify/__init__.py`; `sdk/typescript/src/index.mjs`.
**Status** 🟢 LIVE.
**Details** Python `corelink_verify` + TS `@corelink/verify` (zero deps, `node:crypto`); empty/absent sig = loud
error; both vector-locked.
**Exercised by** S1.5.1, S8.2, S8.3.
**Validated by** SDK vector-lock tests (against `conformance/result_binding_v2.json`).

### F-9.5 — `corelink-check-exec-server`  🟢 / 🔵 live-flip

**What** The in-container check-host exec-server.
**Where** `crates/corelink-check-exec-server/src/lib.rs:84,162`.
**Status** 🟢 LIVE inside the check-host container; live-flip owner-gated.
**Details** `POST /exec` on port 8080; process-group kill on timeout; verbatim capture ≤8 MiB/stream; optional
`EXEC_SERVER_AUTH_TOKEN` bearer gate.
**Exercised by** S2.1.2.
**Validated by** `exec_e2e.rs`; `rota_a_check_host_exec_e2e`.

---

## 10. Observability & ops

### F-10.1 — Golden counters (fabricd, 23)  🟢

**What** Lock-free `Counters` on `/internal/v1/status`.
**Where** `observability.rs:64-128,167-189`; `app.rs:1867`.
**Status** 🟢 LIVE (DEFAULT-OFF until `FABRIC_OBSERVABILITY_KEY`).
**Details** 23 counters: `leases_acquired`; **8 named rejection reasons** `acquire_rejected_{suspended,
invalid_image, bad_request, rate, over_cap, no_plan, compute_ceiling, lease_invalid}` (each a distinct
AtomicU64, so a no-plan mis-provision is never read as genuine over-cap saturation, WP-3b);
`leases_closed`/`_expired`/`_crashed`; `provision_capacity_503`; credential
`mint_attempts`/`_failures`/`revoke_attempts`/`_failures`; `agent_exec_started`/`_done`/`_failed`; operator
`load_shed`/`trigger_dedup_hits`/`suspend_actions`. Relaxed atomics, no lock/alloc/control-flow change.
**Exercised by** S5.2.1.
**Validated by** `status_api`, `occupancy_api`.

### F-10.2 — Golden counters (spawn-worker, 12)  🟢

**What** The durable `MetricsDO` `COUNTER_NAMES`.
**Where** `deploy/cloudflare/src/metrics.ts:24-40`.
**Status** 🟢 LIVE.
**Details** 12 names: `webhook_spawn_claimed`/`_deduped`/`webhook_rate_limited`/`webhook_job_completed`;
`jit_minted`/`runner_spawned`/`spawn_forbidden`/`spawn_at_ceiling`/`spawn_failed`;
`runner_torn_down`/`cas_pat_revoked`/`billing_pushed`. Distinct from fabricd's —
`mint_failures`/`provision_capacity_503`/`revoke_failures` live on the fabricd status surface, not here; the
canary reads both surfaces.
**Exercised by** S5.2.1.
**Validated by** `metrics-do.test.ts`.

### F-10.3 — Boot-honest diagnostics  🟢

**What** Boot-time logs that never claim a capability that isn't armed.
**Where** `server.rs` (ops-boot-arm #356); `cloud_exec.rs` (`cloud_backend_status`).
**Status** 🟢 LIVE.
**Details** One stderr line per boot naming which ops keys are armed (present/absent only); a cloud-backend
diagnostic names the missing var on a partial config and never claims a backend while silently `NoBox`.
**Exercised by** S5.4.1.
**Validated by** `mock_e2e` (boot diagnostics); `cloud_exec` (`cloud_backend_status`).

### F-10.4 — Quota-headroom monitor  🟡

**What** An advisory disk-headroom sweep that logs but adjusts nothing.
**Where** `quota_headroom.rs`; `FABRIC_QUOTA_CHECK_INTERVAL_SECS`.
**Status** 🟡 DEFAULT-OFF (opt-in, no default; task spawned only when set).
**Details** Each tick computes `needed_disk_mib = Σ over (tenant,cap) of cap × runner_storage_floor_mib`
(worst-case, every entitled slot a runner) then, if `allowance_mib` is set: `needed >= allowance` → logs
`QUOTA_HEADROOM_EXCEEDED`; else `pct >= 80` → `QUOTA_HEADROOM_WARNING`; below 80% / no allowance → informational
only (`quota_headroom.rs:181-194,276-299`).
**Exercised by** S5.4.1, S5.4.4.
**Validated by** `quota_headroom.rs` unit tests (threshold branches); GAP — no armed live proof.

### F-10.5 — Runbooks, CI gate & Docker-free image build  🟢

**What** The ops docs, the merge gate, and the Docker-free container-image pipeline.
**Where** `deploy/RUNBOOK.md`, `deploy/cloudflare-fabricd/README.md`, `docs/deploy/secret-rotation-checklist.md`;
`.github/workflows/*`.
**Status** 🟢 LIVE.
**Details** Runbooks: Northflank deploy + gotchas, CF fabricd deploy, per-variable secret rotation,
post-redeploy smoke, corelink-flip, northflank-postgres. **CI gate:** fmt · clippy `-D warnings` · test · deny ·
audit · conformance byte-check on `[self-hosted, mac, corelink-builder]`, plus `dogfood-smoke`,
`corelink-smoke`, `moat-correctness/benchmark`, `canary-smoke`. **Image build:** CF container images (fabricd,
runner, check-host) built + pushed by CI (`build-cf-container-images.yml`, `build-fabricd-image.yml`); wrangler
references pre-built `@sha256` digests.
**Exercised by** S5.4.1, S5.4.3, S5.4.4.
**Validated by** the CI gate itself (green locally + on CI).

---

## 11. Integration seams (family)

- **Consumes CoreLink Cache** — CAS/AC/R2 + tenancy + PAT, as a layer, never forked. Auth Bearer PAT
  (`interop.md §2`). Dedup is **intra-tenant at GA**; cross-tenant is staged (`CAP-DEDUP-CROSS-TENANT`) — the
  tense-discipline rule (`docs/review/2026-06-09-cross-tenant-dedup-claim.md`).
- **hugit (discontinued intended consumer)** — was to be the execution substrate for memoized CI. Historical hugit
  framing (hugit discontinued); the fabric owns these mechanisms (contract v1.4.0). The hugit cutover
  (`HUGIT_RUNNER_HOST` repoint + pubkey re-pin `b1eba792…`→`faa5b7726…`) no longer applies.
- **Consumed by Workspaces** (campaign #2) — agent sandboxes / dev-boxes on the same lease/isolate/attest spine
  (`ws/mod.rs`, F-4.8).
- **Drift tripwire** — the shared `conformance/` set is the byte-identical seam law across all consumers (F-3.3).

---

## 12. Roadmap status snapshot

SHIPPED-LIVE vs BUILT-INERT vs PLANNED.

| Capability | Status |
|---|---|
| Execution core (lease/isolate/teardown/boot/concurrency/expiry/recovery/shim/fence/X4/§13) | 🟢 SHIPPED-LIVE (182+ acceptance tests) |
| Cloudflare moat (native check-host exec + per-job CAS-PAT mint + attested cost) | 🟢 SHIPPED-LIVE (proven 2026-07-09) |
| fabricd control plane (RunnerLease API, admission-reject, caps, attestation, reaper, envelope) | 🟢 SHIPPED-LIVE (singleton) |
| Direct dogfood/first-party runner fleet (`runs-on: corelink`) | 🟢 SHIPPED-LIVE |
| pg durable ledger + vCPU-h ceiling + durable billing export | 🔵 BUILT, OWNER-GATED (needs `DATABASE_URL`) |
| Multi-instance N>1 sharding | 🔵 BUILT-INERT, OWNER-GATED (RAISE-N on volume) |
| Queued fair admission (ADR-0005) | 🟡 BUILT, DEFAULT-OFF |
| Durable cross-instance admission queue | ⚫ BUILT-INERT |
| Global fleet gate (`GlobalGate`) | ⚫ BUILT-ONLY, unwired (zero call sites) |
| Direct **external** customer path (App-token mint code) | 🔵 BUILT; activation OWNER-GATED |
| Multi-size ladder | ⚫ BUILT-INERT, OWNER-GATED (taxonomy input) |
| Check-host live-flip (rota A to prod tenants) | 🔵 BUILT + deployed image; OWNER-GATED |
| CoreLink slot billing push | 🟡 BUILT, DEFAULT-OFF (owner: leave off for launch) |
| Email canary alerting | 🟡 BUILT; DEPLOY OWNER-GATED |
| Northflank substrate | 🟡 BUILT (fallback), not the live substrate |
| Firecracker own-metal (FC1–FC5) | ⚫ PLANNED (frozen upgrade path; blocked on KVM buy) |
| Cross-tenant dedup | ⚫ STAGED post-GA (`CAP-DEDUP-CROSS-TENANT`, cache-side) |
| Actions-YAML shim execution / GH equivalence lane | ⚫ v0-SIMULATED / Partial |
| Workspaces SKUs (campaign #2), GPU runners, multi-region (M3/M4) | ⚫ PLANNED |
| Anti-abuse sustained-pin/mining detection | ⚫ PLANNED (rate rail 🟢 LIVE) |
| GDPR Art.17 erasure of `billing_events` | ⚫ PLANNED (tracked) |

---

## 13. Config-knob index

Grouped by function; **secret** = must be `wrangler secret put` / secret-store, never in a config file. Full
rotation semantics in `docs/deploy/secret-rotation-checklist.md`. Verified against code R3.

**fabricd — bind/keys/auth:** `FABRIC_BIND_ADDR` · `FABRIC_SIGNING_KEY`(secret, seeds the ingest key) ·
`FABRIC_DEV_UNSAFE` (loopback-only) · `FABRIC_AUTH_BACKEND` (static\|corelink) · `FABRIC_PAT`(secret)/
`FABRIC_TENANT` · `CORELINK_INTROSPECT_URL` · `FABRIC_INTROSPECT_AUTH_KEY`(secret) · `FABRIC_INTROSPECT_TIMEOUT_MS`.
**Plans/caps/compute:** `FABRIC_TENANT_MAX_CONCURRENCY` · `FABRIC_TENANT_RATE_PER_MIN` ·
`FABRIC_RUNNER_REPO_ALLOWLIST` · `FABRIC_RUNNER_VCPU` (arms vCPU-h ceiling, needs pg) · `FABRIC_TENANT_MAX_VCPU_H`.
**Ledger:** `FABRIC_LEDGER_BACKEND` (memory\|pg) · `DATABASE_URL`(secret) · `TEST_DATABASE_URL` (tests) ·
`FABRIC_LEDGER_POOL_SIZE` · `FABRIC_PG_TLS` (disable\|require).
**Admission/load-shed:** `FABRIC_ADMISSION_MODE` (reject\|queue) · `FABRIC_ADMISSION_QUEUE_WAIT_MS` ·
`FABRIC_ADMISSION_TICK_MS` (default 50) · `FABRIC_ADMISSION_TICK_SLOTS` (default 64) · `FABRIC_ADMISSION_PARK_CAP`
(default 8) · `FABRIC_MAX_INFLIGHT_REQUESTS` (default 1024) · `FABRIC_CLOSE_ACK_MAX_INFLIGHT` (default 256) ·
`FABRIC_PROVISION_MAX_INFLIGHT` (default 16).
**Reaper:** `FABRIC_REAP_INTERVAL_SECS` (default 30) · `FABRIC_PENDING_MAX_AGE_SECS` (default 300) ·
`FABRIC_CRASH_PROBE_INTERVAL_SECS` (opt-in, no default).
**Ops surfaces (secrets → 404 unset):** `FABRIC_OBSERVABILITY_KEY` · `FABRIC_ADMIN_KEY`. Headers:
`X-Corelink-Internal-Auth`, `X-Fabricd-Shard`, `X-Fabricd-Num-Shards`.
**Cloud substrate:** `NORTHFLANK_API_TOKEN`(secret)+`NORTHFLANK_PROJECT_ID` · `NORTHFLANK_TEAM_ID`/`_BASE_URL`/
`_DEPLOYMENT_PLAN`/`_RUNNER_DEPLOYMENT_PLAN`/`_RUNNER_EPHEMERAL_STORAGE_MB`/`_EPHEMERAL_STORAGE_MB` (advisory,
quota-monitor only — see footnote)/`_PROJECT_DISK_ALLOWANCE_MIB` ·
`NORTHFLANK_HTTP_TIMEOUT_MS` · `CLOUDFLARE_SPAWN_WORKER_URL`+`CLOUDFLARE_SPAWN_AUTH_TOKEN`(secret) ·
`CLOUDFLARE_RUNNER_LABELS`/`_EXPIRY_MS`/`_RUNNER_STORAGE_MB` · `FABRIC_MOCK_EXEC` (dev-unsafe interlock).
**Runner fleet (ADR-0007):** `FABRIC_GITHUB_APP_ID`/`_APP_INSTALLATION_ID`/`_APP_PRIVATE_KEY(_B64)`(secret)/
`_API_BASE`/`_RUNNER_GROUP_ID`/`_RUNNER_NAME` · `FABRIC_GITHUB_MINT_TOKEN`(secret) · autoscaler
`FABRIC_AUTOSCALER_WEBHOOK_SECRET`(secret)/`_PAT`(secret)/`_RUNNER_IMAGE`/`_LABELS`/`_TMP_ROOT`/`_EXPIRY_MS`/
`_REPO_ALLOWLIST`/`_MAX_TRACKED_JOBS`.
**Moat mint / CAS creds / env-0:** `CORELINK_RUNNER_MINT_AUTH_KEY`(secret)+`CORELINK_RUNNER_MINT_URL` ·
`FABRIC_CRED_TICKET_SECRET`(secret) · `FABRIC_PUBLIC_BASE_URL` (the cred-redemption boot-guard gate) · injected
box env `CLW_ENDPOINT`/`_TENANT`/`_TOKEN`/`_REF_DOMAIN`/`_CRED_TICKET`/`_LEASE_ID`/`_FABRIC_ENDPOINT`,
`CORELINK_RUNNER_JITCONFIG`, `TOOLCHAIN_DIGEST`.
**Envelope §13:** `CORELINK_ENVELOPE_INGEST_URL`/`_CREDENTIAL`(secret)/`_BASE_URL` · `FABRIC_EMIT_INTENT_METRICS_SIG`.
**Billing:** `FABRIC_BILLING_EXPORT_INTERVAL_SECS` (opt-in, needs pg) · `BILLING_INGEST_URL`+
`BILLING_INGEST_AUTH_KEY`(secret)+`BILLING_REGION` · `FABRIC_BILLING_PUSH_INTERVAL_SECS` (default 30).
**Multi-instance:** `FABRIC_NUM_SHARDS`.
**Advisory:** `FABRIC_QUOTA_CHECK_INTERVAL_SECS` (opt-in, no default).
**Dev/test escapes:** `FABRIC_DEV_UNSAFE` · `FABRIC_MOCK_EXEC` · `HUGIT_GH_TEST_REPO` · `HUGIT_RUNNER_HOST` ·
`HUGIT_RUNNER_KNOWN_HOSTS` · `PRINT_VECTORS`.
**spawn-worker (Worker secrets):** `CLOUDFLARE_SPAWN_AUTH_TOKEN` · `EXEC_SERVER_AUTH_TOKEN` (check-mode
required) · `GITHUB_WEBHOOK_SECRET` · `GITHUB_MINT_TOKEN` · `GITHUB_APP_ID`/`GITHUB_APP_PRIVATE_KEY` ·
`CORELINK_RUNNER_MINT_AUTH_KEY` · `BILLING_INGEST_AUTH_KEY` · `METRICS_OBSERVABILITY_KEY` · `ALLOW_LEGACY_PAT_ENV`.
Vars: `CLW_TENANT`/`CLW_ENDPOINT`/`CORELINK_MINT_URL`/`RECONCILER_REPOS`/`REPO_INSTALLATION_MAP`/
`SPAWN_WORKER_PUBLIC_URL`/`PINNED_IMAGE_DIGEST`/`AUTOSCALER_LABEL`/`BILLING_INGEST_URL`/`BILLING_REGION`
(the usage-push target read at completion).
**check-exec-server:** `TOOLCHAIN_DIR` · `EXEC_SERVER_AUTH_TOKEN`.
**CLI:** `CORELINK_URL` · `CORELINK_PAT`.
**canary (secrets):** `FABRIC_OBSERVABILITY_KEY` · `METRICS_OBSERVABILITY_KEY` · `RESEND_API_KEY`. Vars:
`FABRIC_STATUS_URL`/`FABRIC_HEALTH_URL`/`SPAWN_METRICS_URL`, `ALERT_COOLDOWN_MINUTES`(30)/`STALENESS_HOURS`(0=off)/
`BUSINESS_HOURS_UTC`/`ALERT_EMAIL_TO`/`ALERT_EMAIL_FROM`. Bindings: KV `CANARY_KV` (snapshot+cooldown state),
**service bindings** `FABRICD_SVC`→corelink-fabricd + `SPAWN_SVC`→corelink-spawn-worker (Worker→Worker direct
calls — a public `fetch()` to a sibling `*.workers.dev` on the same account mis-routes to 404, observed live
2026-07-17; the binding routes directly to the gated `/internal/v1/*` surfaces).

**Config-index footnotes (grep-verified R2/R3/R4):** `NORTHFLANK_EPHEMERAL_STORAGE_MB` **is** read — but only by
the quota-headroom monitor (`quota_headroom.rs:135`, parse-or-bail, per-check-slot storage default 1024 MiB), and
its value is currently **inert in the math**: the worst-case headroom formula deliberately counts every entitled
slot at the larger runner floor `RUNNER_EPHEMERAL_STORAGE_FLOOR_MB=4096` and passes the check-slot value as an
unused `_check_storage_mib` (`quota_headroom.rs:184`; `northflank.rs:146`) — distinct from the `standard-4` 20 GB
disk. (R3 wrongly claimed it was "not an env read" — corrected R4.) The Northflank job engine itself reads only
`NORTHFLANK_RUNNER_EPHEMERAL_STORAGE_MB` (`northflank.rs:253`). `FABRIC_GITHUB_APP_PRIVATE_KEY_B64` (base64
variant) is a real read on the fabricd proxy.

---

## 14. Deliberate exclusions & ambiguities

**Excluded (out of Runners' scope by charter, whitepaper §12):** check *semantics* (what a check means / whether
it passes), the memo *key* (hugit owned the formula (discontinued); the fabric only serves misses), landing/merge, and
provenance (hugit's, discontinued); the cache itself (CoreLink's); per-minute billing exposure. These are not Runners features
and are not inventoried. **Not to be confused with excluded:** the check-*host* execution box, the in-container
`corelink-check-exec-server` (F-9.5), native CF check-exec (F-7.1), and the `agent-exec` seam (F-2.5) **ARE**
Runners features. The line is *execution (ours) vs semantics (hugit's, discontinued)*, not "check = not ours."

**Live-vs-inert ambiguities a validator should resolve on live config, not this doc:**

1. **Check-host / rota-A** — the CF check-host image is *built + deployed* but the live-flip to prod tenants is
   owner-gated (real toolchain snapshot + go). "Deployed image" ≠ "serving check traffic."
2. **The moat mint** — proven live on a hydrating check-host acquire; a plain 200 does NOT prove a mint (only a
   mint-armed 503→200 or a server-side mint log does). Validate the transition, not the status code.
3. **`PINNED_IMAGE_DIGEST`** — the spawn-worker accepts any `@sha256:` ref today (the assertion is inert); arm at
   runner-fleet activation.
4. **TS↔Rust label split-brain** — `matchManagedLabels` (TS family/prefix) vs the Rust webhook subset-gate
   (explicit list) don't mirror; dormant while the fabricd autoscaler is unarmed, must converge before multi-size.
5. **Stabilization wave (2026-07-16)** — R2 code-verified as LANDED: W1 resilience
   (`FABRIC_CRASH_PROBE_INTERVAL_SECS=20` + pg-gated `FABRIC_BILLING_EXPORT_INTERVAL_SECS=60` in
   `deploy/cloudflare-fabricd/wrangler.jsonc` + forwarded into the container; live image `5608d67d`), W3
   credential lifecycle, W7 (`ConcurrencySlotsDO` atomic slot, `spawn:<repo>` `WEBHOOK_LIMITER`, `github_app.ts`
   App-token mint). `RunnerContainer max_instances` raised 6→20. Residual: confirm the *deployed* worker/image
   versions match source (source ≠ deployed is the only remaining unknown, resolvable only on live config).
6. **G2 metadata/link-local egress** — NOT closed on the CF path by the `deniedHosts` mechanism (no CIDR match,
   raw-socket bypass); needs platform-network-layer filtering (F-4.2).
7. **`docs/spec/corelink-fabric-stub.md`** is a deliberate `⟨FILL⟩` skeleton (the CoreLink-techlead side), not a
   feature spec — not inventoried.
8. **`sleepAfter` (R3 over-corrected → restored R4)** — `sleepAfter` IS set, as a TS Container-class property
   (not a wrangler key): `RunnerContainer` `"15m"` (`deploy/cloudflare/src/index.ts:311`), `CheckHostContainer`
   `"45m"` (`:367`), `FabricdContainer` `"1h"` (`deploy/cloudflare-fabricd/src/index.ts:100`). It is an **idle
   backstop**, not the primary lifecycle: completed runners are torn down immediately, and fabricd's 1-min
   keepalive cron means its 1h window never elapses. R3 wrongly deleted these as "fabricated"; R4 restored the
   real values from code.

---

## Appendix A — Glossary

Every coined term, used verbatim thereafter.

| Term | Definition |
|---|---|
| **Attested cost** | The signed §13 `IntentMetrics` on `CloseResponse` (`intent_metrics_sig`, F-5.4) — a per-job spend a consumer can render as tamper-evident (hugit was the intended consumer, discontinued). |
| **Cache-warm boot** | Booting a box with the CAS/AC pre-warmed so the job's inputs are local before the first instruction (F-4.3). |
| **Cred-ticket (C2c)** | A single-use, lease-scoped ticket injected instead of the raw CAS PAT; redeemed at trusted boot (F-5.9). "PAT never on the box." |
| **Dogfood** | HuGR's own first-party use of the fabric (App installation 150584374) — the live-proven path. |
| **env-0** | The environment injected into a runner box at spawn (JITCONFIG, cred-ticket, ingest token) — the injection surface (F-5.8). |
| **Fence** | The set of paths a job may touch; sparse materialization IS the fence (out-of-fence ⇒ ENOENT) (F-4.4). |
| **FLIP-A / FLIP-B** | The moat go-live transitions: FLIP-A = per-job CAS-PAT mint armed; FLIP-B = `intent_metrics_sig` on the wire (F-5.4, F-5.9). |
| **Front door** | A buyer-facing entry to the same fabric: direct / hugit (discontinued) / power-user / workspaces (F-2.1–F-2.4). |
| **Golden counters** | The fixed, lock-free counter set on the status/metrics surfaces (23 fabricd, 12 spawn) (F-10.1, F-10.2). |
| **Memoized execution** | Returning a content-addressed result without running the job when the AC has the key (F-1.2). |
| **The moat** | CoreLink's content-addressed cache; Runners is how it earns compute revenue. |
| **Owed-skip / cursor-rotation** | The in-memory fair scheduler's algorithm: cap-skipped tenants are served first next tick, the rest cursor-rotated (F-5.2). |
| **RAISE-N** | Scaling fabricd past the singleton: `DATABASE_URL` + `FABRIC_NUM_SHARDS` + `max_instances` raised in lockstep (F-5.7). |
| **Reject vs queue mode** | The two over-cap admission semantics: immediate 429 (default) vs fair enqueue (ADR-0005) (F-5.2). |
| **Rota A / Rota B** | Rota A = native CF check-host exec (F-7.1); Rota B = the Hybrid provisioner routing by posture (F-6.3). |
| **`sleepAfter` (idle backstop)** | The CF Container-class idle-reap window (a TS class property, not a wrangler key): `RunnerContainer` 15m, `CheckHostContainer` 45m, `FabricdContainer` 1h. A FAILED/stuck-box backstop only — completed boxes are torn down immediately and fabricd's keepalive cron means its window never elapses (F-7.1, F-7.2). |
| **Sparse materialization** | Hydrating only in-fence paths — the enforcement mechanism of the fence (F-4.4). |
| **X4** | The supply-chain verify-before-spawn oracle (F-4.5); also the badge for external/oracle-only proof. |

---

## Appendix B — Reserved / unwired constants (not features)

Defined in code but NOT wired into the live composition — inventoried here so they are never mistaken for features.

| Constant / symbol | Where | State |
|---|---|---|
| `ADMIN_TENANT_BY_ID` (`/internal/v1/admin/tenants/{id}`) | `paths.rs:128` | Defined, **zero references**, mounted to no handler — reserved/dead. (`ADMIN_TENANTS` by contrast IS wired at `server.rs:1360`.) |
| `GlobalGate` / `try_admit_global` | `global_gate.rs:172`; exported `corelink-fabric/src/lib.rs:75` | Built + unit-tested; **zero call sites** in the server composition — fleet gate is not live (F-5.2). |
| Durable cross-instance admission queue (`pg_queue`) | `admission.rs:301`, `pg_queue.rs` | Built; `with_durable_queue` is unwired — INERT (F-5.2). |
| Multi-size resolver (`SizeRegistry`) | `size.rs` | Single-rung; resolves byte-identical to single-size — INERT until the taxonomy input lands (F-5.10). |
| `clw_drive.rs` / `ac_pre_lease.rs` | those files | INERT stubs (A8 exit-transparency; moat WP-7 AC pre-lease) (F-5.11). |
| Docker `--read-only` / `--user` / `--cpus` | `isolation.rs` | Documented-inert (not in the current run policy) (F-4.2). |

---

## Appendix C — Feature ↔ story cross-reference matrix

`docs/product/USE-SCENARIOS.md`, personas P1–P8, story ids `S<theme>.<n>`. Every feature area is exercised by at
least one story; validators trace them together. (Per-card `Exercised by` fields carry the fine-grained mapping.)

| Feature area (this doc) | Exercised by (persona · theme / story ids) |
|---|---|
| §1 pricing / ceiling / entitlement (F-1.1–1.6) | P6 finance buyer (S6.1–6.3); P1 concurrency+ceiling (S1.3.1–1.3.3) |
| §2 front doors (F-2.1–2.5) | P1 (S1.1–1.2), P2 (S2.1–2.4), P3 (S3.1–3.2), P8 (S8.1–8.3), P4 |
| §3 wire contract & conformance (F-3.1–3.3) | P7 security auditor (S7.x); P2 attested cost (S2.2) |
| §4 execution core (F-4.1–4.10) | P4 the AI agent (S4.1–4.4); P7 red-team (S7.1–7.7) |
| §5.1–5.2 admission / caps / ceiling / suspend | P1 (S1.3.x), P5 operator capacity (S5.2.1–3) |
| §5.4 attestation / result-binding / attested cost | P2 (S2.2.1–2), P8 verify (S8.2–8.3), P7 (S7.3) |
| §5.6 billing / metering / GDPR erasure | P5 billing (S5.3.1–3), P6 |
| §5.8–5.9 runner broker / autoscaler / moat mint / cred-ticket | P1 onboarding (S1.1.1–4), P5 provisioning (S5.1) |
| §6–7 substrate / CF deploy / canary | P5 incident & deploy ops (S5.4.1–4) |
| §8 identity & external-customer path | P1 (S1.1), P5 (S5.1) |
| §9 CLI / SDKs / GH-Action / check-exec-server | P8 power-user (S8.1–8.3); P2 memoized CI (S2.1); P4 |
| §2.5 agent-exec seam | P2 (S2.3.1–2), P4 |

---

## Appendix D — Change log

Round-by-round; content-completeness and craft are tracked separately (DOC-STANDARD process).

| Round | Date | What changed |
|---|---|---|
| **R1 — built** | 2026-07-16 | Initial exhaustive Features & Functionality inventory (every subsystem, table-form, evidence-cited). |
| **R2 — deepened** | 2026-07-17 | Completeness-deepen: counter mis-attribution fix (23 vs 22; spawn-vs-fabricd surface split), counter/knob depth, the §11 cross-ref table, config-index grep-verification footnotes. |
| **R3 — architecture + depth** | 2026-07-17 | Imposed DOC-STANDARD: front-matter + dual Legend (evidence badge ✕ arm-state qualifier), linked TOC, Summary matrix, every feature converted to an ID'd card (`F-<section>.<n>`) with the identical What/Where/Status/Details/Exercised-by/Validated-by template; added Glossary, Reserved-constants appendix, Change log, Coverage summary. **Depth/corrections:** the `sleepAfter` 15m/45m/1h values were removed as "fabricated" — a REGRESSION (they are real TS Container-class properties, `index.ts:311`/`:367`/fabricd `:100`; restored in R4); the in-memory fair scheduler documented as owed-first cursor-rotation (NOT deficit-RR — that is the durable `pg_queue` path) with tick constants `TICK_SLOTS=64`/`TICK_MS=50`/`PARK_CAP=8`; `GlobalGate` reclassified INERT/unwired (zero call sites); `HybridBoxProvisioner::provision` router vs `select_backend` oracle distinguished; reaper 3-tier abnormal flush, `corelink_plans` resolve/no-TTL-cache, `quota_headroom` tick math, `pg_ledger` advisory-lock atomicity, `pg_queue` deficit ordering all spelled out; `ack_timeout=30s` re-cited to `leases.rs:1182`; `ADMIN_TENANT_BY_ID` flagged reserved/unwired. |
| **R4 — final dual-critic** | 2026-07-17 | Fixed the R3 `sleepAfter` regression — restored the real TS Container-class `sleepAfter` idle backstops (`RunnerContainer` `"15m"` `index.ts:311`, `CheckHostContainer` `"45m"` `:367`, `FabricdContainer` `"1h"` fabricd `index.ts:100`, never sleeps under the 1-min keepalive cron) across F-7.1/F-7.2/§14/Glossary. **Completeness critic:** found + fixed the false footnote claiming `NORTHFLANK_EPHEMERAL_STORAGE_MB` was "not an env read" (it is read at `quota_headroom.rs:135`, validated, but inert in the worst-case math) and added it to the knob index; added `BILLING_INGEST_URL`/`_REGION` to the spawn-worker vars. **Craft critic:** added the missing **Details** field to F-6.5; re-verified counter counts (23 fabricd / 12 spawn), all 21 TOC anchors, 55 unique F-ids, badge vocabulary, and the config-knob index against every env read in crates + workers. Both critics DRY. |

---

## Appendix E — Coverage summary

Feature cards by headline evidence grade (a card's grade = its dominant badge; sub-mechanisms may differ, per its
Details). Counts are dominant-badge approximations over the 55 feature cards F-1.1 … F-10.5 (split-badge cards
mean the column need not sum to exactly 55).

| Grade | Count (approx) | Reading |
|---|---|---|
| 🟢 LIVE-proven | ~28 | Execution core, wire contract, moat, CF spawn/fabricd, CLI/SDKs, observability. |
| 🟡 built-not-proven | ~11 | Ceiling enforcement, entitlement, queue mode, billing exporters, Northflank, hybrid, canary, quota monitor, anti-abuse. |
| 🔵 owner-gated | ~6 | pg ledger, N>1 sharding, external-customer activation, check-host live-flip, identity last step. |
| ⚫ INERT / planned | ~5 | Workspaces SKUs, multi-size resolver, durable queue, `GlobalGate`, Firecracker. |
| ⚪ X4 / oracle | (crosscut) | The X4 oracle + red-team + TS-side conformance + every externally-dialed e2e (hugit discontinued). |

**Honest residual (what is NOT yet at 🟢 with an in-repo test):**

- Whole classes gate on an **external actor** and cannot be proven in-repo: the full `[clw] cache hit` smoke,
  external memoized-check / agent-exec consumption (hugit discontinued), the live-account CF Containers SDK smoke (all ⚪ X4).
- The **loss-impossible vCPU-h wall** (F-1.4/F-5.2) is built but has no *armed-live* proof (DEFAULT-OFF, needs pg).
- **Billing exporters/push** (F-5.6) and the **quota-headroom monitor** (F-10.4) are DEFAULT-OFF with unit
  coverage only — no armed-live proof.
- **`GlobalGate`** (F-5.2) is built-only (zero call sites) — a feature on paper, not in the live composition.
- The **shim** (F-4.7) executes v0-simulated; the GH equivalence lane is Partial by design (no live-GH run).
- **Deployed-vs-source parity** for the CF workers/images is the one gap resolvable only against live config
  (§14 item 5), not from this doc.

**R4 (final dual-critic) — both critics DRY.** The R3 `sleepAfter` regression is fixed, and the completeness and
craft critics come back clean against code (counter counts, config-knob index vs every env read in crates +
workers, TOC/anchors, badge vocabulary, and the identical card template all verified). The remaining depth
boundaries are deliberate, not defects: per-card **states** machines (close/ack, lease lifecycle) are summarized
rather than exhaustively enumerated; the `pg_*` SQL DDL is surveyed by behavior, not schema-line-by-line;
`conformance/` per-field semantics live in the vectors themselves; and the Coverage counts are dominant-badge
approximations by design. The honest residual above is an **external-proof** gap (X4-gated — external/live-account (hugit discontinued)
actors), not a doc gap.
