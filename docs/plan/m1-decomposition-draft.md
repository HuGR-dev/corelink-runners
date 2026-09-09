# M1 — the production fabric: decomposition DRAFT (retired historical plan)

> ⚠️ **hugit / campaign #3 is DISCONTINUED (owner-confirmed 2026-07).** This draft was
> authored when hugit was the intended anchor consumer, so it frames the M1 done-gate and
> wire seam in "hugit" terms throughout. Read those as HISTORICAL: the mechanisms
> (RunnerLease/FenceManifest/attestation/§13 envelope/conformance vectors) are the fabric's
> own and live; the fabric is now direct-to-ICP. The seam is no longer "frozen from an
> external hugit side" — it is the fabric's own wire/envelope contract.
>
> **RETIRED HISTORICAL DRAFT — NOT AN ACTIVE PLAN OR RELEASE GATE.** ADR-0014
> supersedes its external-consumer framing. The dated decomposition and citations
> remain for provenance; current status and acceptance criteria live in
> `docs/ROADMAP.md` and the CoreLink API/feature docs.
>
> status: historical proposal · author: techlead session 2026-06-12 · not an active gate
>
> Inputs: `docs/whitepaper/corelink-runners-v1.md` (canonical) · `docs/product/product.md` ·
> `docs/ROADMAP.md` (M1 epic list, agreed) · `docs/spec/hugit-integration-contract.md` v1.2.0
> (historical wire provenance) · `docs/spec/corelink-fabric-stub.md` (retired stub) ·
> `docs/interop.md` (retired M0 seam map) ·
> code on `integ/seed-runner` post `v0.1.0-seed`. Decided principles are respected, not
> reargued: concurrency pricing never per-minute · cache-warm boot · fail-closed isolation ·
> M1 replaces the transport, not the contract · identity via the HuGR account (ADR-0002) ·
> types transcribed, never imported.

---

## 1. The bar (verbatim-cited — what "M1 done" means)

Whitepaper §11 (canonical):

> **M1 — MVP fabric.** Single region, 2/4-vCPU ephemeral microVMs, cache-warm boot, the
> exec/lease/attestation contract green against hugit's client, per-tenant caps + fairness.
> **Replaces hugit's interim transport** (`hugit-runner-01`, which lights live CI at P2)
> **with the production fabric** — same contract, production grade; the highest-value first
> deliverable, because it makes the forge's execution substrate multi-tenant and sellable.

Roadmap restatement (`docs/ROADMAP.md` M1):

> multi-tenant behind the same `RunnerLease` semantics — caps enforced before load, p95
> fairness, measurable non-interference, byte-determinism, signed attestation. "M1 replaces
> the transport, not the contract."

**The mechanized done-gate is the fabric acceptance suite** (historical hugit framing, hugit
discontinued; contract §11, run-not-skip when the
fabric endpoint is set): B2b byte-identity · C3 warm<cold + cache-down fail-closed ·
C2a/C2b/C9 lease lifecycle/crash/expiry/ws · C5b secrets red-team · X11 mid-op broker fault
· X6/X10 non-interference within bound. M1 is done when those flip green against this
fabric's endpoint — plus the §13 obligations on the production job-close path (contract
§13.1–§13.3, which bind "when the runner product hosts agent-driven execution", i.e. M1).

What M1 is **not** (whitepaper §12 non-goals): check semantics, the memo key, landing,
provenance (hugit's); the cache itself (CoreLink's); per-minute billing (never).

---

## 2. Epics → candidate work-packages

Six epics per the agreed roadmap. Prefixes: CP (control plane) · API (lease API) ·
BIL (billing) · FC (Firecracker) · ENV (§13 wiring) · ATT (attestation + secrets seam).
Sizing: S ≤ 1 agent-day equivalent · M ≈ 2–3 · L ≥ 4 or research-heavy.

### Epic 1 — Multi-tenant control plane

