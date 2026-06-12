//! Preventive per-tenant caps — concurrency + rate ceiling (CP2).
//!
//! Admission is decided at acquire time, BEFORE any box/VM is touched
//! (contract §6, hugit X10⑤ "set before load"). [`CapGate::check`] is a
//! **pure decision function**: a read-only pass over the lease ledger plus a
//! caller-held [`RateWindow`] snapshot. No engine, box, or spawn symbol is
//! reachable from this module — the only imports are fabric types — and that
//! absence IS the preventive guarantee: nothing this module can express is
//! able to start work, so a cap can only ever gate a spawn, never chase one.
//! `acquire_over_cap_rejected_before_any_spawn` pins this at the source
//! level (the forbidden engine vocabulary must not appear in this file).
//!
//! Cap source of truth is [`TenantPlan`] (BIL2 feeds it; org = tenant per
//! ADR-0002). Concurrency counted = the tenant's leases in `Pending` or
//! `Held` (active: admitted-or-occupying a slot); terminal states never
//! count. A cap of zero admits nothing.

use crate::ledger::{LeaseLedger, LeaseState};
use crate::tenant::TenantPlan;

/// Outcome of a preventive admission check — decided before any spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapDecision {
    /// Under both the concurrency cap and the rate ceiling: may proceed.
    Admit,
    /// Active leases (`Pending` + `Held`) already at/over `max_concurrency`.
    RejectOverCap,
    /// Acquire attempts in the last 60s at/over `rate_ceiling_per_min`.
    RejectRateCeiling,
}

/// Sliding 60-second window of acquire-attempt timestamps (unix epoch ms).
///
/// The caller pushes one timestamp per acquire attempt; [`CapGate::check`]
/// reads the count to enforce `TenantPlan::rate_ceiling_per_min`. One window
/// per tenant — the window is part of the per-tenant snapshot, never global.
#[derive(Debug, Clone, Default)]
pub struct RateWindow {
    /// Acquire-attempt timestamps, unix epoch ms (pruned on push).
    stamps: Vec<u64>,
}

impl RateWindow {
    /// Empty window.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an acquire attempt at `now_ms`, pruning entries that have
    /// slid out of the 60s window.
    pub fn push(&mut self, now_ms: u64) {
        self.stamps.retain(|&t| now_ms.saturating_sub(t) < 60_000);
        self.stamps.push(now_ms);
    }

    /// Attempts within the last 60 seconds as of `now_ms` (a timestamp
    /// exactly 60 000 ms old has slid out).
    pub fn count_within_60s(&self, now_ms: u64) -> usize {
        self.stamps
            .iter()
            .filter(|&&t| now_ms.saturating_sub(t) < 60_000)
            .count()
    }
}

/// The preventive admission gate (CP2).
///
/// Stateless: every input is passed to [`CapGate::check`], so a burst of
/// checks against the same ledger/window snapshot is deterministic — the
/// gate prevents over-admission up front rather than reacting to load after
/// the fact.
#[derive(Debug, Clone, Copy, Default)]
pub struct CapGate;

impl CapGate {
    /// Decide admission for one acquire attempt by `plan.tenant` at `now_ms`.
    ///
    /// Read-only over `ledger`; checks in order:
    /// 1. **Concurrency cap** — active leases (`Pending` or `Held`) for the
    ///    tenant `>= plan.max_concurrency` → [`CapDecision::RejectOverCap`].
    ///    Cap zero therefore admits nothing.
    /// 2. **Rate ceiling** — `recent_acquires.count_within_60s(now_ms) >=
    ///    plan.rate_ceiling_per_min` → [`CapDecision::RejectRateCeiling`].
    ///
    /// Fail-closed: if the ledger read errors, the decision is
    /// [`CapDecision::RejectOverCap`] — an unreadable ledger never admits.
    pub fn check(
        &self,
        ledger: &dyn LeaseLedger,
        plan: &TenantPlan,
        now_ms: u64,
        recent_acquires: &RateWindow,
    ) -> CapDecision {
        let active = match ledger.by_tenant(&plan.tenant) {
            Ok(records) => records
                .iter()
                .filter(|r| matches!(r.state, LeaseState::Pending) || r.state.is_held())
                .count(),
            // Fail-closed: cannot count occupancy → cannot admit.
            Err(_) => return CapDecision::RejectOverCap,
        };
        if active >= plan.max_concurrency as usize {
            return CapDecision::RejectOverCap;
        }
        if recent_acquires.count_within_60s(now_ms) >= plan.rate_ceiling_per_min as usize {
            return CapDecision::RejectRateCeiling;
        }
        CapDecision::Admit
    }
}

#[cfg(test)]
mod tests {
    use corelink_runners_contracts::RunnerState;

    use super::*;
    use crate::ledger::{InMemoryLedger, LeaseRecord};
    use crate::tenant::TenantId;

