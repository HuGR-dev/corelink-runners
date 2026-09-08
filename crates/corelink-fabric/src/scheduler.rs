//! CP3 — fair multi-tenant scheduler: the dispatch MECHANISM (contract §6,
//! C7).
//!
//! Generalizes the seeded single-box batch core
//! (`corelink-runner::concurrency::Scheduler::run_batch`, the proven
//! ≥8-parallel core — reused via the dispatch seam, never rewritten) into a
//! long-running multi-tenant dispatcher. This module is a **tick-driven pure
//! core**: no threads, no async runtime, no clock — the composition root
//! drives [`FairScheduler::tick`] with `now_ms` and supplies the two seams as
//! closures:
//!
//! - `cap_check(&TenantId) -> bool` — CP2's preventive admission
//!   ([`crate::caps::CapGate`]) stays upstream; the scheduler only consults
//!   the verdict, it never re-implements caps.
//! - `dispatch(&WorkItem) -> bool` — the opaque hand-off to the execution
//!   core. No spawn/exec/container symbol is reachable from this module
//!   (`scheduler_drives_engine_seam_unchanged` pins this at the source
//!   level), so the proven runner seam stays exactly where it is.
//!
//! Determinism is the point: every test drives `now_ms` by hand, no sleeps —
//! the same input sequence always yields the same dispatch order.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::tenant::TenantId;

/// Capacity of the per-tenant ring of completed waits backing
/// [`FairScheduler::p95_wait_ms`] (the CP4 measurement surface). Bounded so a
/// long-running dispatcher's memory is O(tenants), not O(history).
pub const WAIT_RING_CAPACITY: usize = 1024;

/// Maximum pending items a single tenant may hold in its FIFO. Admission is
/// **fail-closed**: an enqueue that would push a tenant strictly over this
/// bound is rejected and the queue stays at the cap, so one tenant flooding
/// `enqueue` cannot grow the dispatcher's memory without bound (a DoS lever).
/// The bound is per-tenant, so a flooder cannot starve other tenants' admission
/// either. Total queue memory is therefore O(tenants × `MAX_TENANT_QUEUE_DEPTH`).
pub const MAX_TENANT_QUEUE_DEPTH: usize = 4096;

/// One unit of pending work, queued per tenant until dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// Opaque work identifier (e.g. a lease/job id) — echoed back in
    /// [`TickReport::dispatched`].
    pub id: String,
    /// Owning tenant (org = tenant, ADR-0002) — the fairness key.
    pub tenant: TenantId,
    /// Submission instant, unix epoch ms — wait = dispatch `now_ms` − this.
    pub enqueued_at_ms: u64,
}

/// Per-tenant FIFO queues of pending work.
///
/// FIFO **within** a tenant (submission order is preserved per tenant);
/// fairness **across** tenants is [`FairScheduler::tick`]'s job, never the
/// queue's. Tenant iteration order is the sorted key order (`BTreeMap`), so
/// rotation is deterministic.
#[derive(Debug, Clone, Default)]
pub struct TenantQueues {
    queues: BTreeMap<TenantId, VecDeque<WorkItem>>,
}

impl TenantQueues {
    /// Empty queue set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Try to append `item` to the back of its tenant's FIFO.
    ///
    /// Fail-closed admission bound: if the tenant already holds
    /// [`MAX_TENANT_QUEUE_DEPTH`] items, the item is **rejected** (returned in
    /// `Err`) and the queue is left exactly at the cap — unbounded per-tenant
    /// growth (a DoS lever) is impossible. Returns `Ok(())` on admission.
    pub fn try_enqueue(&mut self, item: WorkItem) -> Result<(), WorkItem> {
        let queue = self.queues.entry(item.tenant.clone()).or_default();
        if queue.len() >= MAX_TENANT_QUEUE_DEPTH {
            // Reject over the bound; do not leave an empty entry behind for a
            // tenant that never had work admitted.
            if queue.is_empty() {
                self.queues.remove(&item.tenant);
            }
            return Err(item);
        }
        queue.push_back(item);
        Ok(())
    }

    /// Pending items for one tenant.
    pub fn pending(&self, tenant: &TenantId) -> usize {
        self.queues.get(tenant).map_or(0, VecDeque::len)
    }

    /// Pending items across all tenants.
    pub fn total_pending(&self) -> usize {
        self.queues.values().map(VecDeque::len).sum()
    }

