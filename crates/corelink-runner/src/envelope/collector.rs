//! `MetricsCollector` — single-writer accumulator that folds raw
//! [`TranscriptEvent`]s into the §13.1 `IntentMetrics` projection.
//!
//! Owned by the envelope hook (WP-B2 wires it into the job loop); built and
//! tested standalone here. Every emitted field is **observed or derived** —
//! never caller-supplied wholesale: `tokens.total` is the sum of the four
//! classes, `cost_usd_micros` is exact-integer arithmetic over the injected
//! [`PriceCard`], and `active_ms` only ever accumulates from `busy_ms`
//! fields, so idle/queue wait can never leak in by construction.

use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::{Result, bail};
use corelink_runners_contracts::{IntentMetrics, TokenCounts, ToolCount};

use super::event::{PriceCard, TranscriptEvent};

/// Single-writer per-job metrics accumulator.
///
/// Lifecycle: [`new`](Self::new) at job birth → [`observe`](Self::observe)
/// once per transcript event → [`finalize`](Self::finalize) exactly once at
/// job death. A second finalize is refused with `Err` (this backs the
/// close-signal-exactly-once property at the collector layer; WP-B2 adds the
/// channel layer).
#[derive(Debug)]
pub struct MetricsCollector {
    /// Job birth instant; `wall_ms` is measured from here at finalize.
    born: Instant,
    /// Accumulated model turns.
    model_turns: u64,
    /// Accumulated tool calls (== Σ `tool_breakdown` counts by construction).
    tool_calls: u64,
    /// Per-tool call counts, keyed by tool name (BTreeMap for a
    /// deterministic emission order).
    tool_breakdown: BTreeMap<String, u64>,
    /// Accumulated busy time, ms. Only `busy_ms` fields of
    /// [`TranscriptEvent::ModelTurn`] / [`TranscriptEvent::ToolCall`] enter,
    /// so idle/queue wait never accumulates by construction.
    active_ms: u64,
    /// Accumulated input tokens (non-cached).
    input: u64,
    /// Accumulated output tokens.
    output: u64,
    /// Accumulated cache-read tokens.
    cache_read: u64,
    /// Accumulated cache-write tokens.
    cache_write: u64,
    /// Set by the first successful [`finalize`](Self::finalize).
    finalized: bool,
}

impl MetricsCollector {
    /// Start collecting for a job born at `born`.
    #[must_use]
    pub fn new(born: Instant) -> Self {
        Self {
            born,
            model_turns: 0,
            tool_calls: 0,
            tool_breakdown: BTreeMap::new(),
            active_ms: 0,
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            finalized: false,
        }
    }

    /// Fold one transcript event into the accumulators.
    ///
    /// - [`ModelTurn`](TranscriptEvent::ModelTurn): `model_turns += 1`,
    ///   `busy_ms` → active; token fields accumulate from `usage` when
    ///   `Some`. `None` = unknown: accumulate **nothing**, never fabricate.
    /// - [`ToolCall`](TranscriptEvent::ToolCall): `tool_calls += 1`, the
    ///   per-tool breakdown, and `busy_ms` → active.
    /// - [`ToolResult`](TranscriptEvent::ToolResult) /
    ///   [`SystemPrompt`](TranscriptEvent::SystemPrompt): no metric counts
    ///   (the paired call already counted; prompts are blob-capture domain).
    pub fn observe(&mut self, ev: &TranscriptEvent) {
        match ev {
            TranscriptEvent::ModelTurn { usage, busy_ms, .. } => {
                self.model_turns += 1;
                // Saturating accumulation: per-turn usage is untrusted input;
                // a lying job must cap the meters, never wrap them (a wrapped
                // u64 is a falsified wire metric) and never panic the close
                // path (debug-build overflow).
                self.active_ms = self.active_ms.saturating_add(*busy_ms);
                if let Some(u) = usage {
                    self.input = self.input.saturating_add(u.input);
                    self.output = self.output.saturating_add(u.output);
                    self.cache_read = self.cache_read.saturating_add(u.cache_read);
                    self.cache_write = self.cache_write.saturating_add(u.cache_write);
                }
            }
            TranscriptEvent::ToolCall { tool, busy_ms, .. } => {
                self.tool_calls += 1;
                *self.tool_breakdown.entry(tool.clone()).or_insert(0) += 1;
                self.active_ms = self.active_ms.saturating_add(*busy_ms);
            }
            TranscriptEvent::ToolResult { .. } | TranscriptEvent::SystemPrompt { .. } => {}
        }
    }

