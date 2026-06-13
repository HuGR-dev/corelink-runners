//! Parameterized [`LeaseLedger`] conformance suite (CP1).
//!
//! The FULL behavioral contract of [`LeaseLedger`], expressed as generic
//! assertions over a ledger **factory** (`fn() -> Box<dyn LeaseLedger +
//! Send>`). Every backend — [`crate::ledger::InMemoryLedger`] now,
//! [`crate::ledger::FileLedger`] now, the Postgres impl later (ratified
//! decision #3) — is verified by IDENTICAL assertions, so a backend cannot
//! silently diverge from the §1 state machine, the tenant-isolation /
//! ordering guarantees, or the atomic [`LeaseLedger::try_admit`] cap.
//!
//! `#[cfg(test)]` only: this is a test fixture, not shipped API. The crate's
//! in-tree tests at the bottom of this file run the whole suite against BOTH
//! shipped backends.

#![cfg(test)]

use corelink_runners_contracts::RunnerState;

use crate::ledger::{LeaseLedger, LeaseRecord, LeaseState};
use crate::tenant::TenantId;

/// A backend factory: each call yields a fresh, empty ledger.
pub(crate) type LedgerFactory = fn() -> Box<dyn LeaseLedger + Send>;

fn tenant(s: &str) -> TenantId {
    TenantId::new(s).expect("test tenant id must be valid")
}

fn record(lease_id: &str, t: &TenantId, state: LeaseState) -> LeaseRecord {
    LeaseRecord {
        lease_id: lease_id.to_string(),
        tenant: t.clone(),
        state,
        box_ref: format!("box-{lease_id}"),
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
    }
}

fn pending(lease_id: &str, t: &TenantId) -> LeaseRecord {
    record(lease_id, t, LeaseState::Pending)
}

fn held(lease_id: &str, t: &TenantId) -> LeaseRecord {
    record(lease_id, t, LeaseState::Wire(RunnerState::Held))
}

/// The complete contract: run every assertion group against `make`.
pub(crate) fn run_all(make: LedgerFactory) {
    put_rejects_duplicate_and_get_resolves(make);
    transition_matrix_legal_and_illegal(make);
    by_tenant_and_held_are_isolated_and_ordered(make);
    try_admit_atomic_cap(make);
    remove_frees_cap_and_get(make);
}

/// `put` rejects a duplicate `lease_id`; `get` returns None/Some correctly.
fn put_rejects_duplicate_and_get_resolves(make: LedgerFactory) {
    let mut led = make();
    let t = tenant("acme");

    assert!(
        led.get("absent").unwrap().is_none(),
        "get on an unknown lease must be Ok(None)"
    );

    led.put(pending("lease-1", &t)).unwrap();
    let got = led
        .get("lease-1")
        .unwrap()
        .expect("get must resolve a put record");
    assert_eq!(got.lease_id, "lease-1");
    assert_eq!(got.state, LeaseState::Pending);

    assert!(
        led.put(pending("lease-1", &t)).is_err(),
        "duplicate put must Err — put never silently overwrites"
    );
}

/// The §1 transition matrix: legal pairs succeed; illegal / terminal-source /
/// unknown-lease all `Err`, and a rejected transition never mutates.
fn transition_matrix_legal_and_illegal(make: LedgerFactory) {
    let from_states = [
        LeaseState::Pending,
        LeaseState::Wire(RunnerState::Held),
        LeaseState::Wire(RunnerState::Released),
        LeaseState::Wire(RunnerState::Expired),
        LeaseState::Wire(RunnerState::Crashed),
    ];
    let to_states = [
        RunnerState::Held,
        RunnerState::Released,
        RunnerState::Expired,
        RunnerState::Crashed,
    ];

    // §1: only Pending->Held and Held->{Released|Expired|Crashed} are legal.
    fn is_legal(from: &LeaseState, to: &RunnerState) -> bool {
        matches!(
            (from, to),
            (LeaseState::Pending, RunnerState::Held)
                | (
                    LeaseState::Wire(RunnerState::Held),
                    RunnerState::Released | RunnerState::Expired | RunnerState::Crashed,
                )
        )
    }

    let t = tenant("acme");
    for (i, from) in from_states.iter().enumerate() {
        for (j, to) in to_states.iter().enumerate() {
            let mut led = make();
            let id = format!("lease-{i}-{j}");
            led.put(record(&id, &t, from.clone())).unwrap();
            let result = led.transition(&id, to.clone(), 2_000);
            if is_legal(from, to) {
                let rec = result.unwrap_or_else(|e| {
                    panic!("legal transition {from:?} -> {to:?} must succeed: {e}")
                });
                assert_eq!(
                    rec.state,
                    LeaseState::Wire(to.clone()),
                    "transition must apply `to`"
                );
                assert_eq!(rec.updated_at_ms, 2_000, "transition must stamp now_ms");
            } else {
                assert!(
                    result.is_err(),
                    "illegal transition {from:?} -> {to:?} must Err (fail-closed)"
                );
                let after = led.get(&id).unwrap().unwrap();
                assert_eq!(&after.state, from, "rejected transition must not mutate");
            }
        }
    }

    // Unknown lease -> Err (fail-closed).
    let mut led = make();
    assert!(
        led.transition("no-such-lease", RunnerState::Held, 3_000)
            .is_err(),
        "transition on an unknown lease must Err"
    );
}