    /// Tenants that currently have pending work, in sorted (deterministic)
    /// order.
    fn tenants_with_work(&self) -> Vec<TenantId> {
        self.queues
            .iter()
            .filter(|(_, q)| !q.is_empty())
            .map(|(t, _)| t.clone())
            .collect()
    }

    /// Head of one tenant's FIFO, if any.
    fn front(&self, tenant: &TenantId) -> Option<&WorkItem> {
        self.queues.get(tenant).and_then(VecDeque::front)
    }

    /// Remove and return the head of one tenant's FIFO.
    fn pop_front(&mut self, tenant: &TenantId) -> Option<WorkItem> {
        let item = self.queues.get_mut(tenant)?.pop_front();
        if self.queues.get(tenant).is_some_and(VecDeque::is_empty) {
            self.queues.remove(tenant);
        }
        item
    }
}

/// What one [`FairScheduler::tick`] did — the composition root's receipt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    /// Ids of the items handed to `dispatch`, in dispatch order.
    pub dispatched: Vec<String>,
    /// Tenants skipped this tick because `cap_check` said no (one count per
    /// tenant per tick — the skip never consumes the tenant's turn).
    pub skipped_over_cap: u32,
    /// Wait of each dispatched item (`now_ms − enqueued_at_ms`), parallel to
    /// `dispatched` — the raw feed of the CP4 measurement surface.
    pub waits_ms: Vec<(TenantId, u64)>,
}

/// Long-running fair dispatcher over [`TenantQueues`] — a pure mechanism.
///
/// Holds no thread, no runtime, no clock: the composition root calls
/// [`FairScheduler::tick`] whenever it wants placement decisions, passing the
/// current time and the two seam closures. `global_slots` is the per-tick
/// dispatch budget (the box/fabric capacity the root is willing to fill per
/// tick).
#[derive(Debug, Clone)]
pub struct FairScheduler {
    global_slots: u32,
    queues: TenantQueues,
    /// The tenant that dispatched last — rotation resumes AFTER it. Only a
    /// real dispatch ever moves this cursor (cap skips never do).
    cursor: Option<TenantId>,
    /// Tenants whose turn was deferred by a cap-skip and not yet redeemed —
    /// the "owed" set. A tenant skipped over cap in one tick is OWED a turn:
    /// the moment it is eligible again it dispatches BEFORE the cursor-based
    /// rotation resumes, so a capped tenant cannot be passed twice by another
    /// while it waits (the ≥3-tenant fairness fix). Entries are sorted
    /// (`BTreeSet`) for deterministic redemption order and cleared on dispatch.
    owed_skip: BTreeSet<TenantId>,
    /// Bounded ring of completed waits per tenant (CP4 surface).
    waits: BTreeMap<TenantId, VecDeque<u64>>,
}

impl FairScheduler {
    /// New scheduler with a per-tick dispatch budget of `global_slots`.
    pub fn new(global_slots: u32) -> Self {
        Self {
            global_slots,
            queues: TenantQueues::new(),
            cursor: None,
            owed_skip: BTreeSet::new(),
            waits: BTreeMap::new(),
        }
    }

    /// Submit one work item to its tenant's FIFO under the per-tenant
    /// admission bound.
    ///
    /// Fail-closed: an enqueue that would push the tenant strictly over
    /// [`MAX_TENANT_QUEUE_DEPTH`] is **rejected** (the item is returned in
    /// `Err`, the queue stays at the cap), so a tenant flooding `enqueue`
    /// cannot grow the dispatcher's memory without bound. Returns `Ok(())` on
    /// admission.
    pub fn enqueue(&mut self, item: WorkItem) -> Result<(), WorkItem> {
        self.queues.try_enqueue(item)
    }

    /// Pending items for one tenant.
    pub fn pending(&self, tenant: &TenantId) -> usize {
        self.queues.pending(tenant)
    }

    /// Pending items across all tenants.
    pub fn total_pending(&self) -> usize {
        self.queues.total_pending()
    }

