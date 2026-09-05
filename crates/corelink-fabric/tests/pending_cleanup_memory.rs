use corelink_fabric::{
    AdmitOutcome, ComputeGate, InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId,
};
use std::sync::{Arc, Barrier};
use std::thread;

fn pending(id: &str, created: u64) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.into(),
        tenant: TenantId::new("cleanup-memory").unwrap(),
        state: LeaseState::Pending,
        box_ref: "box".into(),
        created_at_ms: created,
        updated_at_ms: created,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}

#[test]
fn claims_fence_race_and_finish_is_idempotent() {
    let ledger = InMemoryLedger::new();
    ledger.put(pending("stale", 1)).unwrap();
    ledger.put(pending("fresh", 900)).unwrap();
    let claimed = ledger.claim_stale_pending_cleanup(1_000, 100).unwrap();
    assert_eq!(
        claimed
            .iter()
            .map(|r| r.lease_id.as_str())
            .collect::<Vec<_>>(),
        ["stale"]
    );
    assert!(
        ledger
            .transition(
                "stale",
                corelink_runners_contracts::RunnerState::Held,
                1_001
            )
            .is_err()
    );
    assert!(ledger.remove("stale").is_err());
    assert!(!ledger.remove_if_pending("stale").unwrap());
    assert_eq!(
        ledger
            .claim_stale_pending_cleanup(1_000, 100)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ledger
            .claim_pending_cleanup("fresh", 1)
            .unwrap()
            .unwrap()
            .lease_id,
        "fresh"
    );
    assert_eq!(
        ledger
            .claim_pending_cleanup("fresh", 2)
            .unwrap()
            .unwrap()
            .lease_id,
        "fresh"
    );
    assert!(
        ledger
            .claim_pending_cleanup("missing", 1)
            .unwrap()
            .is_none()
    );
    assert!(!ledger.try_admit(pending("replacement", 1_000), 2).unwrap());
    assert!(ledger.finish_pending_cleanup("stale").unwrap());
    assert!(!ledger.finish_pending_cleanup("stale").unwrap());
    assert!(ledger.try_admit(pending("replacement", 1_000), 2).unwrap());
    assert!(ledger.get("stale").unwrap().is_none());
    assert!(ledger.get("fresh").unwrap().is_some());

    let held = InMemoryLedger::new();
    held.put(pending("held", 1)).unwrap();
    held.transition("held", corelink_runners_contracts::RunnerState::Held, 2)
        .unwrap();
    assert!(held.claim_pending_cleanup("held", 3).unwrap().is_none());
}

#[test]
fn concurrent_claimers_and_compute_reservation_are_fenced() {
    let ledger = InMemoryLedger::new();
    ledger.put(pending("race", 1)).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let left = ledger.clone();
    let right = ledger.clone();
    let a = {
        let barrier = barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            left.claim_stale_pending_cleanup(1_000, 10).unwrap()
        })
    };
    let b = {
        let barrier = barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            right.claim_stale_pending_cleanup(1_000, 10).unwrap()
        })
    };
    barrier.wait();
    assert_eq!(a.join().unwrap().len(), 1);
    assert_eq!(b.join().unwrap().len(), 1);
    assert!(
        ledger
            .transition("race", corelink_runners_contracts::RunnerState::Held, 1_001)
            .is_err()
    );

    let held_race = InMemoryLedger::new();
    held_race.put(pending("held-race", 1)).unwrap();
    let race_barrier = Arc::new(Barrier::new(3));
    let claimant = held_race.clone();
    let holder = held_race.clone();
    let claim_thread = {
        let barrier = race_barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            claimant.claim_stale_pending_cleanup(1_000, 10).unwrap()
        })
    };
    let hold_thread = {
        let barrier = race_barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            holder.transition(
                "held-race",
                corelink_runners_contracts::RunnerState::Held,
                1_001,
            )
        })
    };
    race_barrier.wait();
    let claimed = claim_thread.join().unwrap();
    let held = hold_thread.join().unwrap();
    assert!((claimed.len() == 1 && held.is_err()) || (claimed.is_empty() && held.is_ok()));

    let accounted = InMemoryLedger::new();
    let gate = ComputeGate {
        period_key: 202609,
        ceiling_vcpu_ms: 150,
        box_vcpu_count: 1,
        new_reserved_vcpu_ms: 100,
    };
    let outcome = accounted
        .try_admit_with_compute(pending("reserved", 1), 10, Some(gate))
        .unwrap();
    assert_eq!(outcome, AdmitOutcome::Admitted);
    assert_eq!(
        accounted
            .claim_stale_pending_cleanup(1_000, 10)
            .unwrap()
            .len(),
        1
    );
    let next = ComputeGate {
        new_reserved_vcpu_ms: 100,
        ..gate
    };
    let outcome = accounted
        .try_admit_with_compute(pending("next", 1), 10, Some(next))
        .unwrap();
    assert_eq!(outcome, AdmitOutcome::OverCompute);
    assert!(accounted.finish_pending_cleanup("reserved").unwrap());
    let outcome = accounted
        .try_admit_with_compute(pending("next", 1), 10, Some(next))
        .unwrap();
    assert_eq!(outcome, AdmitOutcome::Admitted);
}
