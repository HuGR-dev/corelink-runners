//! WP-B2 acceptance oracle — §13.2/§13.3 in-process capture hook + close/ack
//! state machine (`envelope::{hook,close}`).
//!
//! Items (one `#[test]` each; B-codes from the S13 acceptance suite):
//!   B1  `hook_opens_at_lease_acquire_closes_at_job_close`
//!   B20 `events_before_open_unrepresentable`
//!   B2  `raw_events_forwarded_in_flight_never_persisted_and_released`
//!   B26 `secret_shaped_content_never_scrubbed`
//!   B23 `events_stream_as_they_occur`
//!   B13 `all_contracted_event_kinds_forwarded`
//!   B22 `stream_order_and_no_duplication`
//!   B3  `per_turn_metadata_progressive_timestamped_in_ms`
//!   B25 `metadata_optional_fields_honest_both_ways`
//!   B16 `metadata_aligned_with_raw_stream`
//!   B4  `job_close_signal_carries_final_wall_and_active_ms`
//!   B18 `both_channels_drained_before_close_signal`
//!   B5  `unacked_close_fails_closed_with_capture_incomplete`
//!   B29 `in_window_ack_never_treated_as_timeout`
//!   B28 `late_ack_after_timeout_is_inert`
//!   B33 `early_ack_is_inert`
//!   B9  `acked_close_releases_with_capture_complete`
//!   B21 `close_signal_exactly_once`
//!   B19 `abnormal_termination_closes_hook_fail_closed`
//!   B24 `concurrent_leases_never_cross_contaminate`
//!   B10 `buffer_overflow_is_never_silent`
//!   B8  `all_hook_surfaces_credential_gated_both_directions`
//!   B27 `transcript_channel_outside_credential_scan_scope`
//!
//! Entirely in-process (no box, no network): the M1 fabric carries the same
//! hook/close semantics over the wire; the bearer credential here is the
//! injected-token seam the M1 PAT verification replaces.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use corelink_runner::envelope::{
    AbnormalKind, CaptureHook, EnvelopeConfig, JobClose, JobStatus, MetricsCollector, PriceCard,
    Subscriber, TranscriptEvent, TurnMeta, TurnUsage,
};

/// The injected bearer credential for the default test lease ("lease A").
const CRED_A: &str = "bearer-s13-lease-A-cred";
/// A distinct credential for the second lease ("lease B").
const CRED_B: &str = "bearer-s13-lease-B-cred";

fn cfg(ack_ms: u64, capacity: usize) -> EnvelopeConfig {
    EnvelopeConfig {
        ack_timeout: Duration::from_millis(ack_ms),
        buffer_capacity: capacity,
    }
}

/// A price card where every class is free (cost is not under test here;
/// the §13.1 cost derivation is the WP-B1 collector suite's job).
fn zero_price() -> PriceCard {
    PriceCard {
        input_per_mtok_micros: 0,
        output_per_mtok_micros: 0,
        cache_read_per_mtok_micros: 0,
        cache_write_per_mtok_micros: 0,
    }
}

/// Open a hook for a fresh job (lease acquire) under `CRED_A`.
fn open_hook(ack_ms: u64, capacity: usize) -> CaptureHook {
    CaptureHook::open(
        cfg(ack_ms, capacity),
        CRED_A,
        MetricsCollector::new(Instant::now()),
    )
}

/// A model turn with the given payload, no usage, zero busy time.
fn turn(bytes: &[u8]) -> TranscriptEvent {
    TranscriptEvent::ModelTurn {
        bytes: bytes.to_vec(),
        usage: None,
        busy_ms: 0,
    }
}

/// A tool call with the given payload.
fn tool_call(tool: &str, bytes: &[u8], busy_ms: u64) -> TranscriptEvent {
    TranscriptEvent::ToolCall {
        tool: tool.to_string(),
        bytes: bytes.to_vec(),
        busy_ms,
    }
}

/// Drain the raw surface completely.
fn drain_raw(sub: &Subscriber) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    while let Some(e) = sub.next_event() {
        v.push(e);
    }
    v
}

/// Drain the metadata surface completely.
fn drain_meta(sub: &Subscriber) -> Vec<TurnMeta> {
    let mut v = Vec::new();
    while let Some(m) = sub.next_meta() {
        v.push(m);
    }
    v
}

/// Spawn the forge-side acker: wait for the close signal, then ack with
/// `credential`. Returns the observed signal.
fn spawn_acker(
    sub: Subscriber,
    credential: &'static str,
) -> thread::JoinHandle<corelink_runner::envelope::CloseSignal> {
    thread::spawn(move || {
        let sig = sub
            .wait_close_signal(Duration::from_secs(5))
            .expect("close signal must arrive");
        sub.ack(credential).expect("in-window ack must be accepted");
        sig
    })
}

// ── B1 ────────────────────────────────────────────────────────────────────────

