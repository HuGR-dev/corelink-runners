//! Acceptance — WP-CF0d: control-plane core types (CF0 freeze items 4 + 5).
//!
//! Pins: tenant-id shape validation · the contract §1 five-state matrix
//! (fail-closed, exactly five states representable, `RunnerState` reused) ·
//! the FileLedger restart-survival oracle · meter-event serde · the
//! slot-is-the-billable-unit doc invariant.

use std::collections::BTreeSet;

use corelink_fabric::{
    CogsCounters, FileLedger, InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, SlotEventKind,
    SlotOccupancyEvent, TenantId,
};
use corelink_runners_contracts::RunnerState;

fn tenant(s: &str) -> TenantId {
    TenantId::new(s).expect("test tenant id must be valid")
}

fn record(lease_id: &str, t: &TenantId, state: LeaseState) -> LeaseRecord {
    LeaseRecord {
        lease_id: lease_id.to_string(),
        tenant: t.clone(),
        state,
        box_ref: "box-01".to_string(),
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}

/// All five contract §1 states (Pending + the four reused wire states).
fn all_five_states() -> Vec<LeaseState> {
    vec![
        LeaseState::Pending,
        LeaseState::Wire(RunnerState::Held),
        LeaseState::Wire(RunnerState::Released),
        LeaseState::Wire(RunnerState::Expired),
        LeaseState::Wire(RunnerState::Crashed),
    ]
}

// ── tenant ────────────────────────────────────────────────────────────────

#[test]
fn tenant_id_validates_shape() {
    // Valid: non-empty lowercase [a-z0-9-].
    for ok in ["acme", "acme-corp-2", "0-9", "a"] {
        let t = TenantId::new(ok).expect("valid tenant id rejected");
        assert_eq!(t.to_string(), ok, "Display must echo the validated key");
        assert_eq!(t.as_str(), ok);
    }
    // Invalid: empty, uppercase, underscore, space, dot, non-ascii.
    for bad in ["", "Acme", "a_b", "a b", "a.b", "café", "tenant/1"] {
        assert!(
            TenantId::new(bad).is_err(),
            "tenant id {bad:?} must be rejected (non-empty lowercase [a-z0-9-] only)"
        );
    }
    // FromStr path validates too.
    assert!("acme-1".parse::<TenantId>().is_ok());
    assert!("BAD".parse::<TenantId>().is_err());
    // Serde path validates too (try_from = "String").
    assert!(serde_json::from_str::<TenantId>("\"acme-1\"").is_ok());
    assert!(
        serde_json::from_str::<TenantId>("\"NOT-ok\"").is_err(),
        "deserialization must run the same shape validation"
    );
}

// ── ledger: the contract §1 matrix ────────────────────────────────────────

#[test]
fn ledger_legal_transitions_only() {
    let t = tenant("acme");
    let legal: [(LeaseState, RunnerState); 4] = [
        (LeaseState::Pending, RunnerState::Held),
        (LeaseState::Wire(RunnerState::Held), RunnerState::Released),
        (LeaseState::Wire(RunnerState::Held), RunnerState::Expired),
        (LeaseState::Wire(RunnerState::Held), RunnerState::Crashed),
    ];
    let to_states = [
        RunnerState::Held,
        RunnerState::Released,
        RunnerState::Expired,
        RunnerState::Crashed,
    ];

    // Full matrix: every (from, to) pair, exactly the four legal cells Ok.
    let ledger = InMemoryLedger::new();
    let mut checked = 0usize;
    for (i, from) in all_five_states().iter().enumerate() {
        for (j, to) in to_states.iter().enumerate() {
            let id = format!("lease-{i}-{j}");
            ledger
                .put(record(&id, &t, from.clone()))
                .expect("seeding the matrix record must succeed");
            let is_legal = legal.iter().any(|(f, tt)| f == from && tt == to);
            let result = ledger.transition(&id, to.clone(), 2_000);
            if is_legal {
                let rec = result.unwrap_or_else(|e| {
                    panic!("legal transition {from:?} -> {to:?} must succeed: {e}")
                });
                assert_eq!(rec.state, LeaseState::Wire(to.clone()));
                assert_eq!(rec.updated_at_ms, 2_000, "transition must stamp now_ms");
            } else {
                assert!(
                    result.is_err(),
                    "illegal transition {from:?} -> {to:?} must Err (fail-closed)"
                );
                // Fail-closed means untouched: state did not move.
                let after = ledger.get(&id).unwrap().unwrap();
                assert_eq!(&after.state, from, "rejected transition must not mutate");
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 20, "5 from-states x 4 to-states = the full matrix");

    // Fail-closed edges: unknown lease errors; put never overwrites.
    assert!(
        ledger
            .transition("no-such-lease", RunnerState::Held, 3_000)
            .is_err()
    );
    assert!(
        ledger
            .put(record("lease-0-0", &t, LeaseState::Pending))
            .is_err(),
        "duplicate put must Err, never silently overwrite"
    );
}

#[test]
fn ledger_states_are_exactly_the_contract_five() {
    // Exhaustive match, NO wildcard arm: if any state beyond the contract
    // five were representable, this function would not compile. RunnerState
    // is the reused frozen wire type — the inner match is exhaustive over it.
    fn state_token(s: &LeaseState) -> &'static str {
        match s {
            LeaseState::Pending => "pending",
            LeaseState::Wire(w) => match w {
                RunnerState::Held => "held",
                RunnerState::Expired => "expired",
                RunnerState::Crashed => "crashed",
                RunnerState::Released => "released",
            },
        }
    }

    let tokens: BTreeSet<&str> = all_five_states().iter().map(state_token).collect();
    let expected: BTreeSet<&str> = ["pending", "held", "released", "expired", "crashed"]
        .into_iter()
        .collect();
    assert_eq!(tokens, expected, "exactly the contract §1 five states");
    assert_eq!(tokens.len(), 5);

    // The serialized vocabulary is the same flat five tokens (the journal
    // speaks the contract's words, with the wire four byte-identical to
    // RunnerState's own serde form).
    for state in all_five_states() {
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, format!("\"{}\"", state_token(&state)));
        let back: LeaseState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state, "five-token serde must round-trip");
    }
}