/// `by_tenant` and `held` return the right sets, tenant-isolated, and ordered
/// by `lease_id` (deterministic).
fn by_tenant_and_held_are_isolated_and_ordered(make: LedgerFactory) {
    let mut led = make();
    let acme = tenant("acme");
    let globex = tenant("globex");

    // Insert out of lexical order to prove the sort, across two tenants.
    led.put(held("a-3", &acme)).unwrap();
    led.put(pending("a-1", &acme)).unwrap();
    led.put(held("a-2", &acme)).unwrap();
    led.put(held("g-2", &globex)).unwrap();
    led.put(pending("g-1", &globex)).unwrap();

    let acme_ids: Vec<String> = led
        .by_tenant(&acme)
        .unwrap()
        .into_iter()
        .map(|r| r.lease_id)
        .collect();
    assert_eq!(
        acme_ids,
        vec!["a-1", "a-2", "a-3"],
        "by_tenant must be tenant-scoped and ordered by lease_id"
    );

    let globex_ids: Vec<String> = led
        .by_tenant(&globex)
        .unwrap()
        .into_iter()
        .map(|r| r.lease_id)
        .collect();
    assert_eq!(globex_ids, vec!["g-1", "g-2"]);

    // `held` is global but only Held records, ordered by lease_id.
    let held_ids: Vec<String> = led
        .held()
        .unwrap()
        .into_iter()
        .map(|r| r.lease_id)
        .collect();
    assert_eq!(
        held_ids,
        vec!["a-2", "a-3", "g-2"],
        "held must return exactly the Held records, ordered by lease_id"
    );
}

/// `try_admit`: admits while under cap; `Ok(false)` exactly at cap; counts
/// Pending AND Held toward the cap; `max_concurrency == 0` admits nothing; a
/// successful admit is visible to `get`/`by_tenant`.
fn try_admit_atomic_cap(make: LedgerFactory) {
    let acme = tenant("acme");
    let globex = tenant("globex");

    // Zero cap admits nothing, and inserts nothing.
    {
        let mut led = make();
        assert!(
            !led.try_admit(pending("z-1", &acme), 0).unwrap(),
            "max_concurrency 0 must admit nothing"
        );
        assert!(
            led.get("z-1").unwrap().is_none(),
            "a rejected admit must not insert the record"
        );
    }

    // Admit under cap; reject exactly at cap.
    {
        let mut led = make();
        assert!(
            led.try_admit(pending("l-1", &acme), 2).unwrap(),
            "1st admit (0 < 2) must succeed"
        );
        assert!(
            led.try_admit(pending("l-2", &acme), 2).unwrap(),
            "2nd admit (1 < 2) must succeed"
        );
        assert!(
            !led.try_admit(pending("l-3", &acme), 2).unwrap(),
            "3rd admit at cap (2 == 2) must be rejected"
        );
        assert!(
            led.get("l-3").unwrap().is_none(),
            "the at-cap rejection must not insert"
        );

        // A successful admit is visible to get + by_tenant (as Pending).
        let got = led
            .get("l-1")
            .unwrap()
            .expect("admitted record visible to get");
        assert_eq!(got.state, LeaseState::Pending, "admit inserts as Pending");
        let ids: Vec<String> = led
            .by_tenant(&acme)
            .unwrap()
            .into_iter()
            .map(|r| r.lease_id)
            .collect();
        assert_eq!(
            ids,
            vec!["l-1", "l-2"],
            "admitted records visible to by_tenant"
        );
    }

    // Pending AND Held both count toward the cap.
    {
        let mut led = make();
        led.put(held("h-1", &acme)).unwrap();
        led.put(pending("p-1", &acme)).unwrap();
        // 2 active (1 Held + 1 Pending), cap 2 -> at cap, reject.
        assert!(
            !led.try_admit(pending("x-1", &acme), 2).unwrap(),
            "Pending+Held active count must fill the cap"
        );
        // Under a higher cap, the same admit succeeds (counting is correct, not
        // simply always-reject).
        assert!(
            led.try_admit(pending("x-2", &acme), 3).unwrap(),
            "with cap 3 and 2 active, the admit must succeed"
        );

        // Tenant isolation: another tenant at the same cap admits freely.
        assert!(
            led.try_admit(pending("g-1", &globex), 2).unwrap(),
            "a different tenant must not be blocked by acme's active leases"
        );
    }

    // Terminal-state records do NOT count toward the cap.
    {
        let mut led = make();
        led.put(record(
            "t-rel",
            &acme,
            LeaseState::Wire(RunnerState::Released),
        ))
        .unwrap();
        led.put(record(
            "t-exp",
            &acme,
            LeaseState::Wire(RunnerState::Expired),
        ))
        .unwrap();
        led.put(record(
            "t-cr",
            &acme,
            LeaseState::Wire(RunnerState::Crashed),
        ))
        .unwrap();
        assert!(
            led.try_admit(pending("live-1", &acme), 1).unwrap(),
            "terminal leases are inactive — cap 1 still admits one Pending"
        );
    }

    // A duplicate lease_id still errors through the admit path (fail-closed).
    {
        let mut led = make();
        assert!(led.try_admit(pending("dup", &acme), 5).unwrap());
        assert!(
            led.try_admit(pending("dup", &acme), 5).is_err(),
            "admitting a duplicate lease_id must Err like put"
        );
    }
}