/// B1 — the hook opens at lease acquire and closes at job close: writes are
/// accepted strictly between the two; after close, a write to the raw AND
/// the meta surface (one `write` feeds both) is refused with `Err`.
#[test]
fn hook_opens_at_lease_acquire_closes_at_job_close() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    // Open window: writes accepted.
    hook.write(turn(b"during-job-1")).unwrap();
    hook.write(tool_call("Bash", b"during-job-2", 1)).unwrap();
    assert_eq!(drain_raw(&sub).len(), 2);
    let _ = drain_meta(&sub);

    // Job close (acked).
    let acker = spawn_acker(sub, CRED_A);
    let jc = JobClose::new(&hook);
    jc.close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();

    // After close: BOTH surfaces refuse — `write` is the single entry that
    // feeds raw bytes and per-turn metadata, so one refusal covers both;
    // we assert it for a raw-bearing turn and a meta-bearing tool call.
    assert!(hook.write(turn(b"after-close-raw")).is_err());
    assert!(
        hook.write(tool_call("Edit", b"after-close-meta", 0))
            .is_err()
    );
}

// ── B20 ───────────────────────────────────────────────────────────────────────

/// B20 — events before open are unrepresentable. API shape: before
/// `CaptureHook::open`/`pending` there is no hook VALUE at all, hence no
/// write handle — an event "before the hook exists" cannot be expressed
/// (the stronger, compile-time half). The reachable not-yet-open state
/// (`pending`) refuses writes with `Err` until `complete_open`.
#[test]
fn events_before_open_unrepresentable() {
    let hook = CaptureHook::pending(cfg(100, 8), CRED_A, MetricsCollector::new(Instant::now()));
    let err = hook.write(turn(b"too-early")).unwrap_err().to_string();
    assert!(
        err.contains("not open"),
        "a not-yet-open hook must refuse writes; got: {err}"
    );

    hook.complete_open();
    hook.write(turn(b"now-accepted")).unwrap();
}

// ── B2 ────────────────────────────────────────────────────────────────────────

/// B2 — raw events are forwarded in flight, never persisted, and released
/// after delivery: bytes arrive byte-identical, the internal buffer is
/// empty after the drain (released, len 0), and the module source imports
/// no durable backend (filesystem / DB / object store).
#[test]
fn raw_events_forwarded_in_flight_never_persisted_and_released() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    let payloads: Vec<Vec<u8>> = vec![
        b"alpha".to_vec(),
        b"\x00\xffbinary\x01\x02 with nul and high bytes".to_vec(),
        b"third event".to_vec(),
    ];
    for p in &payloads {
        hook.write(turn(p)).unwrap();
    }

    // Byte-identical, in order.
    let got = drain_raw(&sub);
    assert_eq!(got, payloads, "raw bytes must arrive byte-identical");

    // Released after forwarding: internal buffers back at len 0.
    assert!(sub.next_event().is_none());
    assert_eq!(
        hook.raw_buffer_len(),
        0,
        "drained raw entries must be RELEASED"
    );
    let _ = drain_meta(&sub);
    assert_eq!(
        hook.meta_buffer_len(),
        0,
        "drained meta entries must be RELEASED"
    );

    // §13.3 dependency-surface oracle: the hook+close source must contain
    // no durable-backend imports — no filesystem, no DB, no object store.
    let hook_src = include_str!("../src/envelope/hook.rs");
    let close_src = include_str!("../src/envelope/close.rs");
    for needle in ["std::fs", "File::", "rusqlite", "sled", "reqwest"] {
        assert!(
            !hook_src.contains(needle) && !close_src.contains(needle),
            "durable-backend marker {needle:?} found in the §13.3 modules"
        );
    }
}

// ── B26 ───────────────────────────────────────────────────────────────────────

/// B26 — secret-shaped content is NEVER scrubbed: redaction is forge-side
/// (§13.3). AWS-key-shaped, PEM-block, bearer-token, and PII-shaped bytes
/// must all arrive byte-identical.
#[test]
fn secret_shaped_content_never_scrubbed() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    let secrets: Vec<Vec<u8>> = vec![
        b"export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE".to_vec(),
        b"-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA7byte==\n-----END RSA PRIVATE KEY-----"
            .to_vec(),
        b"Authorization: Bearer ghp_16C7e42F292c6912E7710c838347Ae178B4a".to_vec(),
        b"applicant ssn=078-05-1120 email=jane.doe@example.com phone=+1-555-0100".to_vec(),
    ];
    for s in &secrets {
        hook.write(TranscriptEvent::SystemPrompt { bytes: s.clone() })
            .unwrap();
    }

    let got = drain_raw(&sub);
    assert_eq!(
        got, secrets,
        "secret-shaped content must arrive byte-identical (redaction is forge-side, §13.3)"
    );
}

// ── B23 ───────────────────────────────────────────────────────────────────────