    /// Run one scheduling tick at `now_ms`.
    ///
    /// # THE fairness contract (deficit round-robin)
    ///
    /// 1. **Rotation.** Tenants with non-empty queues are cycled in
    ///    deterministic sorted order, starting AFTER the tenant that last
    ///    dispatched (the cursor persists across ticks). Within a rotation
    ///    pass no tenant goes twice before every eligible tenant has gone
    ///    once; passes repeat until the budget or the work runs out.
    /// 2. **Cap skip.** A tenant failing `cap_check` is skipped for the
    ///    remainder of this tick and counted once in
    ///    [`TickReport::skipped_over_cap`]. The skip does **not** consume the
    ///    tenant's turn: the cursor only advances on a real dispatch, so
    ///    preventive caps stay upstream (CP2). It is also recorded as **owed**
    ///    a turn — the moment it is eligible again it dispatches BEFORE the
    ///    cursor rotation resumes, so with ≥3 tenants a tenant capped in one
    ///    tick cannot be passed twice by another while it waits (it resumes at
    ///    full rotation priority, no double-turn, no starvation). Its queue is
    ///    left untouched.
    /// 3. **Budget.** Dispatch proceeds until `global_slots` items have been
    ///    handed out or no eligible work remains. `dispatch` returning
    ///    `false` (backpressure) leaves the item at the head of its queue and
    ///    parks that tenant for the remainder of the tick — also without
    ///    consuming its turn.
    ///
    /// `cap_check` is consulted immediately before **every** dispatch, so a
    /// cap that fills mid-tick is honored mid-tick.
    pub fn tick(
        &mut self,
        now_ms: u64,
        cap_check: impl Fn(&TenantId) -> bool,
        mut dispatch: impl FnMut(&WorkItem) -> bool,
    ) -> TickReport {
        let mut report = TickReport::default();
        let mut remaining = self.global_slots;
        // Tenants parked for the rest of this tick (cap-failed or
        // dispatch-refused). Their cursor position is untouched.
        let mut parked: BTreeSet<TenantId> = BTreeSet::new();

        while remaining > 0 {
            let order = self.rotation_order(&parked);
            if order.is_empty() {
                break;
            }
            let mut progressed = false;
            for tenant in order {
                if remaining == 0 {
                    break;
                }
                if parked.contains(&tenant) {
                    continue;
                }
                if !cap_check(&tenant) {
                    report.skipped_over_cap += 1;
                    // The skip defers this tenant's turn: it is OWED a turn and
                    // redeems it (rotation priority) the moment it is eligible.
                    self.owed_skip.insert(tenant.clone());
                    parked.insert(tenant);
                    continue;
                }
                let Some(item) = self.queues.front(&tenant) else {
                    continue;
                };
                if dispatch(item) {
                    let item = self
                        .queues
                        .pop_front(&tenant)
                        .expect("front() just returned Some");
                    let wait = now_ms.saturating_sub(item.enqueued_at_ms);
                    // NOTE: `tick` SELECTS only — it no longer feeds the internal
                    // p95 ring here. The composition root (admission) calls
                    // [`record_dispatch_wait`] for ONLY the genuinely-dispatched
                    // waits (a live waiter whose response reached the client), so
                    // cap-race losers (re-enqueued by the caller) and orphaned/
                    // timed-out FIFO entries selected here never pollute the ring.
                    // The raw wait is still surfaced in `report.waits_ms` for the
                    // caller to filter (the §6 fix consumes the same feed).
                    report.dispatched.push(item.id);
                    report.waits_ms.push((tenant.clone(), wait));
                    // Turn redeemed: the tenant is no longer owed a skip.
                    self.owed_skip.remove(&tenant);
                    self.cursor = Some(tenant);
                    remaining -= 1;
                    progressed = true;
                } else {
                    parked.insert(tenant);
                }
            }
            if !progressed {
                break;
            }
        }
        // Keep the owed set bounded by tenants with live work: a tenant that
        // drained (or never had work) can never redeem an owed turn, so it
        // must not linger in the set. Bound stays O(tenants-with-work).
        self.owed_skip.retain(|t| self.queues.pending(t) > 0);
        report
    }

    /// p95 of this tenant's completed dispatch waits (nearest-rank over the
    /// bounded ring of the last [`WAIT_RING_CAPACITY`] dispatches) — the CP4
    /// measurement surface backing contract §6 non-interference proof.
    /// `None` until the tenant has at least one completed dispatch.
    pub fn p95_wait_ms(&self, tenant: &TenantId) -> Option<u64> {
        let ring = self.waits.get(tenant)?;
        if ring.is_empty() {
            return None;
        }
        let mut sorted: Vec<u64> = ring.iter().copied().collect();
        sorted.sort_unstable();
        let rank = (sorted.len() * 95).div_ceil(100).max(1);
        Some(sorted[rank - 1])
    }