    /// Close the collector at `died` and emit the derived [`IntentMetrics`].
    ///
    /// - `wall_ms` = `died − born` in ms (saturating: a `died` before `born`
    ///   yields 0, never a panic).
    /// - `active_ms` = accumulated busy. **Defensive clamp:** if accumulated
    ///   busy exceeds wall (impossible with real clocks, possible with lying
    ///   inputs), it is clamped to `wall_ms` so the `active ≤ wall` invariant
    ///   holds on the wire regardless of input honesty.
    /// - `tokens.total` is DERIVED = input+output+cache_read+cache_write —
    ///   there is no caller-supplied total.
    /// - `cost_usd_micros` is DERIVED exact-integer: per class,
    ///   `tokens × per_mtok_micros / 1_000_000` over a `u128` intermediate,
    ///   summed across classes. Rounding is **floor per-class**.
    ///
    /// # Errors
    /// Fails on a second call (finalize is exactly-once), or if the derived
    /// cost overflows `u64` micro-USD.
    pub fn finalize(&mut self, died: Instant, price: &PriceCard) -> Result<IntentMetrics> {
        if self.finalized {
            bail!("MetricsCollector::finalize called twice — job close is exactly-once");
        }
        self.finalized = true;
        Ok(self.project(died, price))
    }

    /// Non-destructive PROJECTION of the current accumulated state to the
    /// §13.1 [`IntentMetrics`] shape — the **turn-boundary checkpoint** read
    /// (ADR-0004 Phase 2b, Decision-3a per-turn cadence).
    ///
    /// This is the **read side** of the once-only finalize: it computes the
    /// exact same derived projection as [`finalize`](Self::finalize) over the
    /// accumulators **as they currently stand**, but WITHOUT setting the
    /// `finalized` latch and WITHOUT requiring `&mut self`. It NEVER consumes
    /// or closes the collector, so a later `finalize` (the real close) still
    /// runs exactly once. `now` is the projection instant for `wall_ms` (the
    /// caller passes the current clock; the durable checkpoint is a mid-flight
    /// summary, not the job's death).
    ///
    /// INTERNAL API only — this projects the *current totals* of the frozen
    /// [`IntentMetrics`] shape (sha256 `2d8d2215…`); it does not alter that
    /// wire type. There is no `finalized` mutation and no error path: a
    /// snapshot can be taken any number of times.
    #[must_use]
    pub fn snapshot(&self, now: Instant, price: &PriceCard) -> IntentMetrics {
        self.project(now, price)
    }

    /// The pure derivation shared by [`finalize`](Self::finalize) (once-only,
    /// at job death) and [`snapshot`](Self::snapshot) (non-destructive, at a
    /// turn boundary). Reads `&self` only — it NEVER touches `finalized` — so
    /// both call sites produce a byte-identical projection of the same
    /// accumulator state, and the snapshot can never disturb the close latch.
    fn project(&self, at: Instant, price: &PriceCard) -> IntentMetrics {
        let wall_ms =
            u64::try_from(at.saturating_duration_since(self.born).as_millis()).unwrap_or(u64::MAX);
        // Defensive clamp (see method docs): active may never exceed wall.
        let active_ms = self.active_ms.min(wall_ms);

        let total = self
            .input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write);

