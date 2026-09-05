use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};

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