/// `remove` is the admission-rollback seam: it erases a record (freeing the
/// cap/occupancy it held) and reports whether anything was removed. A
/// reserved-then-rolled-back `Pending` must leave no trace, and the freed slot
/// must be re-admittable under the same cap.
fn remove_frees_cap_and_get(make: LedgerFactory) {
    let acme = tenant("acme");

    // Removing an absent lease is Ok(false), idempotent.
    {
        let mut led = make();
        assert!(
            !led.remove("ghost").unwrap(),
            "removing an absent lease must be Ok(false)"
        );
    }

    // Reserve a Pending at cap 1 (slot full), remove it, then re-admit: the
    // removal must have freed the cap, and get must read absent.
    {
        let mut led = make();
        assert!(
            led.try_admit(pending("p-1", &acme), 1).unwrap(),
            "first reserve at cap 1 must admit"
        );
        // Cap is full: a second reserve is rejected.
        assert!(
            !led.try_admit(pending("p-2", &acme), 1).unwrap(),
            "cap 1 is full while p-1 is reserved"
        );
        // Roll back the reservation.
        assert!(led.remove("p-1").unwrap(), "remove must report a removal");
        assert!(
            led.get("p-1").unwrap().is_none(),
            "the removed lease must read absent"
        );
        // The freed slot is re-admittable.
        assert!(
            led.try_admit(pending("p-3", &acme), 1).unwrap(),
            "removing the reservation must free the cap"
        );
        // Double-remove is Ok(false) (already gone).
        assert!(
            !led.remove("p-1").unwrap(),
            "removing an already-removed lease must be Ok(false)"
        );
    }
}

// ── The crate's in-tree run: BOTH shipped backends, identical assertions ──
mod runs {
    use super::*;
    use crate::ledger::{FileLedger, InMemoryLedger};

    fn make_in_memory() -> Box<dyn LeaseLedger + Send> {
        Box::new(InMemoryLedger::new())
    }

