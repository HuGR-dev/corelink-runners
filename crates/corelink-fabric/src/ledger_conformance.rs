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
        deadline_ms: None,
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
    deadline_roundtrips_and_transition_preserves_it(make);
}

/// ADR-0004 Decision-1: `deadline_ms` round-trips through `put`/`try_admit` →
/// `get`/`held`/`by_tenant`, a terminal `transition` PRESERVES it unchanged,
/// and `None` round-trips as `None` (the never-overdue fail-safe). Every
/// backend — InMemory, File, and Postgres (nullable `bigint`) — proves this.
fn deadline_roundtrips_and_transition_preserves_it(make: LedgerFactory) {
    let t = tenant("acme");

    // `put` with a concrete deadline → `get` reads it back.
    {
        let mut led = make();
        let mut rec = held("dl-1", &t);
        rec.deadline_ms = Some(7_777);
        led.put(rec).unwrap();
        assert_eq!(
            led.get("dl-1").unwrap().unwrap().deadline_ms,
            Some(7_777),
            "put → get must round-trip deadline_ms"
        );
        // `held()` (the reaper's enumeration seam) also carries it.
        let held_rec = led
            .held()
            .unwrap()
            .into_iter()
            .find(|r| r.lease_id == "dl-1")
            .expect("the Held record must appear in held()");
        assert_eq!(
            held_rec.deadline_ms,
            Some(7_777),
            "held() must carry deadline_ms (the reaper dates leases from it)"
        );
        // A terminal transition must PRESERVE the deadline (a state change never
        // alters it — ADR-0004).
        let after = led.transition("dl-1", RunnerState::Expired, 9_000).unwrap();
        assert_eq!(
            after.deadline_ms,
            Some(7_777),
            "transition must preserve deadline_ms unchanged"
        );
        assert_eq!(
            led.get("dl-1").unwrap().unwrap().deadline_ms,
            Some(7_777),
            "the persisted terminal record still carries the original deadline"
        );
    }

    // `None` round-trips as `None` (never-overdue fail-safe), through both
    // `put`/`get` AND `by_tenant`.
    {
        let mut led = make();
        let rec = pending("dl-none", &t); // helper builds deadline_ms: None
        assert_eq!(rec.deadline_ms, None);
        led.put(rec).unwrap();
        assert_eq!(
            led.get("dl-none").unwrap().unwrap().deadline_ms,
            None,
            "a None deadline must round-trip as None (NULL in Postgres)"
        );
        let via_tenant = led
            .by_tenant(&t)
            .unwrap()
            .into_iter()
            .find(|r| r.lease_id == "dl-none")
            .unwrap();
        assert_eq!(via_tenant.deadline_ms, None, "by_tenant carries None too");
    }

    // `try_admit` (the acquire path) persists the deadline it is handed.
    {
        let mut led = make();
        let mut rec = pending("dl-admit", &t);
        rec.deadline_ms = Some(4_242);
        assert!(
            led.try_admit(rec, 5).unwrap(),
            "admit under cap must succeed"
        );
        assert_eq!(
            led.get("dl-admit").unwrap().unwrap().deadline_ms,
            Some(4_242),
            "try_admit must persist deadline_ms (the acquire path writes it)"
        );
    }
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
        rt.block_on(async {
            // Local conformance DB is plaintext → Disable (the default TLS mode).
            PgLedger::connect(url, 4, crate::pg_ledger::PgTlsMode::Disable)
                .await
                .expect("PgLedger::connect")
        })
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

    /// THE P1 REGRESSION GUARD (ADR-0004 Decision-1, audit finding D3-P1).
    ///
    /// The durable deadline must survive INSTANCE boundaries. One `PgLedger`
    /// handle (instance A — simulating the instance that served the acquire)
    /// writes a `Held` lease with a PAST `deadline_ms`. A SEPARATE `PgLedger`
    /// handle (instance B — which never called any in-memory `record_deadline`)
    /// then `held()`-reads the lease and sees the SAME deadline, and the
    /// reaper-style overdue filter (`now >= rec.deadline_ms`) flags it. Before
    /// this fix the deadline lived only in instance A's in-memory map, so
    /// instance B treated the lease as `u64::MAX` (never-overdue) and leaked the
    /// cap slot forever. This proves the deadline is now read from the ledger,
    /// not from per-instance memory.
    #[test]
    fn durable_deadline_survives_instance_boundary_and_is_reapable_cross_instance() {
        let Some(url) = db_url() else {
            eprintln!(
                "durable_deadline_survives_instance_boundary...: \
                 TEST_DATABASE_URL unset — skipping (expected on CI)"
            );
            return;
        };
        let _serial = PG_TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        let rt = rt();
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let t = tenant(&format!("xdl-{nonce}"));
        let lease_id = format!("dl-{nonce}");
        let past_deadline: u64 = 1_000; // far in the past relative to `now` below

        // ── Instance A: write a Held lease carrying a PAST durable deadline.
        {
            let mut led_a = connect(&rt, &url);
            led_a.truncate_for_test().expect("truncate at start");
            let mut rec = held(&lease_id, &t);
            rec.deadline_ms = Some(past_deadline);
            led_a
                .put(rec)
                .expect("instance A writes the Held+deadline record");
        }

        // ── Instance B: a SEPARATE handle that never saw instance A's memory.
        // It must read the deadline purely from the durable ledger.
        let led_b = connect(&rt, &url);
        let held_b = led_b.held().expect("instance B held() read");
        let rec_b = held_b
            .into_iter()
            .find(|r| r.lease_id == lease_id)
            .expect("instance B must see the Held lease in the durable ledger");
        assert_eq!(
            rec_b.deadline_ms,
            Some(past_deadline),
            "instance B must read the SAME durable deadline instance A wrote \
             (the cross-instance D3-P1 fix)"
        );

        // ── Reaper-style overdue detection on instance B, purely from the
        // durable deadline (this is exactly `reap_once`'s filter).
        let now: u64 = 5_000;
        let overdue = now >= rec_b.deadline_ms.unwrap_or(u64::MAX);
        assert!(
            overdue,
            "instance B must flag the lease overdue from the durable deadline alone"
        );

        // ── And instance B can drive the terminal transition (the reaper's
        // reclaim), which PRESERVES the deadline on the terminal record.
        let mut led_b = led_b;
        let after = led_b
            .transition(&lease_id, RunnerState::Expired, now)
            .expect("instance B reaps the lease it never acquired");
        assert_eq!(
            after.deadline_ms,
            Some(past_deadline),
            "the terminal transition preserves the durable deadline (ADR-0004)"
        );
    }

    /// Spawn an OS thread that owns its OWN `PgLedger` (own pool + own runtime,
    /// i.e. a distinct control-plane *instance*), waits on `barrier` so every
    /// instance fires its DB call SIMULTANEOUSLY (real contention, not a
    /// sequential loop), runs `body`, and returns the body's result. The
    /// per-thread runtime is built and dropped INSIDE the thread.
    fn instance_thread<T, F>(
        url: String,
        barrier: Arc<std::sync::Barrier>,
        body: F,
    ) -> std::thread::JoinHandle<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut PgLedger) -> T + Send + 'static,
    {
        std::thread::spawn(move || {
            // Each thread is its own runtime + ledger = an independent instance.
            let rt = rt();
            let mut led = connect(&rt, &url);
            // Release the runtime's worker context before blocking on the
            // barrier so threads truly rendezvous, then fire the body together.
            barrier.wait();
            body(&mut led)
        })
    }

    /// LOCKS IN invariant #1 (cross-instance cap-exactness). N concurrent
    /// `try_admit` calls for ONE tenant — SPLIT across TWO independent
    /// `PgLedger` instances (separate pools, separate runtimes, one database) —
    /// admit EXACTLY `cap`, never more. WHY it matters for multi-instance: an
    /// in-memory per-instance counter would let each container admit up to `cap`
    /// (2x over-admit at cap on two boxes); only the per-tenant
    /// `pg_advisory_xact_lock` in `try_admit` serializes admission across
    /// instances, so the shared DB count is the single source of truth.
    #[test]
    fn cross_instance_concurrent_admit_respects_cap_exactly() {
        let Some(url) = db_url() else {
            eprintln!(
                "cross_instance_concurrent_admit...: TEST_DATABASE_URL unset — \
                 skipping (expected on CI)"
            );
            return;
        };
        let _serial = PG_TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        // The shared `leases` table is TRUNCATEd at the start of every Pg test
        // (mutex-serialized) so the cross-instance count starts from empty.
        {
            let rt = rt();
            let led = connect(&rt, &url);
            led.truncate_for_test().expect("truncate at start");
        }

        const CAP: u32 = 3;
        const N: usize = 25; // 25 contenders, far over the cap of 3.

        // Unique tenant + lease-id nonce per run so parallel test processes (and
        // repeat runs) never collide on the PRIMARY KEY — the cap is the ONLY
        // gate under test.
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let t = tenant(&format!("xcap-{nonce}"));

        // All N contenders rendezvous on the barrier, then hit the DB at once.
        let barrier = Arc::new(std::sync::Barrier::new(N));
        let handles: Vec<_> = (0..N)
            .map(|i| {
                let url = url.clone();
                let barrier = Arc::clone(&barrier);
                let t = t.clone();
                let id = format!("cap-{nonce}-{i}");
                // SPLIT across two instances: each thread is a fresh instance,
                // so the N admits are served by N independent pools — exactly
                // the "two control-plane containers, one DB" shape, generalized.
                instance_thread(url, barrier, move |led| {
                    led.try_admit(pending(&id, &t), CAP)
                        .expect("try_admit must not error (distinct ids, real cap)")
                })
            })
            .collect();

        let admitted = handles
            .into_iter()
            .map(|h| h.join().expect("admit thread must not panic"))
            .filter(|ok| *ok)
            .count();

        assert_eq!(
            admitted, CAP as usize,
            "cross-instance concurrent admission must admit EXACTLY the cap \
             (got {admitted}, cap {CAP}); over-cap calls must return Ok(false). \
             A higher count means the advisory lock did not serialize and an \
             instance over-admitted."
        );

        // The DB itself holds exactly `cap` rows for the tenant — the count the
        // next admit (on any instance) would read.
        let rt = rt();
        let active = led_active_count(&rt, &url, &t);
        assert_eq!(
            active, CAP as usize,
            "the shared ledger must hold exactly `cap` admitted leases"
        );
    }

    /// LOCKS IN invariant #2 (terminal-transition CAS-dedup). One `Held` lease;
    /// TWO independent instances concurrently `transition(id, Expired, now)`.
    /// EXACTLY ONE returns `Ok`, the other `Err`. WHY it matters for
    /// multi-instance: the reaper runs on every instance, so two reapers can
    /// race to expire the same lease; the conditional `UPDATE ... WHERE
    /// state='held'` matches 0 rows on the loser (CAS), so only ONE instance
    /// gets the `Ok` that authorizes the GC/flush — no double-free, no
    /// double-billing-close.
    #[test]
    fn cross_instance_concurrent_terminal_transition_is_cas_deduped() {
        let Some(url) = db_url() else {
            eprintln!(
                "cross_instance_concurrent_terminal_transition...: \
                 TEST_DATABASE_URL unset — skipping (expected on CI)"
            );
            return;
        };
        let _serial = PG_TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        // Truncate, then seed exactly one Held lease the two reapers will race on.
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let t = tenant(&format!("xcas-{nonce}"));
        let lease_id = format!("cas-{nonce}");
        {
            let rt = rt();
            let mut led = connect(&rt, &url);
            led.truncate_for_test().expect("truncate at start");
            led.put(held(&lease_id, &t)).expect("seed one Held lease");
        }

        // Two instances rendezvous, then both attempt the SAME terminal CAS.
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let now_ms = 9_999;
        let mk = || {
            let url = url.clone();
            let barrier = Arc::clone(&barrier);
            let id = lease_id.clone();
            instance_thread(url, barrier, move |led| {
                led.transition(&id, RunnerState::Expired, now_ms).is_ok()
            })
        };
        let h_a = mk();
        let h_b = mk();
        let a_ok = h_a.join().expect("transition thread A must not panic");
        let b_ok = h_b.join().expect("transition thread B must not panic");

        assert!(
            a_ok ^ b_ok,
            "concurrent terminal transition must be CAS-deduped: EXACTLY ONE \
             instance gets Ok (the winner GCs), the other gets Err — got \
             a_ok={a_ok}, b_ok={b_ok}. Both-Ok means the conditional UPDATE \
             matched twice (double-free); both-Err means the lease vanished."
        );

        // The lease ended up Expired exactly once, owned by one instance.
        let rt = rt();
        let led = connect(&rt, &url);
        let rec = led
            .get(&lease_id)
            .expect("get must not error")
            .expect("lease must still exist");
        assert_eq!(
            rec.state,
            LeaseState::Wire(RunnerState::Expired),
            "the winning instance moved the lease to Expired"
        );
    }
}
