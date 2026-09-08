//! Context-envelope emission: per-job metrics derived from observed
//! transcript events (contract §13.1, WP-S13b mechanism; built WP-B1) plus
//! the in-process §13.2/§13.3 capture hook + close/ack state machine
//! (WP-B2).
//!
//! Implements the metrics half of the envelope emission obligations in
//! the legacy integration contract v1.2.0 §13.1: at job close the
//! runner reports an
//! [`IntentMetrics`](corelink_runners_contracts::IntentMetrics)-consistent
//! payload — token spend with the **mandatory cache split**, derived total,
//! wall/active split, tool counts + breakdown, model turns, and the derived
//! exact-integer `cost_usd_micros` COGS figure.
//!
//! # Scope (WP-B1)
//! - [`event`] — the observed-event vocabulary ([`TranscriptEvent`],
//!   [`TurnUsage`], [`PriceCard`]); all data owned, no lifetimes.
//! - [`collector`] — [`MetricsCollector`], the single-writer accumulator:
//!   `new(born)` → `observe(event)*` → `finalize(died, price)` exactly once.
//! - [`CloseOutcome`] / [`JobStatus`] — the one atomic job-close struct:
//!   metrics are a **required** field, so a successful close without metrics
//!   is unrepresentable (§13.1 "never optional when the job succeeded").
//!
//! # Scope (WP-B2)
//! - [`hook`] — [`CaptureHook`], the two §13.2 capture surfaces (raw
//!   transcript events + per-turn [`TurnMeta`]): bounded in-memory
//!   forwarding only, bearer-gated [`Subscriber`] drain, per-surface
//!   overflow flags — never durable, never silent, never scrubbed (§13.3).
//! - [`close`] — [`JobClose`], the §13.2 item-3 close/ack state machine:
//!   finalize exactly once → publish [`CloseSignal`] (same metrics value as
//!   the outcome) → bearer-gated ack window (`cfg.ack_timeout`) →
//!   fail-closed [`CloseOutcome`] with the honest `capture_incomplete`
//!   flag; abnormal closes ([`AbnormalKind`]) share the exactly-once rule.
//!
//! The metrics type itself is the transcribed wire contract
//! (`corelink-runners-contracts`, the frozen contract snapshot @ 443ff1b / schema 1.2.0)
//! — never redefined here. In-process mechanism only: M1 puts the fabric
//! transport + PAT verification behind the same hook/close semantics.

pub mod close;
pub mod collector;
pub mod event;
pub mod hook;

pub use close::{AbnormalKind, CloseReason, CloseSignal, JobClose};
pub use collector::MetricsCollector;
pub use event::{PriceCard, TranscriptEvent, TurnUsage};
pub use hook::{CaptureHook, EnvelopeConfig, Subscriber, TurnMeta};

use corelink_runners_contracts::IntentMetrics;

/// Terminal status of a job at close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// The job ran to a clean exit.
    Succeeded,
    /// The job ran and exited non-zero / errored.
    Failed,
    /// The job was killed (expiry hard-kill, teardown, operator action).
    Killed,
}

/// The one atomic job-close outcome (§13.1 delivery rule).
///
/// Status, metrics, and the capture-completeness flag travel as **one
/// struct**: `metrics` is a required field, not an `Option`, so "succeeded
/// but no metrics" is unrepresentable at the type level — the forge reads
/// them in the same atomic step as the result.
#[derive(Debug, Clone, PartialEq)]
pub struct CloseOutcome {
    /// Terminal status of the job.
    pub status: JobStatus,
    /// The derived per-job metrics (never optional — see type docs).
    pub metrics: IntentMetrics,
    /// `true` iff transcript capture was lossy (events may be missing); the
    /// metrics then form a lower bound, flagged rather than fabricated.
    pub capture_incomplete: bool,
    /// WRAPPER-level close-metadata: WHY this close fired
    /// (`normal|expired|crashed`, §13.5). This rides on the close machinery,
    /// NOT inside the frozen §13.4 [`IntentMetrics`] vector. A partial
    /// envelope from an abnormal end carries `Expired`/`Crashed` here AND
    /// `capture_incomplete: true` above.
    pub close_reason: CloseReason,
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Build a `CloseOutcome` the only way the type permits: with all three
    /// fields present.
    fn outcome() -> CloseOutcome {
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        c.observe(&TranscriptEvent::ModelTurn {
            bytes: b"turn".to_vec(),
            usage: None,
            busy_ms: 0,
        });
        let metrics = c
            .finalize(
                born,
                &PriceCard {
                    input_per_mtok_micros: 0,
                    output_per_mtok_micros: 0,
                    cache_read_per_mtok_micros: 0,
                    cache_write_per_mtok_micros: 0,
                },
            )
            .unwrap();
        CloseOutcome {
            status: JobStatus::Succeeded,
            metrics,
            capture_incomplete: false,
            close_reason: CloseReason::Normal,
        }
    }

    #[test]
    fn metrics_never_optional_on_success() {
        // `CloseOutcome.metrics` is `IntentMetrics`, not `Option<…>`: a
        // successful close without metrics does not compile. This test
        // documents that unrepresentability by reading the metrics off a
        // succeeded outcome unconditionally — no unwrap, no None branch.
        let o = outcome();
        assert_eq!(o.status, JobStatus::Succeeded);
        assert_eq!(o.metrics.model_turns, 1);
        assert_eq!(o.metrics.tokens.total, 0);
    }

    #[test]
    fn close_outcome_is_one_atomic_struct() {
        // Constructing a CloseOutcome requires all three fields (compile-time
        // fact); assert presence of each on a built value.
        let o = outcome();
        assert_eq!(o.status, JobStatus::Succeeded);
        assert!(!o.capture_incomplete);
        assert_eq!(o.metrics.tool_calls, 0);
    }
}
