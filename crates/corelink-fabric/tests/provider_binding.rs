use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use corelink_fabric::ledger::{AdmitOutcome, ComputeGate};
use corelink_fabric::{
    FileLedger, InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, PgLedger, PgTlsMode, TenantId,
};
use corelink_runners_contracts::RunnerState;

const REF: &str = r#"provider-ref:v1:{"backend":"cf","id":"vm-1"}"#;
const OTHER_REF: &str = r#"provider-ref:v1:{"backend":"cf","id":"vm-2"}"#;

fn pending(id: &str) -> LeaseRecord {
    pending_for(id, &TenantId::new("provider-binding-test").unwrap())
}

fn pending_for(id: &str, tenant: &TenantId) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.to_owned(),
        tenant: tenant.clone(),
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
    ledger
        .transition("terminal", RunnerState::Released, 4)
        .unwrap();
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
    assert!(reopened.bind_provider_ref("claimed", REF).is_err());
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
        &format!("provider-ref:v1:{{\"id\":\"{}\"}}", "x".repeat(4090)),
    ] {
        assert!(ledger.bind_provider_ref("invalid", value).is_err());
    }
    assert_eq!(
        ledger.get("invalid").unwrap().unwrap().box_ref,
        "box:invalid"
    );

    let mut adversarial = pending("adversarial");
    adversarial.box_ref = "box:adversarial-suffix".into();
    ledger.put(adversarial).unwrap();
    assert!(ledger.bind_provider_ref("adversarial", REF).is_err());
}

fn db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("provider-binding-pg-{label}-{nanos}")
}

fn connect(url: &str) -> (tokio::runtime::Runtime, PgLedger) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let ledger = rt
        .block_on(PgLedger::connect(url, 4, PgTlsMode::Disable))
        .unwrap();
    (rt, ledger)
}

#[test]
fn pg_bind_vs_cleanup_claim_is_fenced() {
    let Some(url) = db_url() else {
        eprintln!("provider_binding_pg race: TEST_DATABASE_URL unset — UNRUN");
        return;
    };
    let (_rt, ledger) = connect(&url);
    let tenant = TenantId::new(nonce("race-tenant")).unwrap();
    let id = nonce("race");
    assert!(ledger.try_admit(pending_for(&id, &tenant), 1).unwrap());

    let gate = Arc::new(Barrier::new(3));
    let claim_ledger = ledger.clone();
    let claim_id = id.clone();
    let claim_gate = Arc::clone(&gate);
    let claim = std::thread::spawn(move || {
        claim_gate.wait();
        claim_ledger.claim_pending_cleanup(&claim_id, 2).unwrap()
    });
    let bind_ledger = ledger.clone();
    let bind_id = id.clone();
    let bind_gate = Arc::clone(&gate);
    let bind = std::thread::spawn(move || {
        bind_gate.wait();
        bind_ledger.bind_provider_ref(&bind_id, REF)
    });
    gate.wait();
    let claimed = claim.join().unwrap();
    let bound = bind.join().unwrap();
    let record = ledger.get(&id).unwrap().unwrap();
    assert!(
        claimed.is_some(),
        "the named Pending row must remain claimable"
    );
    assert!(record.box_ref == format!("box:{id}") || record.box_ref == REF);
    if bound.is_err() {
        assert_eq!(record.box_ref, format!("box:{id}"));
    }
}

#[test]
fn pg_binding_survives_new_ledger_and_does_not_consume_reservation() {
    let Some(url) = db_url() else {
        eprintln!("provider_binding_pg persistence: TEST_DATABASE_URL unset — UNRUN");
        return;
    };
    let (rt, ledger) = connect(&url);
    let tenant = TenantId::new(nonce("persist-tenant")).unwrap();
    let id = nonce("persist");
    let gate = ComputeGate {
        period_key: 202609,
        ceiling_vcpu_ms: 15,
        box_vcpu_count: 1,
        new_reserved_vcpu_ms: 10,
    };
    assert_eq!(
        ledger
            .try_admit_with_compute(pending_for(&id, &tenant), 100, Some(gate))
            .unwrap(),
        AdmitOutcome::Admitted
    );
    ledger.bind_provider_ref(&id, REF).unwrap();
    assert_eq!(
        ledger
            .try_admit_with_compute(pending_for(&nonce("blocked"), &tenant), 100, Some(gate))
            .unwrap(),
        AdmitOutcome::OverCompute
    );
    drop(ledger);
    let reopened = rt
        .block_on(PgLedger::connect(&url, 4, PgTlsMode::Disable))
        .unwrap();
    assert_eq!(reopened.get(&id).unwrap().unwrap().box_ref, REF);
}