/// B23 — events stream as they occur: a subscriber thread observes event N
/// before the writer produces event N+1 (synchronized over a channel).
#[test]
fn events_stream_as_they_occur() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let forwarder = thread::spawn(move || {
        for _ in 0..3 {
            let ev = loop {
                if let Some(e) = sub.next_event() {
                    break e;
                }
                thread::sleep(Duration::from_millis(1));
            };
            tx.send(ev).unwrap();
        }
    });

    for i in 0..3u32 {
        let payload = format!("streamed-event-{i}").into_bytes();
        hook.write(turn(&payload)).unwrap();
        // The subscriber must hand event N back BEFORE we write N+1:
        // recv blocks until the forwarder observed exactly this event.
        let observed = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("event must stream while the job is still producing");
        assert_eq!(observed, payload, "event {i} must stream in real time");
    }
    forwarder.join().unwrap();
}

// ── B13 ───────────────────────────────────────────────────────────────────────

/// B13 — every contracted event kind (§13.2 item 1: model turns, tool calls
/// + results, system/charter prompts) is forwarded through the hook.
#[test]
fn all_contracted_event_kinds_forwarded() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"kind-model-turn".to_vec(),
        usage: Some(TurnUsage {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
        }),
        busy_ms: 1,
    })
    .unwrap();
    hook.write(tool_call("Bash", b"kind-tool-call", 1)).unwrap();
    hook.write(TranscriptEvent::ToolResult {
        tool: "Bash".to_string(),
        bytes: b"kind-tool-result".to_vec(),
    })
    .unwrap();
    hook.write(TranscriptEvent::SystemPrompt {
        bytes: b"kind-system-prompt".to_vec(),
    })
    .unwrap();

    let got = drain_raw(&sub);
    assert_eq!(
        got,
        vec![
            b"kind-model-turn".to_vec(),
            b"kind-tool-call".to_vec(),
            b"kind-tool-result".to_vec(),
            b"kind-system-prompt".to_vec(),
        ],
        "all four contracted event kinds must forward"
    );
    assert_eq!(drain_meta(&sub).len(), 4, "one meta entry per event kind");
}

// ── B22 ───────────────────────────────────────────────────────────────────────

/// B22 — a mixed interleaved sequence arrives in exact order, exactly once,
/// byte-identical.
#[test]
fn stream_order_and_no_duplication() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    let sequence: Vec<TranscriptEvent> = vec![
        TranscriptEvent::SystemPrompt {
            bytes: b"e0".to_vec(),
        },
        turn(b"e1"),
        tool_call("Bash", b"e2", 1),
        TranscriptEvent::ToolResult {
            tool: "Bash".to_string(),
            bytes: b"e3".to_vec(),
        },
        turn(b"e4"),
        tool_call("Edit", b"e5", 1),
        TranscriptEvent::ToolResult {
            tool: "Edit".to_string(),
            bytes: b"e6".to_vec(),
        },
        turn(b"e7"),
    ];
    for ev in sequence {
        hook.write(ev).unwrap();
    }

    let got = drain_raw(&sub);
    let want: Vec<Vec<u8>> = (0..8u32).map(|i| format!("e{i}").into_bytes()).collect();
    assert_eq!(got, want, "exact order, byte-identical");
    assert!(
        sub.next_event().is_none(),
        "exactly once: nothing re-delivered"
    );
    assert!(sub.next_event().is_none());
}

// ── B3 ────────────────────────────────────────────────────────────────────────

/// B3 — per-turn metadata is observable progressively (right after each
/// write, before close), and `timestamp_ms` is a real occurrence clock in
/// MILLISECONDS: a ~50ms injected delay between turns yields a delta in
/// [30, 300] (pins the unit — seconds would give 0, micros ~50_000).
#[test]
fn per_turn_metadata_progressive_timestamped_in_ms() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    hook.write(turn(b"t0")).unwrap();
    let m0 = sub
        .next_meta()
        .expect("metadata must be observable progressively, right after the write");

    thread::sleep(Duration::from_millis(50));

    hook.write(turn(b"t1")).unwrap();
    let m1 = sub.next_meta().expect("second meta entry");

    assert_eq!(m0.turn_index, 0);
    assert_eq!(m1.turn_index, 1);
    let delta = m1.timestamp_ms - m0.timestamp_ms;
    assert!(
        (30..=300).contains(&delta),
        "timestamp delta {delta} outside [30, 300]ms for a ~50ms gap — unit must be ms"
    );
}

// ── B25 ───────────────────────────────────────────────────────────────────────

/// B25 — metadata optionals are honest BOTH ways: present when known
/// (tool name on a tool call; exact token total when usage is reported),
/// `None` when unknown — never a fabricated `Some(0)`.
#[test]
fn metadata_optional_fields_honest_both_ways() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    // Tool-call turn: tool is Some(name); no usage → tokens None.
    hook.write(tool_call("Bash", b"call", 2)).unwrap();
    // Usage-known model turn: tokens Some(exact derived total); tool None.
    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"turn-known".to_vec(),
        usage: Some(TurnUsage {
            input: 7,
            output: 11,
            cache_read: 13,
            cache_write: 17,
        }),
        busy_ms: 1,
    })
    .unwrap();
    // Usage-unknown model turn: tokens None (NEVER Some(0)); tool None.
    hook.write(turn(b"turn-unknown")).unwrap();

    let metas = drain_meta(&sub);
    assert_eq!(metas.len(), 3);

    assert_eq!(metas[0].tool.as_deref(), Some("Bash"));
    assert_eq!(metas[0].tokens, None, "a tool call carries no usage");

    assert_eq!(metas[1].tool, None, "a model turn is not a tool");
    assert_eq!(
        metas[1].tokens,
        Some(7 + 11 + 13 + 17),
        "known usage → the exact derived total"
    );

    assert_eq!(metas[2].tool, None);
    assert_eq!(
        metas[2].tokens, None,
        "unknown usage must be None — never fabricated as Some(0)"
    );
}

