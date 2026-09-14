//! Observed-event vocabulary for the envelope hook: the raw transcript
//! events a job's agent loop produces, plus the per-turn usage and the
//! injected price card the collector derives cost from.
//!
//! All data is **owned** (no lifetimes): events outlive the loop iteration
//! that produced them, and the collector accumulates by reference without
//! tying itself to any transport buffer.

/// One raw transcript event, as it occurs in the job's agent loop.
///
/// The envelope hook observes these in order; the
/// [`MetricsCollector`](super::MetricsCollector) folds them into the
/// §13.1 `IntentMetrics` projection. `bytes` carry the raw transcript
/// content for trajectory-blob capture (§13.2, WP-B2); the collector
/// itself never interprets them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEvent {
    /// A model turn: raw bytes + usage if known.
    ///
    /// `usage: None` means the API did not report usage for this turn —
    /// the collector accumulates **nothing** for tokens in that case
    /// (unknown is never fabricated). `busy_ms` is the model-busy span of
    /// the turn and always accumulates into `active_ms`.
    ModelTurn {
        /// Raw transcript bytes of the turn.
        bytes: Vec<u8>,
        /// Per-turn token usage as reported by the model API, if known.
        usage: Option<TurnUsage>,
        /// Model-busy wall span of this turn, ms.
        busy_ms: u64,
    },
    /// A tool call (paired result arrives as [`TranscriptEvent::ToolResult`]).
    ToolCall {
        /// Tool name (e.g. `"Bash"`, `"Edit"`).
        tool: String,
        /// Raw transcript bytes of the call.
        bytes: Vec<u8>,
        /// Tool-busy wall span of this call, ms.
        busy_ms: u64,
    },
    /// A tool result.
    ///
    /// Counts nothing (its [`TranscriptEvent::ToolCall`] already counted);
    /// carried for transcript completeness / blob capture.
    ToolResult {
        /// Tool name the result belongs to.
        tool: String,
        /// Raw transcript bytes of the result.
        bytes: Vec<u8>,
    },
    /// System/charter prompt content.
    ///
    /// Counts nothing; carried for transcript completeness / blob capture.
    SystemPrompt {
        /// Raw prompt bytes.
        bytes: Vec<u8>,
    },
}

/// Per-turn token usage as reported by the model API (cache split mandatory).
///
/// The `cache_read`/`cache_write` split is a §13.1 contract requirement:
/// without it the memoization economics are not computable. There is no
/// `total` field here — totals are **derived** at finalize, never supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnUsage {
    /// Input tokens (non-cached).
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Tokens read from prompt cache.
    pub cache_read: u64,
    /// Tokens written to prompt cache.
    pub cache_write: u64,
}

/// Price card in integer **micro-USD per 1M tokens** of each class.
///
/// Injected by the caller; real pricing is billing/M1 domain — this type
/// carries the rates only so `cost_usd_micros` can be derived exact-integer
/// (no f64, no epsilon) for the trust/audit COGS figure. It is NEVER a
/// billable meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceCard {
    /// Micro-USD per 1M input tokens.
    pub input_per_mtok_micros: u64,
    /// Micro-USD per 1M output tokens.
    pub output_per_mtok_micros: u64,
    /// Micro-USD per 1M cache-read tokens.
    pub cache_read_per_mtok_micros: u64,
    /// Micro-USD per 1M cache-write tokens.
    pub cache_write_per_mtok_micros: u64,
}