**CP1 — Durable lease ledger + authoritative state machine.** The control-plane source of
truth for `Pending → Held → (Released | Expired | Crashed)` — exactly the contract §1
states, no invented intermediates, surviving control-plane restart.
- Seams: `corelink-runners-contracts/src/runner_lease.rs:RunnerState` / `RunnerLease`
  (frozen, transcribed); `crates/corelink-runner/src/expiry/mod.rs:is_expired` /
  `enforce_expiry`; `crates/corelink-runner/src/recovery/mod.rs:detect_and_recover` /
  `LostJob::is_surfaced_not_green`.
- Acceptance: `ledger_states_are_exactly_the_contract_five` ·
  `expiry_kills_and_marks_expired_no_partial_result` ·
  `crash_marks_crashed_no_duplicate_result` · `ledger_survives_process_restart` ·
  `held_lease_with_dead_box_recovers_surfaced_not_green`.
- Size: **M**. Deps: CF0 (§3 freeze: ledger types + storage choice, open decision #3).

**CP2 — Per-tenant caps, enforced before load.** Preventive admission: concurrency cap +
request-rate ceiling per tenant, checked at acquire time, before any box/VM is touched
(contract §6, hugit X10⑤ "set before load").
- Seams: CP1 ledger; tenant key derived from PAT (API1); cap source of truth = plan tier
  (BIL2).
- Acceptance: `acquire_over_cap_rejected_before_any_spawn` ·
  `rate_ceiling_rejects_before_load` · `cap_zero_admits_nothing` ·
  `cap_is_preventive_under_burst_not_reactive` · `caps_are_per_tenant_not_global`.
- Size: **S/M**. Deps: CP1, API1 (tenant identity), BIL2 (cap values; stubbable).

**CP3 — Fair multi-tenant scheduler.** Per-tenant queues + placement over the existing
engine seam; generalizes the seeded single-box batch scheduler into a long-running,
multi-tenant dispatcher with a p95-wait fairness bound (contract §6, hugit C7).
- Seams: `crates/corelink-runner/src/concurrency/mod.rs:Scheduler` / `run_batch` /
  `BatchReport.peak_concurrency` (the proven ≥8-parallel core, reused not rewritten);
  `crates/corelink-runner/src/isolation.rs:Engine`; `crates/corelink-runner/src/ws/mod.rs:DedupSpawner`
  (the coalescing pattern for identical concurrent work).
- Acceptance: `fairness_p95_wait_bounded_under_two_tenant_contention` ·
  `no_tenant_starved_under_storm` · `scheduler_drives_engine_seam_unchanged` ·
  `teardown_forensic_clean_after_multi_tenant_batch` (reuses
  `teardown.rs:ForensicReport::is_clean`).
- Size: **L**. Deps: CP1, CP2.

**CP4 — Non-interference measurement surface.** Expose per-tenant latency/wait metrics so
hugit's X6/X10 "other-tenant latency unmoved under hugit load" is provable, not assumed
(contract §6).
- Seams: CP3 scheduler metrics; read-only metrics endpoint on API1.
- Acceptance: `other_tenant_p95_unmoved_under_storm_tenant_load` ·
  `metrics_surface_reports_per_tenant_wait_histogram` ·
  `interference_measurement_is_tenant_scoped_no_cross_leak`.
- Size: **S**. Deps: CP3, API1.

### Epic 2 — Public lease API

**API1 — API skeleton + PAT auth (CAS/AC pattern).** HTTP service, Bearer PAT auth mapping
token → tenant (same scheme as the cache product, interop §2); fail-closed when the token
store is unreachable.
- Seams: new `crates/corelink-fabric-api` (proposed); tenant key feeds CP2; auth pattern
  inherited from CoreLink Cache, consumed not forked.
- Acceptance: `missing_pat_is_401` · `valid_pat_maps_to_tenant` ·
  `cross_tenant_pat_cannot_touch_other_lease_404_not_403` (no existence oracle) ·
  `token_store_down_fails_closed_503_never_open`.
- Size: **M**. Deps: CF0 (endpoint/DTO/error-vocabulary freeze, §3).

**API2 — acquire / status / cancel.** The lease lifecycle over REST: acquire returns a
`RunnerLease`-conformant body (lease id, exec endpoint, deadline — contract §1); status
mirrors the CP1 ledger exactly; cancel releases + tears down.
- Seams: `runner_lease.rs:RunnerLease` JSON (conformance vectors
  `conformance/RunnerLease.json` are the wire oracle); `lease.rs:ContainerSpec::from_lease`
  (validation reused: pinned image, isolated net_policy, tmp_root injection guard);
  CP1/CP2.
- Acceptance: `acquire_returns_runnerlease_byte_conformant_to_vector` ·
  `acquire_unpinned_image_rejected_400_before_box_contact` ·
  `status_reflects_ledger_exactly_no_invented_states` ·
  `cancel_releases_and_forensic_teardown_clean` · `acquire_over_cap_429_preventive`.
- Size: **M**. Deps: API1, CP1, CP2.

**API3 — Exec + result path (`CheckDef` → `CheckResult`).** The transport replacement
itself: execute the check inside the leased box/VM and return the `CheckResult` (exit,
canonical bytes, duration, output content-digest) — retiring tenant-facing SSH
(`lease.rs:SshBox` stays a dev oracle only, open decision #8).
- Seams: `lease.rs:BoxExec` (the transport seam built to be swapped); `isolation.rs:Engine`
  — **gap: `Engine::exec` returns `Result<Option<i32>>` only; the result path needs
  captured output bytes** (today only `ws/mod.rs:run_remote` captures stdout, via raw
  `BoxExec`). Needs the CF0 Engine-v2 freeze (open decision #1). `CheckDef`/`CheckResult`
  must be transcribed into `corelink-runners-contracts` with conformance vectors (they are
  named as frozen IDL in contract §0 but are not yet in this repo's contracts crate).
- Acceptance: `exec_returns_checkresult_with_content_digest` ·
  `byte_identity_same_checkdef_same_digest_two_runners` (hugit B2b target) ·
  `result_stored_to_ac_only_after_clean_exit` · `expired_job_stores_nothing_ever` ·
  `cache_down_explicit_error_never_silent_cold_result`.
- Size: **L**. Deps: API2, CF0 (Engine v2 + CheckDef/CheckResult transcription); runs on
  DockerEngine first, FC1 slots in behind the same seam.

**API4 — QueueApi trigger endpoint.** The §9 seam: hugit's landing queue triggers
execution of an uncached check on demand.
- Seams: API2/API3; `QueueApi` shape transcribed from hugit-contracts (same protocol as
  CheckDef/CheckResult).
- Acceptance: `queue_trigger_executes_uncached_check` · `trigger_is_tenant_scoped_and_capped`
  · `trigger_idempotent_on_duplicate_delivery`.
- Size: **S/M**. Deps: API3.

### Epic 3 — Billing (concurrency slots, never minutes)

**BIL1 — Slot model + COGS meters.** Define the parallel-runner slot as the billing unit;
meter slot-occupancy and internal COGS counters (core-seconds, cache I/O) strictly
underneath — never surfaced as a customer meter (whitepaper §7; contract §10).
- Seams: CP1 ledger (slot occupancy = held leases); `envelope/collector.rs:MetricsCollector`
  / `IntentMetrics.cost_usd_micros` (the exact-integer COGS pattern, contract §13.1 —
  "NEVER a billable meter; for trust/audit").
- Acceptance: `billing_unit_is_slot_never_minutes` · `memoized_hit_consumes_no_slot_bills_zero`
  · `cogs_meters_internal_only_absent_from_tenant_api` ·
  `slot_occupancy_reconciles_with_lease_ledger`.
- Size: **M**. Deps: CP1.

**BIL2 — Plan → cap enforcement (org = tenant).** Map plan tier (product §5 ladder) to the
per-tenant concurrency cap CP2 enforces; tenant keyed by org per ADR-0002 (mechanism only —
self-serve onboarding is M2).
- Seams: CP2 (cap source of truth); `docs/adr/0002-hugr-identity.md` (org = tenant keys
  caps/fairness/billing).
- Acceptance: `plan_tier_sets_cap_exactly` · `plan_change_takes_effect_without_restart` ·
  `unknown_tenant_has_zero_cap_fail_closed`.
- Size: **S/M**. Deps: CP2, BIL1.

**BIL3 — Invoicing export.** Slot-plan invoice lines (flat) + add-on runners; Stripe
export. Candidate **M2-deferral** — see open decision #4; included so the owner decides
explicitly rather than by omission.
- Acceptance: `invoice_lines_are_flat_slot_plans_only` · `no_minutes_appear_on_any_invoice`
  · `hugit_tenant_marked_internal_cogs_no_invoice` (one product, one bill).
- Size: **S**. Deps: BIL1, BIL2.

### Epic 4 — Firecracker isolation engine

**FC1 — `FirecrackerEngine` behind the frozen `Engine` seam.** microVM-per-job (jailer,
rootfs from pinned image) implementing spawn/probe/exec/is_alive — the upgrade path the
seed explicitly built for ("engine-agnostic on purpose", `lease.rs:ContainerSpec` docs;
"the Firecracker upgrade adds another against the same contract", `isolation.rs:Engine`).
- Seams: `isolation.rs:Engine` (+ Engine v2 per CF0) · `isolation.rs:IsolationProbe`
  (engine-independent contract) · `lease.rs:ContainerSpec` (reused unchanged) ·
  `teardown.rs:teardown` (same forensic oracle).
- Acceptance: `firecracker_engine_passes_c2a_suite_unchanged` ·
  `probe_reports_fully_isolated_in_microvm` · `one_lease_one_microvm_never_reused` ·
  `teardown_forensic_clean_after_microvm` · `microvm_boot_within_slo_budget`.
- Size: **L**. Deps: CF0 (Engine v2); a Linux/KVM host + CI runner (open decision #5).
  Parallel-safe with CP/API epics (seam already frozen).

**FC2 — Verify-before-spawn on the microVM surface (X4 parity).** The supply-chain floor
ported: pinned `sha256:` digest re-parsed and integrity-verified before any VM exists.
- Seams: `pin.rs:PinnedImageRef::{parse,verify_on_box}` · `x4/pin.rs:VerifiedSpawn` /
  `GuardedSpawn::rejected_before_spawn` (the ordering oracle).
- Acceptance: `unpinned_image_never_reaches_microvm_spawn` ·
  `tampered_digest_rejected_no_vm_created` · `verify_ordering_parse_then_integrity_then_spawn`.
- Size: **S/M**. Deps: FC1.

**FC3 — Cache-warm boot off real CAS/AC.** The core technical bet (fabric stub A3): a
microVM boots with the job's working set local before the first instruction; warm<cold
measurable; cache-down fail-closed (contract §2).
- Seams: `boot/mod.rs:BootCas` / `HydrationPlan` / `hydrate` / `cold_hydrate` /
  `BootError` (the seeded mechanism + fail-closed error vocabulary) · CoreLink Cache
  CAS/AC client (consumed, never forked — interop §2).
- Acceptance: `warm_boot_inputs_local_before_first_instruction` ·
  `warm_measurably_faster_than_cold` (hugit C3) ·
  `cache_unreachable_boot_refused_explicit_error` ·
  `no_silent_cold_result_dressed_as_warm` · `hydration_is_content_addressed_no_adhoc_fetch`
  (contract §8).
- Size: **L**. Deps: FC1; CAS endpoint + tenant PAT.

**FC4 — Fence materialization + enforcement inside the microVM (C5a parity).** Sparse
materialization of exactly the fence's path_set; outside-fence access is ENOENT; red-team
vectors hold (`..` escape, absolute-path injection, `srcfoo` vs `src/` collision —
contract §4).
- Seams: `materialize/mod.rs:materialize_sparse` / `select_in_fence` ·
  `enforce/mod.rs:classify` / `probe_outside_enoent` / `EnoentProof` ·
  `redteam.rs` vectors · `fence_manifest.rs:FenceManifest` (frozen).
- Acceptance: `fence_outside_path_is_enoent_in_microvm` ·
  `dotdot_escape_denied` · `absolute_path_injection_denied` ·
  `prefix_collision_srcfoo_vs_src_denied` · `materialized_view_is_exactly_path_set`.
- Size: **M**. Deps: FC1.

**FC5 — Determinism knobs.** The fabric controls or surfaces clock, RNG, locale, paths,
parallelism, artifact timestamps; injects no per-boot values (contract §3 — "the single
most load-bearing requirement"; whitepaper §5.2 "determinism is sacred").
- Seams: FC1 VM config (env scrubbing, fixed clock policy) · `ws/mod.rs:WorkspaceResult::result_identity`
  (the local ≡ remote identity oracle pattern).
- Acceptance: `same_checkdef_byte_identical_across_two_boots` ·
  `no_per_boot_value_in_job_env` · `artifact_timestamps_normalized` ·
  `locale_and_path_fixed_across_vms` · `parallelism_knob_surfaced_not_random`.
- Size: **M/L**. Deps: FC1, FC3 (inputs by content), API3 (digest comparison harness).

### Epic 5 — §13 wiring (envelope on the production path)

The mechanism exists and is green (`envelope/`, acceptance `acceptance_s13.rs`); the
roadmap scope-note says exactly this: "wiring it into the production lease/API path is M1
work."

**ENV1 — Authenticated hook transport.** Expose `CaptureHook`'s two surfaces (raw events +
`TurnMeta`) over a fabric stream endpoint, Bearer-PAT-gated (contract §13.2: "the hook
point is authenticated (Bearer PAT, same as the CAS/AC boundary)"); bounded in-flight
forwarding only, never durable (§13.3).
- Seams: `envelope/hook.rs:CaptureHook` / `Subscriber` / `EnvelopeConfig` / `TurnMeta`;
  API1 auth.
- Acceptance: `subscribe_requires_bearer_pat` · `wrong_tenant_credential_cannot_subscribe`
  · `overflow_flags_capture_incomplete_never_silent` ·
  `no_durable_write_anywhere_on_forward_path` · `hook_opens_at_acquire_closes_at_job_close`.
- Size: **M**. Deps: API1; ENV semantics already frozen in code.

**ENV2 — Job-close + metrics on the production release path.** Wire `JobClose`'s
finalize-once → `CloseSignal` → ack-window → fail-closed `CloseOutcome` into the real
lease release; metrics travel in the same atomic step as the `CheckResult` (contract §13.1
delivery rule — "never optional when the job succeeded").
- Seams: `envelope/close.rs:JobClose::{close,close_abnormal,ack}` / `CloseSignal` ·
  `envelope/mod.rs:CloseOutcome` (metrics a required field — unrepresentable without) ·
  `envelope/collector.rs:MetricsCollector::finalize` · API3 result path · CP1 (lease may
  not reach `Released` before close completes).
- Acceptance: `checkresult_carries_intentmetrics_atomically` ·
  `ack_timeout_closes_lease_anyway_with_capture_incomplete_flag` ·
  `abnormal_close_exactly_once_expiry_and_crash` ·
  `cache_token_split_present_for_agent_jobs` (§13.1 mandatory split) ·
  `lease_not_released_before_close_signal_published`.
- Size: **M**. Deps: ENV1, API3, CP1.

**ENV3 — Cross-repo `IntentMetrics` conformance vector.** Close the open P1 item: §13.4
requires the vector byte-identical in both repos; blocked on a hugit-side PR (hugit
techlead) + mirror here.
- Seams: `corelink-runners-contracts/src/intent_metrics.rs:IntentMetrics` /
  `CONTEXT_ENVELOPE_SCHEMA_VERSION` ("1.2.0") · `conformance/manifest.sha256`.
- Acceptance: `intentmetrics_vector_pinned_in_manifest` ·
  `vector_byte_identical_to_hugit_side` · `tampered_vector_breaks_golden`.
- Size: **S**. Deps: **external** (hugit techlead PR — schedule early, off the critical
  path).

### Epic 6 — Attestation chain (§7) + secrets-broker wire seam (§5)

**ATT1 — Signed execution attestation.** Per execution, sign {image digest, resolved
inputs, result hash}; a result without a valid attestation is rejected by hugit, so
emission is mandatory (contract §7).
- Seams: `pin.rs:PinnedImageRef::digest_hex` (image identity) · API3 result digest · FC3
  resolved inputs · `AttestationChain` shape **to be transcribed** from hugit-contracts
  with a conformance vector (open decision #2: signature scheme + key custody).
- Acceptance: `every_execution_emits_signed_attestation` ·
  `attestation_covers_image_inputs_result_exactly` ·
  `attestation_verifies_against_published_fabric_key` ·
  `no_attestation_no_result_fail_closed`.
- Size: **M/L**. Deps: CF0 (AttestationChain transcription — a §12-protocol contract
  event with hugit), API3, FC2.

**ATT2 — Attestation at the API surface.** The attestation travels with the `CheckResult`
on the job-close wire call (same atomic step as §13.1 metrics — one close payload).
- Seams: API3 + ENV2 close path; `AttestationChain` wire shape.
- Acceptance: `checkresult_response_carries_attestation` ·
  `attestation_and_metrics_share_one_atomic_close` ·
  `hugit_client_accepts_fabric_attestation` (against hugit's X8 verifier).
- Size: **S/M**. Deps: ATT1, ENV2.

**ATT3 — Secrets-broker wire seam.** The fabric-side half of §5: a delivery channel
through which a brokered secret reaches the job without landing on image/disk/argv, plus
the credential-scan attestation (`env=0, proc=0, disk=0`, fail-closed on unparseable
scan). C5b broker *logic* stays forge-side (roadmap epic text); the fabric hosts the
channel and the guarantee.
- Seams: `shim/broker.rs:Broker` trait / `SecretResolution` / `BrokerError` (the seeded
  seam) · `lease.rs:BoxExec::run_with_stdin` (the untrusted-bytes-over-stdin pattern —
  never shell-interpolated) · FC1 VM channel.
- Acceptance: `secret_never_on_image_disk_or_argv` ·
  `credential_scan_env0_proc0_disk0_attested` ·
  `unparseable_scan_fails_closed` · `mid_job_broker_fault_degrades_fail_closed` (hugit
  X11) · `lease_carries_no_raw_credentials_by_construction`.
- Size: **M/L**. Deps: FC1 (channel mechanics), API1 (authn); seam definition with hugit
  techlead (what exactly crosses the wire — open decision #7).

---

## 3. The dependency DAG + what must be contract-frozen first

```
CF0 (freeze wave — before parallel dispatch)
 │
 ├──────────────┬─────────────────┬──────────────┬──────────────┐
 ▼              ▼                 ▼              ▼              ▼
CP1 ──► CP2 ──► CP3 ──► CP4     FC1 ──┬─► FC2   BIL1 ──► BIL2  ENV3 (external,
 │       ▲       │                    ├─► FC3    │        │     off critical path)
 │       │       │                    ├─► FC4    │        ▼
 ▼       │       │                    └─► FC5    └──► (BIL3 — owner decides M1/M2)
API1 ────┘       │
 │  └─► ENV1     │
 ▼               │
API2 ◄───────────┘
 ▼
API3 ◄── (CheckDef/CheckResult + Engine v2 from CF0; FC1 slots in behind the seam)
 ├─► API4
 ▼
ENV2 ──► ATT2 ◄── ATT1 ◄── (FC2, AttestationChain freeze)
                  ATT3 ◄── (FC1)

M1 done-gate: hugit suite (contract §11) run-not-skip green against the fabric endpoint
              + §13 obligations live on the production close path.
```

Four parallel streams after CF0: **control plane** (CP1→CP4), **API** (API1→API4),
**Firecracker** (FC1→FC5, on its own Linux/KVM host), **billing** (BIL1→BIL2). ENV and ATT
join streams late (they wire frozen mechanisms onto API3's close path). The longest path
is CF0 → API1 → API2 → API3 → ENV2 → ATT2 → done-gate; FC3/FC5 are the schedule risk on
the gate itself (C3 + B2b need real warm boot + determinism).

**Must be contract-frozen in CF0, before any parallel dispatch:**

1. **Engine v2** — extend `isolation.rs:Engine` with captured-output exec (today
   `exec → Result<Option<i32>>` only; `CheckResult` needs bytes + digest). FC1 and API3
   both build against it; freezing it late forks them.
2. **`CheckDef` / `CheckResult` / `QueueApi` / `AttestationChain` transcriptions** into
   `corelink-runners-contracts` + conformance vectors byte-identical with hugit
   (`conformance/manifest.sha256` pattern; contract §0 names them as frozen IDL; the
   transcription itself is a §12-protocol event coordinated with the hugit techlead).
3. **API DTOs + endpoint paths + error vocabulary** (401/403-vs-404/429/503 semantics,
   fail-closed defaults) — API1..4, CP2, ENV1 all cite it.
4. **Ledger + tenant + slot types** (TenantId, lease record, slot account) — CP1, BIL1,
   API2 share them.
5. **Meter event schema** (slot-occupancy event, COGS counters) — BIL1 emits, CP1/ENV2
   produce.

The already-frozen and not-to-be-touched set: `RunnerLease`/`RunnerState`/`FenceManifest`/
`MaterializedEntry`/`IntentMetrics` (transcribed wire contract), the `BoxExec` and
`Engine` *semantics* (v2 is additive), `CaptureHook`/`JobClose` semantics (§13 mechanism),
and the hugit integration contract itself.

---

## 4. Risks & open decisions for the owner (each with a recommendation)

1. **Engine exec/capture seam gap.** `Engine::exec` returns exit code only
   (`isolation.rs:67`); the result path needs canonical output bytes + content digest, and
   Firecracker has no `docker exec` analogue. *Recommendation:* freeze Engine v2 in CF0
   (an `exec_captured(&self, c, argv) -> Result<CmdOutput>`-shaped addition mirroring
   `lease.rs:CmdOutput`), additive so DockerEngine and the C2a/C2b suites stay untouched.
2. **Attestation scheme + key custody.** §7 mandates signed attestation;
   `AttestationChain` is frozen in hugit-contracts but not yet transcribed here, and the
   signature scheme/keying is undecided. *Recommendation:* ed25519, one fabric signing key
   per region, public key published at a well-known endpoint; transcribe `AttestationChain`
   + conformance vector via the §12 protocol with the hugit techlead — schedule this
   contract event in CF0, it is on the critical path of ATT1.
3. **Control-plane state store.** The ledger must survive restart with leases fail-closed.
   *Recommendation:* Postgres (managed, single region at M1); in-memory is disqualified by
   `ledger_survives_process_restart`; SQLite acceptable only if M1 stays single-node by
   design — say which.
4. **Billing scope inside M1.** The whitepaper M1 bar does not include invoicing; the
   roadmap lists billing as an M1 epic. *Recommendation:* M1 = BIL1+BIL2 (slot metering +
   plan-cap enforcement — needed for caps and for "memoized bills zero" honesty); defer
   BIL3 invoicing/Stripe to M2 where self-serve onboarding lives anyway (ADR-0002).
5. **Firecracker host + CI.** Firecracker needs Linux/KVM; the gate runs on
   `[self-hosted, mac, corelink-builder]` and the interim box is shared with hugit's live
   CI. *Recommendation:* dedicated Hetzner bare-metal (KVM-capable) as the fabric's first
   metal + a new self-hosted runner label (`linux, kvm, corelink-fabric`) so FC acceptance
   runs as CI, not folklore; never colocate with `hugit-runner-01` duties.
6. **Container/VM name-prefix rename** (`hugit-job-`/`hugit-c2b-`/`hugit-c9-` →
   `corelink-*`). Ops-visible on the shared interim box; flagged in P1 as a seam change.
   *Recommendation:* rename only on the new fabric metal as part of FC1 (fresh namespace,
   no shared-box blast radius), with owner/hugit-techlead sign-off recorded; leave the
   interim box's prefixes untouched until it is retired.
7. **Secrets seam split.** Roadmap says "C5b implementation stays forge-side"; the exact
   wire (what the broker sends, what the fabric attests) is undefined. *Recommendation:*
   fabric hosts the delivery channel + credential-scan attestation (ATT3); broker
   resolution logic stays in hugit; the channel payload shape is a CF0 contract item
   agreed with the hugit techlead before ATT3 dispatch.
8. **SSH/Docker path retirement.** M1 replaces the transport — does the seed path survive?
   *Recommendation:* keep `SshBox`+`DockerEngine` as the dev/CI oracle (the suites C2a/C2b/
   C9/E4 keep the seam honest and Engine v2 regression-checked), never tenant-facing,
   never behind the public API.
9. **Warm-boot mechanism** (fabric stub A3, still `⟨FILL⟩`). Snapshot/restore vs
   pre-seeded overlay. *Recommendation:* v1 = pre-seeded CAS overlay (content-addressed,
   composes with `boot/mod.rs:HydrationPlan` as built); evaluate VM snapshot/restore as an
   M3 latency optimization — do not couple M1's done-gate to it.
10. **PriceCard source for `cost_usd_micros`.** §13.1 requires the derived exact-integer
    COGS figure; `envelope/event.rs:PriceCard` exists but who owns the canonical price
    table (and its update cadence) is undecided. *Recommendation:* fabric-owned config,
    versioned in-repo, stamped into the close payload; never tenant-visible (it is
    trust/audit COGS, not a meter).

---

## 5. What this draft deliberately does NOT decide

- **Anything priced.** Slot prices, the size ladder, oversubscription aggressiveness,
  free-tier shape — owner decisions (product §9), unchanged by this draft.
- **The wire contract.** §1–§13 carry the historical hugit framing (hugit discontinued);
  they are the fabric's own wire/envelope contract now, not an external frozen side. The
  contract events above (CheckDef/CheckResult/AttestationChain/IntentMetrics-vector
  transcriptions, prefix rename, secrets seam payload) are the fabric's own mechanisms; the
  §12 coordination protocol is moot with no external counterparty.
- **M2+ scope.** Self-serve onboarding, the GitHub-Actions front door as a product
  surface, dashboards, SLO publication, multi-region, autoscale/oversubscription, GPU —
  out (whitepaper §11 M2–M4). The Actions-YAML shim code that exists (`shim/`) is an
  asset, not an M1 commitment.
- **Fabric-stub economics.** Real metal $/vCPU, density factors, margin validation
  (stub §F) are inputs the fabric techlead owes the owner; this decomposition assumes the
  >50%-margin bar holds but does not validate it.
- **Final WP boundaries and sizing.** These are candidates for the techlead to curate; the
  acceptance-item sketches are seeds for the test-first suites, not the suites themselves
  (each WP gets its red→green suite + cold-critic pass at dispatch time).
- **Wave assignment.** The DAG constrains merge order; dispatch order, agent routing, and
  worktree topology are execution-time calls.