    /// Tenants with work, minus the tenants parked this tick, in dispatch
    /// order: tenants OWED a turn (cap-skipped on an earlier tick) come FIRST
    /// in sorted order, then the rest cursor-rotated to start AFTER the tenant
    /// that last dispatched.
    ///
    /// The owed-first prefix is the ≥3-tenant fairness fix: a tenant skipped
    /// over cap is owed a turn, so when it becomes eligible it redeems that
    /// turn before the normal rotation resumes — another tenant cannot take a
    /// second turn ahead of it while it waits.
    fn rotation_order(&self, parked: &BTreeSet<TenantId>) -> Vec<TenantId> {
        let eligible: Vec<TenantId> = self
            .queues
            .tenants_with_work()
            .into_iter()
            .filter(|t| !parked.contains(t))
            .collect();

        // Owed prefix: eligible tenants that are owed a deferred turn, in
        // sorted (deterministic) order. They redeem ahead of the rotation.
        let mut order: Vec<TenantId> = eligible
            .iter()
            .filter(|t| self.owed_skip.contains(*t))
            .cloned()
            .collect();

        // Remainder: the rest, cursor-rotated to resume after the last
        // dispatch (the unchanged deficit-round-robin rule).
        let mut rest: Vec<TenantId> = eligible
            .into_iter()
            .filter(|t| !self.owed_skip.contains(t))
            .collect();
        if let Some(cursor) = &self.cursor {
            // First tenant strictly after the cursor (wrapping); the list is
            // sorted, so this is the deterministic resume point.
            let start = rest.iter().position(|t| t > cursor).unwrap_or(0);
            rest.rotate_left(start);
        }

        order.extend(rest);
        order
    }

