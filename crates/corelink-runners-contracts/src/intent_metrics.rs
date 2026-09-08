// Transcribed from frozen wire-contracts @ 443ff1b (context_envelope, schema 1.2.0) — wire-contract seam, no git dep.

//! IntentMetrics — the per-intent spend/effort block of the context envelope.
//!
//! Transcribed from frozen wire-contracts @ 443ff1b (`context_envelope`, schema
//! 1.2.0) per the wire-contract rule — no git dep; golden fixture under
//! `tests/fixtures/` in this crate.

use serde::{Deserialize, Serialize};

/// Schema version of the context envelope these metrics belong to.
///
/// Mirrors the frozen `context_envelope` contract schema version
/// (1.2.0, owner-ratified 2026-06-11).
pub const CONTEXT_ENVELOPE_SCHEMA_VERSION: &str = "1.2.0";

/// Token spend with the cache split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenCounts {
    /// Input tokens (non-cached).
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Tokens read from prompt cache.
    pub cache_read: u64,
    /// Tokens written to prompt cache.
    pub cache_write: u64,
    /// Total tokens.
    pub total: u64,
}

/// Per-tool call count (ADR-0001 §2.2 `metrics.tool_breakdown[]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCount {
    /// Tool name (e.g. `"Edit"`, `"Bash"`).
    pub tool: String,
    /// Number of calls to this tool.
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentMetrics {
    /// Token spend with the cache split.
    pub tokens: TokenCounts,
    /// Born → die wall-clock, ms.
    pub wall_ms: u64,
    /// Model + tool busy time, ms (excludes idle).
    pub active_ms: u64,
    /// Total tool calls.
    pub tool_calls: u64,
    /// Per-tool call breakdown.
    pub tool_breakdown: Vec<ToolCount>,
    /// Number of model turns.
    pub model_turns: u64,
    /// Derived COGS in integer **micro-USD** (`1 USD = 1_000_000`); NOT what
    /// the customer is billed. Integer minor units so the cost identity is
    /// bit-exact (WA4 amendment, schema 1.2.0).
    pub cost_usd_micros: u64,
}