/// B25b — the TurnMeta token total is SATURATING, not wrapping/panicking.
/// `usage` is UNTRUSTED job input; a near-`u64::MAX` value across the four
/// §13.1 classes must CAP at `u64::MAX` (release: no silent wrap; debug: no
/// `attempt to add with overflow` panic on the write path). This mirrors the
/// collector's saturating posture and is an INTERNAL fix — the TurnMeta total
/// is the side-channel, not the frozen IntentMetrics wire shape.
#[test]
fn metadata_token_total_saturates_on_malicious_usage() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    // Four near-MAX classes: a plain `+` would overflow (panic in debug, wrap
    // in release). The honest cap is u64::MAX.
    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"malicious-usage".to_vec(),
        usage: Some(TurnUsage {
            input: u64::MAX,
            output: u64::MAX,
            cache_read: u64::MAX,
            cache_write: 7,
        }),
        busy_ms: 0,
    })
    .expect("write must not panic on a near-MAX usage sum");

    let metas = drain_meta(&sub);
    assert_eq!(metas.len(), 1);
    assert_eq!(
        metas[0].tokens,
        Some(u64::MAX),
        "untrusted usage sum must saturate at u64::MAX, never wrap or panic"
    );
}

// ── B16 ───────────────────────────────────────────────────────────────────────

/// B16 — the metadata side-channel is aligned 1:1 with the raw stream:
/// indices 0..n, strictly monotone, one entry per event.
#[test]
fn metadata_aligned_with_raw_stream() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    let n = 5u64;
    for i in 0..n {
        if i % 2 == 0 {
            hook.write(turn(format!("ev-{i}").as_bytes())).unwrap();
        } else {
            hook.write(tool_call("Bash", format!("ev-{i}").as_bytes(), 1))
                .unwrap();
        }
    }

    let raw = drain_raw(&sub);
    let metas = drain_meta(&sub);
    assert_eq!(raw.len(), metas.len(), "1:1 — one meta entry per raw event");
    let indices: Vec<u64> = metas.iter().map(|m| m.turn_index).collect();
    assert_eq!(
        indices,
        (0..n).collect::<Vec<u64>>(),
        "indices 0..n, monotone"
    );
}

// ── B4 ────────────────────────────────────────────────────────────────────────

/// B4 — the job-close signal carries the FINAL wall/active figures, and
/// they are the SAME metrics value the outcome carries (single source of
/// truth: finalized exactly once, then shared).
#[test]
fn job_close_signal_carries_final_wall_and_active_ms() {
    let hook = open_hook(1_000, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"turn".to_vec(),
        usage: None,
        busy_ms: 25,
    })
    .unwrap();
    hook.write(tool_call("Bash", b"call", 17)).unwrap();
    let _ = drain_raw(&sub);
    let _ = drain_meta(&sub);
    thread::sleep(Duration::from_millis(60)); // wall must exceed busy (42ms)

    let acker = spawn_acker(sub, CRED_A);
    let jc = JobClose::new(&hook);
    let out = jc
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    let signal = acker.join().unwrap();

    assert_eq!(
        signal.metrics, out.metrics,
        "the signal and the outcome must carry the SAME finalized metrics value"
    );
    assert_eq!(signal.status, out.status);
    assert_eq!(out.metrics.active_ms, 42, "final active = busy sum");
    assert!(
        out.metrics.wall_ms >= 60,
        "final wall covers the whole job window; got {}",
        out.metrics.wall_ms
    );
}

// ── B18 ───────────────────────────────────────────────────────────────────────