        // Exact-integer COGS: u128 intermediate per class, floor division,
        // summed, then narrowed back to u64.
        let cost: u128 = class_cost_micros(self.input, price.input_per_mtok_micros)
            + class_cost_micros(self.output, price.output_per_mtok_micros)
            + class_cost_micros(self.cache_read, price.cache_read_per_mtok_micros)
            + class_cost_micros(self.cache_write, price.cache_write_per_mtok_micros);
        // Saturating narrow: absurd usage×price caps at u64::MAX micro-USD
        // instead of erroring — finalize failure here would wedge the close
        // path and leave the lease unreleasable (fail-closed law: the lease
        // never hangs on lying input).
        let cost_usd_micros = u64::try_from(cost).unwrap_or(u64::MAX);

        IntentMetrics {
            tokens: TokenCounts {
                input: self.input,
                output: self.output,
                cache_read: self.cache_read,
                cache_write: self.cache_write,
                total,
            },
            wall_ms,
            active_ms,
            tool_calls: self.tool_calls,
            tool_breakdown: self
                .tool_breakdown
                .iter()
                .map(|(tool, count)| ToolCount {
                    tool: tool.clone(),
                    count: *count,
                })
                .collect(),
            model_turns: self.model_turns,
            cost_usd_micros,
        }
    }
}

