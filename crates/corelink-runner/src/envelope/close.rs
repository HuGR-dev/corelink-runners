//! `JobClose` — the §13.2 item-3 close state machine over the capture hook:
//! finalize → signal → optional in-process ack window → outcome (WP-B2).
//!
//! In-process mechanism only (same std `Mutex`/`Condvar` shared state as
//! [`super::hook`]); production standalone runtime has no ack transport or
//! consumer. The lease itself is a SEAM:
//! this module exposes [`JobClose::released`] for the lease lifecycle to
//! poll/consume — it never touches `lease.rs` or the `RunnerState`
//! transitions directly.
//!
//! Dependency-surface law (§13.3): like the hook, this module imports no
//! filesystem, database, or object-store module — the outcome and the
//! signal are in-memory values only (the S13 source-inclusion oracle pins
//! this).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use corelink_runners_contracts::IntentMetrics;
use serde::Serialize;

use super::event::PriceCard;
use super::hook::{CaptureHook, HookPhase, Shared, Subscriber, credential_matches};
use super::{CloseOutcome, JobStatus};

/// The job-close signal (§13.2 item 3): the value the subscriber's
/// ack-wait API hands back when the close fires.
///
/// `metrics` is the **same finalized [`IntentMetrics`] value** the
/// [`CloseOutcome`] carries — single source of truth, finalized exactly
/// once; the forge and the runner can never disagree on the final
/// wall/active/token figures.
#[derive(Debug, Clone, PartialEq)]
pub struct CloseSignal {
    /// Terminal status of the job.
    pub status: JobStatus,
    /// The finalized per-job metrics (identical to the outcome's).
    pub metrics: IntentMetrics,
}

/// Why an abnormal close fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbnormalKind {
    /// Lease expiry hard-kill.
    Expiry,
    /// Runner/job crash while the lease was held.
    Crash,
}

/// Why a close fired — the WRAPPER-level close-metadata discriminant
/// (§13.5 ruling, owner-ratified 2026-06-13).
///
/// This is close/JobClose-machinery metadata, NOT a field of the frozen
/// §13.4 [`IntentMetrics`] vector — `close_reason` rides on the
/// [`CloseOutcome`] wrapper, never inside the metrics. The serde
/// representation is exactly `normal|expired|crashed` (snake_case) so the
/// wire strings are stable across the external consumer seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    /// A clean `POST /close` close.
    Normal,
    /// Lease-expiry hard-kill (deadline reaper) — a PARTIAL envelope.
    Expired,
    /// Runner/job crash (crash sweep) — a PARTIAL envelope.
    Crashed,
}

impl From<AbnormalKind> for CloseReason {
    fn from(kind: AbnormalKind) -> Self {
        match kind {
            AbnormalKind::Expiry => CloseReason::Expired,
            AbnormalKind::Crash => CloseReason::Crashed,
        }
    }
}

/// The close/ack state machine for one job's capture hook.
///
/// Exactly-once: across ALL handles to the same hook (the state lives in
/// the shared hook state, not in this handle), only one
/// [`close`](Self::close) / [`close_abnormal`](Self::close_abnormal) may
/// succeed; every later attempt is `Err`.
#[derive(Clone)]
pub struct JobClose {
    /// State shared with the hook and its subscribers.
    shared: Arc<Shared>,
}

impl JobClose {
    /// Build the close state machine over `hook` (shares its state; the
    /// hook handle remains usable by the job loop until close).
    #[must_use]
    pub fn new(hook: &CaptureHook) -> Self {
        Self {
            shared: hook.shared(),
        }
    }