/// B18 — `capture_incomplete` reflects drainage at close time: both
/// channels drained before the close signal → false; undelivered residue
/// on the RAW or the META surface at close time → true (two sub-cases).
#[test]
fn both_channels_drained_before_close_signal() {
    // Sub-case 0: fully drained before close + in-window ack → complete.
    let hook = open_hook(1_000, 64);
    let sub = hook.subscribe(CRED_A).unwrap();
    hook.write(turn(b"drained-1")).unwrap();
    hook.write(tool_call("Bash", b"drained-2", 1)).unwrap();
    let _ = drain_raw(&sub);
    let _ = drain_meta(&sub);
    let acker = spawn_acker(sub, CRED_A);
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();
    assert!(
        !out.capture_incomplete,
        "drained + acked → capture complete"
    );

    // Sub-case 1: RAW residue at close time (meta drained) → incomplete,
    // even with an in-window ack.
    let hook = open_hook(1_000, 64);
    let sub = hook.subscribe(CRED_A).unwrap();
    hook.write(turn(b"raw-residue")).unwrap();
    let _ = drain_meta(&sub);
    let acker = spawn_acker(sub, CRED_A);
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();
    assert!(out.capture_incomplete, "raw residue at close → incomplete");

    // Sub-case 2: META residue at close time (raw drained) → incomplete.
    let hook = open_hook(1_000, 64);
    let sub = hook.subscribe(CRED_A).unwrap();
    hook.write(turn(b"meta-residue")).unwrap();
    let _ = drain_raw(&sub);
    let acker = spawn_acker(sub, CRED_A);
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();
    assert!(out.capture_incomplete, "meta residue at close → incomplete");

    // Sub-case 3: residue at SIGNAL time but drained IN-WINDOW before the
    // ack → complete. §13.2(3): the forge finalises the blobs between the
    // close signal and its ack, so in-window draining counts as delivered.
    let hook = open_hook(2_000, 64);
    let sub = hook.subscribe(CRED_A).unwrap();
    hook.write(turn(b"in-window-drain")).unwrap();
    let h = hook.clone();
    let drainer_acker = std::thread::spawn(move || {
        // Give close() time to publish the signal first.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = drain_raw(&sub);
        let _ = drain_meta(&sub);
        sub.ack(CRED_A).unwrap();
        drop(h);
    });
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    drainer_acker.join().unwrap();
    assert!(
        !out.capture_incomplete,
        "residue drained in-window before ack → capture complete"
    );
}

// ── B5 ────────────────────────────────────────────────────────────────────────

/// B5 — an unacked close FAILS CLOSED: after ~`ack_timeout` the outcome is
/// produced with `capture_incomplete: true`, and `released() == true`.
/// Fail-closed means the lease still CLOSES — the job is over and the lease
/// must never hang on a missing forge ack; "closed without forge ack" is
/// expressed by the flag, not by withholding the release.
#[test]
fn unacked_close_fails_closed_with_capture_incomplete() {
    let hook = open_hook(100, 64);
    let sub = hook.subscribe(CRED_A).unwrap();
    hook.write(turn(b"job")).unwrap();
    let _ = drain_raw(&sub);
    let _ = drain_meta(&sub);
    // No acker: the subscriber never acknowledges.

    let jc = JobClose::new(&hook);
    let t0 = Instant::now();
    let out = jc
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    assert!(
        t0.elapsed() >= Duration::from_millis(80),
        "the close must have waited out the configured ack window"
    );
    assert!(
        out.capture_incomplete,
        "no ack in the window → capture_incomplete (the forge never confirmed the blobs)"
    );
    assert!(
        jc.released(),
        "fail-closed still closes: the lease is released after the window, never hangs"
    );
}

// ── B29 ───────────────────────────────────────────────────────────────────────

/// B29 — an ack INSIDE the window (here at ~half of it) is never treated as
/// a timeout: the outcome is capture-complete.
#[test]
fn in_window_ack_never_treated_as_timeout() {
    let hook = open_hook(300, 64);
    let sub = hook.subscribe(CRED_A).unwrap();
    hook.write(turn(b"job")).unwrap();
    let _ = drain_raw(&sub);
    let _ = drain_meta(&sub);

    let acker = thread::spawn(move || {
        sub.wait_close_signal(Duration::from_secs(5))
            .expect("close signal");
        thread::sleep(Duration::from_millis(120)); // ~half the 300ms window
        sub.ack(CRED_A).expect("in-window ack must be accepted");
    });
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();
    assert!(
        !out.capture_incomplete,
        "an in-window ack must never be misread as a timeout"
    );
}

// ── B28 ───────────────────────────────────────────────────────────────────────

