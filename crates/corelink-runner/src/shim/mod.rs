// Transplanted from transferred runner implementation @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! Actions-YAML compatibility shim v0 (WP-E4).
//!
//! ⚠️ **NON-PRODUCTION harness — never a real execution result.** This module
//! is an equivalence/migration-verification harness only. Its step-execution
//! is **SIMULATED**: [`executor::simulate_run_digest`] returns a deterministic
//! stand-in digest (`digest-len-…-cmd-…`) computed from the command *string*,
//! not from any process output — no live container is ever spawned. That
//! simulated digest, and every [`StepOutcome`] the shim produces, **must never
//! feed a billing, memoization, or cache decision**. It exists solely to
//! compare the shim's observable outcomes against real GitHub Actions.
//!
//! Because of this, the whole module is compiled behind
//! `#[cfg(any(test, feature = "shim-dev"))]` (see the crate root + `Cargo.toml`
//! `shim-dev` feature): it is physically absent from a default-features release
//! binary. **Real execution lives elsewhere** — the
//! `corelink-check-exec-server` (genuine `Command::new().spawn()`) and the
//! GitHub-Actions runner fleet. Never wire a production path through this shim.
//!
//! A migration-lubricant shim that runs a **published supported subset** of
//! GitHub Actions workflow YAML on the external consumer's runners. Design invariants:
//!
//! 1. **Supported subset is a published contract** — every listed feature is
//!    proven-to-execute by a passing fixture, not merely documented
//!    (`docs/shim/supported-subset.md`).
//! 2. **Out-of-contract → explicit actionable report** — any construct outside
//!    the supported subset produces an [`OutOfContractReport`] naming the
//!    unsupported construct; zero silent skips.
//! 3. **Secrets via broker only** — raw secret material never enters the
//!    runner environment or logs. Missing/denied secrets fail CLOSED, naming
//!    the secret.
//! 4. **Execution equivalence** — a deterministic fixture workflow
//!    (determinism precondition: pinned toolchain/inputs, no wall-clock or net
//!    nondeterminism) produces equivalent observable outcomes (steps, env,
//!    artifacts, exit states) on real GitHub Actions and on the shim.
//!
//! **Not in scope here:** concurrency/throughput, expiry hard-kill, crash
//! recovery, fence ENOENT enforcement. Those are C2b and C5a respectively.

pub mod broker;
pub mod executor;
pub mod parser;
pub mod report;
pub mod subset;

pub use broker::{Broker, BrokerError, SecretResolution};
pub use executor::{EquivalenceOutcome, ExecutionResult, ShimExecutor, StepOutcome};
pub use parser::{ParsedWorkflow, Step, WorkflowParseError, parse_workflow};
pub use report::{OutOfContractReport, ShimDiagnostic, UnsupportedConstruct};
pub use subset::{SUPPORTED_SUBSET, SubsetFeature, is_supported};