    /// Normal job close: finalize → signal → optional ack window → outcome.
    ///
    /// 1. Both capture surfaces close (further writes refused) and the
    ///    collector finalizes exactly once into the final [`IntentMetrics`].
    /// 2. The close signal is published carrying that SAME metrics value
    ///    (single source of truth) — subscribers see it via
    ///    [`Subscriber::wait_close_signal`].
    /// 3. A nonzero `cfg.ack_timeout` retains the legacy in-process seam and
    ///    waits for a credential-valid [`Subscriber::ack`]. `Duration::ZERO`
    ///    is standalone runtime mode: it publishes the signal but completes
    ///    immediately because production has no ack transport or consumer.
    /// 4. In ack mode, a timeout marks capture incomplete. In standalone mode,
    ///    the absence of that retired ack alone never does.
    /// 5. In every mode `capture_incomplete` is `true` if either surface
    ///    overflowed, or undelivered residue remained at close time.
    ///
    /// # Errors
    /// A second close attempt (normal or abnormal) on the same hook, or a
    /// collector finalize failure. Exactly-once is enforced on the shared
    /// state, so clones of this handle cannot double-close either.
    pub fn close(
        &self,
        status: JobStatus,
        died: Instant,
        price: &PriceCard,
    ) -> Result<CloseOutcome> {
        let mut inner = self.shared.lock();
        if inner.close_done || inner.close_signal.is_some() {
            bail!("job close refused: close already signalled (job close is exactly-once)");
        }

        // (1) Close both surfaces. Overflow lossiness is latched from the
        // job's lifetime; RESIDUE is judged after the ack window (§13.2(3):
        // the forge finalises the blobs between signal and ack, so in-window
        // draining must count as delivered).
        inner.phase = HookPhase::Closed;

        let metrics = inner
            .collector
            .finalize(died, price)
            .context("finalizing the metrics collector at job close")?;

        // (2) Publish the signal with the SAME metrics value the outcome
        // will carry. A nonzero timeout is the explicit legacy test seam;
        // standalone production runs it disabled and never waits for an
        // external transport that does not exist.
        inner.close_signal = Some(CloseSignal {
            status,
            metrics: metrics.clone(),
        });
        let ack_required = !inner.cfg.ack_timeout.is_zero();
        inner.ack_window_open = ack_required;
        let ack_timeout = inner.cfg.ack_timeout;
        self.shared.cv.notify_all();

        // (3) Wait only for an explicitly-enabled in-process ack seam. The
        // condvar releases the lock while waiting, so a test subscriber can
        // drain and ack. Standalone production keeps the lock and closes now.
        let mut inner = if ack_required {
            self.shared
                .cv
                .wait_timeout_while(inner, ack_timeout, |i| !i.acked)
                .unwrap_or_else(|p| p.into_inner())
                .0
        } else {
            inner
        };

        // (4) Outcome: the window is over either way; a later ack is inert.
        // Residue is judged NOW — events the forge drained in-window count
        // as delivered; what is still sitting in either buffer was lost.
        let acked = inner.acked;
        let residue = !inner.raw.is_empty() || !inner.meta.is_empty();
        let lossy = residue || inner.raw_overflow || inner.meta_overflow;
        inner.ack_window_open = false;
        inner.close_done = true;
        inner.released = true;
        drop(inner);
        self.shared.cv.notify_all();

        Ok(CloseOutcome {
            status,
            metrics,
            capture_incomplete: lossy || (ack_required && !acked),
            close_reason: CloseReason::Normal,
        })
    }

    /// Abnormal close (expiry hard-kill / crash): both surfaces close, the
    /// collector finalizes with what was honestly observed (wall =
    /// born→kill, active = busy accumulated so far), and the single outcome
    /// carries `capture_incomplete: true` unconditionally — an abnormal end
    /// can never claim confirmed capture, so no ack window is armed (the
    /// signal is still published for any live subscriber; an ack against it
    /// is inert).
    ///
    /// Status mapping: [`AbnormalKind::Expiry`] → [`JobStatus::Killed`]
    /// (expiry hard-kill); [`AbnormalKind::Crash`] → [`JobStatus::Failed`].
    /// Lease-side, the terminal `RunnerState` is `Expired`/`Crashed` — that
    /// transition stays behind the [`released`](Self::released) seam.
    ///
    /// # Errors
    /// Shares exactly-once with [`close`](Self::close): any second close
    /// attempt is `Err`.
    pub fn close_abnormal(
        &self,
        kind: AbnormalKind,
        died: Instant,
        price: &PriceCard,
    ) -> Result<CloseOutcome> {
        let mut inner = self.shared.lock();
        if inner.close_done || inner.close_signal.is_some() {
            bail!("job close refused: close already signalled (job close is exactly-once)");
        }

        inner.phase = HookPhase::Closed;
        let metrics = inner
            .collector
            .finalize(died, price)
            .context("finalizing the metrics collector at abnormal job close")?;

        let status = match kind {
            AbnormalKind::Expiry => JobStatus::Killed,
            AbnormalKind::Crash => JobStatus::Failed,
        };
        inner.close_signal = Some(CloseSignal {
            status,
            metrics: metrics.clone(),
        });
        // No ack window: close_done immediately; a later ack is inert.
        inner.close_done = true;
        inner.released = true;
        drop(inner);
        self.shared.cv.notify_all();

        Ok(CloseOutcome {
            status,
            metrics,
            capture_incomplete: true,
            close_reason: CloseReason::from(kind),
        })
    }