/// B28 — an ack AFTER the timeout fired is inert: it errors, and it cannot
/// flip the already-produced outcome (the flag stays true).
#[test]
fn late_ack_after_timeout_is_inert() {
    let hook = open_hook(50, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    let jc = JobClose::new(&hook);
    let out = jc
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    assert!(out.capture_incomplete, "timed out without an ack");

    // The window is gone: the late ack is refused and changes nothing.
    let err = sub.ack(CRED_A).unwrap_err().to_string();
    assert!(
        err.contains("inert") || err.contains("already"),
        "late ack must be inert; got: {err}"
    );
    assert!(
        out.capture_incomplete,
        "the produced outcome cannot be flipped"
    );
    assert!(jc.released());
}

// ── B33 ───────────────────────────────────────────────────────────────────────

/// B33 — an ack BEFORE any close was signalled is inert (`Err` — an ack
/// cannot be pre-armed); a subsequent normal close + in-window ack then
/// behaves exactly as if no early ack had happened.
#[test]
fn early_ack_is_inert() {
    let hook = open_hook(500, 64);
    let sub = hook.subscribe(CRED_A).unwrap();

    let err = sub.ack(CRED_A).unwrap_err().to_string();
    assert!(
        err.contains("no close signal"),
        "pre-arming an ack must be refused; got: {err}"
    );

    // The early attempt left nothing armed: the close still requires (and
    // honors) a real in-window ack.
    let acker = spawn_acker(sub, CRED_A);
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();
    assert!(
        !out.capture_incomplete,
        "the post-signal ack counted normally"
    );
}

// ── B9 ────────────────────────────────────────────────────────────────────────

/// B9 — an acked close releases with capture complete, and ONLY after the
/// ack: `released()` is false while the close is signalled-but-unacked,
/// true once the ack lands.
#[test]
fn acked_close_releases_with_capture_complete() {
    let hook = open_hook(2_000, 64);
    let sub = hook.subscribe(CRED_A).unwrap();
    hook.write(turn(b"job")).unwrap();
    let _ = drain_raw(&sub);
    let _ = drain_meta(&sub);

    let jc = JobClose::new(&hook);
    let jc_bg = jc.clone();
    let closer = thread::spawn(move || {
        jc_bg
            .close(JobStatus::Succeeded, Instant::now(), &zero_price())
            .unwrap()
    });

    // The signal is out, the ack is not: the lease must NOT be released yet.
    sub.wait_close_signal(Duration::from_secs(2))
        .expect("close signal");
    assert!(
        !jc.released(),
        "released() must stay false until the forge acks (§13.2: blobs first)"
    );

    sub.ack(CRED_A).unwrap();
    let out = closer.join().unwrap();
    assert!(!out.capture_incomplete);
    assert!(jc.released(), "released() flips true only after the ack");
}

// ── B21 ───────────────────────────────────────────────────────────────────────

/// B21 — the close signal is exactly-once: a second close attempt (normal
/// or abnormal, on any handle of the same hook) is refused with `Err`.
#[test]
fn close_signal_exactly_once() {
    let hook = open_hook(50, 64);
    let jc = JobClose::new(&hook);
    jc.close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();

    let err = jc
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("exactly-once"),
        "second close must be refused; got: {err}"
    );
    // Also via a fresh handle and via the abnormal path: same shared state.
    assert!(
        JobClose::new(&hook)
            .close_abnormal(AbnormalKind::Expiry, Instant::now(), &zero_price())
            .is_err()
    );
}

// ── B19 ───────────────────────────────────────────────────────────────────────

/// B19 — abnormal termination closes the hook fail-closed: both surfaces
/// refuse further writes, a single outcome is produced with
/// `capture_incomplete: true` and honestly finalized metrics (wall =
/// born→kill, active = busy so far); a second close is refused. Sub-cases
/// for Expiry and Crash.
#[test]
fn abnormal_termination_closes_hook_fail_closed() {
    // ── Expiry ────────────────────────────────────────────────────────────
    let hook = open_hook(500, 64);
    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"t".to_vec(),
        usage: None,
        busy_ms: 30,
    })
    .unwrap();
    hook.write(tool_call("Bash", b"c", 20)).unwrap();
    thread::sleep(Duration::from_millis(60)); // wall window > 50ms busy

    let jc = JobClose::new(&hook);
    let out = jc
        .close_abnormal(AbnormalKind::Expiry, Instant::now(), &zero_price())
        .unwrap();
    assert_eq!(
        out.status,
        JobStatus::Killed,
        "expiry hard-kill maps to Killed"
    );
    assert!(
        out.capture_incomplete,
        "abnormal close can never claim full capture"
    );
    assert_eq!(
        out.metrics.active_ms, 50,
        "active = busy accumulated so far"
    );
    assert!(
        (55..=2_000).contains(&out.metrics.wall_ms),
        "wall = born→kill; got {}",
        out.metrics.wall_ms
    );
    // Surfaces refuse writes after the abnormal close.
    assert!(hook.write(turn(b"post-mortem")).is_err());
    assert!(
        hook.write(tool_call("Edit", b"post-mortem-meta", 0))
            .is_err()
    );
    // Single outcome: any second close is refused.
    assert!(
        jc.close_abnormal(AbnormalKind::Expiry, Instant::now(), &zero_price())
            .is_err()
    );
    assert!(
        jc.close(JobStatus::Failed, Instant::now(), &zero_price())
            .is_err()
    );
    assert!(jc.released());

    // ── Crash (sub-case) ──────────────────────────────────────────────────
    let hook = open_hook(500, 64);
    hook.write(turn(b"before-crash")).unwrap();
    let jc = JobClose::new(&hook);
    let out = jc
        .close_abnormal(AbnormalKind::Crash, Instant::now(), &zero_price())
        .unwrap();
    assert_eq!(out.status, JobStatus::Failed, "a crash is a failed job");
    assert!(out.capture_incomplete);
    assert!(hook.write(turn(b"post-crash")).is_err());
    assert!(
        jc.close_abnormal(AbnormalKind::Crash, Instant::now(), &zero_price())
            .is_err()
    );
}

// ── B24 ───────────────────────────────────────────────────────────────────────

