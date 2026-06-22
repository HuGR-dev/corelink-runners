//! Tenant-aware GLOBAL admission ceiling (WP-GLOBAL-CAP).
//!
//! The per-tenant [`crate::caps::CapGate`] bounds ONE tenant's slots; it knows
//! nothing about the fabric-wide wall. The instance-level
//! `tower::limit::GlobalConcurrencyLimitLayer` (server side) bounds total
//! in-flight HTTP but is **tenant-blind**: a single tenant's storm can park all
//! global permits and 503 everyone (single-tenant global starvation). This gate
//! closes that gap on the ADMISSION side — a fabric-wide ceiling checked
//! ALONGSIDE the per-tenant cap so that:
//!
//! 1. total admitted-in-flight across ALL tenants cannot exceed a configured
//!    global wall (the ~600-slot wall), AND
//! 2. no single tenant can monopolize that wall — each tenant gets a bounded
//!    SHARE of the global, so one tenant's flood never starves the others.
//!
//! Mirrors `CapGate`'s primitives: pure in-memory (an [`Arc`]-shared count
//! map behind a [`Mutex`], no async, no engine/box/spawn symbol), and admission
//! returns an RAII [`GlobalAdmit`] guard that releases its slot on drop —
//! exactly the acquire/release discipline a held lease wants. Cross-INSTANCE
//! coordination is explicitly NOT here (that is WP-CROSS-INSTANCE-QUEUE's job);
//! this gate is authoritative only within one fabric process.
//!
//! Owner decision: the global ceiling is **configurable; default ADVISORY**.
//! [`GlobalGatePolicy::Advisory`] always admits but meters the breach (so the
//! wall is observable before it is enforced); [`GlobalGatePolicy::HardReject`]
//! refuses at the wall. The per-tenant SHARE bound is enforced under BOTH
//! policies' accounting but only REJECTS under `HardReject` — under `Advisory`
//! a share breach is metered, never refused.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::tenant::TenantId;

/// Enforcement posture of the global ceiling (owner decision: default
/// [`GlobalGatePolicy::Advisory`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlobalGatePolicy {
    /// Meter breaches but ALWAYS admit. The wall is observed, never enforced —
    /// the safe default so flipping the ceiling on cannot 503 live traffic.
    #[default]
    Advisory,
    /// Refuse admission once the global wall (or a tenant's share) is reached.
    HardReject,
}

/// Why the global gate refused (only ever returned under
/// [`GlobalGatePolicy::HardReject`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalReject {
    /// Total in-flight across all tenants is at/over the global wall.
    GlobalWall,
    /// This tenant already holds its per-tenant share of the global wall.
    TenantShare,
}

/// Outcome of a global admission check.
///
/// On [`GlobalAdmit::Admitted`] the caller holds an RAII guard whose `Drop`
/// frees the slot; on [`GlobalAdmit::Rejected`] no slot was taken.
#[derive(Debug)]
pub enum GlobalAdmit {
    /// Slot granted. The guard MUST be kept alive for the in-flight duration;
    /// dropping it releases the slot back to the global wall.
    Admitted(GlobalAdmitGuard),
    /// Admission refused (HardReject only). Carries the reason for metering.
    Rejected(GlobalReject),
}

impl GlobalAdmit {
    /// Whether a slot was granted.
    pub fn is_admitted(&self) -> bool {
        matches!(self, GlobalAdmit::Admitted(_))
    }

    /// The guard if admitted (consumes self), else `None`.
    pub fn into_guard(self) -> Option<GlobalAdmitGuard> {
        match self {
            GlobalAdmit::Admitted(g) => Some(g),
            GlobalAdmit::Rejected(_) => None,
        }
    }
}

/// Snapshot of the gate's meters — observability surface (the Advisory point:
/// breaches are visible before they are enforced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlobalMeters {
    /// Total slots currently held across all tenants.
    pub in_flight: usize,
    /// Times the global wall was reached on admit (admitted-anyway under
    /// Advisory, refused under HardReject).
    pub wall_breaches: u64,
    /// Times a tenant's per-tenant share was reached on admit.
    pub share_breaches: u64,
}

/// Inner shared state — guarded by one `Mutex`; small + short critical section.
#[derive(Debug, Default)]
struct Inner {
    /// Per-tenant in-flight count. Entries at zero are pruned on release so the
    /// map is bounded by ACTIVE tenants (mirrors `caps::RateWindow` idle-prune).
    per_tenant: HashMap<TenantId, usize>,
    /// Total in-flight = sum of `per_tenant` values, kept incrementally.
    total: usize,
    wall_breaches: u64,
    share_breaches: u64,
}