    /// `true` once the close state machine has produced its outcome and the
    /// lease may proceed to its terminal state. The lease seam:
    ///
    /// - An explicitly configured acked close: the outcome (and so
    ///   `released() == true`) happens only after the ack.
    /// - An explicitly configured timed-out close: **fail-closed still
    ///   closes** with `capture_incomplete: true`.
    /// - Standalone runtime (`ack_timeout == Duration::ZERO`): the outcome
    ///   and release are immediate; only actual local loss marks capture
    ///   incomplete.
    /// - Abnormal close: the outcome is immediate; the lease's terminal
    ///   `RunnerState` is `Expired`/`Crashed` rather than `Released`, but
    ///   the seam boolean has the same meaning (the close completed; the
    ///   lease lifecycle may transition).
    #[must_use]
    pub fn released(&self) -> bool {
        self.shared.lock().released
    }
}

impl Subscriber {
    /// The ack-wait API (§13.2 item 3): block up to `timeout` for the close
    /// signal and return it. Returns the signal immediately if the close
    /// already fired; `None` if no close fires within `timeout`.
    #[must_use]
    pub fn wait_close_signal(&self, timeout: Duration) -> Option<CloseSignal> {
        let inner = self.shared.lock();
        let (inner, _timeout) = self
            .shared
            .cv
            .wait_timeout_while(inner, timeout, |i| i.close_signal.is_none())
            .unwrap_or_else(|p| p.into_inner());
        inner.close_signal.clone()
    }

    /// Acknowledge the close signal — bearer-gated, window-bound.
    ///
    /// Only a credential-valid ack inside the open ack window counts; the
    /// forge calls this after both transcript blobs are durably written
    /// (its obligation, not the runner's).
    ///
    /// # Errors
    /// - Wrong/absent credential → `Err`, and the ack does NOT count.
    /// - Before any close was signalled → `Err` (an ack cannot be
    ///   pre-armed).
    /// - After the window closed (timeout fired, or abnormal close) →
    ///   `Err` and inert: the produced outcome cannot be flipped.
    pub fn ack(&self, credential: &str) -> Result<()> {
        let mut inner = self.shared.lock();
        if !credential_matches(&inner.expected_credential, credential.as_bytes()) {
            bail!("ack refused: bearer credential mismatch for this hook");
        }
        if inner.close_signal.is_none() {
            bail!("ack refused: no close signal yet (an ack cannot be pre-armed)");
        }
        if !inner.ack_window_open {
            bail!("ack inert: the close already completed; the produced outcome cannot change");
        }
        inner.acked = true;
        drop(inner);
        self.shared.cv.notify_all();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The §13.5 wrapper-level close-reason strings are exactly
    /// `normal|expired|crashed` on the wire (snake_case).
    #[test]
    fn close_reason_serializes_to_snake_case_wire_strings() {
        assert_eq!(
            serde_json::to_string(&CloseReason::Normal).unwrap(),
            r#""normal""#
        );
        assert_eq!(
            serde_json::to_string(&CloseReason::Expired).unwrap(),
            r#""expired""#
        );
        assert_eq!(
            serde_json::to_string(&CloseReason::Crashed).unwrap(),
            r#""crashed""#
        );
    }

    /// `AbnormalKind` maps to the partial-envelope close reasons:
    /// Expiry→Expired, Crash→Crashed (never Normal).
    #[test]
    fn abnormal_kind_maps_to_partial_close_reason() {
        assert_eq!(
            CloseReason::from(AbnormalKind::Expiry),
            CloseReason::Expired
        );
        assert_eq!(CloseReason::from(AbnormalKind::Crash), CloseReason::Crashed);
    }
}