/// B24 — two concurrent leases never cross-contaminate: distinct hooks with
/// distinct credentials, interleaved writes from two threads; each
/// subscriber sees ONLY its own stream; lease A's ack does not satisfy
/// lease B's pending close; each outcome carries exact per-job metrics.
#[test]
fn concurrent_leases_never_cross_contaminate() {
    let hook_a = CaptureHook::open(
        cfg(2_000, 64),
        CRED_A,
        MetricsCollector::new(Instant::now()),
    );
    let hook_b = CaptureHook::open(cfg(200, 64), CRED_B, MetricsCollector::new(Instant::now()));

    // Interleaved writers: A = 5 model turns + 5 Bash calls; B = 3 model
    // turns + 7 Edit calls (distinct shapes so conflation is detectable).
    let ha = hook_a.clone();
    let writer_a = thread::spawn(move || {
        for i in 0..5u32 {
            ha.write(turn(format!("A-turn-{i}").as_bytes())).unwrap();
            ha.write(tool_call("Bash", format!("A-call-{i}").as_bytes(), 1))
                .unwrap();
        }
    });
    let hb = hook_b.clone();
    let writer_b = thread::spawn(move || {
        for i in 0..3u32 {
            hb.write(turn(format!("B-turn-{i}").as_bytes())).unwrap();
        }
        for i in 0..7u32 {
            hb.write(tool_call("Edit", format!("B-call-{i}").as_bytes(), 1))
                .unwrap();
        }
    });
    writer_a.join().unwrap();
    writer_b.join().unwrap();

    // Each subscriber sees only its own lease's bytes.
    let sub_a = hook_a.subscribe(CRED_A).unwrap();
    let sub_b = hook_b.subscribe(CRED_B).unwrap();
    let raw_a = drain_raw(&sub_a);
    let raw_b = drain_raw(&sub_b);
    assert_eq!(raw_a.len(), 10);
    assert_eq!(raw_b.len(), 10);
    for e in &raw_a {
        assert!(e.starts_with(b"A-"), "lease A saw a foreign event: {e:?}");
    }
    for e in &raw_b {
        assert!(e.starts_with(b"B-"), "lease B saw a foreign event: {e:?}");
    }
    let _ = drain_meta(&sub_a);
    let _ = drain_meta(&sub_b);

    // Both closes pending concurrently; ONLY lease A is acked.
    let jc_a = JobClose::new(&hook_a);
    let jc_b = JobClose::new(&hook_b);
    let jc_a_bg = jc_a.clone();
    let closer_a = thread::spawn(move || {
        jc_a_bg
            .close(JobStatus::Succeeded, Instant::now(), &zero_price())
            .unwrap()
    });
    let jc_b_bg = jc_b.clone();
    let closer_b = thread::spawn(move || {
        jc_b_bg
            .close(JobStatus::Succeeded, Instant::now(), &zero_price())
            .unwrap()
    });

    sub_a
        .wait_close_signal(Duration::from_secs(2))
        .expect("lease A close signal");
    sub_a.ack(CRED_A).unwrap();

    let out_a = closer_a.join().unwrap();
    let out_b = closer_b.join().unwrap();
    assert!(!out_a.capture_incomplete, "A drained + acked");
    assert!(
        out_b.capture_incomplete,
        "A's ack must NOT satisfy B's pending close — B times out"
    );

    // Exact per-job metrics: no bleed between the two collectors.
    assert_eq!(out_a.metrics.model_turns, 5);
    assert_eq!(out_a.metrics.tool_calls, 5);
    assert_eq!(out_b.metrics.model_turns, 3);
    assert_eq!(out_b.metrics.tool_calls, 7);
}

// ── B10 ───────────────────────────────────────────────────────────────────────

/// B10 — buffer overflow is never silent: with capacity 2 and a stalled
/// subscriber, overfilling a surface drops the new entries (bounded,
/// in-memory) but ALWAYS surfaces as `capture_incomplete: true` at close —
/// even with an in-window ack. Sub-cases for the raw and the META surface;
/// the no-durable-spill law is re-asserted on the module source.
#[test]
fn buffer_overflow_is_never_silent() {
    // ── Raw surface overflow ──────────────────────────────────────────────
    let hook = open_hook(1_000, 2);
    let sub = hook.subscribe(CRED_A).unwrap();
    for i in 0..5u32 {
        // Stalled subscriber: nothing drained while 5 events arrive.
        hook.write(turn(format!("raw-{i}").as_bytes())).unwrap();
    }
    assert_eq!(
        hook.raw_buffer_len(),
        2,
        "bounded: never grows past capacity"
    );
    // Drain what survived so residue can't mask the overflow attribution.
    assert_eq!(drain_raw(&sub).len(), 2);
    assert_eq!(drain_meta(&sub).len(), 2);
    let acker = spawn_acker(sub, CRED_A);
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();
    assert!(
        out.capture_incomplete,
        "raw overflow must surface at close even with a drained buffer and an in-window ack"
    );

    // ── Meta surface overflow (sub-case) ──────────────────────────────────
    // Raw is drained after every write (never full); meta is left to fill.
    let hook = open_hook(1_000, 2);
    let sub = hook.subscribe(CRED_A).unwrap();
    for i in 0..5u32 {
        hook.write(turn(format!("meta-{i}").as_bytes())).unwrap();
        assert!(sub.next_event().is_some(), "raw kept drained");
    }
    assert_eq!(hook.raw_buffer_len(), 0, "raw never overflowed");
    assert_eq!(hook.meta_buffer_len(), 2, "meta bounded at capacity");
    assert_eq!(drain_meta(&sub).len(), 2);
    let acker = spawn_acker(sub, CRED_A);
    let out = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .unwrap();
    acker.join().unwrap();
    assert!(
        out.capture_incomplete,
        "META overflow must surface at close exactly like raw overflow"
    );

    // ── No durable spill (re-assert the B2 source oracle) ────────────────
    let hook_src = include_str!("../src/envelope/hook.rs");
    let close_src = include_str!("../src/envelope/close.rs");
    for needle in ["std::fs", "File::", "rusqlite", "sled", "reqwest"] {
        assert!(
            !hook_src.contains(needle) && !close_src.contains(needle),
            "overflow must never spill to a durable medium ({needle:?} found)"
        );
    }
}