/// The tenant-aware global admission ceiling.
///
/// Cheaply cloneable (`Arc` inside) so the admission path and the RAII guards
/// share one count map. Pure in-memory/atomic; authoritative within one fabric
/// instance only.
#[derive(Debug, Clone)]
pub struct GlobalGate {
    /// Global wall — total in-flight across all tenants may not exceed this
    /// under `HardReject`. A wall of zero admits nothing under `HardReject`.
    wall: usize,
    /// Per-tenant ceiling as a fraction of the wall, expressed as the MAX slots
    /// one tenant may hold. Pre-computed from `share_numerator`/`denominator`.
    per_tenant_share: usize,
    policy: GlobalGatePolicy,
    inner: Arc<Mutex<Inner>>,
}

impl GlobalGate {
    /// Construct a gate with an explicit global `wall`, a per-tenant `share`
    /// (max slots one tenant may hold), and a `policy`.
    ///
    /// `share` is clamped to `[1, wall]` when `wall > 0` (a share of zero would
    /// admit no tenant; a share above the wall is meaningless) so a single
    /// tenant can never be configured to exceed the wall. With `wall == 0` the
    /// gate admits nothing under `HardReject` (and the share is irrelevant).
    pub fn new(wall: usize, share: usize, policy: GlobalGatePolicy) -> Self {
        let per_tenant_share = if wall == 0 { 0 } else { share.clamp(1, wall) };
        Self {
            wall,
            per_tenant_share,
            policy,
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    /// Construct a gate whose per-tenant share is `wall / max_tenants_expected`
    /// (the fair-division convenience): with N expected tenants each gets a
    /// `1/N` slice of the wall, so no one tenant can take more than its share.
    /// `max_tenants_expected` of zero (or a wall of zero) yields the clamped
    /// minimum share.
    pub fn fair_share(wall: usize, max_tenants_expected: usize, policy: GlobalGatePolicy) -> Self {
        // `checked_div` yields `None` on a zero divisor → fall back to the full
        // wall (no division by an expected-tenant count).
        let share = wall.checked_div(max_tenants_expected).unwrap_or(wall);
        Self::new(wall, share, policy)
    }

    /// The configured global wall.
    pub fn wall(&self) -> usize {
        self.wall
    }

    /// The per-tenant share (max slots one tenant may hold).
    pub fn per_tenant_share(&self) -> usize {
        self.per_tenant_share
    }

    /// The enforcement policy.
    pub fn policy(&self) -> GlobalGatePolicy {
        self.policy
    }

    /// Try to admit one in-flight unit for `tenant` against the global ceiling.
    ///
    /// Checked ALONGSIDE the per-tenant `CapGate` (this is the GLOBAL wall, not
    /// the tenant's own cap). Order:
    /// 1. **Per-tenant share** — `tenant`'s current count `>= per_tenant_share`
    ///    is a share breach.
    /// 2. **Global wall** — `total >= wall` is a wall breach.
    ///
    /// Under [`GlobalGatePolicy::HardReject`] a breach returns
    /// [`GlobalAdmit::Rejected`] and takes NO slot. Under
    /// [`GlobalGatePolicy::Advisory`] a breach is metered but the slot is
    /// granted anyway (so the guard still tracks it). On admit, the per-tenant
    /// count and the total are incremented and an RAII [`GlobalAdmitGuard`] is
    /// returned; its `Drop` decrements both.
    pub fn try_admit_global(&self, tenant: &TenantId) -> GlobalAdmit {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            // Fail-closed on a poisoned lock: never admit if the count map may
            // be inconsistent. (Mirrors CapGate's fail-closed-on-unreadable.)
            Err(_) => return GlobalAdmit::Rejected(GlobalReject::GlobalWall),
        };

        let tenant_count = inner.per_tenant.get(tenant).copied().unwrap_or(0);
        let share_breached = tenant_count >= self.per_tenant_share;
        let wall_breached = inner.total >= self.wall;

        match self.policy {
            GlobalGatePolicy::HardReject => {
                // Share first: a monopolizing tenant is refused even if the wall
                // has headroom, so one tenant can never take all the slots.
                if share_breached {
                    inner.share_breaches += 1;
                    return GlobalAdmit::Rejected(GlobalReject::TenantShare);
                }
                if wall_breached {
                    inner.wall_breaches += 1;
                    return GlobalAdmit::Rejected(GlobalReject::GlobalWall);
                }
            }
            GlobalGatePolicy::Advisory => {
                // Meter breaches but admit regardless — the wall is observed,
                // not enforced. Both meters can tick on one over-limit admit.
                if share_breached {
                    inner.share_breaches += 1;
                }
                if wall_breached {
                    inner.wall_breaches += 1;
                }
            }
        }

        *inner.per_tenant.entry(tenant.clone()).or_insert(0) += 1;
        inner.total += 1;
        GlobalAdmit::Admitted(GlobalAdmitGuard {
            tenant: tenant.clone(),
            inner: Arc::clone(&self.inner),
            released: false,
        })
    }

    /// A point-in-time snapshot of the meters (observability / the Advisory
    /// signal). Fail-closed-safe: a poisoned lock reports zeroes.
    pub fn meters(&self) -> GlobalMeters {
        match self.inner.lock() {
            Ok(inner) => GlobalMeters {
                in_flight: inner.total,
                wall_breaches: inner.wall_breaches,
                share_breaches: inner.share_breaches,
            },
            Err(_) => GlobalMeters::default(),
        }
    }

    /// Current in-flight slots held by `tenant` (0 if none / lock poisoned).
    pub fn tenant_in_flight(&self, tenant: &TenantId) -> usize {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.per_tenant.get(tenant).copied())
            .unwrap_or(0)
    }
}

