# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
