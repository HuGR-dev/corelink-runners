//! Real-Postgres proof for the internal stale-Pending cleanup fence.
//!
//! These tests intentionally skip when `TEST_DATABASE_URL` is absent; the normal
//! builder has no disposable Postgres service. They are not a mock substitute
//! for the transaction race.

use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use corelink_fabric::ledger::{AdmitOutcome, ComputeGate};
use corelink_fabric::{LeaseLedger, LeaseRecord, LeaseState, PgLedger, PgTlsMode, TenantId};
use corelink_runners_contracts::RunnerState;

fn db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    format!("pending-cleanup-pg-{label}-{nanos}")
}

fn pending(id: &str, tenant: &TenantId, created_at_ms: u64) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.to_owned(),
        tenant: tenant.clone(),
        state: LeaseState::Pending,
        box_ref: "opaque-box".to_owned(),
        created_at_ms,
        updated_at_ms: created_at_ms,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}

fn connect(url: &str) -> (tokio::runtime::Runtime, PgLedger) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let ledger = rt
        .block_on(PgLedger::connect(url, 4, PgTlsMode::Disable))
        .expect("isolated test database must accept PgLedger DDL");
    (rt, ledger)
}

#[test]
fn claim_fences_pending_to_held_race_and_finish_is_idempotent() {
    let Some(url) = db_url() else {
        eprintln!("pending_cleanup_pg race: TEST_DATABASE_URL unset — UNRUN");
        return;
    };
    let (_rt, ledger) = connect(&url);
    let tenant = TenantId::new(nonce("tenant")).unwrap();
    let id = nonce("race");
    assert!(ledger.try_admit(pending(&id, &tenant, 1), 2).unwrap());

    let gate = Arc::new(Barrier::new(3));
    let claim_ledger = ledger.clone();
    let claim_gate = Arc::clone(&gate);
    let claim = std::thread::spawn(move || {
        claim_gate.wait();
        claim_ledger
            .claim_stale_pending_cleanup(100, 10)
            .expect("claim transaction")
    });
    let held_ledger = ledger.clone();
    let held_id = id.clone();
    let held_gate = Arc::clone(&gate);
    let held = std::thread::spawn(move || {
        held_gate.wait();
        held_ledger.transition(&held_id, RunnerState::Held, 100)
    });
    gate.wait();

    let claimed = claim.join().expect("claim thread");
    let held = held.join().expect("transition thread");
    if claimed.iter().any(|row| row.lease_id == id) {
        assert!(held.is_err(), "a cleanup claim must fence Pending->Held");
        assert!(ledger.finish_pending_cleanup(&id).unwrap());
        assert!(!ledger.finish_pending_cleanup(&id).unwrap());
        assert!(ledger.get(&id).unwrap().is_none());
    } else {
        assert!(held.is_ok(), "if Held won, claim must leave it untouched");
        assert!(!ledger.finish_pending_cleanup(&id).unwrap());
        assert!(ledger.get(&id).unwrap().unwrap().state.is_held());
    }
}

#[test]
fn accounting_pending_claim_keeps_slot_and_reservation_until_finish() {
    let Some(url) = db_url() else {
        eprintln!("pending_cleanup_pg accounting: TEST_DATABASE_URL unset — UNRUN");
        return;
    };
    let (_rt, ledger) = connect(&url);
    let tenant = TenantId::new(nonce("accounting-tenant")).unwrap();
    let gate = ComputeGate {
        period_key: 202609,
        ceiling_vcpu_ms: 15,
        box_vcpu_count: 1,
        new_reserved_vcpu_ms: 10,
    };
    let id = nonce("accounting-old");
    assert_eq!(
        ledger
            .try_admit_with_compute(pending(&id, &tenant, 1), 1, Some(gate))
            .unwrap(),
        AdmitOutcome::Admitted
    );
    assert!(
        ledger
            .claim_stale_pending_cleanup(100, 10)
            .unwrap()
            .iter()
            .any(|row| row.lease_id == id),
        "the accounting-on Pending row is claimed"
    );
    assert_eq!(
        ledger
            .try_admit_with_compute(pending(&nonce("blocked"), &tenant, 1), 1, Some(gate))
            .unwrap(),
        AdmitOutcome::OverConcurrency,
        "claimed Pending remains in the concurrency and compute reservation set"
    );
    assert_eq!(
        ledger
            .try_admit_with_compute(
                pending(&nonce("compute-blocked"), &tenant, 1),
                100,
                Some(gate)
            )
            .unwrap(),
        AdmitOutcome::OverCompute,
        "with concurrency nonbinding, the claimed Pending reservation still consumes Σ"
    );
    assert!(ledger.finish_pending_cleanup(&id).unwrap());
    assert_eq!(
        ledger
            .try_admit_with_compute(pending(&nonce("freed"), &tenant, 1), 100, Some(gate))
            .unwrap(),
        AdmitOutcome::Admitted,
        "conditional finish releases the slot and reservation"
    );
}