/// Floor-rounded micro-USD cost of one token class:
/// `tokens × per_mtok_micros / 1_000_000` over `u128` (cannot overflow:
/// `u64::MAX² < u128::MAX`).
fn class_cost_micros(tokens: u64, per_mtok_micros: u64) -> u128 {
    (u128::from(tokens) * u128::from(per_mtok_micros)) / 1_000_000
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::event::TurnUsage;
    use super::*;

    /// A price card where every class is free (cost is not under test).
    fn zero_price() -> PriceCard {
        PriceCard {
            input_per_mtok_micros: 0,
            output_per_mtok_micros: 0,
            cache_read_per_mtok_micros: 0,
            cache_write_per_mtok_micros: 0,
        }
    }

    /// A model turn with the given usage and zero busy time.
    fn turn(usage: Option<TurnUsage>) -> TranscriptEvent {
        TranscriptEvent::ModelTurn {
            bytes: b"turn".to_vec(),
            usage,
            busy_ms: 0,
        }
    }

    #[test]
    fn emitted_tokens_total_is_derived_sum() {
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        c.observe(&turn(Some(TurnUsage {
            input: 101,
            output: 53,
            cache_read: 29,
            cache_write: 7,
        })));
        let m = c.finalize(born, &zero_price()).unwrap();
        assert_eq!(m.tokens.total, 190, "total must be the derived sum");
    }

    #[test]
    fn token_counts_accumulated_with_distinct_expected_values() {
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        // Two turns with distinct primes per field, plus one turn with
        // usage None (unknown: must contribute NOTHING).
        c.observe(&turn(Some(TurnUsage {
            input: 2,
            output: 3,
            cache_read: 5,
            cache_write: 7,
        })));
        c.observe(&turn(Some(TurnUsage {
            input: 11,
            output: 13,
            cache_read: 17,
            cache_write: 19,
        })));
        c.observe(&turn(None));
        let m = c.finalize(born, &zero_price()).unwrap();
        // Each field equals its exact per-field sum: cross-field conflation
        // or a fabricated None contribution fails one of these.
        assert_eq!(m.tokens.input, 13);
        assert_eq!(m.tokens.output, 16);
        assert_eq!(m.tokens.cache_read, 22);
        assert_eq!(m.tokens.cache_write, 26);
        assert_eq!(m.tokens.total, 77);
    }

    #[test]
    fn counts_derived_from_observed_events() {
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        for _ in 0..4 {
            c.observe(&turn(None));
        }
        for tool in ["Bash", "Bash", "Edit", "Bash", "Edit"] {
            c.observe(&TranscriptEvent::ToolCall {
                tool: tool.to_string(),
                bytes: b"call".to_vec(),
                busy_ms: 0,
            });
            c.observe(&TranscriptEvent::ToolResult {
                tool: tool.to_string(),
                bytes: b"result".to_vec(),
            });
        }
        let m = c.finalize(born, &zero_price()).unwrap();
        assert_eq!(m.model_turns, 4);
        assert_eq!(m.tool_calls, 5);
        let breakdown: Vec<(&str, u64)> = m
            .tool_breakdown
            .iter()
            .map(|t| (t.tool.as_str(), t.count))
            .collect();
        assert_eq!(breakdown, vec![("Bash", 3), ("Edit", 2)]);
        assert_eq!(
            m.tool_calls,
            m.tool_breakdown.iter().map(|t| t.count).sum::<u64>(),
            "tool_calls must equal the breakdown sum"
        );
    }

    #[test]
    fn active_accumulates_busy_excludes_idle_and_queue_wait() {
        let born = Instant::now();
        // A much larger wall window: 60s of wall, of which only 100ms busy.
        let died = born + Duration::from_secs(60);
        let mut c = MetricsCollector::new(born);
        for busy_ms in [30, 20] {
            c.observe(&TranscriptEvent::ModelTurn {
                bytes: vec![],
                usage: None,
                busy_ms,
            });
        }
        for busy_ms in [25, 25] {
            c.observe(&TranscriptEvent::ToolCall {
                tool: "Bash".to_string(),
                bytes: vec![],
                busy_ms,
            });
        }
        let m = c.finalize(died, &zero_price()).unwrap();
        assert_eq!(
            m.active_ms, 100,
            "active is the busy sum; idle/queue wait inflates wall only"
        );
        assert_eq!(m.wall_ms, 60_000);
    }

    #[test]
    fn wall_and_active_are_real_milliseconds() {
        // REAL clock: ~50ms busy (attributed) + ~50ms idle.
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        std::thread::sleep(Duration::from_millis(50));
        c.observe(&TranscriptEvent::ModelTurn {
            bytes: vec![],
            usage: None,
            busy_ms: 50,
        });
        std::thread::sleep(Duration::from_millis(50));
        let died = Instant::now();
        let m = c.finalize(died, &zero_price()).unwrap();
        assert!(
            (95..=400).contains(&m.wall_ms),
            "wall_ms {} outside the CI-safe tolerance band [95, 400]",
            m.wall_ms
        );
        assert_eq!(m.active_ms, 50, "active is the injected busy figure");
        assert!(m.wall_ms > m.active_ms);
    }

    #[test]
    fn active_ms_never_exceeds_wall_ms() {
        // Lying input: 10_000ms of claimed busy inside a ~10ms wall window.
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        c.observe(&TranscriptEvent::ModelTurn {
            bytes: vec![],
            usage: None,
            busy_ms: 10_000,
        });
        std::thread::sleep(Duration::from_millis(10));
        let died = Instant::now();
        let m = c.finalize(died, &zero_price()).unwrap();
        assert!(
            m.wall_ms < 10_000,
            "wall window must be small for this test"
        );
        assert_eq!(
            m.active_ms, m.wall_ms,
            "defensive clamp: active_ms == wall_ms under lying busy input"
        );
    }

    #[test]
    fn cost_usd_micros_is_derived_exact_integer_micro_scale() {
        // 500_000 input tokens at 2_000_000 micro-USD/MTok == exactly 1 USD.
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        c.observe(&turn(Some(TurnUsage {
            input: 500_000,
            output: 0,
            cache_read: 0,
            cache_write: 0,
        })));
        let price = PriceCard {
            input_per_mtok_micros: 2_000_000,
            output_per_mtok_micros: 0,
            cache_read_per_mtok_micros: 0,
            cache_write_per_mtok_micros: 0,
        };
        let m = c.finalize(born, &price).unwrap();
        assert_eq!(m.cost_usd_micros, 1_000_000, "exactly 1 USD in micro-USD");

        // Mixed classes, hand-computed exact integer:
        //   input      1_000_000 ×  3_000_000 / 1e6 = 3_000_000
        //   output       200_000 × 15_000_000 / 1e6 = 3_000_000
        //   cache_read 2_000_000 ×    300_000 / 1e6 =   600_000
        //   cache_write  400_000 ×  3_750_000 / 1e6 = 1_500_000
        //   total                                   = 8_100_000
        let mut c = MetricsCollector::new(born);
        c.observe(&turn(Some(TurnUsage {
            input: 1_000_000,
            output: 200_000,
            cache_read: 2_000_000,
            cache_write: 400_000,
        })));
        let price = PriceCard {
            input_per_mtok_micros: 3_000_000,
            output_per_mtok_micros: 15_000_000,
            cache_read_per_mtok_micros: 300_000,
            cache_write_per_mtok_micros: 3_750_000,
        };
        let m = c.finalize(born, &price).unwrap();
        assert_eq!(m.cost_usd_micros, 8_100_000);
    }

    #[test]
    fn snapshot_is_non_destructive_and_reflects_accumulated_state() {
        // ADR-0004 Phase 2b: a turn-boundary snapshot PROJECTS current totals
        // without consuming/closing the collector. After two turns the
        // snapshot reflects two turns; a third turn then a finalize still
        // succeeds (the once-only latch was never tripped by the snapshots).
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        c.observe(&turn(Some(TurnUsage {
            input: 10,
            output: 5,
            cache_read: 3,
            cache_write: 1,
        })));
        c.observe(&turn(Some(TurnUsage {
            input: 20,
            output: 7,
            cache_read: 2,
            cache_write: 0,
        })));

        let snap = c.snapshot(born, &zero_price());
        assert_eq!(snap.model_turns, 2, "snapshot reflects 2 accumulated turns");
        assert_eq!(snap.tokens.input, 30);
        assert_eq!(snap.tokens.total, 30 + 12 + 5 + 1);

        // A second snapshot is still allowed (no exactly-once latch on read).
        let snap2 = c.snapshot(born, &zero_price());
        assert_eq!(snap2.model_turns, 2);

        // The real close still finalizes exactly once: the snapshots did not
        // trip the `finalized` latch.
        c.observe(&turn(None));
        let m = c
            .finalize(born, &zero_price())
            .expect("finalize still works");
        assert_eq!(m.model_turns, 3, "finalize sees all three turns");
        assert!(
            c.finalize(born, &zero_price()).is_err(),
            "finalize remains exactly-once after snapshots"
        );
    }

    #[test]
    fn snapshot_matches_finalize_projection() {
        // The snapshot and a finalize over the SAME accumulator state produce
        // the byte-identical projection (shared `project` derivation).
        let born = Instant::now();
        let mut snap_collector = MetricsCollector::new(born);
        let mut fin_collector = MetricsCollector::new(born);
        let price = PriceCard {
            input_per_mtok_micros: 3_000_000,
            output_per_mtok_micros: 15_000_000,
            cache_read_per_mtok_micros: 300_000,
            cache_write_per_mtok_micros: 3_750_000,
        };
        let usage = Some(TurnUsage {
            input: 1_000_000,
            output: 200_000,
            cache_read: 2_000_000,
            cache_write: 400_000,
        });
        snap_collector.observe(&turn(usage));
        fin_collector.observe(&turn(usage));

        let snap = snap_collector.snapshot(born, &price);
        let fin = fin_collector.finalize(born, &price).unwrap();
        assert_eq!(snap, fin, "snapshot projection == finalize projection");
    }

    #[test]
    fn double_finalize_refused() {
        let born = Instant::now();
        let mut c = MetricsCollector::new(born);
        assert!(c.finalize(born, &zero_price()).is_ok());
        let err = c.finalize(born, &zero_price()).unwrap_err().to_string();
        assert!(
            err.contains("twice"),
            "second finalize must be refused with a clear error; got: {err}"
        );
    }
}
