//! `CaptureHook` — the in-process §13.2 capture hook: the two capture
//! surfaces (raw transcript events + per-turn metadata) the job's agent
//! loop writes into and the forge-side subscriber drains (WP-B2).
//!
//! Single-process mechanism only (std `Mutex`/`Condvar`, mirroring the
//! `ws::DedupSpawner` style): the network transport and the real PAT wiring
//! are M1 fabric domain. The bearer seam here is an **injected expected
//! token** compared constant-time-ish at [`CaptureHook::subscribe`] and
//! `Subscriber::ack` (see [`super::close`]); the M1 binding replaces the
//! injected token with the Bearer PAT of the CAS/AC boundary (§13.2
//! "authenticated hook point") verified by the control plane — the gate
//! shape (credential in, `Err` on mismatch, mismatch never counts) is the
//! frozen seam, the token source is what M1 swaps.
//!
//! §13.3 law: the runner never persists transcript bytes. Both surfaces are
//! **bounded in-memory queues** (in-flight forwarding only): delivered
//! entries are removed on drain; an over-capacity write drops the NEW event
//! and records a per-surface overflow flag (bounded, never durable, never
//! silent); and the dependency surface of this module proves there is no
//! durable backend — no filesystem, database, or object-store module is
//! imported here or in [`super::close`] (pinned by a source-inclusion
//! oracle in the S13 acceptance suite). Bytes are forwarded EXACTLY as
//! given: redaction is on the forge's write path (§13.3); the runner never
//! inspects, scrubs, or modifies transcript content.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use super::close::CloseSignal;
use super::collector::MetricsCollector;
use super::event::TranscriptEvent;

/// Runner configuration for the envelope capture hook.
///
/// `ack_timeout` is the **runner-configured** job-close ack window of §13.2
/// item 3: how long the close state machine waits for the forge-side
/// subscriber to acknowledge the close signal before closing anyway
/// (fail-closed). `buffer_capacity` bounds EACH in-flight surface (raw and
/// metadata) — the §13.3 "bounded FIFO" allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeConfig {
    /// How long a close waits for the subscriber's ack before fail-closing.
    pub ack_timeout: Duration,
    /// Capacity of each in-flight surface (raw events / per-turn metadata).
    pub buffer_capacity: usize,
}

/// Per-turn metadata (§13.2 item 2) — the lightweight side-channel that
/// powers the forge's compacted-transcript summariser without re-parsing
/// the raw stream.
///
/// The optionals are **honest**: `tool` is `Some` only for a
/// [`TranscriptEvent::ToolCall`]; `tokens` is `Some` only when the event
/// actually carried usage (the derived total of the four §13.1 token
/// classes). Unknown is `None`, never a fabricated `0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnMeta {
    /// Monotone per-hook turn index (0-based, one per written event).
    pub turn_index: u64,
    /// Real occurrence clock: milliseconds since the hook opened.
    pub timestamp_ms: u64,
    /// Tool name, `Some` only for a tool-call event.
    pub tool: Option<String>,
    /// Token count (derived total of the turn's usage), `Some` only when
    /// the event carried usage.
    pub tokens: Option<u64>,
}

/// Lifecycle phase of the hook's capture surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HookPhase {
    /// Allocated but not yet open: writes are refused.
    NotOpen,
    /// Open (lease acquired): writes are accepted.
    Open,
    /// Closed (job close): writes are refused, forever.
    Closed,
}

/// The shared state behind the hook, its subscribers, and the close state
/// machine ([`super::close::JobClose`]). One `Inner` per lease — two
/// concurrent leases share NOTHING (cross-contamination is structurally
/// impossible; oracle B24).
pub(super) struct Inner {
    /// Runner configuration (ack window + per-surface capacity).
    pub(super) cfg: EnvelopeConfig,
    /// The injected expected bearer token (the M1-PAT seam; module docs).
    pub(super) expected_credential: Vec<u8>,
    /// Instant the hook opened; `TurnMeta.timestamp_ms` is measured from it.
    pub(super) opened_at: Instant,
    /// Capture-surface lifecycle phase.
    pub(super) phase: HookPhase,
    /// Raw-event surface: bytes exactly as written, in order, bounded.
    pub(super) raw: VecDeque<Vec<u8>>,
    /// Metadata surface: one [`TurnMeta`] per written event, bounded.
    pub(super) meta: VecDeque<TurnMeta>,
    /// Set when a write found the raw surface full (the new event's bytes
    /// were dropped — bounded and never silent).
    pub(super) raw_overflow: bool,
    /// Set when a write found the metadata surface full.
    pub(super) meta_overflow: bool,
    /// Next turn index (increments on every accepted write).
    pub(super) next_turn_index: u64,
    /// The per-job metrics accumulator (finalized exactly once at close).
    pub(super) collector: MetricsCollector,
    /// The close signal, published exactly once by the close state machine.
    pub(super) close_signal: Option<CloseSignal>,
    /// `true` only while the close state machine is waiting for the ack.
    pub(super) ack_window_open: bool,
    /// Set by a credential-valid `ack` inside the ack window.
    pub(super) acked: bool,
    /// Set once the close state machine produced its outcome (exactly-once).
    pub(super) close_done: bool,
    /// `true` once the close completed and the lease may transition to its
    /// terminal state (see `JobClose::released`).
    pub(super) released: bool,
}