    fn make_file() -> Box<dyn LeaseLedger + Send> {
        // A unique journal path per factory call (mirrors the CF0 acceptance
        // test's tempfile idiom — no tempfile-crate dep in this crate).
        let path = std::env::temp_dir().join(format!(
            "corelink-fabric-conformance-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Box::new(FileLedger::open(&path).expect("fresh journal must open"))
    }

    #[test]
    fn conformance_in_memory_ledger() {
        run_all(make_in_memory);
    }

    #[test]
    fn conformance_file_ledger() {
        run_all(make_file);
    }
}

// ── Postgres backend: the SAME conformance suite + a cross-instance proof ──
//
// Gated on `TEST_DATABASE_URL`. ABSENT (the builder Mac has no Postgres) → the
// tests return early so CI stays green; PRESENT → the full suite plus an
// advisory-lock cross-instance proof runs against a real `PgLedger`.
mod pg_runs {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::pg_ledger::PgLedger;

    /// Both Pg tests share ONE `leases` table and each TRUNCATEs it; cargo runs
    /// them in parallel by default, so without serialization one test's truncate
    /// wipes the other's rows (the cross-instance count then sees 0/2, not 1).
    /// This lock serializes the Pg tests within the process — dep-free (no
    /// `serial_test` crate). CI is unaffected (the tests skip without a DB).
    static PG_TEST_SERIAL: Mutex<()> = Mutex::new(());

    /// `TEST_DATABASE_URL`, or `None` (the gate is OFF — skip cleanly).
    fn db_url() -> Option<String> {
        std::env::var("TEST_DATABASE_URL").ok()
    }

    /// A multi-thread runtime (required for `PgLedger`'s `block_in_place`).
    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("multi-thread runtime")
    }

    /// Connect a `PgLedger` against `url` on `rt`.
    fn connect(rt: &tokio::runtime::Runtime, url: &str) -> PgLedger {
        rt.block_on(async { PgLedger::connect(url, 4).await.expect("PgLedger::connect") })
    }

    /// A `LedgerFactory` (`fn`, so no captures) that TRUNCATEs the shared table
    /// on every call, handing each conformance assertion-group a fresh ledger.
    /// The runtime + url live in process-wide statics because the factory must
    /// be a bare `fn` pointer.
    fn make_pg() -> Box<dyn LeaseLedger + Send> {
        thread_local! {
            static RT: tokio::runtime::Runtime = rt();
        }
        let url = db_url().expect("make_pg only called when TEST_DATABASE_URL is set");
        RT.with(|rt| {
            let led = connect(rt, &url);
            led.truncate_for_test().expect("truncate between groups");
            // SAFETY-OF-LIFETIME: the thread-local runtime outlives every ledger
            // built on this thread within a single `run_all` call; conformance
            // runs are single-threaded per test.
            Box::new(led)
        })
    }

    #[test]
    fn conformance_pg_ledger() {
        if db_url().is_none() {
            eprintln!("conformance_pg_ledger: TEST_DATABASE_URL unset — skipping (expected on CI)");
            return;
        }
        let _serial = PG_TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        // Clean slate before the suite (the factory also truncates per group).
        let rt = rt();
        let led = connect(&rt, &db_url().unwrap());
        led.truncate_for_test().expect("initial truncate");
        drop(led);
        run_all(make_pg);
    }

    /// The advisory-lock proof: two INDEPENDENT `PgLedger` handles on the SAME
    /// database, concurrent `try_admit` at cap-1 → EXACTLY ONE admits. This is
    /// the cross-instance cap-safety guarantee that InMemory/File cannot give.
    #[test]
    fn cross_instance_try_admit_admits_exactly_one_at_cap_1() {
        let Some(url) = db_url() else {
            eprintln!(
                "cross_instance_try_admit...: TEST_DATABASE_URL unset — skipping (expected on CI)"
            );
            return;
        };
        let _serial = PG_TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        let rt = rt();
        // Unique tenant per run so parallel test processes don't collide.
        let tenant_str = format!(
            "xinst-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let t = tenant(&tenant_str);
        // DISTINCT lease ids per run (suffixed with the unique tenant nonce) so
        // neither admit fails on the PRIMARY KEY across repeated runs / parallel
        // processes — the ONLY gate under test is the cap.
        let id_a = format!("a-{tenant_str}");
        let id_b = format!("b-{tenant_str}");

        // Self-contained: clear any residue before the race (the suite is not
        // guaranteed to have truncated first when this test runs alone).
        {
            let led = connect(&rt, &url);
            led.truncate_for_test().expect("pre-race truncate");
        }

        // Two independent handles = two "instances" of the control plane.
        let mut led_a = connect(&rt, &url);
        let mut led_b = connect(&rt, &url);

        // Both race to admit into the same tenant at cap 1. Run the two admits on
        // two OS threads; each enters the runtime via `block_on` in `try_admit`.
        let outcome_a = Arc::new(Mutex::new(None::<bool>));
        let outcome_b = Arc::new(Mutex::new(None::<bool>));
        let oa = Arc::clone(&outcome_a);
        let ob = Arc::clone(&outcome_b);
        let t_a = t.clone();
        let t_b = t.clone();

        rt.block_on(async {
            let ha = tokio::task::spawn_blocking(move || {
                let r = led_a.try_admit(pending(&id_a, &t_a), 1).unwrap();
                *oa.lock().unwrap() = Some(r);
            });
            let hb = tokio::task::spawn_blocking(move || {
                let r = led_b.try_admit(pending(&id_b, &t_b), 1).unwrap();
                *ob.lock().unwrap() = Some(r);
            });
            ha.await.unwrap();
            hb.await.unwrap();
        });

        let a = outcome_a.lock().unwrap().unwrap();
        let b = outcome_b.lock().unwrap().unwrap();
        assert!(
            a ^ b,
            "advisory lock must serialize: EXACTLY ONE of the two cross-instance \
             admits succeeds at cap 1 (got a={a}, b={b})"
        );

        // And the ledger holds exactly one active lease for the tenant.
        let active = led_active_count(&rt, &url, &t);
        assert_eq!(
            active, 1,
            "exactly one lease admitted across both instances"
        );
    }

    fn led_active_count(rt: &tokio::runtime::Runtime, url: &str, t: &TenantId) -> usize {
        let led = connect(rt, url);
        led.by_tenant(t).unwrap().len()
    }
}
