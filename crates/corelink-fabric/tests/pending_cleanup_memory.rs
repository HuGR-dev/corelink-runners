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
    assert!(!ledger.try_admit(pending("replacement", 1_000), 2).unwrap());
    assert!(ledger.finish_pending_cleanup("stale").unwrap());
    assert!(!ledger.finish_pending_cleanup("stale").unwrap());
    assert!(ledger.try_admit(pending("replacement", 1_000), 2).unwrap());
    assert!(ledger.get("stale").unwrap().is_none());
    assert!(ledger.get("fresh").unwrap().is_some());
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

    let accounted = InMemoryLedger::new();
    let gate = ComputeGate {
        period_key: 202609,
        ceiling_vcpu_ms: 150,
        box_vcpu_count: 1,
        new_reserved_vcpu_ms: 100,
    };
    assert_eq!(
        accounted.try_admit_with_compute(pending("reserved", 1), 10, Some(gate)),
        Ok(AdmitOutcome::Admitted)
    );
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
    assert_eq!(
        accounted.try_admit_with_compute(pending("next", 1), 10, Some(next)),
        Ok(AdmitOutcome::OverCompute)
    );
    assert!(accounted.finish_pending_cleanup("reserved").unwrap());
    assert_eq!(
        accounted.try_admit_with_compute(pending("next", 1), 10, Some(next)),
        Ok(AdmitOutcome::Admitted)
    );
}