/// Mutex + condvar pair shared by hook, subscribers, and `JobClose`.
pub(super) struct Shared {
    /// The guarded state.
    pub(super) inner: Mutex<Inner>,
    /// Signalled on every state change (new event, close signal, ack, done).
    pub(super) cv: Condvar,
}

impl Shared {
    /// Lock with poison recovery (a panicking writer must not DoS the
    /// close path — same discipline as `ws::DedupSpawner`).
    pub(super) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Constant-time-ish bearer comparison: no early exit on the first
/// mismatching byte — the difference is OR-folded across the full
/// `max(len)` walk, with the length difference folded in as well. (The
/// *lengths* still bound the loop; the M1 binding delegates verification to
/// the control plane's PAT check, which owns the full hardening.)
pub(super) fn credential_matches(expected: &[u8], presented: &[u8]) -> bool {
    let mut diff = expected.len() ^ presented.len();
    let n = expected.len().max(presented.len());
    for i in 0..n {
        let a = expected.get(i).copied().unwrap_or(0);
        let b = presented.get(i).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    diff == 0
}

/// The §13.2 capture hook: opened at lease acquire by the lease owner,
/// written by the job's agent loop, drained by the (bearer-gated)
/// forge-side subscriber, closed by [`super::close::JobClose`].
///
/// **Events before open are unrepresentable** at the API level: there is no
/// write surface at all until [`CaptureHook::open`] (or
/// [`CaptureHook::pending`]) constructs the hook — no hook value, no write
/// handle. The reachable not-yet-open state ([`CaptureHook::pending`])
/// additionally refuses writes with `Err` until
/// [`complete_open`](CaptureHook::complete_open).
///
/// Cloning the hook clones the *handle*; all clones share the same lease's
/// surfaces.
#[derive(Clone)]
pub struct CaptureHook {
    /// State shared with subscribers and the close state machine.
    shared: Arc<Shared>,
}

impl CaptureHook {
    /// Allocate the hook in the **not-yet-open** state: writes are refused
    /// until [`complete_open`](Self::complete_open).
    ///
    /// `expected_credential` is the injected bearer token subscribers (and
    /// acks) must present — the M1-PAT seam (module docs). `collector` is
    /// the job's metrics accumulator, finalized exactly once at close.
    #[must_use]
    pub fn pending(
        cfg: EnvelopeConfig,
        expected_credential: &str,
        collector: MetricsCollector,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    cfg,
                    expected_credential: expected_credential.as_bytes().to_vec(),
                    opened_at: Instant::now(),
                    phase: HookPhase::NotOpen,
                    raw: VecDeque::new(),
                    meta: VecDeque::new(),
                    raw_overflow: false,
                    meta_overflow: false,
                    next_turn_index: 0,
                    collector,
                    close_signal: None,
                    ack_window_open: false,
                    acked: false,
                    close_done: false,
                    released: false,
                }),
                cv: Condvar::new(),
            }),
        }
    }

    /// Open the hook (lease acquire): construct + complete the open in one
    /// step. The hook accepts writes from here until job close.
    #[must_use]
    pub fn open(
        cfg: EnvelopeConfig,
        expected_credential: &str,
        collector: MetricsCollector,
    ) -> Self {
        let hook = Self::pending(cfg, expected_credential, collector);
        hook.complete_open();
        hook
    }

    /// Complete the open: transition not-yet-open → open and start the
    /// `timestamp_ms` occurrence clock. A no-op on an already-open or
    /// closed hook (close is one-way; this never reopens).
    pub fn complete_open(&self) {
        let mut inner = self.shared.lock();
        if inner.phase == HookPhase::NotOpen {
            inner.phase = HookPhase::Open;
            inner.opened_at = Instant::now();
        }
    }

    /// Write one transcript event into BOTH capture surfaces.
    ///
    /// - Raw surface: the event's bytes are pushed **exactly as given** —
    ///   byte-identical, never inspected, never scrubbed (§13.3: redaction
    ///   is forge-side).
    /// - Metadata surface: a [`TurnMeta`] with the monotone turn index, the
    ///   real occurrence timestamp (ms since open), and the honest
    ///   optionals.
    /// - The metrics collector observes the event (metrics derive from
    ///   observed events even when a full buffer drops the in-flight copy —
    ///   incompleteness is the overflow flag's job, not the collector's).
    /// - A full surface drops the NEW entry and records that surface's
    ///   overflow flag: bounded, in-memory, never durable, never silent.
    ///
    /// # Errors
    /// Refused (`Err`) before the open is complete and after close.
    pub fn write(&self, ev: TranscriptEvent) -> Result<()> {
        let mut inner = self.shared.lock();
        match inner.phase {
            HookPhase::NotOpen => {
                bail!(
                    "capture hook write refused: hook is not open yet (open completes at lease acquire)"
                )
            }
            HookPhase::Closed => {
                bail!("capture hook write refused: hook is closed (job close already ran)")
            }
            HookPhase::Open => {}
        }

        inner.collector.observe(&ev);

        let timestamp_ms = u64::try_from(inner.opened_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        let turn_index = inner.next_turn_index;
        inner.next_turn_index += 1;

        // Honest optionals: tool only for a ToolCall; tokens only when the
        // event carried usage (derived total of the four classes).
        let (bytes, tool, tokens) = match ev {
            TranscriptEvent::ModelTurn { bytes, usage, .. } => {
                let tokens = usage.map(|u| u.input + u.output + u.cache_read + u.cache_write);
                (bytes, None, tokens)
            }
            TranscriptEvent::ToolCall { tool, bytes, .. } => (bytes, Some(tool), None),
            TranscriptEvent::ToolResult { bytes, .. } => (bytes, None, None),
            TranscriptEvent::SystemPrompt { bytes } => (bytes, None, None),
        };

        if inner.raw.len() < inner.cfg.buffer_capacity {
            inner.raw.push_back(bytes);
        } else {
            // Bounded and never silent: drop the NEW event, record the flag.
            inner.raw_overflow = true;
        }

        let meta = TurnMeta {
            turn_index,
            timestamp_ms,
            tool,
            tokens,
        };
        if inner.meta.len() < inner.cfg.buffer_capacity {
            inner.meta.push_back(meta);
        } else {
            inner.meta_overflow = true;
        }

        drop(inner);
        self.shared.cv.notify_all();
        Ok(())
    }

    /// Subscribe to BOTH capture surfaces — the bearer seam.
    ///
    /// # Errors
    /// A wrong or absent credential is refused with `Err` (and the
    /// comparison is constant-time-ish; see [`credential_matches`]). The
    /// credential is **per-hook**: lease A's credential does not open
    /// lease B's hook.
    pub fn subscribe(&self, credential: &str) -> Result<Subscriber> {
        let inner = self.shared.lock();
        if !credential_matches(&inner.expected_credential, credential.as_bytes()) {
            bail!("subscribe refused: bearer credential mismatch for this hook");
        }
        drop(inner);
        Ok(Subscriber {
            shared: Arc::clone(&self.shared),
        })
    }

    /// Shared-state handle for the close state machine (crate-internal seam
    /// to [`super::close::JobClose`]).
    pub(super) fn shared(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }

    /// Test-only observability: entries currently in flight on the raw
    /// surface. Proves drained entries were RELEASED (len returns to 0) —
    /// in-flight forwarding only, never retention. (`#[doc(hidden)]`
    /// because `#[cfg(test)]` items are invisible to the integration-test
    /// oracle that needs it; not a product API.)
    #[doc(hidden)]
    #[must_use]
    pub fn raw_buffer_len(&self) -> usize {
        self.shared.lock().raw.len()
    }

    /// Test-only observability: entries currently in flight on the
    /// metadata surface. See [`raw_buffer_len`](Self::raw_buffer_len).
    #[doc(hidden)]
    #[must_use]
    pub fn meta_buffer_len(&self) -> usize {
        self.shared.lock().meta.len()
    }
}

/// A bearer-authenticated drain handle over both capture surfaces.
///
/// Delivered entries are **removed** from the surface (released after
/// forwarding — the §13.3 in-flight-only rule). The ack half of the close
/// handshake (`wait_close_signal` / `ack`) lives in [`super::close`].
#[derive(Clone)]
pub struct Subscriber {
    /// State shared with the hook and the close state machine.
    pub(super) shared: Arc<Shared>,
}

impl Subscriber {
    /// Drain the next raw event, if one is in flight. The returned bytes
    /// are byte-identical to what the writer gave; the entry is removed
    /// from the surface (released).
    #[must_use]
    pub fn next_event(&self) -> Option<Vec<u8>> {
        self.shared.lock().raw.pop_front()
    }

    /// Drain the next per-turn metadata entry, if one is in flight. The
    /// entry is removed from the surface (released).
    #[must_use]
    pub fn next_meta(&self) -> Option<TurnMeta> {
        self.shared.lock().meta.pop_front()
    }
}
