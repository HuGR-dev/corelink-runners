# CoreLink Runners — Features & Functionality (canonical capability map)

> **What this is.** The exhaustive, evidence-grounded inventory of every feature and
> capability in CoreLink Runners — the product's canonical capability map. It is the
> baseline the validation campaign's "every feature validated" is measured against.
> Depth + completeness are the whole point.
>
> **Sources of truth (in precedence order).** Code + config `file:line` (the ultimate
> authority) · the frozen wire contracts (`docs/spec/`, `conformance/`) · the ADRs
> (`docs/adr/`) · the canonical vision (`docs/whitepaper/corelink-runners-v1.md`) ·
> the deploy/ops docs. Where a doc and the code disagree, the code wins and the doc is
> flagged. Compiled 2026-07-17.
>
> **Live-deploy reality (read first).** The LIVE substrate is **Cloudflare-first**
> (ADR-0008): `fabricd` runs as a CF Container + proxy Worker
> (`deploy/cloudflare-fabricd/`), runner/check boxes spawn on `corelink-spawn-worker`
> (`deploy/cloudflare/`), and the moat (native check-host exec + per-job CAS-PAT mint +
> attested cost) is **proven live** (2026-07-09). The current CF deploy is a **singleton**
> (`FABRIC_NUM_SHARDS=1`, no `DATABASE_URL` ⇒ in-memory ledger). Northflank is the ADR-0008
> **fallback**, not the live substrate — its "MULTI-INSTANCE on Postgres LIVE" framing in
> older docs is historical.

## Status legend

| Tag | Meaning |
|---|---|
| **LIVE** | Deployed and proven on the live path (dogfood/first-party). |
| **DEFAULT-OFF** | Code-complete + gate-green; inert until its env/secret is armed. Fail-closed when absent. |
| **OWNER-GATED** | Built but gated on an owner action, a product decision, cross-repo move, or real volume (e.g. RAISE-N). |
| **INERT** | Built + tested but not wired into the live composition (e.g. multi-size resolver, durable admission queue). |
| **PLANNED / v0-SIM** | Not built, or present as a v0 simulation / stub / documented upgrade path. |
| **ORACLE / TEST** | Verification-only machinery (red-team, conformance, X4 oracle) — not a production code path. |

---

## 1. Core value proposition & pricing model

CoreLink Runners is **ephemeral, cache-warm CI/build compute, billed by concurrency not by the minute.** It is CoreLink expansion campaign #1 — the compute layer that turns CoreLink's content-addressed cache (the moat) into compute revenue.

| Feature | What it is | Evidence | Status |
|---|---|---|---|
| **Concurrency pricing, never per-minute** | Customer buys N parallel runners flat/month; minutes unlimited. The deliberate inversion of GitHub Actions' per-minute model. | `docs/product/pricing.md §1`; whitepaper §3,§7 | LIVE (principle; billing meter DEFAULT-OFF) |
| **Never charge for the customer's own compute twice** | Cache-warm boot + memoization mean a re-run of an already-computed state costs ~0 and is billed ~0. | whitepaper §2,§7; `docs/product/product.md §5` | LIVE (principle) |
| **Cache-warm by construction** | A runner boots with CAS/AC pre-warmed; the job's inputs are local before the first instruction. The cache *is* the moat. | whitepaper §2,§5.2; `boot/mod.rs` | LIVE |
| **Memoized execution** | Result content-addressed by `H(inputs ‖ command ‖ toolchain)`; if the AC has the key, the result is returned and the job never runs. | whitepaper §2,§5.2; `exec.rs::compute_memo_key` | LIVE (hugit owns the memo key; fabric serves misses) |
| **The 5-tier flat ladder (SKUs)** | Starter $16 / Pro $40 / Team $100 / Scale $200 / Max $400 — concurrency caps 20/40/80/160/320, vCPU-h ceilings 100/240/600/1,200/2,400. No free tier; 5-day trial. | `pricing.md §2`; `crates/corelink-fabric/src/plans.rs:76` `PlanTier`, `plan_for`, `ceiling_for` | LIVE (ladder in code); prices OWNER-tunable |
| **Loss-impossible hard-ceiling guarantee** | Two hard limits per tier — concurrency cap + a hard active-compute ceiling (vCPU-h/mo) — so max COGS a user can incur (`ceiling × $0.10/vCPU-h`) is structurally below the price. | `pricing.md §3`; `compute_meter.rs` (`ceiling_vcpu_ms`, `MS_PER_VCPU_HOUR`) | Ceiling enforcement DEFAULT-OFF (armed by `FABRIC_RUNNER_VCPU>0` + tenant `max_vcpu_h`, requires pg) |
| **Entitlement model (Runners axis)** | Per-tenant concurrency cap derived from a **separate Runners entitlement** (not the Cache tier) via CoreLink introspect (`max_concurrency`). Ratified §B Option-B. | `corelink_plans.rs` (`CoreLinkPlanStore`); `conformance/corelink-introspect.json` | DEFAULT-OFF (corelink auth mode); dogfood entitlement = 20 |
| **Anti-abuse (sustained-pin / mining detection)** | Within-ceiling abuse rail on `CapGate` + slot metering catches slot-pinning to burn the ceiling on junk. | `pricing.md §5`; `caps.rs` (`CapGate`, `RateWindow`) | LIVE (rate rail); mining-detection heuristic PLANNED |
| **Acquire-rate abuse rail** | `rate_ceiling_per_min` = `max_concurrency × 10` — an acquire-*request* abuse rail (not a price; never binding in honest use). | `caps.rs`; ROADMAP "Rate-ceiling tier formula" | LIVE |

**Margin thesis (context, not a code feature):** three levers all downstream of the cache — warm ⇒ shorter jobs, memoized ⇒ jobs that never run, flat-for-concurrency ⇒ idle is margin (whitepaper §6 "the moat as a theorem"; `pricing.md §0` platform thesis). Typical margin 85–95%, worst-case floor ~37–40%. Real hit-rate is **unmeasured** (`pricing.md §6`).

---

## 2. The two front doors, two execution models (one fabric)

Same lease/isolate/cap/teardown spine; two buyers, two distinct execution models (whitepaper §9; `interop.md §4`; ADR-0007).