    /// Record one GENUINELY-dispatched wait into the tenant's bounded p95 ring.
    ///
    /// `tick` SELECTS dispatch order but no longer records here: the caller
    /// (admission's `run_admission_tick`) calls this for ONLY the waits that
    /// reached a live client. Cap-race losers (re-enqueued and retried) and
    /// orphaned/timed-out FIFO entries the scheduler "dispatched" for draining
    /// must NOT feed the ring, or they pollute the §6 p95 surface — mirroring
    /// the `wait_stats` non-interference filter. A genuine dispatch's wait is
    /// recorded exactly once (when it actually serves a client), never on the
    /// loser's intermediate selections.
    pub fn record_dispatch_wait(&mut self, tenant: &TenantId, wait_ms: u64) {
        let ring = self.waits.entry(tenant.clone()).or_default();
        if ring.len() == WAIT_RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(wait_ms);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn tid(s: &str) -> TenantId {
        TenantId::new(s).unwrap()
    }

    fn item(id: &str, tenant: &str, enqueued_at_ms: u64) -> WorkItem {
        WorkItem {
            id: id.to_string(),
            tenant: tid(tenant),
            enqueued_at_ms,
        }
    }

    /// Tenant A floods 100 items, tenant B submits 5, equal caps: round-robin
    /// must serve B promptly (starvation-free bound) and keep B's p95 wait at
    /// or below the flooder's.
    #[test]
    fn fairness_p95_wait_bounded_under_two_tenant_contention() {
        let mut sched = FairScheduler::new(4);
        for i in 0..100 {
            sched.enqueue(item(&format!("a-{i}"), "alpha", 0)).unwrap();
        }
        for i in 0..5 {
            sched.enqueue(item(&format!("b-{i}"), "beta", 0)).unwrap();
        }

        let beta = tid("beta");
        let alpha = tid("alpha");
        let mut beta_done_tick: Option<u64> = None;
        let mut ticks = 0u64;
        while sched.total_pending() > 0 {
            ticks += 1;
            assert!(ticks <= 200, "scheduler failed to drain in bounded ticks");
            let now_ms = ticks * 1_000;
            // `tick` selects only; record the dispatched waits into the p95 ring
            // exactly as the composition root does for genuine dispatches (here
            // every selected item dispatches genuinely — `dispatch` is `|_| true`).
            let report = sched.tick(now_ms, |_| true, |_| true);
            for (t, w) in &report.waits_ms {
                sched.record_dispatch_wait(t, *w);
            }
            if beta_done_tick.is_none() && sched.pending(&beta) == 0 {
                beta_done_tick = Some(ticks);
            }
        }

        // Starvation-free bound: B's 5 items are fully served within 5 ticks
        // even while A floods (with 4 slots and round-robin, B gets 2/tick).
        let beta_done = beta_done_tick.expect("beta must drain");
        assert!(
            beta_done <= 5,
            "beta (5 items) must be fully served within 5 ticks under flood, took {beta_done}"
        );

        let p95_beta = sched.p95_wait_ms(&beta).expect("beta dispatched");
        let p95_alpha = sched.p95_wait_ms(&alpha).expect("alpha dispatched");
        assert!(
            p95_beta <= p95_alpha,
            "light tenant p95 wait ({p95_beta} ms) must not exceed the \
             flooder's ({p95_alpha} ms)"
        );
    }

    /// 10 tenants, one storming: every tenant with pending work dispatches at
    /// least once per full rotation (here, per tick — budget == tenant count).
    #[test]
    fn no_tenant_starved_under_storm() {
        let mut sched = FairScheduler::new(10);
        for i in 0..100 {
            sched.enqueue(item(&format!("storm-{i}"), "t0", 0)).unwrap();
        }
        for t in 1..10 {
            for i in 0..5 {
                sched
                    .enqueue(item(&format!("t{t}-{i}"), &format!("t{t}"), 0))
                    .unwrap();
            }
        }

        let tenants: Vec<TenantId> = (0..10).map(|t| tid(&format!("t{t}"))).collect();
        for tick_n in 1..=5u64 {
            let with_work: Vec<TenantId> = tenants
                .iter()
                .filter(|t| sched.pending(t) > 0)
                .cloned()
                .collect();
            let report = sched.tick(tick_n * 1_000, |_| true, |_| true);
            for t in &with_work {
                let served = report.waits_ms.iter().filter(|(wt, _)| wt == t).count();
                assert!(
                    served >= 1,
                    "tick {tick_n}: tenant {t} had pending work but dispatched \
                     {served} — starved by the storm"
                );
            }
        }
        // The quiet tenants (5 items each, 1 per rotation) are all drained.
        for t in &tenants[1..] {
            assert_eq!(sched.pending(t), 0, "tenant {t} should be drained");
        }
        assert!(sched.pending(&tenants[0]) > 0, "the storm keeps going");
    }

    /// The dispatch callback is opaque: the scheduler hands over a
    /// `&WorkItem` and learns only accept/refuse. The execution-core seam
    /// (the runner crate's spawn machinery) is not reachable from this
    /// module — pinned at the source level, with the needles assembled at
    /// runtime so this test cannot trip on its own text.
    #[test]
    fn scheduler_drives_engine_seam_unchanged() {
        // Behavioral half: dispatch sees the item; the scheduler holds no
        // execution state of its own.
        let mut sched = FairScheduler::new(1);
        sched.enqueue(item("w-1", "acme", 0)).unwrap();
        let mut seen: Vec<String> = Vec::new();
        let report = sched.tick(
            500,
            |_| true,
            |w: &WorkItem| {
                seen.push(w.id.clone());
                true
            },
        );
        assert_eq!(seen, vec!["w-1".to_string()]);
        assert_eq!(report.dispatched, vec!["w-1".to_string()]);

        // Source half: no execution-core vocabulary in this module.
        let src = include_str!("scheduler.rs");
        let forbidden = [
            format!("{}{}", "Eng", "ine"),
            format!("{}{}", "isol", "ation"),
            format!("{}{}", "use corelink_", "runner"),
            format!("{}{}", "spa", "wn("),
        ];
        for needle in &forbidden {
            assert!(
                !src.contains(needle.as_str()),
                "scheduler.rs must not reference {needle:?}: the dispatch \
                 closure is the only seam to the execution core"
            );
        }
    }

    /// Cursor determinism: each tick resumes the rotation immediately after
    /// the tenant that dispatched last, across tick boundaries.
    #[test]
    fn rotation_resumes_after_last_dispatched() {
        // One slot per tick: strict a, b, c, a, b, c, ... interleave.
        let mut sched = FairScheduler::new(1);
        for t in ["a", "b", "c"] {
            for i in 0..3 {
                sched.enqueue(item(&format!("{t}-{i}"), t, 0)).unwrap();
            }
        }
        let mut order = Vec::new();
        for tick_n in 1..=9u64 {
            let report = sched.tick(tick_n * 100, |_| true, |_| true);
            order.extend(report.dispatched);
        }
        assert_eq!(
            order,
            [
                "a-0", "b-0", "c-0", "a-1", "b-1", "c-1", "a-2", "b-2", "c-2"
            ],
            "one-slot ticks must rotate strictly, resuming after the cursor"
        );

        // Two slots per tick: the cursor carries across ticks, so tick 2
        // starts at c (after b), not back at a.
        let mut sched = FairScheduler::new(2);
        for t in ["a", "b", "c"] {
            for i in 0..3 {
                sched.enqueue(item(&format!("{t}-{i}"), t, 0)).unwrap();
            }
        }
        let per_tick: Vec<Vec<String>> = (1..=5u64)
            .map(|n| sched.tick(n * 100, |_| true, |_| true).dispatched)
            .collect();
        assert_eq!(
            per_tick,
            vec![
                vec!["a-0".to_string(), "b-0".to_string()],
                vec!["c-0".to_string(), "a-1".to_string()],
                vec!["b-1".to_string(), "c-1".to_string()],
                vec!["a-2".to_string(), "b-2".to_string()],
                vec!["c-2".to_string()],
            ]
        );
    }

    /// A cap-failed tenant is skipped (its slot flows to the next eligible
    /// tenant, the skip is counted) but its TURN is not consumed: the cursor
    /// never advances onto it, so the moment the cap clears it dispatches
    /// first — before any other tenant goes again.
    #[test]
    fn over_cap_tenant_skipped_without_consuming_turn() {
        let mut sched = FairScheduler::new(1);
        for t in ["a", "b"] {
            for i in 0..3 {
                sched.enqueue(item(&format!("{t}-{i}"), t, 0)).unwrap();
            }
        }
        let b = tid("b");
        let b_capped = Cell::new(false);
        let cap_check = |t: &TenantId| !(t == &b && b_capped.get());

        // Tick 1: no cursor yet — sorted order starts at a.
        let r1 = sched.tick(100, cap_check, |_| true);
        assert_eq!(r1.dispatched, vec!["a-0".to_string()]);
        assert_eq!(r1.skipped_over_cap, 0);

        // Tick 2: rotation starts at b (after a), but b is over cap — the
        // skip is counted, the slot flows on to a, and b's queue is intact.
        b_capped.set(true);
        let r2 = sched.tick(200, cap_check, |_| true);
        assert_eq!(r2.dispatched, vec!["a-1".to_string()]);
        assert_eq!(r2.skipped_over_cap, 1);
        assert_eq!(sched.pending(&b), 3, "skip must leave b's queue untouched");

        // Tick 3: cap cleared — b's turn was never consumed, so b goes FIRST
        // (before a gets a third consecutive dispatch).
        b_capped.set(false);
        let r3 = sched.tick(300, cap_check, |_| true);
        assert_eq!(r3.dispatched, vec!["b-0".to_string()]);
        assert_eq!(r3.skipped_over_cap, 0);
    }

    /// [P2 regression] Per-tenant queue admission bound (DoS): a tenant cannot
    /// flood the queue without bound. Enqueue past `MAX_TENANT_QUEUE_DEPTH`
    /// is rejected (the item is handed back), the queue stays exactly at the
    /// cap, and a SECOND tenant is unaffected (the bound is per-tenant).
    #[test]
    fn enqueue_is_bounded_per_tenant_and_sheds_over_the_cap() {
        let mut sched = FairScheduler::new(1);
        let flooder = tid("flooder");

        // Fill the flooder's queue exactly to the cap — all admitted.
        for i in 0..MAX_TENANT_QUEUE_DEPTH {
            assert!(
                sched.enqueue(item(&format!("f-{i}"), "flooder", 0)).is_ok(),
                "item {i} within the bound must be admitted"
            );
        }
        assert_eq!(sched.pending(&flooder), MAX_TENANT_QUEUE_DEPTH);

        // Every further enqueue is rejected and the queue stays bounded — no
        // unbounded growth from a flooding tenant.
        for i in 0..1_000 {
            let over = item(&format!("over-{i}"), "flooder", 0);
            let rejected = sched.enqueue(over.clone());
            assert_eq!(rejected, Err(over), "over-bound enqueue must be rejected");
            assert_eq!(
                sched.pending(&flooder),
                MAX_TENANT_QUEUE_DEPTH,
                "queue must stay at the cap, never grow"
            );
        }

        // The bound is PER TENANT: a different tenant still admits freely.
        let other = tid("other");
        assert!(sched.enqueue(item("o-0", "other", 0)).is_ok());
        assert_eq!(sched.pending(&other), 1);
    }

    /// [P2 regression] ≥3-tenant cap-skip fairness: a tenant capped on one
    /// tick must resume in the CORRECT rotation position when it clears — it
    /// is owed its turn and redeems it before any other tenant takes a SECOND
    /// turn ahead of it. No starvation, no double-turn.
    #[test]
    fn capped_tenant_resumes_in_order_with_three_tenants() {
        // a, b, c each with work; one slot per tick. b is capped for tick 2
        // only. Without the owed-turn fix, the cursor (at c after tick 1's
        // a,_,c is impossible with one slot — here we drive it explicitly):
        // a would take a second turn ahead of the previously-skipped b.
        let mut sched = FairScheduler::new(1);
        for t in ["a", "b", "c"] {
            for i in 0..3 {
                sched.enqueue(item(&format!("{t}-{i}"), t, 0)).unwrap();
            }
        }
        let b = tid("b");
        let b_capped = Cell::new(false);
        let cap_check = |t: &TenantId| !(t == &b && b_capped.get());

        // Tick 1: a (no cursor → sorted start). cursor=a.
        assert_eq!(sched.tick(100, cap_check, |_| true).dispatched, ["a-0"]);

        // Tick 2: rotation resumes after a → b, but b is capped. b is skipped
        // (owed a turn); the slot flows to c. cursor=c.
        b_capped.set(true);
        let r2 = sched.tick(200, cap_check, |_| true);
        assert_eq!(r2.dispatched, vec!["c-0".to_string()]);
        assert_eq!(r2.skipped_over_cap, 1);

        // Tick 3: cap cleared. b is OWED a turn, so it redeems FIRST — NOT a,
        // even though the cursor (c) would otherwise hand the next turn to a.
        // This is the bug: a must not take a second turn ahead of the
        // skipped-then-eligible b.
        b_capped.set(false);
        let r3 = sched.tick(300, cap_check, |_| true);
        assert_eq!(
            r3.dispatched,
            vec!["b-0".to_string()],
            "the previously-capped b must redeem its owed turn before a goes again"
        );

        // Tick 4: with b's owed turn redeemed, the cursor (now b) resumes
        // normal rotation → c is next (after b), then a.
        assert_eq!(sched.tick(400, cap_check, |_| true).dispatched, ["c-1"]);
        assert_eq!(sched.tick(500, cap_check, |_| true).dispatched, ["a-1"]);

        // Dispatch order across the five ticks was a, c, b, c, a — b redeemed
        // its owed turn (tick 3) before a took a second turn (tick 5). a and c
        // served twice (pending 1 each), b once (pending 2): no starvation, no
        // double-turn ahead of the skipped tenant.
        let a = tid("a");
        let c = tid("c");
        assert_eq!(sched.pending(&a), 1);
        assert_eq!(sched.pending(&b), 2);
        assert_eq!(sched.pending(&c), 1);
    }

    /// p95 surface sanity: nearest-rank over the ring, None before any
    /// dispatch, per-tenant scoped.
    #[test]
    fn p95_wait_is_per_tenant_and_bounded_ring() {
        let mut sched = FairScheduler::new(1);
        let a = tid("a");
        let b = tid("b");
        assert_eq!(sched.p95_wait_ms(&a), None);

        // 20 dispatches for a, enqueued so waits are 100, 200, ..., 2000.
        for i in 0..20u64 {
            sched.enqueue(item(&format!("a-{i}"), "a", 0)).unwrap();
        }
        for i in 1..=20u64 {
            // `tick` selects; record the dispatched waits as genuine (the
            // composition root's discipline) so the p95 ring is populated.
            let report = sched.tick(i * 100, |_| true, |_| true);
            for (t, w) in &report.waits_ms {
                sched.record_dispatch_wait(t, *w);
            }
        }
        // nearest-rank p95 of 100..=2000 step 100 is rank 19 → 1900.
        assert_eq!(sched.p95_wait_ms(&a), Some(1_900));
        assert_eq!(
            sched.p95_wait_ms(&b),
            None,
            "waits never leak across tenants"
        );
    }
}
