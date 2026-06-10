# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