/// RAII slot held against the [`GlobalGate`]. Dropping it releases the slot
/// back to the global wall (decrements the tenant count + the total, pruning a
/// tenant's entry at zero). Mirrors the acquire/release discipline of a held
/// lease — the guard's lifetime IS the in-flight duration.
#[derive(Debug)]
pub struct GlobalAdmitGuard {
    tenant: TenantId,
    inner: Arc<Mutex<Inner>>,
    released: bool,
}

impl GlobalAdmitGuard {
    /// The tenant this slot is held for.
    pub fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    /// Release the slot eagerly (idempotent; `Drop` also calls this).
    pub fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(count) = inner.per_tenant.get_mut(&self.tenant) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    inner.per_tenant.remove(&self.tenant);
                }
            }
            inner.total = inner.total.saturating_sub(1);
        }
    }
}

impl Drop for GlobalAdmitGuard {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(s: &str) -> TenantId {
        TenantId::new(s).unwrap()
    }

    #[test]
    fn hard_reject_enforces_the_global_wall() {
        // Wall 3, per-tenant share also 3 (== wall). Spread the 3 slots across
        // THREE tenants so no single tenant hits its share — the WALL is then
        // the binding limit, and the 4th admit must be refused for GlobalWall
        // (not TenantShare, since the share check is evaluated first).
        let gate = GlobalGate::new(3, 3, GlobalGatePolicy::HardReject);
        let a = tid("acme");
        let b = tid("bigco");
        let c = tid("carol");

        let _g1 = gate.try_admit_global(&a).into_guard().unwrap();
        let _g2 = gate.try_admit_global(&b).into_guard().unwrap();
        let _g3 = gate.try_admit_global(&c).into_guard().unwrap();
        assert_eq!(gate.meters().in_flight, 3);

        // Wall reached → a FOURTH (fresh) tenant is refused for the wall, no
        // slot taken. (A fresh tenant has zero of its share, so only the wall
        // can bind here.)
        let d = tid("dave");
        match gate.try_admit_global(&d) {
            GlobalAdmit::Rejected(GlobalReject::GlobalWall) => {}
            other => panic!("expected GlobalWall reject, got {other:?}"),
        }
        assert_eq!(gate.meters().in_flight, 3, "rejected admit took no slot");
        assert_eq!(gate.meters().wall_breaches, 1);
    }

    #[test]
    fn advisory_admits_but_meters_the_breach() {
        let gate = GlobalGate::new(2, 2, GlobalGatePolicy::Advisory);
        let a = tid("acme");

        let _g1 = gate.try_admit_global(&a).into_guard().unwrap();
        let _g2 = gate.try_admit_global(&a).into_guard().unwrap();
        // Over the wall — under Advisory this STILL admits.
        let over = gate.try_admit_global(&a);
        assert!(over.is_admitted(), "advisory must admit over the wall");
        let _g3 = over.into_guard().unwrap();

        let m = gate.meters();
        assert_eq!(m.in_flight, 3, "advisory admitted the over-wall slot");
        assert_eq!(m.wall_breaches, 1, "advisory metered the breach");
    }