#[test]
fn claimed_pending_refuses_every_normal_mutator_for_accounting_off_and_on() {
    let Some(url) = db_url() else {
        eprintln!("pending_cleanup_pg mutator fence: TEST_DATABASE_URL unset — UNRUN");
        return;
    };
    let (_rt, ledger) = connect(&url);
    let tenant = TenantId::new(nonce("fence-tenant")).unwrap();

    let off = nonce("fence-off");
    assert!(ledger.try_admit(pending(&off, &tenant, 1), 10).unwrap());
    let first_claim = ledger.claim_stale_pending_cleanup(100, 10).unwrap();
    assert!(first_claim.iter().any(|row| row.lease_id == off));
    let retry_claim = ledger.claim_stale_pending_cleanup(100, 10).unwrap();
    assert!(
        retry_claim.iter().any(|row| row.lease_id == off),
        "claimed Pending retries"
    );
    assert!(!ledger.remove(&off).unwrap());
    assert!(!ledger.remove_if_pending(&off).unwrap());
    assert!(ledger.transition(&off, RunnerState::Held, 100).is_err());
    assert!(matches!(
        ledger.get(&off).unwrap().unwrap().state,
        LeaseState::Pending
    ));
    assert!(ledger.finish_pending_cleanup(&off).unwrap());

    let on = nonce("fence-on");
    let compute = ComputeGate {
        period_key: 202609,
        ceiling_vcpu_ms: 100,
        box_vcpu_count: 1,
        new_reserved_vcpu_ms: 10,
    };
    assert_eq!(
        ledger
            .try_admit_with_compute(pending(&on, &tenant, 1), 10, Some(compute))
            .unwrap(),
        AdmitOutcome::Admitted
    );
    ledger
        .set_envelope_checkpoint(&on, "{\"redacted\":true}")
        .unwrap();
    assert!(
        ledger
            .claim_stale_pending_cleanup(100, 10)
            .unwrap()
            .iter()
            .any(|row| row.lease_id == on)
    );
    assert!(!ledger.remove(&on).unwrap());
    assert!(!ledger.remove_if_pending(&on).unwrap());
    assert!(ledger.transition(&on, RunnerState::Held, 100).is_err());
    assert!(matches!(
        ledger.get(&on).unwrap().unwrap().state,
        LeaseState::Pending
    ));
    assert!(ledger.finish_pending_cleanup(&on).unwrap());
    assert!(ledger.get_envelope_checkpoint(&on).unwrap().is_none());
}

#[test]
fn named_pending_claim_is_exact_and_never_claims_held_for_accounting_off_or_on() {
    let Some(url) = db_url() else {
        eprintln!("pending_cleanup_pg named claim: TEST_DATABASE_URL unset — UNRUN");
        return;
    };
    let (_rt, ledger) = connect(&url);
    let tenant = TenantId::new(nonce("named-tenant")).unwrap();

    let off = nonce("named-off");
    let untouched = nonce("named-untouched");
    assert!(ledger.try_admit(pending(&off, &tenant, 100), 10).unwrap());
    assert!(
        ledger
            .try_admit(pending(&untouched, &tenant, 100), 10)
            .unwrap()
    );
    assert_eq!(
        ledger
            .claim_pending_cleanup(&off, 1)
            .unwrap()
            .unwrap()
            .lease_id,
        off,
        "named rollback claims its fresh Pending without using a fake stale cutoff"
    );
    assert!(matches!(
        ledger.get(&untouched).unwrap().unwrap().state,
        LeaseState::Pending
    ));
    assert_eq!(
        ledger
            .claim_pending_cleanup(&off, 2)
            .unwrap()
            .unwrap()
            .lease_id,
        off,
        "the named claim is returned for rollback retry"
    );
    assert!(ledger.finish_pending_cleanup(&off).unwrap());

    let held = nonce("named-held");
    assert!(ledger.try_admit(pending(&held, &tenant, 100), 10).unwrap());
    ledger.transition(&held, RunnerState::Held, 101).unwrap();
    assert!(ledger.claim_pending_cleanup(&held, 102).unwrap().is_none());
    assert!(ledger.get(&held).unwrap().unwrap().state.is_held());

    let on = nonce("named-on");
    let compute = ComputeGate {
        period_key: 202609,
        ceiling_vcpu_ms: 100,
        box_vcpu_count: 1,
        new_reserved_vcpu_ms: 10,
    };
    assert_eq!(
        ledger
            .try_admit_with_compute(pending(&on, &tenant, 100), 10, Some(compute))
            .unwrap(),
        AdmitOutcome::Admitted
    );
    assert_eq!(
        ledger
            .claim_pending_cleanup(&on, 1)
            .unwrap()
            .unwrap()
            .lease_id,
        on,
        "accounting-on Pending uses the same named row/advisory fence"
    );
    assert!(ledger.transition(&on, RunnerState::Held, 102).is_err());
    assert!(ledger.finish_pending_cleanup(&on).unwrap());
}
