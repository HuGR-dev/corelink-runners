// Transcribed from hugit-contracts @ 7736d02 (frozen WP-00) — wire-contract seam, no git dep.

//! CheckDef — a check-as-code definition (decomposition §1, item 1).
//!
//! Transcribed from hugit-contracts @ 7736d02 (frozen WP-00) per the
//! wire-contract rule — no git dep.

use serde::{Deserialize, Serialize};

/// A check-as-code definition (decomposition §1, item 1).
///
/// `def_digest` is the canonical digest over the definition body; it forms
/// the second axis of the `CheckResult` memo key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckDef {
    /// SHA-256 hex digest of the definition body (canonical, second axis of
    /// memo key).
    pub def_digest: String,

    /// The command to execute (argv[0] + args as a single shell string or
    /// structured invocation).
    pub command: String,

    /// Declared input paths / globs that affect the check.
    pub inputs: Vec<String>,

    /// Reference to the toolchain (e.g. a content-addressed toolchain digest
    /// or version string).
    pub toolchain_ref: String,

    /// Reference to an environment manifest (content-addressed blob ref).
    pub env_manifest: String,

    /// File glob patterns that scope materialization for this check.
    pub glob_set: Vec<String>,
}