| Front door | Execution model | Box command | Result | Status |
|---|---|---|---|---|
| **Direct (ICP-B: CI/platform teams)** | Ephemeral GitHub-Actions runner fleet — `runs-on: corelink[-<size>]`; the customer's **unmodified** workflow runs on a cache-warm microVM, one ephemeral runner/job. | The GH runner agent (`config + run --ephemeral --jitconfig`) | the customer's own GitHub check status | LIVE for dogfood/first-party; **external customer path OWNER-GATED** (see §8.1) |
| **Via hugit (ICP-C: the anchor tenant, COGS)** | Memoized, attested *check* execution — hugit owns the memo key, the fabric sees only misses; `CheckDef → CheckResult` + attestation. A hugit customer never sees a "Runners" line item. | fabric sets `sh -lc <check.command>` per `/exec` | `CheckResult` + `AttestationChain` + `result_binding_sig(_v2)` | LIVE (moat proven; hugit cutover OWNER-gated) |
| **Power-user primitive** | `corelink run` — one attested command (the check path in CLI clothing). | `--check '<cmd>'` | verified verdict | LIVE |
| **Via Workspaces (M4 adjacency)** | Agent sandboxes / dev boxes as Workspace SKUs on the same fabric (`clw snapshot/hydrate` state). | workspace manifest | workspace object | PLANNED (campaign #2); `ws/mod.rs` spine built |

---

## 3. The frozen wire contract & conformance (the seam law)

The seam with hugit is **frozen from hugit's side** (`docs/spec/hugit-integration-contract.md` v1.4.0). Types are **transcribed** on each side (hugit-contracts is never imported; `deny.toml` forbids git/path deps). Drift is caught by shared byte-identical conformance vectors.

### 3.1 Wire types (`crates/corelink-runners-contracts/src/`)

| Type | What it carries | Evidence | Status |
|---|---|---|---|
| `RunnerLease` | lease_id, principal_chain, path_set, expiry(epoch-ms), net_policy, tmp_root, state | `runner_lease.rs:33` | LIVE |
| `RunnerState` | `held`/`expired`/`crashed`/`released` (snake_case) | `runner_lease.rs:16` | LIVE |
| `FenceManifest` / `MaterializedEntry` | sparse-materialization claim: path_set, `deny_default(must=true)`, materialized[(path,digest)]. "Sparse materialization IS the fence." | `fence_manifest.rs:35,19` | LIVE |
| `CheckDef` | def_digest, command, inputs, toolchain_ref, env_manifest, glob_set (2nd memo axis) | `check_def.rs:16` | LIVE |
| `CheckResult` / `Artifact` | frozen memo_key formula `lower_hex(SHA256(LP(tree)‖LP(def)‖LP(toolchain)))`, exit, artifacts, stdout/stderr_ref, … | `check_result.rs:28,13` | LIVE |
| `AttestationChain` | tree/def/runner/model/principal + `sig` (frozen ed25519 pre-image) | `attestation_chain.rs:18` | LIVE |
| `IntentMetrics` / `TokenCounts` / `ToolCount` | §13.1 per-job spend (cache-split tokens, wall/active ms, tool breakdown, `cost_usd_micros` integer micro-USD) — schema `"1.2.0"` | `intent_metrics.rs:45,20,36,15` | LIVE |
| `QueueApi` (`LandableEntry`, `UnionResult`, `MinimalFailingPair`, `BatchSeal`) | §9 landing-queue surface | `queue_api.rs:13,31,45,56,78` | LIVE |

### 3.2 API DTOs (`crates/corelink-fabric-api/src/dto.rs`, all `deny_unknown_fields`)

`AcquireRequest`/`AcquireResponse` (`:25,170`, with additive `runner`/`agent`/`toolchain_digest`/`repo_full_name`/`installation_id`) · `AgentSpec`/`RunnerSpec`/`RunnerTargetDto` (`:94,104,117`) · `ExecRequest`/`ExecResponse` (`:220,240`, carries `result_binding_sig`+`_v2`+`fabric_key_id`) · `AgentExecRequest`/`Ack`/`Result` (`:298,325,341`) · `TriggerRequest`/`Response` (`:371,401`) · `CloseRequest`/`Response` (`:442,476`, `metrics` REQUIRED, `intent_metrics_sig` additive) · `EnvelopeIngest` (`:144`, redacting Debug) · `KeyEntry`/`AttestationKeySetResponse`/`select_attestation_key` (`:551,581,632`, rotation-capable). **Error vocabulary** (`error.rs:15`): 400 `invalid` / 401 `unauthorized` / 404 `not_found` / 429 `over_cap` / 503 `fail_closed` — **no 403 by design** (cross-tenant = 404, no existence oracle). Path constants frozen in `paths.rs`.

### 3.3 Conformance vectors (`conformance/`, the drift tripwire)

17 byte-identical vectors + `manifest.sha256` (membership derived from the on-disk set, not hardcoded). Golden tests recompute SHA-256, verify manifest membership, and prove tamper-rejection (`corelink-runners-contracts/src/lib.rs:201,232,266`). Vectors: `RunnerLease`, `FenceManifest`, `IntentMetrics`, `AcquireRequest/Response`, `CloseRequest/Response`, `AgentExecRequest/Ack/Result`, `attestation_key_set`, `attestation_keyset_selection`, `result_binding_v2`, `intent_metrics_sig`, `cloudflare-spawn`, `corelink-introspect`, `lease_shard`. **Status: LIVE** (byte-checked in CI both repos). *Gap:* the TS side of `cloudflare-spawn.json` is not yet golden-tested (Rust-only tripwire — 2026-07-16 F4).

---

## 4. Execution core — `crates/corelink-runner`

The engine-agnostic execution primitives (lease → spec → spawn → isolate → exec → teardown). Docker-driven in v0; Firecracker is the frozen upgrade path behind the `Engine` seam (`lib.rs:30-44`).

### 4.1 Lease lifecycle (`lease.rs`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `ContainerSpec` | Engine-agnostic per-job spec from a frozen `RunnerLease` (name/image/tmp_root/no_network/allow_egress/run_on_create/path_set/env). | `lease.rs:43-83` | LIVE |
| Redacting Debug | Every `env` value prints `***REDACTED***` (no JITCONFIG/ingest-token log leak). | `lease.rs:89-107` | LIVE |
| Check-lease `from_lease` | Hermetic posture: `no_network=true`, no egress, no run-on-create; isolated net policies only. | `lease.rs:120-143` | LIVE |
| Runner-lease `from_runner_lease` | The **only** path setting `allow_egress=true`+`run_on_create=true`; requires `net_policy="egress-runner"` (ADR-0007 C2). | `lease.rs:158-180` | LIVE (trusted acquire path) |
| Agent-lease `from_agent_lease` | Egress-allowed but exec-driven (`net_policy="egress-agent"`). | `lease.rs:198-222` | LIVE |
| Image/tmp_root validation | X4 pin required; `tmp_root` shell-injection guard `^/[A-Za-z0-9._/-]+$`, fail-closed. | `lease.rs:225-271` | LIVE |
| `BoxExec` seam | `run(argv)` + `run_with_stdin` (safe channel for untrusted bytes; default refuses stdin). | `lease.rs:293-314` | LIVE |
| `SshBox` transport (interim) | Drives `ssh` to `hugit-runner-01`; TOFU host-key pin (`StrictHostKeyChecking=accept-new`, pinned known_hosts). | `lease.rs:341-469` | LIVE (interim transport; M1 replaces it) |

**Lease states** `Pending → Held → (Released | Expired | Crashed)` (`ledger.rs::LeaseState`). One lease = one isolated job; no reuse of a dirty box.

### 4.2 Isolation & security (`isolation.rs`, `teardown.rs`, `redteam.rs`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `Engine` seam | spawn/probe/exec/exec_captured/is_alive — the frozen substrate interface. | `isolation.rs:103-128` | LIVE |
| Fail-closed isolation floor | `DockerEngine::spawn` refuses `no_network=false`; X4 verify-before-spawn ordering (parse pin → verify → run). | `isolation.rs:145-205` | LIVE |
| Untrusted-container hardening | Fixed docker-run policy: `--network none`, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pids-limit 4096`, `--memory 12g` (= swap), `--tmpfs 64m`. | `isolation.rs:169-197` (consts :27,33,69,74) | LIVE (`--read-only`/`--user`/`--cpus` documented-inert) |
| Isolation probe | Tmp-privacy (tmpfs + host-leak check) + net-isolation (no non-lo iface + outbound connect BLOCKED). | `isolation.rs:207-265` | LIVE |
| Teardown + 4-surface forensic re-scan | `docker rm -f` then scans containers/processes/mounts/network; `is_clean` requires all empty AND zero scan-failures. | `teardown.rs:106-176,23-53` | LIVE |
| Fail-closed scan-failure detection | `set -o pipefail`, no `2>/dev/null` swallow; ambiguous grep exit ≥2 = scan-failed, not clean. | `teardown.rs:68-97,149-153` | LIVE |
| Red-team escape harness | 6 live escape vectors against a real container: traversal, symlink-escape, out-of-fence write, fork-bomb (cgroup-v2 observed), disk-fill (ENOSPC), fence-materialized-escape. | `redteam.rs:162-610` | ORACLE (box-gated; hermetic no-box proof :809-865) |
| Guard-the-guard | The fence-materialized-escape vector goes RED under a no-op `classify` — proves the fence is the only control. | `redteam.rs:567-610,872-893` | ORACLE |

**Isolation sign-off (ADR-0009):** Cloudflare Containers meet the Firecracker-class bar (each container = its own Firecracker microVM, one-per-job, destroyed after). Own-metal Firecracker is the frozen upgrade path. Track-C software hardening on the CF path is **PARTIAL** — deployed: image-pin, revoke-on-complete, egress kill-switch, required exec-server auth, app-layer `ulimit` (pids); **NOT closed on CF:** G2 metadata/link-local egress (the `deniedHosts` denylist has no CIDR match + raw-socket bypass). **Egress posture (ADR-0003):** cross-tenant isolation is the hard guarantee; outbound internet egress is *accepted at launch* on the managed tier (bounded by no-free-tier identity), full lockdown is a BYOC-tier upgrade.

### 4.3 Cache-warm boot (`boot/mod.rs`, `cas_http.rs`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `BootCas` seam | `is_cached`/`fetch_layer`/`write_layer`, 200=Hit / 404=None-miss / 5xx=SubstrateDown. | `boot/mod.rs:155-186` | LIVE (seam) |
| Warm-path `hydrate` | Skips cached layers — zero fetches when warm (≤10s target). | `boot/mod.rs:237-275` | LIVE |
| Cold-path `cold_hydrate` | Force-fetch all layers (≥60s) — the fallback baseline. | `boot/mod.rs:302-333` | LIVE |
| Fail-closed substrate handling | `SubstrateDown` propagates; zero poisoned writes; 404-miss never written back; `ForcedCold` on AC write-back failure. | `boot/mod.rs:56-108,191-219` | LIVE (§2 cache-down fail-closed) |
| `BoxHydrate` live-box driver | Drives `clw hydrate [--cold] <keys>` over `BoxExec`. | `boot/mod.rs:347-407` | LIVE |
| `CasHttpClient` | CAS/AC over HTTP: `/v1/cas\|ac/{tenant}/{blake3}`, Bearer per-job PAT; tenant in path, never a header. | `cas_http.rs:249-370` | LIVE |
| `Blake3Key` | Canonical content-addressed key (byte-identity). SHA-256 is never a CAS key. | `cas_http.rs:54-89` | LIVE |
| 3-way CAS status guard | 2xx=Hit / 404=Miss(cold) / 401/403/5xx=FailClosed — never a silent miss. | `cas_http.rs:99-129,218-236` | LIVE |
| `HttpBootCas` | BootCas-over-HTTP; write key = BLAKE3(data); 404-on-PUT = fail-closed not miss. | `cas_http.rs:411-631` | LIVE |
| Namespace routing (A13) | `_public:` cross-tenant keyspace routing is inert unless `with_public_routing` opts in. | `cas_http.rs:465-494` | **DEFAULT-OFF** (public routing) |

### 4.4 Fence enforcement (`materialize/mod.rs`, `enforce/mod.rs`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `materialize_sparse` | Writes only in-fence candidates under workspace_root; out-of-fence → dropped → ENOENT. **Sparse hydrate = the fence.** | `materialize/mod.rs:166-190` | LIVE |
| `validate_manifest` fail-closed | Requires `deny_default=true`; rejects allow-all / root-cover path_set. | `materialize/mod.rs:111-123` | LIVE |
| Traversal re-guard + base64 transport | Defense-in-depth `path_escapes_root` re-check; SHA-256 `content_digest` (frozen). | `materialize/mod.rs:197-239,92-96` | LIVE |
| `classify` (pure verdict) | Directory-prefix (segment-wise) vs exact-file; absolute / `..` = Outside. Covers `..`-escape, absolute-injection, `srcfoo`-vs-`src/` prefix-collision. | `enforce/mod.rs:91-121` | LIVE |
| `is_admitted` / `check_access` | The single fail-closed admission predicate materialize routes through. | `enforce/mod.rs:137-160` | LIVE |
| `probe_outside_enoent` | Box-backed ENOENT proof (exit-code probe, locale-independent). | `enforce/mod.rs:194-229` | LIVE |

### 4.5 Supply chain — verify-before-spawn (`pin.rs`, `x4/`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `PinnedImageRef::parse` | Accepts only `repo@sha256:<64-lc-hex>`; rejects tags/bare/uppercase/non-hex/shell-metachar. | `pin.rs:103-145` | LIVE |
| `verify_on_box` | `docker pull` + `RepoDigests` cross-check before spawn; permanent/transient failure classifier; retry (4 attempts). | `pin.rs:195-272,64-80` | LIVE |
| `digest_lock` | Per-digest mutex serializes concurrent same-digest pulls (race fix). | `pin.rs:42-53` | LIVE |
| `require_pinned` chokepoint | Single call every spec-build uses. | `pin.rs:282-284` | LIVE |
| X4 supply-chain oracle | 3 invariants (pinned+verified image, pinned deps, tampered→fail-closed-before-tenant-work); `VerifiedSpawn`/`GuardedSpawn` prove no tenant byte processed on rejection. | `x4/mod.rs:9-26`, `x4/pin.rs:158-230` | ORACLE (live enforcement is in `pin.rs`) |

### 4.6 Concurrency / expiry / recovery

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| Scheduler fan-out `run_batch` | Spawns N containers, samples peak concurrency (target ≥8), tears each down; injective naming avoids undercount collisions. | `concurrency/mod.rs:169-321,56-86` | LIVE |
| No-swallow teardown surfacing | A failed per-box teardown is recorded (leaked box visible); rest of batch still reclaimed. | `concurrency/mod.rs:121-162` | LIVE |
| Expiry hard-kill | Deterministic `is_expired` (epoch-ms; `u64::MAX` = never); `hard_kill` = SIGKILL then forensic teardown. | `expiry/mod.rs:42-107` | LIVE |
| Crash recovery | `probe_liveness` (Alive/Lost); `recover_lost` marks `Crashed` + cleanup + records `LostJob` (requeued/surfaced, never silent-green). | `recovery/mod.rs:88-158` | LIVE |

### 4.7 Actions-YAML shim (`shim/`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `parse_workflow` | Hand-rolled minimal YAML parser (no YAML dep); requires `on:`/`jobs:`; unsupported keys → `OutOfContractReport`, zero silent skips. | `shim/parser.rs:105-135,181-477` | LIVE |
| `ShimExecutor::execute` | Per-step outcomes; break on Failure/SecretDenied/OutOfContract. | `shim/executor.rs:148-207` | LIVE (**execution v0-SIMULATED** :321-345) |
| Secrets fail-CLOSED | `${{ secrets.X }}` in `run:`/`env:` resolved via broker; denied/unavailable → step refused. | `shim/executor.rs:248-319` | LIVE |
| `if:` fail-closed | Only `always()`/`true` run; unrecognized condition SKIPS (no fail-open force-run). | `shim/executor.rs:217-228` | LIVE |
| Determinism precondition | Flags multi-job, floating action refs (`@latest`/`@main`), `github.*` context in env → NotSatisfied. | `shim/executor.rs:368-415` | LIVE |
| Equivalence harness | shim lane runs; live-GH lane returns `Partial` (never fake-green). | `shim/executor.rs:444-479` | LIVE-but-Partial (GH lane gated on `HUGIT_GH_TEST_REPO`) |
| Secrets broker trait | Opaque injection tokens only, never raw material; FenceManifest is the access anchor. | `shim/broker.rs:91-176` | LIVE (`NullBroker` default-deny; `StubBroker` test) |
| Published subset contract | 15 `SubsetFeature`s + `is_supported`; explicit `OutOfContractReport`/`ShimDiagnostic`. | `shim/subset.rs:101-123`, `report.rs:32-117` | LIVE |

### 4.8 Workspace lifecycle (`ws/mod.rs`) — the Workspaces (campaign #2) spine

`spawn_workspace` (<1s warm contract) · `DedupSpawner` (coalesce concurrent identical spawns to one materialization; lock never held across `docker run`) · identity binding fail-closed (slot keyed by lease's container name, cross-lease share refused) · liveness-probe before reuse · bounded slot map + reaping (teardown OUTSIDE the lock) · `attach`/`resume` (resume cannot widen the fence) · `run_remote`≡`run_local` identity. Evidence: `ws/mod.rs:42-913`. **Status: LIVE (spine); Workspaces SKUs PLANNED.** *(Note: `ws` is workspace lifecycle, NOT a websocket module.)*

### 4.9 §13 envelope — agent-execution metrics (`envelope/`)

The contract §13 mechanism: metrics emission + capture-hook points + no-persistence (in-process; M1 adds transport + PAT).

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `MetricsCollector` | Single-writer accumulator; saturating on untrusted usage; derived `tokens.total`; exact-integer `cost_usd_micros`; exactly-once finalize. | `envelope/collector.rs:65-231,149-155` | LIVE |
| Tool-breakdown DoS bound | `MAX_DISTINCT_TOOLS=256`, `MAX_TOOL_NAME_LEN=128`, `<overflow>` bucket; Σ breakdown == tool_calls. | `envelope/collector.rs:23-29,109-127` | LIVE |
| Non-destructive `snapshot`/`project` | Turn-boundary checkpoint without tripping finalize (ADR-0004 Phase 2b). | `envelope/collector.rs:174-231` | LIVE |
| `CaptureHook` (2 surfaces) | Raw-event + per-turn `TurnMeta` bounded in-memory queues; bearer-gated subscribe; per-surface overflow flags; bytes forwarded byte-identical (redaction is forge-side, §13.3). | `envelope/hook.rs:167-381` | LIVE |
| Drain (in-flight-only) | `next_event`/`next_meta` pop-front, released after forwarding — no durable persistence (§13.3). | `envelope/hook.rs:394-409` | LIVE |
| JobClose ack state machine | `close`: finalize → publish CloseSignal → bearer-gated ack window (`ack_timeout` 30s) → fail-closed CloseOutcome; residue/overflow ⇒ `capture_incomplete`. | `envelope/close.rs:126-185` | LIVE |
| `close_abnormal` (§13.5) | Expiry/crash: partial flush, `capture_incomplete=true`, `close_reason` (Normal/Expired/Crashed) on the wrapper (never inside frozen IntentMetrics). | `envelope/close.rs:203-240,44-78` | LIVE |

### 4.10 Attestation signing (`attest/mod.rs`)

`FabricSigner` (ed25519, one fabric key/region) — `sign_chain` over the FROZEN pre-image `LP(tree)‖LP(def)‖LP(runner)‖LP(model)‖VEC(principal)` (`attest/mod.rs:43-132`); `verify_chain`/`verify_raw` use `verify_strict` (malleability-rejecting); `key_id` = SHA-256(pubkey)[..8]. **Status: LIVE.**

---

## 5. Control plane — `crates/corelink-fabric-server` (fabricd) + `crates/corelink-fabric`

The multi-tenant server behind the frozen `/v1` API. Composition root `server.rs::build_app_and_state` (`:999`) selects auth store, ledger, cloud backend, mint, cred-signer, admin, autoscaler — all default-off / fail-closed.

### 5.1 RunnerLease HTTP API (`app.rs::app_full`, handlers)

| Route | Handler | Evidence | Status |
|---|---|---|---|
| `POST /v1/leases` (acquire) | Shard-guard → plan resolve → runner repo-allowlist authz → rate check → atomic `try_admit` → finalize/enqueue | `app.rs:1915`, `leases.rs:257` | LIVE |
| `GET /v1/leases` (list, tenant-scoped) | scatter-gather at N>1 | `app.rs:1914`, `lease_list.rs` | LIVE |
| `GET /v1/leases/{id}` (status) | mirrors ledger exactly, no invented states | `app.rs:1916` | LIVE |
| `POST /v1/leases/{id}/cancel` | release + forensic teardown, idempotent | `app.rs:1918` | LIVE |
| `POST /v1/leases/{id}/exec` | CheckDef→CheckResult, gate order (scope/held/expired/exec/attest) | `app.rs:1921`, `exec_handler.rs` | LIVE |
| `POST /v1/leases/{id}/agent-exec` + `GET .../{step_id}` | arbitrary-command drive (egress, never memoized), async step-store | `app.rs:1925,1929`, `agent_exec.rs` | LIVE |
| `POST /v1/queue/trigger` | §9 hugit landing-queue trigger, attested, at-least-once dedup (cap 4096) | `app.rs:1933`, `queue.rs` | LIVE |
| `POST /v1/leases/{id}/close` | §13 close: teardown-first → ack window → Held→Released → atomic metrics+result | `app.rs:1934`, `close.rs` | LIVE |
| `GET /v1/leases/{id}/envelope/events\|meta` | §13.2 drain (tenant PAT) | `app.rs:1939,1940` | LIVE |
| `POST /v1/leases/{id}/envelope/ingest` | §13.2 write, per-lease scoped-token auth (outside PAT layer) | `app.rs:1891` | LIVE |
| `POST /v1/leases/{id}/cas-cred` | C2c cred-ticket redeem → per-job CAS PAT (single-use) | `app.rs:1896`, `cas_cred.rs` | DEFAULT-OFF |
| `GET /v1/usage` + `/v1/usage/history` | tenant-facing live usage (cap · active · peak) + history | `app.rs:1910,1913`, `usage.rs`,`usage_history.rs` | LIVE |
| `GET /v1/metrics/tenant` | §6 per-tenant wait histogram (non-interference surface) | `app.rs:1911`, `metrics.rs` | LIVE (count 0 in reject mode) |
| `GET /v1/attestation/key` (unauth) | published keyset (rotation-capable) | `app.rs:1856` | LIVE |
| `GET /v1/health`, `/`, `/health` (unauth) | liveness, outside the load-shed limiter (+ CF container health probe) | `app.rs:2008-2017` | LIVE |
| `GET /internal/v1/occupancy`, `/internal/v1/status` | ops surfaces, observability-key gated (404 unset) | `app.rs:1864,1867`, `occupancy.rs`,`status.rs` | DEFAULT-OFF |
| `POST /internal/v1/admin/tenants[/{t}/suspend\|unsuspend]` | onboarding + tenant suspend, admin-key gated | `server.rs:1360`, `app.rs:1872,1876`, `admin.rs`,`enforcement.rs` | DEFAULT-OFF |
| `POST /webhooks/github` | Stage-B autoscaler, HMAC-authed | `server.rs:1418`, `webhook.rs` | DEFAULT-OFF |

**Load-shed layer:** `GlobalConcurrencyLimit` + `LoadShed` → 503 on work routes (health/key excluded); defaults `MAX_INFLIGHT_REQUESTS=1024`, `CLOSE_ACK_MAX_INFLIGHT=256`, `PROVISION_MAX_INFLIGHT=16` (`app.rs:1971,633-651`). LIVE.

### 5.2 Admission, caps, fairness, compute ceiling

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| Atomic `try_admit` (reject mode) | Reserve-before-provision under the ledger lock; immediate over-cap 429. The default over-cap semantics. | `ledger.rs:178`, `leases.rs:494` | LIVE |
| Queued fair admission (ADR-0005) | `FABRIC_ADMISSION_MODE=queue`: over-cap enqueue → deficit-round-robin fair dispatch → authoritative `try_admit` inside `dispatch`; bounded async wait → 503; per-tenant park-cap wedge. | `admission.rs:64,421,757`; `scheduler.rs` (`FairScheduler`) | DEFAULT-OFF (reject is default) |
| Durable cross-instance queue | pg-backed deficit-ordered fair queue (`with_durable_queue`, `pg_queue.rs`). | `admission.rs:301`, `pg_queue.rs` | INERT (unwired) |
| Per-tenant concurrency + rate caps | `CapGate::check` + `RateWindow` (60s sliding); preventive (before load), fail-closed. | `caps.rs` | LIVE |
| Global fleet gate | `GlobalGate` fleet-wide wall + per-tenant fair-share. | `global_gate.rs:461` | BUILT (fleet cap; wiring per composition) |
| Compute (vCPU-h) ceiling | `ComputeGate` reserves vCPU·ms per lease (`try_admit_with_compute`); ceiling from tier (`compute_meter.rs`). Boot guards require pg + non-zero ceiling. | `ledger.rs:194`, `compute_meter.rs`, `server.rs:788-849`, `leases.rs:170` | DEFAULT-OFF (armed by `FABRIC_RUNNER_VCPU>0`, requires pg) |
| TTL clamp | Acquire TTL clamped to 60 min (bounds slot hold + vCPU·ms reservation). | `leases.rs:146` | LIVE |
| Tenant suspend (Track-C AUP1) | Block acquires + kill live leases; durable cross-instance (`fabric_suspended_tenants`). | `enforcement.rs`, `pg_ledger.rs` (`set_tenant_suspended`) | DEFAULT-OFF (admin-key) |
| Plan downgrade grace | Grace window on a plan downgrade. | `downgrade_grace.rs` | LIVE |

### 5.3 Ledger & durable state (`corelink-fabric`)

`LeaseLedger` trait (`ledger.rs:226`) with 3 impls — **`InMemoryLedger`** (live default), **`FileLedger`** (fsync + torn-journal tolerance), **`PgLedger`** (advisory-lock atomic count+insert = cross-instance cap-safe, `pg_ledger.rs:296`). `FABRIC_LEDGER_BACKEND` selector, **never a silent fallback** (`server.rs:571`). Durable per-lease reap state (ADR-0004): durable `deadline_ms` (cross-instance reaper backstop) + `envelope_checkpoint` (3-tier abnormal flush: local hook → durable checkpoint → explicit `no_capture` marker). Opt-in pg TLS (`FABRIC_PG_TLS`). Ledger conformance suite proves InMemory/File/Pg parity (`ledger_conformance.rs`, test). **Status: InMemory LIVE; pg BUILT + OWNER-GATED (needs `DATABASE_URL`).**

### 5.4 Attestation / result-binding / attested cost (`attestation.rs`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `build_attestation` | Frozen `AttestationChain` (§7) per execution; result without attestation is unrepresentable. | `attestation.rs`, api §exec | LIVE |
| `result_binding_sig` (v1) | Detached ed25519 over `LP(memo_key)‖LP(stdout_ref)‖LP(stderr_ref)`. | `attestation.rs` (`result_binding_preimage`) | LIVE |
| `result_binding_sig_v2` | Full-outcome binding — adds `i32_be(exit) ‖ u32_be(artifacts.len) ‖ ∀ LP(path)‖LP(digest)`; closes the forgeable-verdict gap. Additive alongside v1. | `attestation.rs` (v2 pre-image); vector `result_binding_v2.json` | LIVE |
| memo-key integrity check | Close path rejects (400) any `CheckResult` whose `memo_key` ≠ SHA-256 of its input axes before attesting. | api §close gate 4 | LIVE |
| `intent_metrics_sig` (attested cost, FLIP-B) | Signed §13 metrics on `CloseResponse` so hugit can render ATTESTED cost; signs honest-zero until a provider `/usage` source. | `attestation.rs` (`sign_intent_metrics`), dto `:541`; vector `intent_metrics_sig.json` | LIVE on fabricd (`FABRIC_EMIT_INTENT_METRICS_SIG=true`); hugit consumption pending |
| Keyset rotation | `AttestationKeySetResponse` + fail-closed `select_attestation_key` (UnknownKeyId/Expired). Prod key `faa5b7726…`. | dto `:581,632`; vector `attestation_keyset_selection.json` | LIVE (M1 = 1 key) |
| Per-lease scoped ingest token (ADR-0006) | HMAC-SHA256(ingest_secret, lease_id) write-only capability injected in place of the tenant PAT — an exfiltrated token authorizes ingest only to that one dying lease. | `ingest_token.rs`; `server.rs:1017` | LIVE |

### 5.5 Reaper & lifecycle (`reaper.rs`, `lifecycle.rs`)

Always-on `reap_once` (teardown-first, deadline from ledger — cross-instance backstop) · `sweep_stale_pending` (reclaim leaked Pending slots, `FABRIC_PENDING_MAX_AGE_SECS` default 300) · `surface_crashes` (opt-in crash probe, `FABRIC_CRASH_PROBE_INTERVAL_SECS`) · `flush_partial_envelope` (§13.5 abnormal) · `BoxRegistry` orphan GC (unbind on expiry). `leases_expired`/`leases_crashed` golden counters. **Status: reaper LIVE; crash-sweep DEFAULT-OFF (armed to `20s` on live fabricd).**

### 5.6 Billing / metering (`billing.rs`, `billing_sink.rs`, `corelink_billing.rs`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `SlotMeter` occupancy | Records held-lease occupancy (occupied/peak/journal, bounded `JOURNAL_CAP` with never-silent `journal_dropped`). Raw occupancy only — no minutes/cost math. | `billing.rs:58` | LIVE |
| Durable billing exporter | `PgBillingSink` + `spawn_export_loop` drain the journal into `billing_events` (exactly-once by PK + `ON CONFLICT DO NOTHING`). | `billing_sink.rs`, `billing_export.rs`; `FABRIC_BILLING_EXPORT_INTERVAL_SECS` | DEFAULT-OFF (requires pg) |
| CoreLink usage push | `runner_slot_seconds` events to corelink-billing (`CorelinkBillingTarget`, `flush_now`, push-flush loop 30s). | `corelink_billing.rs:58`; `BILLING_INGEST_URL/AUTH_KEY/REGION` | DEFAULT-OFF (owner: leave off for launch) |
| GDPR Art.17 erasure (billing_events) | Tenant-prefix-bounded `DELETE FROM billing_events WHERE tenant=$1`; PK-bounded, fail-closed, idempotent. | `docs/privacy/gdpr-erasure-billing-events.md`; `billing_sink.rs` DDL | PLANNED (tracked; depends on org-wide erasure orchestration + retention legal call) |

### 5.7 Multi-instance / shard routing (`shard.rs`, ADR "option-3")

`fnv1a_32` / `shard_of` / `mint_lease_id_for_shard` — a frozen cross-language contract with `deploy/cloudflare-fabricd/src/shard.ts` (vector `lease_shard.json`). `FABRIC_NUM_SHARDS` learned at boot; N>1 acquire on a non-pg ledger is **REFUSED fail-closed** (cap-guard). All N>1 gaps closed (cap-safety, rate÷N, durable suspend, Worker shard-routing). **Status: BUILT + INERT at N=1; RAISE-N is OWNER-GATED — needs `DATABASE_URL` + `FABRIC_NUM_SHARDS` + `max_instances` raised in lockstep on real volume.**

### 5.8 Runner-registration broker & Stage-B autoscaler (ADR-0007)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| `RunnerRegistrationBroker` | Mints a short-lived JIT `--ephemeral` runner registration config per runner-lease; App private key never on the box. | `runner_broker.rs:851` | DEFAULT-OFF |
| `GitHubAppBroker` vs `PatBroker` | App-JWT (App-ID/installation/private-key via `RingRsaJwtSigner`) for customer repos, or static `FABRIC_GITHUB_MINT_TOKEN` for first-party. `MockBroker` backs tests. | `runner_broker.rs` (`runner_broker_from_env`) | DEFAULT-OFF |
| Stage-B autoscaler webhook | `POST /webhooks/github` `workflow_job` → 1 runner lease/queued job, cancel on completion; HMAC-authed, drives the audited acquire/cancel path (no admission bypass); bounded job-tracking + delivery-dedup. | `webhook.rs:784`, `github_webhook` | DEFAULT-OFF (mounts only with `FABRIC_AUTOSCALER_WEBHOOK_SECRET`) |
| Runner repo-allowlist authz (Track-C C1) | Bounds `runner:` acquires to allowlisted repos, fail-closed 400. | `leases.rs:440`; `FABRIC_RUNNER_REPO_ALLOWLIST` | DEFAULT-OFF |
| env-0 injection | `inject_runner_jitconfig` (CORELINK_RUNNER_JITCONFIG), `inject_clw_env` (pre-C2c PAT-in-env), `inject_cred_ticket_env` (C2c ticket), `inject_ingest_env`. | `runner_inject.rs`, `envelope_inject.rs:57` | LIVE / DEFAULT-OFF per path |

### 5.9 The moat mint & env-0 cred-ticket (`runner_cas_mint.rs`, `cred_ticket.rs`)

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| Per-job CAS-PAT mint (D-9) | `CasPatMint`/`HttpCasPatMint` mints + revokes a per-job, tenant-scoped `cas:rw` PAT via CoreLink; `MockMint` for tests; weak-secret refusal. | `runner_cas_mint.rs:590`; `CORELINK_RUNNER_MINT_AUTH_KEY/URL` | DEFAULT-OFF (armed FLIP-A, proven live) |
| C2c cred-ticket (PAT-never-on-box) | A single-use lease-scoped ticket injected instead of the raw PAT; redeemed at trusted boot at `{FABRIC_PUBLIC_BASE_URL}/v1/leases/{id}/cas-cred`. `CredTicketSigner` + `StashedCred`. | `cred_ticket.rs`; `cas_cred.rs`; `FABRIC_CRED_TICKET_SECRET` | DEFAULT-OFF (proven live both sides 2026-07-09) |
| Mint-arm boot guard | `validate_mint_arm` fails boot closed if the mint is armed without cred-signer + `CLW_ENDPOINT` + `FABRIC_PUBLIC_BASE_URL` — so a healthy boot proves redemption is wired. | `server.rs:965` | LIVE guard |

### 5.10 Multi-size ladder resolver (`size.rs`)

`SizeSpec`/`SizeRegistry`/`resolve_from_labels` — derives a box size from `corelink-<size>` labels. Single-rung registry resolves every acquire to the default size (byte-identical to single-size today). **Status: INERT (unwired). OWNER-GATED on the size taxonomy input** (rungs + vCPU + CF `instance_type` + $/slot-second per rung, ratified design with server-TL 2026-07-10).

### 5.11 CoreLink integration seams (auth / plans / clw)

`CoreLinkTokenStore`/`UreqIntrospect` — per-request PAT→tenant via the frozen `POST /internal/v1/auth/introspect` (`X-Corelink-Internal-Auth`; only `200 valid:true` admits, 401/5xx→503, never a false 401) (`corelink_auth.rs`; `FABRIC_AUTH_BACKEND=corelink`). `CoreLinkPlanStore` — per-acquire concurrency cap from introspect `max_concurrency` (`corelink_plans.rs`; `max_vcpu_h` ceiling deferred, owner-gated entitlement vector). `clw_drive.rs` — A8 exit-transparency seam (`CLW_INTERNAL_EXIT_CODE=125` = non-zero-not-cached; INERT stub). `ac_pre_lease.rs` — AC pre-lease cache-hit hook (INERT stub, moat WP-7). **Status: corelink auth/plan DEFAULT-OFF (static is default); armed live on fabricd.**

---

## 6. Compute substrate — the `Engine` seam (`crates/corelink-cloud-engine`)

Two backends behind the frozen `Engine` trait; the composition root selects 4-way (`server.rs:1204`, `cloud_exec.rs`).

| Backend | What it is | Evidence | Status |
|---|---|---|---|
| **CloudflareEngine** (default, moat) | HTTP client to the spawn-Worker (`/v1/spawn`,`/v1/status`,`/v1/teardown`,`/v1/exec`); R2-co-located, in-network cache hydration (ADR-0008). Runner-lease (egress, runner-direct) + check-host-lease (check-mode spawn + `exec_captured`). Digest-pinned, fail-closed, redacting Debug. | `cloudflare.rs:330,494,591` | LIVE (armed `CLOUDFLARE_SPAWN_*`) |
| **NorthflankEngine** (fallback) | Job-run lifecycle (create→run→poll→capture→delete); typed `ProviderCapacityError` (graceful degrade); disk-floor boot warn; fail-closed run-status classification (exact `SUCCESS` only). | `northflank.rs:582,403,472` | DEFAULT-OFF (fallback substrate) |
| **HybridBoxProvisioner** (rota B) | Both env present ⇒ runner+check-host→Cloudflare, plain-check→Northflank; routes on `spec.allow_egress` (red-team-blessed discriminator, can't be spoofed); one shared `BoxRegistry`, exec-engine == spawn-engine per lease. | `cloud_exec.rs` (`HybridLeasedExec`, `select_backend`) | DEFAULT-OFF |
| **NoBox** (fail-closed default) | Neither backend ⇒ every exec 503; lease lifecycle still works (S2 fail-closed). | `cloud_exec.rs` (`NoBoxExec`) | LIVE default |
| Spawn-Worker HTTP contract | The frozen seam transcribed each side (`docs/spec/cloudflare-spawn-worker-contract.md` + check-host `cf-check-host-contract.md`); conformance `cloudflare-spawn.json`. | specs; `UreqTransport` (only real net impl) | LIVE (Rust side); TS golden pending |

---

## 7. Cloudflare deploy surface — `deploy/cloudflare*`

### 7.1 `corelink-spawn-worker` (`deploy/cloudflare/`) — spawn + autoscaler. **LIVE in prod.**

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| Bearer auth gate | Constant-time `Bearer <CLOUDFLARE_SPAWN_AUTH_TOKEN>`; empty secret ⇒ deny. | `index.ts:431`, `lib.ts:159` | LIVE |
| `POST /v1/spawn` (runner) | Start `RunnerContainer` with runtime env; assert `image_digest @sha256:` (+ optional `PINNED_IMAGE_DIGEST` match, 409). | `index.ts:1202,1267` | LIVE (PINNED assertion INERT — accepts any pinned ref) |
| `POST /v1/spawn` (check mode) | `mode:"check"` → `CheckHostContainer`; requires `toolchain_digest` (400) + `EXEC_SERVER_AUTH_TOKEN` (503 fail-closed). | `index.ts:1228-1264` | OWNER-GATED (not live-flipped) |
| `POST /v1/exec` (check-exec, rota A) | Relay argv to in-container exec-server (port 8080) via `containerFetch`; non-2xx fail-closed. | `index.ts:1290-1341` | OWNER-GATED |
| `GET /v1/status/{handle}` / `POST /v1/teardown` | Liveness (mode-routed) / idempotent SIGKILL destroy. | `index.ts:1344,1364` | LIVE |
| `POST /v1/egress-cutoff` (O7 kill-switch) | Sever outbound egress at runtime (`setDeniedHosts` + `"*"`) without destroy. | `index.ts:1404-1427` | LIVE (operator) |
| `POST /webhook` autoscaler | `workflow_job.queued` → HMAC-verify → label-gate → claim → mint JIT → spawn ephemeral runner; 202 + background drive. | `index.ts:981-1159`, `lib.ts:170` | DEFAULT-OFF (needs webhook+mint secrets, else 503) |
| Managed-label family gate | Serve bare `corelink` + `corelink-*` minus reserved; subset-gate refuses foreign labels; `self-hosted` passthrough; mints the full requested set. | `lib.ts:661-716` | LIVE |
| Reserved labels | Never mint for `corelink-builder` (persistent builder pool). | `lib.ts:671` | LIVE |
| Spawn / completion dedup | Per-job `spawn:` / `done:` KV claims; redelivery no-op (spawn short-circuits; completion gates only the metric bump). | `lib.ts:88,147`; `index.ts:1078,1142` | LIVE |
| Per-tenant rate limiting | `WEBHOOK_LIMITER` keyed `spawn:<repo>` (per-repo bucket), 30/60s; 429 on shed. | `index.ts:1100`, `wrangler.jsonc:46` | LIVE (fixes cross-tenant DoS) |
| GitHub-App JIT-token mint (vs static) | App creds + `installationId` ⇒ per-installation token (foreign repos); else static `GITHUB_MINT_TOKEN`; App-token KV-cached `ghtok:<id>`, isolated per installation. | `index.ts:449-455`, `github_app.ts:87,132` | DEFAULT-OFF (App path inert w/o `GITHUB_APP_*`) |
| Warm-mint / per-job CAS PAT | `mintCasPat` via `/internal/v1/runner/mint`; server-derives tenant; 403 hard-deny, 5xx fail-open cold. | `lib.ts:220,414`; `MintForbiddenError` | DEFAULT-OFF (needs `CORELINK_RUNNER_MINT_AUTH_KEY`) |
| env-0 cred-ticket + `CredStashDO` | Stash PAT in a per-lease DO, inject `CLW_CRED_TICKET` not `CLW_TOKEN`; multi-use until TTL; wiped at completion; `POST /v1/leases/{id}/cas-cred` redemption (401/410/404). | `index.ts:219-264,1169`; `lib.ts:389` | LIVE (SPAWN_WORKER_PUBLIC_URL set) |
| `ConcurrencySlotsDO` (atomic cap) | Singleton DO holds the authoritative in-flight slot list — per-tenant entitlement THEN fleet cap, atomic single-threaded (replaces the fail-open KV counter). `FLEET_MAX=20`, `COLD_REPO_CAP=8`, `SLOT_TTL=2700`. | `index.ts:274-297,731`; `lib.ts:527-553` | LIVE |
| Container-start retry | 3 attempts, fresh DO handle each, 8s timeout + linear backoff. | `index.ts:528-565` | LIVE |
| Completion actions | Revoke per-job PAT by `pat_id` · teardown container immediately (vs sleepAfter) · wipe cred-stash · optional usage-push. | `index.ts:608-701` | LIVE (billing push DEFAULT-OFF) |
| Re-drive reconciler (GitHub scan) | Cron scans `RECONCILER_REPOS` for queued+labeled+runnerless jobs >90s; clears stale claim, WARM re-drive. | `index.ts:1439-1485`, `lib.ts:718` | DEFAULT-OFF (needs `RECONCILER_REPOS`) |
| Dead-letter orphan retry | `orphan:<jobId>` records warm-recoverable spawn failures (any repo); cron retries WARM, bounded 3 attempts / 30min. | `index.ts:855-875,1498-1580` | LIVE (when armed) |
| `MetricsDO` (golden counters) | Singleton DO holds 12 fixed `COUNTER_NAMES` (see §10 for the list) under one storage key (snapshot = single read, bump = one serialized read-modify-write); bumped at each lifecycle seam; `GET /internal/v1/metrics` snapshot, `METRICS_OBSERVABILITY_KEY`-gated (404 unset). Adding a name + a `bumpMetrics` call at its seam is the whole extension surface. | `metrics.ts:24-40,58`; `index.ts:969` | LIVE (default-off if unbound) |
| `REPO_INSTALLATION_MAP` inject | Inject the known installation-id for first-party repos (plain webhook has none) ⇒ WARM mint without an App webhook. | `index.ts:1130`, `lib.ts:623` | LIVE (first-party) |
| Legacy PAT escape hatch | `ALLOW_LEGACY_PAT_ENV=1` injects raw `CLW_TOKEN`; refused in prod (SPAWN_WORKER_PUBLIC_URL set). | `lib.ts:462-495` | DEFAULT-OFF / prod-refused |

**wrangler config:** 5 DOs — 2 Container classes (`RunnerContainer` = `RUNNER_CONTAINER`, std-4 = 4 vCPU/12 GiB/20 GB disk, `max_instances 20` raised to cover a tenant's default entitlement=20, `sleepAfter 15m` idle-out backstop; `CheckHostContainer` = `CHECK_HOST_CONTAINER`, std-4, `max_instances 4`, `sleepAfter 45m`) + 3 plain-storage DOs (`CRED_STASH`, `METRICS`, `CONCURRENCY_SLOTS`); 5 sqlite migrations `v1..v5` (one per DO class in add-order). KV `RUNNER_JOB_PATS` (multiplexed keyspaces `spawn:`/`done:`/`jtenant:`/`jhandle:`/`orphan:`/`ghtok:`/bare jobId; job→pat_id map TTL'd 7200s as a self-cleaning revoke backstop); cron `* * * * *`; ratelimit `WEBHOOK_LIMITER` (`namespace_id 1001`, 30/60s). Both container images pinned by immutable `@sha256` digest in-config at deploy time (not per-spawn — ADR-0008 wrinkle). R2 binding intentionally omitted (Cache-TL coordination item). Container-image builds are Docker-free (CI pushes to the CF managed registry).

### 7.2 `corelink-fabricd` proxy Worker (`deploy/cloudflare-fabricd/`) — control-plane host. **LIVE / moat proven.**

`FabricdContainer` singleton (std-2 = 2 vCPU, `max_instances 1`, `sleepAfter 1h` — never actually sleeps because the keep-alive cron pings `/v1/health` every minute; in-memory lease state survives between requests, only a restart/redeploy resets it) fronted by a proxy Worker. Features: FNV-1a shard routing + inert-at-N=1 singleton collapse (`shard.ts:20,34`; `index.ts:237`); acquire round-robin placement + lease-op hash routing + scatter-gather list/metrics + §9 trigger body-routing + webhook round-robin (all N>1, inert at N=1); per-request 30s proxy timeout (long-lived routes exempt) (`index.ts:510-572`); keep-alive cron + watchdog self-heal (3 fails ~30s → destroy → fresh boot) + boot-grace/reboot-backoff (`index.ts:743-859,587-644`); pg + vCPU-ceiling conditional arming on `DATABASE_URL`; forwards moat mint / cred-ticket / attested-cost / crash-probe / observability / autoscaler envVars into the container. Live image `5608d67d` (W1-resilience). **Status: LIVE singleton; N>1 OWNER-GATED.**

### 7.3 `corelink-canary` (`deploy/cloudflare-canary/`) — golden-counter alerting. **BUILT, owner-arm-to-deploy.**

Scheduled (every 5 min) monitor: fetches fabricd `/internal/v1/status` + `/v1/health` + spawn `/internal/v1/metrics` (via service bindings to dodge same-zone 404), diffs a KV snapshot, runs a pure rule engine (health-down/surface-unreachable⇒CRIT; mint_failures/spawn_failed⇒CRIT; capacity-503/revoke_failures⇒WARN; 404⇒silent; counter-reset⇒INFO; optional staleness), cooldown-dedups (30 min), and emails via Resend (default-off no-op until `RESEND_API_KEY` set). `rules.ts:107-278`, `notify.ts:58`, `index.ts:136`. **Status: BUILT; DEPLOY OWNER-GATED (KV id + obs keys + Resend key).**

---

## 8. Identity, onboarding & the external-customer path

### 8.1 Direct external `runs-on: corelink` activation (ADR-0007)

The build half is complete; activation is owner/cross-repo gated. Gap taxonomy (`docs/handoff/2026-07-11-*`):

| # | Item | Closed by | Status |
|---|---|---|---|
| 1 | GitHub-App installation-token minting (code) | `github_app.ts` (WP-1) | BUILT (`appJwt`/`installationToken`) |
| 2 | Bind `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY` on spawn-worker | owner `wrangler secret put` | OWNER-GATED |
| 3 | App `Administration:write` + re-approval | owner GitHub UI | OWNER-GATED |
| 4 | `RunnerContainer max_instances` ≥ entitlement | owner (cost decision) | DONE (raised to 20) |
| 5 | Multi-size taxonomy → resolver activation | owner product input | OWNER-GATED |
| 6 | Webhook delivery from external repos → spawn-worker | owner/server-TL routing decision | CROSS-REPO OWNER-GATED |
| 7 | TS↔Rust label split-brain | converge at multi-size activation | INERT |

### 8.2 Identity (ADR-0002)

User-facing identity is the **HuGR account** everywhere; underneath is CoreLink machinery (Clerk sessions, org = tenant, PATs) behind a frozen contract. Runners builds **no** identity machinery — it consumes PAT verification + tenancy from CoreLink. The lease API stays Bearer-PAT. M2 direct GA onboards via the HuGR account; per-tenant caps/fairness/billing key off org = tenant. Signup chain (Clerk → tenant, Stripe → `runners_entitlement`, App-install callback → `installation→tenant` + `repo_allowlist`) is coded server-side (`docs/handoff/2026-07-10-OWNER-GO-LIVE-checklist`). **Status: LIVE server-side; last operator step OWNER-GATED.**

---

## 9. Client tools, front-door integrations & SDKs

| Surface | What it provides | Evidence | Status |
|---|---|---|---|
| `corelink` CLI | `run` (acquire→exec→verify→close; unpinned image → exit 2 before box contact; no lease leak), `smoke` (health/key/fail-closed gates; `--full` = real acquire→cancel), `verify` (offline v2 binding verify). | `crates/corelink-cli/src/{run.rs:114,smoke.rs,binding.rs:39}`; `docs/cli.md` | LIVE |
| GitHub Action (attested-run) | "CoreLink Run" wraps `corelink run --json`; exit-2 hard-fail; `verified!=true` hard-fail; PAT never logged. | `integrations/github-actions/action.yml` | LIVE (binary-availability gated) |
| Buildkite plugin | Mirrors the GH Action (`corelink run --json`, same gates, agent annotate). | `integrations/buildkite/plugin.yml` + `hooks/command` | LIVE (binary-availability gated) |
| `corelink-memoize` composite action | Wraps `clw run` for cache memoization (`--input`/`--env`/tool-version fold into key); **fail-open** (moat absent / clw exit 125 ⇒ COLD run); accepts `CLW_TOKEN` or `CLW_CRED_TICKET`. | `actions/corelink-memoize/action.yml` | LIVE (fail-open by design) |
| Python SDK `corelink_verify` | In-language v2 result-binding verifier, vector-locked; empty/absent sig = loud error. | `sdk/python/corelink_verify/__init__.py` | LIVE |
| TypeScript SDK `@corelink/verify` | Node reference verifier (zero deps, `node:crypto`), vector-locked. | `sdk/typescript/src/index.mjs` | LIVE |
| `corelink-check-exec-server` | In-container check-host exec-server (`POST /exec` on port 8080; process-group kill on timeout; verbatim capture ≤8 MiB/stream; optional `EXEC_SERVER_AUTH_TOKEN` bearer gate). | `crates/corelink-check-exec-server/src/lib.rs:84,162` | LIVE (inside check-host container; live-flip owner-gated) |

---

## 10. Observability & ops

| Feature | What it does | Evidence | Status |
|---|---|---|---|
| Golden counters (fabricd) | Lock-free `Counters` on `/internal/v1/status` — **23 counters**: `leases_acquired`; **8 named rejection reasons** `acquire_rejected_{suspended, invalid_image, bad_request, rate, over_cap, no_plan, compute_ceiling, lease_invalid}` (each a distinct AtomicU64 so a no-plan mis-provision is never read as genuine over-cap saturation, WP-3b); `leases_closed`/`leases_expired`/`leases_crashed`; `provision_capacity_503`; credential `mint_attempts`/`mint_failures`/`revoke_attempts`/`revoke_failures`; `agent_exec_started`/`_done`/`_failed`; operator `load_shed`/`trigger_dedup_hits`/`suspend_actions`. Relaxed atomics, no lock/alloc/control-flow change. Obs-key gated. | `observability.rs:64-128`; `app.rs:1867` | LIVE (DEFAULT-OFF until `FABRIC_OBSERVABILITY_KEY`) |
| Golden counters (spawn-worker) | The **12** durable `MetricsDO` `COUNTER_NAMES`: `webhook_spawn_claimed`/`_deduped`/`webhook_rate_limited`/`webhook_job_completed`; `jit_minted`/`runner_spawned`/`spawn_forbidden`/`spawn_at_ceiling`/`spawn_failed`; `runner_torn_down`/`cas_pat_revoked`/`billing_pushed`. (Distinct from fabricd's — `mint_failures`/`provision_capacity_503`/`revoke_failures` live on the fabricd status surface, **not** here; the canary reads both surfaces. Doc-drift corrected R2.) | `metrics.ts:24-40` | LIVE |
| Boot-time arm-state log | One stderr line per boot naming which ops keys are armed (present/absent only). | `server.rs` (ops-boot-arm #356) | LIVE |
| Cloud-backend boot diagnostic | Names the missing var on a partial cloud config; never claims a backend while silently `NoBox`. | `cloud_exec.rs` (`cloud_backend_status`) | LIVE |
| Quota-headroom monitor | Advisory disk-headroom sweep (logs `QUOTA_HEADROOM_WARNING/EXCEEDED`, adjusts nothing). | `quota_headroom.rs`; `FABRIC_QUOTA_CHECK_INTERVAL_SECS` | DEFAULT-OFF |
| Email canary | See §7.3. | canary worker | BUILT (deploy owner-gated) |
| Runbooks | Northflank deploy + gotchas (`deploy/RUNBOOK.md`), CF fabricd deploy (`deploy/cloudflare-fabricd/README.md`), secret-rotation per-variable (`docs/deploy/secret-rotation-checklist.md`), post-redeploy smoke, corelink-flip, northflank-postgres. | those files | LIVE (docs) |
| Docker-free image build (CI) | CF container images (fabricd, runner, check-host) built + pushed by CI (`.github/workflows/build-cf-container-images.yml`, `build-fabricd-image.yml`); wrangler references pre-built `@sha256` digests. | those workflows; wrangler configs | LIVE |
| CI gate | fmt · clippy `-D warnings` · test · deny · audit · conformance byte-check, on the self-hosted `[self-hosted, mac, corelink-builder]`. Plus `dogfood-smoke`, `corelink-smoke`, `moat-correctness/benchmark`, `canary-smoke` workflows. | `.github/workflows/*` | LIVE |

---

## 11. Integration seams (family)

- **Consumes CoreLink Cache** — CAS/AC/R2 + tenancy + PAT, as a layer, never forked. Auth Bearer PAT (`interop.md §2`). Dedup is **intra-tenant at GA**; cross-tenant is staged (`CAP-DEDUP-CROSS-TENANT`) — the tense-discipline rule (`docs/review/2026-06-09-cross-tenant-dedup-claim.md`).
- **Consumed by hugit** — the execution substrate for memoized CI. Contract frozen from hugit's side (v1.4.0). hugit cutover (`HUGIT_RUNNER_HOST` repoint + pubkey re-pin `b1eba792…`→`faa5b7726…`) is OWNER-GATED.
- **Consumed by Workspaces** (campaign #2) — agent sandboxes / dev-boxes on the same lease/isolate/attest spine (`ws/mod.rs`).
- **Drift tripwire** — the shared `conformance/` set is the byte-identical seam law across all consumers.

**Feature → use-story cross-reference** (`docs/product/USE-SCENARIOS.md`, personas P1–P8, story ids `S<theme>.<n>`). Every feature area here is exercised by at least one story; validators trace them together:

| Feature area (this doc) | Exercised by (persona · theme / story ids) |
|---|---|
| §1 pricing / ceiling / entitlement | P6 finance buyer (S6.1–S6.3); P1 concurrency+ceiling (Theme 1.3: S1.3.1–S1.3.3) |
| §2 front doors (direct / hugit / power-user / workspaces) | P1 (Theme 1.2 S1.2.1–5), P2 (Theme 2.1–2.4), P3 (S3.1–S3.2), P8 (S8.1–S8.3) |
| §3 wire contract & conformance | P7 security auditor (S7.x); P2 attested cost (Theme 2.2) |
| §4 execution core (isolation/fence/boot/teardown/§13) | P4 the AI agent (S4.1–S4.4); P7 red-team (S7.1–S7.7) |
| §5.1–5.2 admission / caps / ceiling / suspend | P1 (Theme 1.3), P5 operator capacity (Theme 5.2 S5.2.1–3) |
| §5.4 attestation / result-binding / attested cost | P2 (Theme 2.2 S2.2.1–2), P8 verify (S8.2–S8.3) |
| §5.6 billing / metering / GDPR erasure | P5 billing (Theme 5.3 S5.3.1–3), P6 |
| §5.8–5.9 runner broker / autoscaler / moat mint / cred-ticket | P1 onboarding (Theme 1.1 S1.1.1–4), P5 provisioning (Theme 5.1) |
| §6–7 substrate / CF deploy / canary | P5 incident & deploy ops (Theme 5.4 S5.4.1–4) |
| §8 identity & external-customer path | P1 (Theme 1.1), P5 (Theme 5.1) |
| §9 CLI / SDKs / GH-Action / check-exec-server | P8 power-user (S8.1–S8.3); P2 memoized CI (Theme 2.1); P4 |
| §2.3 agent-exec seam | P2 (Theme 2.3 S2.3.1–2), P4 |

---

## 12. Roadmap status snapshot (SHIPPED-LIVE vs BUILT-INERT vs PLANNED)

| Capability | Status |
|---|---|
| Execution core (lease/isolate/teardown/boot/concurrency/expiry/recovery/shim/fence/X4/§13) | **SHIPPED-LIVE** (182+ acceptance tests) |
| Cloudflare moat (native check-host exec + per-job CAS-PAT mint + attested cost) | **SHIPPED-LIVE** (proven 2026-07-09) |
| fabricd control plane (RunnerLease API, admission-reject, caps, attestation, reaper, envelope) | **SHIPPED-LIVE** (singleton) |
| Direct dogfood/first-party runner fleet (`runs-on: corelink`) | **SHIPPED-LIVE** |
| pg durable ledger + vCPU-h ceiling + durable billing export | **BUILT, OWNER-GATED** (needs `DATABASE_URL`) |
| Multi-instance N>1 sharding | **BUILT-INERT, OWNER-GATED** (RAISE-N on volume) |
| Queued fair admission (ADR-0005) | **BUILT, DEFAULT-OFF** |
| Durable cross-instance admission queue | **BUILT-INERT** |
| Direct **external** customer path (App-token mint code) | **BUILT; activation OWNER-GATED** (secrets + webhook routing) |
| Multi-size ladder | **BUILT-INERT, OWNER-GATED** (taxonomy input) |
| Check-host live-flip (rota A to prod tenants) | **BUILT + deployed image; OWNER-GATED** (real toolchain snapshot + go) |
| CoreLink slot billing push | **BUILT, DEFAULT-OFF** (owner: leave off for launch) |
| Email canary alerting | **BUILT; DEPLOY OWNER-GATED** |
| Northflank substrate | **BUILT (fallback), not the live substrate** |
| Firecracker own-metal (FC1–FC5) | **PLANNED** (frozen upgrade path behind the Engine seam; blocked on KVM buy) |
| Cross-tenant dedup | **STAGED post-GA** (`CAP-DEDUP-CROSS-TENANT`, cache-side) |
| Actions-YAML shim execution / GH equivalence lane | **v0-SIMULATED / Partial** |
| Workspaces SKUs (campaign #2), GPU runners, multi-region (M3/M4) | **PLANNED** |
| Anti-abuse sustained-pin/mining detection | **PLANNED** (rate rail LIVE) |
| GDPR Art.17 erasure of `billing_events` | **PLANNED** (tracked) |

---

## 13. Config-knob index (env vars / secrets / wrangler vars)

Grouped by function; **secret** = must be `wrangler secret put` / secret-store, never in a config file. Full rotation semantics in `docs/deploy/secret-rotation-checklist.md`.

**fabricd — bind/keys/auth:** `FABRIC_BIND_ADDR` · `FABRIC_SIGNING_KEY`(secret, seeds the ingest key) · `FABRIC_DEV_UNSAFE` (loopback-only) · `FABRIC_AUTH_BACKEND` (static\|corelink) · `FABRIC_PAT`(secret)/`FABRIC_TENANT` · `CORELINK_INTROSPECT_URL` · `FABRIC_INTROSPECT_AUTH_KEY`(secret) · `FABRIC_INTROSPECT_TIMEOUT_MS`.
**Plans/caps/compute:** `FABRIC_TENANT_MAX_CONCURRENCY` · `FABRIC_TENANT_RATE_PER_MIN` · `FABRIC_RUNNER_REPO_ALLOWLIST` · `FABRIC_RUNNER_VCPU` (arms vCPU-h ceiling, needs pg) · `FABRIC_TENANT_MAX_VCPU_H`.
**Ledger:** `FABRIC_LEDGER_BACKEND` (memory\|pg) · `DATABASE_URL`(secret) · `FABRIC_LEDGER_POOL_SIZE` · `FABRIC_PG_TLS` (disable\|require).
**Admission/load-shed:** `FABRIC_ADMISSION_MODE` (reject\|queue) · `FABRIC_ADMISSION_QUEUE_WAIT_MS` · `_TICK_MS` · `_TICK_SLOTS` · `_PARK_CAP` · `FABRIC_MAX_INFLIGHT_REQUESTS` · `FABRIC_CLOSE_ACK_MAX_INFLIGHT` · `FABRIC_PROVISION_MAX_INFLIGHT`.
**Reaper:** `FABRIC_REAP_INTERVAL_SECS` · `FABRIC_PENDING_MAX_AGE_SECS` · `FABRIC_CRASH_PROBE_INTERVAL_SECS`.
**Ops surfaces (secrets → 404 unset):** `FABRIC_OBSERVABILITY_KEY` · `FABRIC_ADMIN_KEY`. Headers: `X-Corelink-Internal-Auth`, `X-Fabricd-Shard`, `X-Fabricd-Num-Shards`.
**Cloud substrate:** `NORTHFLANK_API_TOKEN`(secret)+`NORTHFLANK_PROJECT_ID` · `NORTHFLANK_TEAM_ID`/`_BASE_URL`/`_DEPLOYMENT_PLAN`/`_RUNNER_DEPLOYMENT_PLAN`/`_RUNNER_EPHEMERAL_STORAGE_MB`/`_PROJECT_DISK_ALLOWANCE_MIB` · `NORTHFLANK_HTTP_TIMEOUT_MS` · `CLOUDFLARE_SPAWN_WORKER_URL`+`CLOUDFLARE_SPAWN_AUTH_TOKEN`(secret) · `CLOUDFLARE_RUNNER_LABELS`/`_EXPIRY_MS`/`_RUNNER_STORAGE_MB` · `FABRIC_MOCK_EXEC` (dev-unsafe interlock).
**Runner fleet (ADR-0007):** `FABRIC_GITHUB_APP_ID`/`_APP_INSTALLATION_ID`/`_APP_PRIVATE_KEY(_B64)`(secret)/`_API_BASE`/`_RUNNER_GROUP_ID`/`_RUNNER_NAME` · `FABRIC_GITHUB_MINT_TOKEN`(secret) · autoscaler `FABRIC_AUTOSCALER_WEBHOOK_SECRET`(secret)/`_PAT`(secret)/`_RUNNER_IMAGE`/`_LABELS`/`_TMP_ROOT`/`_EXPIRY_MS`/`_REPO_ALLOWLIST`/`_MAX_TRACKED_JOBS`.
**Moat mint / CAS creds / env-0:** `CORELINK_RUNNER_MINT_AUTH_KEY`(secret)+`CORELINK_RUNNER_MINT_URL` · `FABRIC_CRED_TICKET_SECRET`(secret) · `FABRIC_PUBLIC_BASE_URL` · injected box env `CLW_ENDPOINT`/`_TENANT`/`_TOKEN`/`_REF_DOMAIN`/`_CRED_TICKET`/`_LEASE_ID`/`_FABRIC_ENDPOINT`, `CORELINK_RUNNER_JITCONFIG`, `TOOLCHAIN_DIGEST`.
**Envelope §13:** `CORELINK_ENVELOPE_INGEST_URL`/`_CREDENTIAL`(secret)/`_BASE_URL` · `FABRIC_EMIT_INTENT_METRICS_SIG`.
**Billing:** `FABRIC_BILLING_EXPORT_INTERVAL_SECS` (needs pg) · `BILLING_INGEST_URL`+`BILLING_INGEST_AUTH_KEY`(secret)+`BILLING_REGION` · `FABRIC_BILLING_PUSH_INTERVAL_SECS`.
**Multi-instance:** `FABRIC_NUM_SHARDS`.
**spawn-worker (Worker secrets):** `CLOUDFLARE_SPAWN_AUTH_TOKEN` · `EXEC_SERVER_AUTH_TOKEN` (check-mode required) · `GITHUB_WEBHOOK_SECRET` · `GITHUB_MINT_TOKEN` · `GITHUB_APP_ID`/`GITHUB_APP_PRIVATE_KEY` · `CORELINK_RUNNER_MINT_AUTH_KEY` · `BILLING_INGEST_AUTH_KEY` · `METRICS_OBSERVABILITY_KEY` · `ALLOW_LEGACY_PAT_ENV`. Vars: `CLW_TENANT`/`CLW_ENDPOINT`/`CORELINK_MINT_URL`/`RECONCILER_REPOS`/`REPO_INSTALLATION_MAP`/`SPAWN_WORKER_PUBLIC_URL`/`PINNED_IMAGE_DIGEST`/`AUTOSCALER_LABEL`.
**check-exec-server:** `TOOLCHAIN_DIR` · `EXEC_SERVER_AUTH_TOKEN`.
**CLI:** `CORELINK_URL` · `CORELINK_PAT`. **runner box (SSH interim):** `HUGIT_RUNNER_HOST` · `HUGIT_RUNNER_KNOWN_HOSTS`. **shim:** `HUGIT_GH_TEST_REPO`.
**canary (secrets):** `FABRIC_OBSERVABILITY_KEY` · `METRICS_OBSERVABILITY_KEY` · `RESEND_API_KEY`. Vars: `FABRIC_STATUS_URL`/`FABRIC_HEALTH_URL`/`SPAWN_METRICS_URL`, `ALERT_COOLDOWN_MINUTES`(30)/`STALENESS_HOURS`(0=off)/`BUSINESS_HOURS_UTC`/`ALERT_EMAIL_TO`/`ALERT_EMAIL_FROM`. Bindings: KV `CANARY_KV` (snapshot+cooldown state), **service bindings** `FABRICD_SVC`→corelink-fabricd + `SPAWN_SVC`→corelink-spawn-worker (Worker→Worker direct calls — a public `fetch()` to a sibling `*.workers.dev` on the same account mis-routes to 404, observed live 2026-07-17; the binding routes directly to the gated `/internal/v1/*` surfaces).

**Small config-index footnotes (R2 grep-verify):** `NORTHFLANK_EPHEMERAL_STORAGE_MB` (non-runner) is **not** an env read — only `NORTHFLANK_RUNNER_EPHEMERAL_STORAGE_MB` is; the base workload storage is a code default (1024 MiB), so the index above is complete. `FABRIC_GITHUB_APP_PRIVATE_KEY_B64` (the base64 variant of `_APP_PRIVATE_KEY`) is a real read on the fabricd proxy.

---

## 14. Deliberate exclusions & ambiguities (for the validation campaign)

**Excluded (out of Runners' scope by charter, whitepaper §12):** check *semantics* (what a check means / whether it passes), the memo *key* (hugit owns the formula; the fabric only serves misses), landing/merge, and provenance (hugit's); the cache itself (CoreLink's); per-minute billing exposure. These are not Runners features and are not inventoried. **Not to be confused with excluded:** the check-*host* execution box, the in-container `corelink-check-exec-server`, native CF check-exec, and the `agent-exec` seam **ARE** Runners features and are inventoried (§9, §7.1, §5.1, §6, §2.3) — Runners executes the check and mints the attestation; hugit decides what the result means. The line is *execution (ours) vs semantics (hugit's)*, not "check = not ours."

**Not inventoried in exhaustive per-symbol detail (surveyed by symbol + doc-comment, not line-by-line):** internal branch bodies of `corelink_plans.rs`, `quota_headroom.rs` tick math, `global_gate.rs` wiring, and the `pg_ledger`/`pg_queue` SQL — line numbers there point to the primary struct/fn.

**Live-vs-inert ambiguities a validator should resolve on live config, not this doc:**
1. **Check-host / rota-A** — the CF check-host image is *built + deployed* but the live-flip to prod tenants is owner-gated (real toolchain snapshot + go). "Deployed image" ≠ "serving check traffic."
2. **The moat mint** — proven live on a hydrating check-host acquire; a plain 200 does NOT prove a mint (only a mint-armed 503→200 or a server-side mint log does). Validate the transition, not the status code.
3. **`PINNED_IMAGE_DIGEST`** — the spawn-worker accepts any `@sha256:` ref today (the assertion is inert); arm at runner-fleet activation.
4. **TS↔Rust label split-brain** — `matchManagedLabels` (TS family/prefix) vs the Rust webhook subset-gate (explicit list) don't mirror; dormant while the fabricd autoscaler is unarmed, must converge before multi-size.
5. **Stabilization wave (2026-07-16)** — **R2 code-verified as LANDED, not proposal-only:** W1 resilience (`FABRIC_CRASH_PROBE_INTERVAL_SECS=20` + pg-gated `FABRIC_BILLING_EXPORT_INTERVAL_SECS=60` now set in `deploy/cloudflare-fabricd/wrangler.jsonc` vars + forwarded into the container; live image `5608d67d`), W3 credential lifecycle, and W7 (`ConcurrencySlotsDO` atomic slot with `FLEET_MAX 20`/`COLD_REPO_CAP 8`/`SLOT_TTL 2700`, `spawn:<repo>` `WEBHOOK_LIMITER`, `github_app.ts` App-token mint) are all present in current source. The `RunnerContainer max_instances` was raised 6→20 (`wrangler.jsonc`) so the physical class cap meets the per-tenant entitlement (was a silent multi-minute spawn-thrash root cause). Residual validator step: confirm the *deployed* worker/image versions match source (source ≠ deployed is the only remaining unknown, resolvable only on live config).
6. **G2 metadata/link-local egress** — NOT closed on the CF path by the `deniedHosts` mechanism (no CIDR match, raw-socket bypass); needs platform-network-layer filtering.
7. **`docs/spec/corelink-fabric-stub.md`** is a deliberate `⟨FILL⟩` skeleton (the CoreLink-techlead side), not a feature spec — not inventoried as capabilities.