    #[test]
    fn per_tenant_share_caps_one_tenant_under_hard_reject() {
        // Wall 10 (lots of headroom), but each tenant may hold at most 2.
        let gate = GlobalGate::new(10, 2, GlobalGatePolicy::HardReject);
        let a = tid("acme");
        let b = tid("bigco");

        let _a1 = gate.try_admit_global(&a).into_guard().unwrap();
        let _a2 = gate.try_admit_global(&a).into_guard().unwrap();

        // acme is at its share — refused even though the WALL has 8 free slots.
        match gate.try_admit_global(&a) {
            GlobalAdmit::Rejected(GlobalReject::TenantShare) => {}
            other => panic!("expected TenantShare reject, got {other:?}"),
        }
        assert_eq!(gate.meters().share_breaches, 1);

        // A DIFFERENT tenant is unaffected: the monopolist cannot starve it.
        assert!(
            gate.try_admit_global(&b).is_admitted(),
            "other tenant must still be admitted while acme is share-capped"
        );
    }

    #[test]
    fn advisory_meters_share_breach_but_still_admits() {
        let gate = GlobalGate::new(10, 1, GlobalGatePolicy::Advisory);
        let a = tid("acme");

        let _a1 = gate.try_admit_global(&a).into_guard().unwrap();
        // acme over its share of 1 — Advisory admits and meters.
        let over = gate.try_admit_global(&a);
        assert!(over.is_admitted());
        let _a2 = over.into_guard().unwrap();
        assert_eq!(gate.meters().share_breaches, 1);
        assert_eq!(gate.tenant_in_flight(&a), 2);
    }

    #[test]
    fn release_frees_a_slot() {
        let gate = GlobalGate::new(1, 1, GlobalGatePolicy::HardReject);
        let a = tid("acme");

        let g1 = gate.try_admit_global(&a).into_guard().unwrap();
        // Wall full.
        assert!(matches!(
            gate.try_admit_global(&a),
            GlobalAdmit::Rejected(_)
        ));

        // Drop the guard → slot returns to the wall.
        drop(g1);
        assert_eq!(gate.meters().in_flight, 0, "drop released the slot");
        assert_eq!(gate.tenant_in_flight(&a), 0, "tenant entry pruned at zero");

        // Now a fresh admit succeeds.
        assert!(gate.try_admit_global(&a).is_admitted());
    }

    #[test]
    fn release_is_idempotent() {
        let gate = GlobalGate::new(4, 4, GlobalGatePolicy::HardReject);
        let a = tid("acme");
        let mut g = gate.try_admit_global(&a).into_guard().unwrap();
        g.release();
        g.release(); // second release is a no-op, not a double-decrement
        drop(g); // Drop after explicit release is also a no-op
        assert_eq!(gate.meters().in_flight, 0);
    }

    #[test]
    fn fair_share_divides_the_wall() {
        // 600-slot wall, 6 expected tenants → 100 each.
        let gate = GlobalGate::fair_share(600, 6, GlobalGatePolicy::HardReject);
        assert_eq!(gate.wall(), 600);
        assert_eq!(gate.per_tenant_share(), 100);
    }

    #[test]
    fn share_is_clamped_into_range() {
        // share above the wall is clamped to the wall (cannot exceed it).
        let g1 = GlobalGate::new(5, 100, GlobalGatePolicy::HardReject);
        assert_eq!(g1.per_tenant_share(), 5);
        // share of zero is clamped up to 1 (a tenant can always take one slot).
        let g2 = GlobalGate::new(5, 0, GlobalGatePolicy::HardReject);
        assert_eq!(g2.per_tenant_share(), 1);
        // wall of zero → share is zero (admits nothing under HardReject).
        let g3 = GlobalGate::new(0, 10, GlobalGatePolicy::HardReject);
        assert_eq!(g3.per_tenant_share(), 0);
        assert!(matches!(
            g3.try_admit_global(&tid("acme")),
            GlobalAdmit::Rejected(GlobalReject::TenantShare)
        ));
    }

    #[test]
    fn default_policy_is_advisory() {
        assert_eq!(GlobalGatePolicy::default(), GlobalGatePolicy::Advisory);
    }

    #[test]
    fn total_is_the_sum_across_tenants() {
        let gate = GlobalGate::new(10, 10, GlobalGatePolicy::HardReject);
        let a = tid("acme");
        let b = tid("bigco");
        let _a1 = gate.try_admit_global(&a).into_guard().unwrap();
        let _b1 = gate.try_admit_global(&b).into_guard().unwrap();
        let _b2 = gate.try_admit_global(&b).into_guard().unwrap();
        assert_eq!(gate.meters().in_flight, 3);
        assert_eq!(gate.tenant_in_flight(&a), 1);
        assert_eq!(gate.tenant_in_flight(&b), 2);
    }
}