// ── B8 ────────────────────────────────────────────────────────────────────────

/// B8 — every hook surface is credential-gated in BOTH directions: a
/// wrong/absent credential is refused on subscribe AND on ack (and a
/// refused ack never counts); credentials are per-hook, so lease A's valid
/// credential is refused on lease B's hook.
#[test]
fn all_hook_surfaces_credential_gated_both_directions() {
    let hook_a = CaptureHook::open(cfg(200, 8), CRED_A, MetricsCollector::new(Instant::now()));
    let hook_b = CaptureHook::open(cfg(200, 8), CRED_B, MetricsCollector::new(Instant::now()));

    // Subscribe direction.
    assert!(hook_a.subscribe("wrong-credential").is_err());
    assert!(hook_a.subscribe("").is_err(), "absent credential refused");
    assert!(
        hook_b.subscribe(CRED_A).is_err(),
        "lease A's credential must be refused on lease B's hook (per-hook gate)"
    );
    let sub_a = hook_a
        .subscribe(CRED_A)
        .expect("the right credential subscribes");

    // Ack direction: a wrong-credential ack errors AND does not count —
    // the close still times out as unacked.
    let jc = JobClose::new(&hook_a);
    let jc_bg = jc.clone();
    let closer = thread::spawn(move || {
        jc_bg
            .close(JobStatus::Succeeded, Instant::now(), &zero_price())
            .unwrap()
    });
    sub_a
        .wait_close_signal(Duration::from_secs(2))
        .expect("close signal");
    assert!(
        sub_a.ack(CRED_B).is_err(),
        "foreign credential refused on ack"
    );
    assert!(sub_a.ack("").is_err(), "absent credential refused on ack");
    let out = closer.join().unwrap();
    assert!(
        out.capture_incomplete,
        "refused acks must not count: the close timed out unacked"
    );
}

// ── B27 ───────────────────────────────────────────────────────────────────────

/// B27 — the transcript channel is OUTSIDE the credential-scan scope (§13.3
/// last bullet): with the hook live and secret-shaped bytes in flight, the
/// process environment contains no secret material introduced by the hook,
/// and no file appears on disk. This is the in-process analogue of the §5
/// `env=0, proc=0, disk=0` scan; the box-level §5 oracle itself stays
/// runner-side at C2a (`acceptance_c2a`) and is NOT re-run here.
#[test]
fn transcript_channel_outside_credential_scan_scope() {
    let marker = "S13-B27-SECRET-MARKER-7f3a91";
    let secret = format!("export INJECTED_KEY=AKIA{marker}EXAMPLE");

    // A fresh, empty scratch dir to detect any file creation by the hook.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("s13-b27-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listing = |d: &std::path::Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    let before = listing(&dir);
    assert!(before.is_empty(), "scratch dir starts empty");

    // Hook live, secret-shaped bytes IN FLIGHT (written, not yet drained).
    let hook = open_hook(500, 8);
    hook.write(TranscriptEvent::SystemPrompt {
        bytes: secret.clone().into_bytes(),
    })
    .unwrap();
    assert_eq!(hook.raw_buffer_len(), 1, "the secret is in flight");

    // env scan: the hook must not have leaked the marker into the process
    // environment (transcript bytes are a separate, authenticated stream —
    // never env material).
    for (k, v) in std::env::vars_os() {
        let k = k.to_string_lossy();
        let v = v.to_string_lossy();
        assert!(
            !k.contains(marker) && !v.contains(marker),
            "secret marker leaked into the process environment at {k}"
        );
    }

    // disk scan: the scratch dir is unchanged (no spill file appeared).
    let after = listing(&dir);
    assert_eq!(before, after, "no file may be created by the hook");

    // The in-flight bytes are still byte-identical for the subscriber.
    let sub = hook.subscribe(CRED_A).unwrap();
    assert_eq!(sub.next_event().as_deref(), Some(secret.as_bytes()));

    std::fs::remove_dir_all(&dir).unwrap();
}
