// Transplanted from hugit/crates/hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! corelink-runner — ephemeral runner v0 (WP-C2a, transplanted from hugit-runner).
//!
//! The lifecycle + isolation half of the ephemeral runner: acquire a
//! [`RunnerLease`](corelink_runners_contracts::RunnerLease), run **one** job in an
//! isolated container on a Hetzner-class box, and tear it down so that a
//! forensic re-scan of the box (disk + mounts + process table + network)
//! finds **zero** residue.
//!
//! # Scope (WP-C2a)
//! - [`lease`] — derive the per-job container spec from a frozen `RunnerLease`
//!   and drive commands on the runner box.
//! - [`isolation`] — spawn the per-job container with a **private tmp** and an
//!   **isolated network namespace**, and probe that isolation holds.
//! - [`teardown`] — destroy the container and forensically re-scan the box to
//!   prove nothing was left behind.
//!
//! Concurrency/throughput, expiry hard-kill, and crash recovery are **WP-C2b**;
//! cache-warm boot is **C3**; the Actions-YAML shim is **E4**. Fence path
//! enforcement (ENOENT, **C5a**) arrived with WP-R4: [`materialize`] (sparse
//! hydrate by path-set — sparse materialization IS the fence) and [`enforce`]
//! (the in/out classifier + the box-backed ENOENT probe), together with the
//! container-escape red-team harness ([`redteam`], six vectors incl. the
//! load-bearing fence-materialized-escape that would go RED under a no-op
//! classifier) and the WP-X4 supply-chain oracle ([`x4`]: content-pinning +
//! verify-before-spawn fail-closed ordering over the LIVE spawn surface).
//! The secrets broker (**C5b**) stays hugit-side (forge domain); it reaches
//! the job container over the wire seam, never via a crate link.
//!
//! # Runtime: container-per-job (Firecracker upgrade path)
//! v0 runs each job as a single Docker container on one Hetzner-class box.
//! Isolation is provided by Docker's default mount namespace plus an explicit
//! `--tmpfs` for the private tmp and `--network none` for the isolated network
//! namespace.
//!
//! **Firecracker upgrade path (documented, NOT built):** the same lease →
//! spec → spawn → teardown lifecycle is intended to retarget from a Docker
//! container to a Firecracker microVM. The lease carries no Docker-specific
//! fields, [`ContainerSpec`](lease::ContainerSpec) is engine-agnostic, and the
//! [`BoxExec`](lease::BoxExec) seam abstracts the box. To upgrade, implement a
//! Firecracker [`isolation::Engine`] (microVM per job, jailer for the mount
//! namespace, a tap-less / no-network device for net isolation) and the same
//! [`teardown`] forensic re-scan. The acceptance contract (destroy leaves
//! nothing; tmp/net isolated) is unchanged across engines.

/// Attestation signing: the frozen sig-preimage + ed25519 `FabricSigner` and
/// `verify_chain` (WP-ATT1a; ratified decision #2).
pub mod attest;
pub mod boot;
/// WP-2 — CAS/AC HTTP client + `BootCas`-over-HTTP skeleton (moat build).
pub mod cas_http;
pub mod concurrency;
pub mod enforce;
/// Context-envelope emission: per-job `IntentMetrics` from observed
/// transcript events (contract §13.1, WP-B1).
pub mod envelope;
pub mod expiry;
pub mod isolation;
pub mod lease;
pub mod materialize;
/// CoreLink-owned runtime names, labels, and workspace paths.
pub mod namespace;
pub mod pin;
pub mod recovery;
pub mod redteam;
// WP-A (defense-in-depth): the Actions-YAML equivalence shim is a
// NON-PRODUCTION migration/equivalence-verification harness whose
// step-execution is SIMULATED (see `shim::executor::simulate_run_digest`).
// It must never compile into a release binary, so the whole module is gated
// behind `cfg(test)` (unit tests) and the `shim-dev` feature (integration
// tests, via the self dev-dependency in Cargo.toml). Default-features release
// builds exclude it entirely. Real execution lives in the
// `corelink-check-exec-server` (genuine `Command::new().spawn()`) and the
// GitHub-Actions runner fleet — never here.
#[cfg(any(test, feature = "shim-dev"))]
pub mod shim;
pub mod teardown;
mod util;
pub mod ws;
pub mod x4;

pub use cas_http::{
    Blake3Key, CasHttpClient, CasMethod, CasOutcome, CasRequest, CasResponse, CasTransport,
    HttpBootCas,
};
pub use enforce::{
    FenceVerdict, FenceViolation, check_access, classify, is_admitted, probe_outside_enoent,
};
pub use isolation::{Engine, IsolationProbe, RunningContainer};
pub use lease::{BoxExec, ContainerSpec, SshBox};
pub use materialize::{CandidateEntry, MaterializeError, materialize_sparse};
pub use pin::{PinnedImageRef, require_pinned};
pub use redteam::{
    AttackVector, ContainerLimits, ContainmentReport, RedTeamHarness, RedTeamOutcome,
};
pub use teardown::{ForensicReport, teardown};
