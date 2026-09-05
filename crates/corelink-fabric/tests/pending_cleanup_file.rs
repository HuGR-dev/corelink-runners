use corelink_fabric::{FileLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};

fn pending(id: &str) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.into(),
        tenant: TenantId::new("cleanup-file").unwrap(),
        state: LeaseState::Pending,
        box_ref: "box".into(),
        created_at_ms: 1,
        updated_at_ms: 1,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}

#[test]
fn claim_replays_and_tombstone_clears_on_restart() {
    let path = std::env::temp_dir().join(format!(
        "corelink-cleanup-{}-{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let ledger = FileLedger::open(&path).unwrap();
        ledger.put(pending("stale")).unwrap();
        assert_eq!(
            ledger.claim_stale_pending_cleanup(100, 10).unwrap().len(),
            1
        );
        assert!(ledger.remove("stale").is_err());
        assert!(!ledger.remove_if_pending("stale").unwrap());
    }
    {
        let ledger = FileLedger::open(&path).unwrap();
        assert_eq!(
            ledger.claim_stale_pending_cleanup(100, 10).unwrap().len(),
            1
        );
        assert!(ledger.finish_pending_cleanup("stale").unwrap());
    }
    let reopened = FileLedger::open(&path).unwrap();
    assert!(reopened.get("stale").unwrap().is_none());
    assert!(!reopened.finish_pending_cleanup("stale").unwrap());
    let _ = std::fs::remove_file(path);
}