    fn plan(tenant: &str, cap: u32, rate: u32) -> TenantPlan {
        TenantPlan {
            tenant: TenantId::new(tenant).unwrap(),
            max_concurrency: cap,
            rate_ceiling_per_min: rate,
        }
    }

    fn record(lease_id: &str, tenant: &str, state: LeaseState) -> LeaseRecord {
        LeaseRecord {
            lease_id: lease_id.to_string(),
            tenant: TenantId::new(tenant).unwrap(),
            state,
            box_ref: format!("box-{lease_id}"),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
        }
    }

    fn held(lease_id: &str, tenant: &str) -> LeaseRecord {
        record(lease_id, tenant, LeaseState::Wire(RunnerState::Held))
    }

    #[test]
    fn acquire_over_cap_rejected_before_any_spawn() {
        let mut ledger = InMemoryLedger::new();
        ledger.put(held("l-1", "acme")).unwrap();
        ledger.put(held("l-2", "acme")).unwrap();
        let plan = plan("acme", 2, 100);

        let decision = CapGate.check(&ledger, &plan, 10_000, &RateWindow::new());
        assert_eq!(decision, CapDecision::RejectOverCap);

        // Pending counts as active too: 1 Held + 1 Pending also fills cap 2.
        let mut ledger2 = InMemoryLedger::new();
        ledger2.put(held("l-1", "acme")).unwrap();
        ledger2
            .put(record("l-2", "acme", LeaseState::Pending))
            .unwrap();
        assert_eq!(
            CapGate.check(&ledger2, &plan, 10_000, &RateWindow::new()),
            CapDecision::RejectOverCap
        );

        // Source-pinned preventive guarantee: the rejection is decided before
        // any spawn because no engine symbol is even reachable from this
        // module. Needles assembled at runtime so this test cannot trip on
        // its own source text.
        let src = include_str!("caps.rs");
        let forbidden = [
            format!("{}{}", "isol", "ation"),
            format!("{}{}", "Eng", "ine"),
        ];
        for needle in &forbidden {
            assert!(
                !src.contains(needle.as_str()),
                "caps.rs must not reference {needle:?}: admission is a pure \
                 decision before any spawn"
            );
        }
    }

    #[test]
    fn rate_ceiling_rejects_before_load() {
        // Empty ledger — zero load — yet the rate ceiling still rejects:
        // the ceiling gates the acquire REQUEST rate, before any work exists.
        let ledger = InMemoryLedger::new();
        let plan = plan("acme", 100, 3);
        let mut window = RateWindow::new();
        window.push(70_000);
        window.push(80_000);
        window.push(90_000);

        assert_eq!(
            CapGate.check(&ledger, &plan, 100_000, &window),
            CapDecision::RejectRateCeiling
        );

        // Once the oldest attempt slides out of the 60s window, admit again.
        assert_eq!(
            CapGate.check(&ledger, &plan, 130_001, &window),
            CapDecision::Admit
        );
    }

    #[test]
    fn cap_zero_admits_nothing() {
        let ledger = InMemoryLedger::new();
        let plan = plan("acme", 0, 100);
        assert_eq!(
            CapGate.check(&ledger, &plan, 10_000, &RateWindow::new()),
            CapDecision::RejectOverCap
        );
    }

    #[test]
    fn cap_is_preventive_under_burst_not_reactive() {
        // A burst of checks against the SAME snapshot: the decision is a pure
        // function of (ledger, plan, now, window) — deterministic across the
        // whole burst, no interleaved state mutation needed to stay correct.
        let mut full = InMemoryLedger::new();
        full.put(held("l-1", "acme")).unwrap();
        let at_cap = plan("acme", 1, 1_000);

        let mut under = InMemoryLedger::new();
        under.put(held("l-1", "bigco")).unwrap();
        let under_cap = plan("bigco", 8, 1_000);

        let window = RateWindow::new();
        for _ in 0..100 {
            assert_eq!(
                CapGate.check(&full, &at_cap, 10_000, &window),
                CapDecision::RejectOverCap
            );
            assert_eq!(
                CapGate.check(&under, &under_cap, 10_000, &window),
                CapDecision::Admit
            );
        }
    }

    #[test]
    fn caps_are_per_tenant_not_global() {
        // One shared ledger: tenant A at cap must not consume tenant B's slots.
        let mut ledger = InMemoryLedger::new();
        ledger.put(held("a-1", "acme")).unwrap();
        ledger.put(held("a-2", "acme")).unwrap();

        let plan_a = plan("acme", 2, 100);
        let plan_b = plan("bigco", 2, 100);
        let window = RateWindow::new();

        assert_eq!(
            CapGate.check(&ledger, &plan_a, 10_000, &window),
            CapDecision::RejectOverCap
        );
        assert_eq!(
            CapGate.check(&ledger, &plan_b, 10_000, &window),
            CapDecision::Admit
        );
    }
}
