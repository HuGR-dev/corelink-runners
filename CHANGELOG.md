# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
