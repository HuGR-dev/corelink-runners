# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### 2026-06-15 — Direct-CI runner-lease lifecycle wired (ADR-0007 Stage A)

- **feat(runner-lease): acquire-time runner-fleet wiring — `AcquireRequest.runner`,
  broker-gated JIT mint, egress fork, `/exec` refusal.** A runner-mode acquire
  (`runner: Some`) forces the lease's `net_policy` to `"egress-runner"` server-side,
  builds the box through `ContainerSpec::from_runner_lease` (the C2 egress floor, #69),
  mints an ephemeral GitHub Actions JIT registration config via the (default-off)
  `RunnerRegistrationBroker`, and injects it into the box env as
  `CORELINK_RUNNER_JITCONFIG`. `/exec` is refused on a runner lease (it runs its own
  ephemeral agent). The mint runs outside the ledger lock and fails closed — a mint
  failure frees the reserved slot and never provisions a config-less egress box. Both
  the immediate and the queued (`FABRIC_ADMISSION_MODE=queue`) admission paths converge
  on the shared `finalize_admitted_lease`, so the JIT mint covers both.
- **Default-off, byte-unchanged check path.** With no broker wired (`with_runner_broker`),
  a runner acquire is rejected `400` before any slot is reserved, and the classic hugit
  check-exec lease is byte-for-byte unchanged (still hermetic `no_network`, §13.2 ingest
  token injected, no JIT config). Egress is granted ONLY via the runner constructor,
  never inferred from a caller `net_policy` string (proven end-to-end in
  `acceptance_runner_lease`).
- **Adversarial-review fix:** `forget_lease` now runs on the normal `/close` path, GC'ing
  the runner-lease marker (and closing a latent `images` side-table leak) — the reaper
  only sweeps `Held` leases, so a closed lease was never reclaimed.
- **Deferred (creds-gated):** the production `GitHubAppBroker`-from-env composition wiring
  (App private key + `ureq` transport) — the lifecycle is fully exercised today via
  `MockBroker`.

### 2026-06-14 — M1 last-mile: billing exporter, tenant onboarding, registry GC, CLI, contract v1.4.0

- **feat(fabric): Wave-6 M1 last-mile — durable billing exporter + runtime tenant onboarding +
  BoxRegistry orphan GC (#51).** `SlotMeter` drains into Postgres `billing_events` table with
  exactly-once upsert by PK; export cadence driven by `FABRIC_BILLING_EXPORT_INTERVAL_SECS`.
  `POST /internal/v1/admin/tenants` enables runtime tenant provisioning without a redeploy,
  guarded by `FABRIC_ADMIN_KEY`. BoxRegistry periodic GC reaps orphaned box entries.
- **docs(plans): rate_ceiling_per_min ratified as an abuse rail, not a price (#53).** Owner-
  confirmed: the cap is a DoS/runaway-cost guardrail only; it does not appear on the pricing
  sheet or in any billing calculation. Closes the last owner-gated M1 flag.
- **feat(conformance): result_binding_v2 cross-repo conformance vector + contract v1.4.0
  RATIFIED (#52).** Signed drift tripwire committed byte-identical in both repos; hugit-side
  v2 verifier PR merged. §7.1 amendment (v1.4.0) closes the P0 attestation-forgery fix
  (binding `CheckResult.exit`/`.artifacts` into `result_binding_sig_v2`). Seams §7/§13 now
  fully closed on both sides.
- **feat(cli): `corelink` client/ops CLI — adoption last-mile smoke + verify (#54).** Thin
  CLI binary in the workspace covering the core operator workflows; smoke tests and a verify
  suite confirm the happy path end-to-end.

### 2026-06-14 — multi-instance, durable state, exhaustive audit

- **feat(fabric): persistent Postgres ledger DEPLOYED + multi-instance proven
  live.** `PgLedger` cross-instance cap-safe (`pg_advisory_xact_lock` + atomic
  count-and-insert); deployed on the Northflank `corelink-ledger` addon;
  `instances=2` proven cap-safe (25 acquires → cap held at 20, advisory-lock
  serialized). Live-only `lease_id` collision fixed via UUID minting (#39).
  Opt-in PG TLS `FABRIC_PG_TLS=disable|require` (#40, default unchanged).
- **feat(fabric): ADR-0004 durable-reap-state.** Phase 1 durable lease deadline
  in the `leases` row → the reaper is a true cross-instance backstop (#43, closed
  the cap-slot leak on instance death). Phase 2a durable envelope checkpoint +
  3-tier abnormal flush (local hook → durable checkpoint → `no_capture` marker)
  → an abnormal reap on any instance always emits a forensic record, never
  silently dropped (#45, closes hugit §13 Item-3 SLA). Owner-ratified Decision-3
  (per-turn cadence, `no_capture` marker).
- **fix(security): comprehensive adversarial audit — P0 attestation forgery +
  27 more, all closed (#46/#47).** 16-dimension workflow (96 agents, each finding
  double-verified): 40 raw → 28 confirmed (1 P0, 10 P1, 11 P2, 6 INFO). **P0:
  `result_binding_sig` did not bind `CheckResult.exit`/`.artifacts`** → a
  forgeable pass/fail verdict on an otherwise-valid attestation under untrusted
  compute → fixed with **`result_binding_sig_v2`** binding the full outcome
  (backward-compat, no flag-day; hugit must add the v2 verifier — §7.1 amendment
  v1.4.0). Plus: memo_key validation before attest, ed25519 `verify_strict`,
  cloud-engine `classify_run_status` fail-closed + injective container names,
  FileLedger `fsync` + torn-journal tolerance, forensic re-scan fail-closed,
  batch-teardown leak surfacing, stale-`Pending` cap-slot sweep, close
  ack-window + global concurrency-limit/load-shed, saturating token sum,
  introspect-vector `deny_unknown_fields` tripwire, X4 oracle single-sourced to
  the production path, real fence red-team escape vectors. Lead cold-verify
  caught a committed-disabled supply-chain gate + a spawn-in-acquire invariant
  break before they shipped.
- **docs: ADR-0004 (durable-reap-state) · ADR-0005 (queued fair admission,
  proposed) · the Northflank+Postgres multi-instance RUNBOOK · the comprehensive
  audit findings tracker · SECURITY + turn-feed handoffs to hugit.**

## [0.1.0-seed] — 2026-06-12

The seed milestone: the proven ephemeral-runner execution core, shipped to
`main` through the repo's first real CI run on `corelink-runners-builder-01`.

- feat(envelope): **S13 wave — §13 contract obligations as mechanism**
  (audit 2026-06-11 → P0). `IntentMetrics`/`TokenCounts`/`ToolCount`
  transcribed @ hugit-contracts 443ff1b with in-crate golden fixture +
  `CONTEXT_ENVELOPE_SCHEMA_VERSION` pin; conformance tripwire hardened
  (real SHA-256 per vector, manifest membership, tamper proof); envelope
  mechanism — derivation collector (saturating meters, exact-integer
  micro-USD), CaptureHook (two bounded in-memory surfaces, bearer seam
  both directions), JobClose ack state machine (fail-closed timeout,
  in-window drain, exactly-once incl. abnormal paths). 41-item acceptance
  suite, cold-reviewed (FIX-FIRST findings closed in-PR).
- docs: ROADMAP (P0 closed · P1 ship-the-seed · M1 fabric · M2 GA);
  contract title v1.2.0; CLAUDE.md refresh; transplant prose fixes.
- ci: default branch `main`; install-action v2.81.10 + pinned tool
  versions (cargo-audit@0.22.2, cargo-deny@0.19.8).

- docs(spec): **contract 1.2.0 — `cost_usd_micros|u64` (E-DOCS, 2026-06-11)**.
  §13.1 money field renamed: `cost_usd|f64` → `cost_usd_micros|u64` (integer
  micro-USD, 1 USD = 1,000,000 units; exact-integer, no f64 epsilon; owner-
  ratified 2026-06-11 as hugit WA4). §13.4 schema version updated to 1.2.0
  SHIPPED. §12 amendment-log entry added. Additive — all other §0–§12 and
  §13.2/13.3/13.4 content unchanged. Conformance-vector drift tripwire: new
  vectors must be committed byte-identical in both repos (tracked).

- docs: **WP-R5 — runner-transfer campaign records** (2026-06-10). CLAUDE.md
  advanced from spec-phase → CODE: workspace status, gate commands, wire-contract
  law, CI labels, and "seeded ≠ shipped" scope statement documented. Handoff note
  `docs/handoff/2026-06-10-runner-seed.md` authored for campaign-#1 sessions:
  what arrived (execution core + fence enforcement + X4 oracle + suites, all gate
  green @ b6319a3; contracts + vectors @ 78702d6; integration contract v1.1 @
  9796aa8), what it proves (lease loop · fence enforcement real · supply-chain
  proven over live spawn surface · wire seam tripwired · envelope emission
  contracted), what the PRODUCT still needs (multi-tenant control plane · public
  API · billing · Firecracker · C5b broker stays hugit-side), and the v1.1
  obligations (per-job metric emission + trajectory blob hook points). hugit side:
  supersession appendix in `docs/plan/decomposition.md`, absorption-map touchup,
  and hugit CHANGELOG entry — all in the same campaign. (runner-transfer-campaign)

- feat(runner): **WP-R4④ — receive fence enforcement (materialize/enforce) +
  X4 oracle (runner-transfer campaign)**. The fence's runner-side half arrives
  from hugit-fence: `materialize` (sparse hydrate by path-set — sparse
  materialization IS the fence; SHA-256 content digests, new `sha2 =0.10.9`
  workspace pin) and `enforce` (in/out classifier + box-backed ENOENT probe),
  WITH the container-escape red-team harness (`redteam`: six vectors incl.
  the load-bearing fence-materialized-escape that would go RED under a no-op
  classifier, plus its hermetic FakeFsBox twin in the bare gate) and its
  box-gated acceptance (`tests/acceptance_redteam.rs`, c5b item ⑤) — moved
  rather than re-pointed so every red-team assertion keeps driving the REAL
  classifier in-process (relocated, never weakened). The WP-X4 supply-chain
  oracle arrives too (`x4::pin`: content-pinning + verify-before-spawn
  fail-closed ordering over the LIVE spawn surface, `tests/acceptance_x4.rs`
  with the hermetic ordering proof in the bare gate; item ② retargeted to
  THIS workspace's pinned lockfile/CI — same invariant, honest home). All
  env-gated lanes preserved exactly (`HUGIT_RUNNER_HOST`: FAIL-not-skip when
  set, short-circuit when unset); `tests/acceptance_c5a.rs` moved with its
  deterministic allow-all-rejection lane. Every file carries a provenance
  header citing hugit @ 69e28e5 (removed hugit-side by WP-R4②). The secrets
  broker (C5b items ②③④⑥) stays hugit-side with the forge; the seam is the
  wire contract. Full gate green: fmt · clippy `-D warnings --locked` · test
  `--locked` · `cargo deny check` · `cargo audit --deny warnings`.

- feat(runner): **WP-R2 — transplant the execution core (runner-transfer
  campaign)**. Moved `hugit/crates/hugit-runner` → `crates/corelink-runner`
  with **zero behavioral change**: every module transplanted intact (`lease`,
  `isolation`, `teardown`, `pin`, `boot/`, `concurrency/`, `expiry/`,
  `recovery/`, `shim/{broker,executor,parser,report,subset}`, `ws/`, `lib`) —
  **minus** the F2 envelope-capture files (`src/envelope/`,
  `tests/acceptance_f2.rs`), which stay in hugit (relocate to
  `hugit-ledger::envelope` in R4 per R0). Every moved file carries a provenance
  header citing hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1.
  Imports retargeted `hugit_contracts::{RunnerLease, RunnerState,
  FenceManifest}` → `corelink_runners_contracts::…` (the R0-frozen triplet, the
  runner's whole hugit-contracts surface — nothing else needed) and the crate
  self-path `hugit_runner::` → `corelink_runner::`. Acceptance suites
  C2a/C2b/C3/C9/E4 + `hermetic_supply_chain` moved unmodified except imports +
  header; C9's box-lane behavior preserved exactly (FAIL on unreachable box
  when `HUGIT_RUNNER_HOST` set; SKIP-return when unset). Crate deps trimmed to
  the scout budget: `corelink-runners-contracts` + `anyhow =1.0.102` (new
  workspace pin) + dev `serde_json =1.0.150`; no `hugit-ledger`/`sha2`/`hex`
  (those left with the envelope). Full gate green: fmt · clippy `-D warnings
  --locked` · test `--locked` · `cargo deny check` · `cargo audit --deny
  warnings`. Plan: hugit `docs/plan/2026-06-10-runner-transfer-campaign.md` §3
  (WP-R2) + R0 FREEZE.

- feat(contracts): **WP-R1b — wire-contract types + conformance vectors
  (runner-transfer campaign)**. Transcribed `RunnerLease`, `RunnerState`,
  `FenceManifest`, and closure type `MaterializedEntry` into
  `corelink-runners-contracts` from hugit-contracts @
  7c2f1e64bc1ba46d4941dc3e5b4a6247c21b0ec0 — same fields, same serde
  attributes (`deny_unknown_fields` etc.), same doc comments, plus a
  provenance note per type.  Conformance vectors (`RunnerLease.json`,
  `FenceManifest.json`) committed byte-identical to hugit under
  `conformance/`, with `conformance/manifest.sha256` tying both repos to the
  same digests (A3 criterion). Golden round-trip tests pin every vector
  byte-exact (4 golden + 1 manifest-coverage = 5 total).  Added
  workspace-level exact-pinned deps: `serde =1.0.228`, `serde_json =1.0.150`,
  `schemars =1.2.1` (all matching hugit's versions). Full gate green: fmt ·
  clippy `-D warnings --locked` · test `--locked` · `cargo deny check` ·
  `cargo audit --deny warnings`.

- feat(workspace): **WP-R1a — workspace foundation (runner-transfer campaign,
  skeleton half)**. Cargo workspace with one member crate,
  `corelink-runners-contracts` — a placeholder lib (real wire-contract types
  arrive in R1b after the R0 transcription freeze) with a trivial test so the
  gate exercises something from day 1. Toolchain pinned to 1.96.0
  (`rust-toolchain.toml`, copied verbatim from hugit), `deny.toml` with the
  HuGR house policy (crates.io only · multiple-versions deny · no skips — the
  empty workspace needs none), CI + DCO workflows on the self-hosted fleet
  (`[self-hosted, mac, corelink-builder]`, never GitHub-hosted) running the
  full gate: fmt · clippy `--workspace --all-targets --locked -D warnings` ·
  test `--workspace --locked` · `cargo deny check` · `cargo audit --deny
  warnings`. Plan: hugit `docs/plan/2026-06-10-runner-transfer-campaign.md` §3.