// ── ledger: restart survival (the FileLedger oracle) ──────────────────────

#[test]
fn file_ledger_survives_reopen() {
    let path = std::env::temp_dir().join(format!(
        "corelink-fabric-cf0d-{}-{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let t_a = tenant("acme");
    let t_b = tenant("globex");

    // Write: two tenants, a put + a transition each path.
    let (rec_a_final, rec_b_final) = {
        let ledger = FileLedger::open(&path).expect("fresh journal must open");
        ledger
            .put(record("lease-a", &t_a, LeaseState::Pending))
            .unwrap();
        ledger
            .put(record("lease-b", &t_b, LeaseState::Pending))
            .unwrap();
        let a = ledger
            .transition("lease-a", RunnerState::Held, 2_000)
            .unwrap();
        let b = ledger
            .transition("lease-b", RunnerState::Held, 2_500)
            .unwrap();
        let b = {
            let _ = b;
            ledger
                .transition("lease-b", RunnerState::Released, 3_000)
                .unwrap()
        };
        (a, b)
        // drop = process "restart"
    };

    // Reopen: replay must reconstruct the exact same authoritative view.
    let reopened = FileLedger::open(&path).expect("journal must replay on open");
    assert_eq!(
        reopened.get("lease-a").unwrap().as_ref(),
        Some(&rec_a_final)
    );
    assert_eq!(
        reopened.get("lease-b").unwrap().as_ref(),
        Some(&rec_b_final)
    );
    assert_eq!(reopened.by_tenant(&t_a).unwrap(), vec![rec_a_final.clone()]);
    assert_eq!(reopened.by_tenant(&t_b).unwrap(), vec![rec_b_final]);

    // The replayed state machine still enforces the matrix (fail-closed
    // across restart): lease-b is terminal, lease-a can still release.
    assert!(
        reopened
            .transition("lease-b", RunnerState::Held, 4_000)
            .is_err()
    );
    let a = reopened
        .transition("lease-a", RunnerState::Released, 4_000)
        .unwrap();
    assert_eq!(a.state, LeaseState::Wire(RunnerState::Released));

    // And that post-restart transition survives ANOTHER restart.
    drop(reopened);
    let again = FileLedger::open(&path).expect("second replay must open");
    assert_eq!(again.get("lease-a").unwrap().unwrap().state, a.state);

    let _ = std::fs::remove_file(&path);
}

// ── meter ─────────────────────────────────────────────────────────────────

#[test]
fn meter_events_roundtrip_serde() {
    let event = SlotOccupancyEvent {
        tenant: tenant("acme"),
        lease_id: "lease-a".to_string(),
        kind: SlotEventKind::Acquired,
        at_ms: 1_717_003_600_000,
        acquired_at_ms: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: SlotOccupancyEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event, "SlotOccupancyEvent must round-trip via serde");

    // All four kinds round-trip with the snake_case vocabulary.
    for (kind, token) in [
        (SlotEventKind::Acquired, "\"acquired\""),
        (SlotEventKind::Released, "\"released\""),
        (SlotEventKind::Expired, "\"expired\""),
        (SlotEventKind::Crashed, "\"crashed\""),
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, token);
        let back: SlotEventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    let cogs = CogsCounters {
        cpu_ms: 1234,
        mem_mb_ms: 567_890,
        cost_usd_micros: 4_200,
    };
    let json = serde_json::to_string(&cogs).unwrap();
    let back: CogsCounters = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cogs, "CogsCounters must round-trip via serde");
}

#[test]
fn slot_is_the_billable_unit_doc_pinned() {
    // Source-inclusion oracle: the COGS counters carry the never-billable
    // doc commitment verbatim (concurrency pricing, never per-minute).
    let meter_src = include_str!("../src/meter.rs");
    assert!(
        meter_src.contains("NEVER a billable meter"),
        "meter.rs must pin the 'NEVER a billable meter' COGS commitment"
    );
    assert!(
        meter_src.contains("never minutes") || meter_src.contains("never per-minute"),
        "meter.rs must pin the slot-not-minutes billing principle"
    );
}
