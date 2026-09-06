use corelink_fabric::{FileLedger, InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};
use corelink_runners_contracts::RunnerState;

const REF: &str = r#"provider-ref:v1:{"backend":"cf","id":"vm-1"}"#;
const OTHER_REF: &str = r#"provider-ref:v1:{"backend":"cf","id":"vm-2"}"#;

fn pending(id: &str) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.to_owned(),
        tenant: TenantId::new("provider-binding-test").unwrap(),
        state: LeaseState::Pending,
        box_ref: format!("box:{id}"),
        created_at_ms: 1,
        updated_at_ms: 1,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}

fn exercise_pending_binding(ledger: &impl LeaseLedger) {
    ledger.put(pending("pending")).unwrap();
    let bound = ledger.bind_provider_ref("pending", REF).unwrap();
    assert_eq!(bound.box_ref, REF);
    assert_eq!(ledger.bind_provider_ref("pending", REF).unwrap(), bound);
    assert!(ledger.bind_provider_ref("pending", OTHER_REF).is_err());
    assert!(ledger.bind_provider_ref("missing", REF).is_err());

    ledger.put(pending("claimed")).unwrap();
    ledger.claim_pending_cleanup("claimed", 2).unwrap().unwrap();
    assert!(ledger.bind_provider_ref("claimed", REF).is_err());

    ledger.put(pending("terminal")).unwrap();
    ledger.transition("terminal", RunnerState::Held, 3).unwrap();
    assert!(ledger.bind_provider_ref("terminal", REF).is_err());
}

#[test]
fn in_memory_binding_is_fenced_and_idempotent() {
    let ledger = InMemoryLedger::new();
    exercise_pending_binding(&ledger);
}

#[test]
fn file_binding_replays_after_reopen() {
    let path = std::env::temp_dir().join(format!(
        "corelink-provider-binding-{}-{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let ledger = FileLedger::open(&path).unwrap();
        exercise_pending_binding(&ledger);
    }
    let reopened = FileLedger::open(&path).unwrap();
    assert_eq!(reopened.get("pending").unwrap().unwrap().box_ref, REF);
    let _ = std::fs::remove_file(path);
}

#[test]
fn malformed_provider_refs_fail_before_mutation() {
    let ledger = InMemoryLedger::new();
    ledger.put(pending("invalid")).unwrap();
    for value in [
        "box:invalid",
        "provider-ref:v1:{}",
        "provider-ref:v1:[]",
        "provider-ref:v1:{bad json}",
    ] {
        assert!(ledger.bind_provider_ref("invalid", value).is_err());
    }
    assert_eq!(
        ledger.get("invalid").unwrap().unwrap().box_ref,
        "box:invalid"
    );
}
