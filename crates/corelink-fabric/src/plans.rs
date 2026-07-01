//! Plan tiers → per-tenant cap enforcement (BIL2).
//!
//! Maps the canonical plan ladder to the [`TenantPlan`] values CP2 enforces.
//! The registry is keyed by **org = tenant** (ADR-0002 §3: "per-tenant caps,
//! fairness, and billing key off the same org = tenant" — one identity, not
//! three): the org IS the tenant key, so a plan set here is the same key the
//! cap gate, fairness, and the Stripe customer all hang off.
//!
//! ## The ladder (pricing.md §2, RATIFIED 2026-06-16)
//!
//! | Tier    | $/mo | max_concurrency | vCPU-h/mo ceiling |
//! |---------|------|-----------------|-------------------|
//! | Starter | $16  | 20              | 100               |
//! | Pro     | $40  | 40              | 240               |
//! | Team    | $100 | 80              | 600               |
//! | Scale   | $200 | 160             | 1200              |
//! | Max     | $400 | 320             | 2400              |
//!
//! The vCPU-h/mo ceiling is the COGS wall (pricing.md §2 40/60 ladder,
//! ratified 2026-06-16). It is expressed internally as vCPU·ms
//! (`ceiling_for(tier)`) and enforced by the compute gate (a later WP);
//! adding it here changes NO runtime behavior — the gate activates only
//! when the acquire path passes a `ComputeGate`.
//!
//! Concurrency caps transcribe the `pricing.md §2` ladder exactly (Starter 20 ·
//! Pro 40 · Team 80 · Scale 160 · Max 320). Above Max is **Enterprise**
//! (custom / governance / BYOC per contract) — it has no fixed cap, so it is
//! deliberately NOT a [`PlanTier`] variant; an enterprise tenant gets a bespoke
//! `TenantPlan` through whatever M2+ contract tooling provisions it, never
//! through this fixed table.
//!
//! ### `rate_ceiling_per_min` is an ABUSE RAIL, not a price (RATIFIED 2026-06-14)
//!
//! `rate_ceiling_per_min` is **NOT a billing dimension** and there is no
//! per-tier rate table — by design. `pricing.md` is explicit: *"Flat by
//! concurrency, never per-minute. Minutes unlimited."* The only two tier limits
//! that price anything are (1) the **concurrency cap** (the `§2` ladder,
//! transcribed verbatim below) and (2) the **vCPU-h/mo compute ceiling** (the
//! COGS wall). A per-minute *price* would directly violate the product's
//! load-bearing inversion of GitHub Actions' per-minute model.
//!
//! This field limits acquire-**REQUEST** throughput (how many `POST /acquire`
//! attempts/min a tenant may fire) — a DoS / hammering guard on the API, never a
//! charge. It is **derived from purchased concurrency**: `max_concurrency * 10`,
//! i.e. ~10 acquire-attempts per minute per slot the tenant bought. That is
//! generous and proportional (a 20-slot Starter gets 200/min), so it is never
//! the binding limit in honest use — only a runaway client trips it.
//!
//! **Decision (tech-lead, ratified 2026-06-14):** keep the `* 10` derivation as
//! the M1+ abuse rail. There is nothing for product to "sign off" here because
//! it is not a price; inventing a per-tier rate table would contradict
//! `pricing.md`. If abuse telemetry ever shows the rail is mis-tuned, it is a
//! one-line operational adjustment, not a pricing change. (A tenant under
//! genuine abuse pressure can also be clamped live via `set_plan` — see below.)
//!
//! ## No caching layer — deliberate
//!
//! [`PlanRegistry`] is the **live** cap source: CP2 reads it on every
//! admission check ([`PlanRegistry::tenant_plan`] builds the `TenantPlan`
//! fresh from the current tier map). There is no snapshot, memo, or derived
//! cache of plan → cap anywhere, because a cache would reintroduce exactly
//! the failure BIL2 exists to prevent: a plan change (upgrade, downgrade,
//! abuse clamp) that only takes effect after a restart or TTL. With the
//! registry as the single mutable source, `set_plan` is visible to the very
//! next `check` — `plan_change_takes_effect_without_restart` pins this.

use std::collections::HashMap;

use crate::compute_meter::MS_PER_VCPU_HOUR;
use crate::tenant::{TenantId, TenantPlan};

/// Self-serve plan tiers from the `pricing.md §2` ladder (fixed-cap rows only;
/// Enterprise — above Max — is custom-contract and intentionally absent, see
/// module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanTier {
    /// 20 parallel runners — solo dev / entry tier ($8/mo).
    Starter,
    /// 40 parallel runners — growing dev + agents ($20/mo).
    Pro,
    /// 80 parallel runners — small team / fleet ($50/mo).
    Team,
    /// 160 parallel runners — busy fleet ($100/mo).
    Scale,
    /// 320 parallel runners — heavy fleet, volume tier ($200/mo).
    Max,
}

impl PlanTier {
    /// Every fixed tier, for table-driven exhaustive checks.
    pub const ALL: [PlanTier; 5] = [
        PlanTier::Starter,
        PlanTier::Pro,
        PlanTier::Team,
        PlanTier::Scale,
        PlanTier::Max,
    ];
}

/// The tier → cap table: `(max_concurrency, rate_ceiling_per_min)`.
///
/// Concurrency is the `pricing.md §2` ladder verbatim. The rate ceiling is the
/// ratified acquire-request abuse rail `max_concurrency * 10` — NOT a price (see
/// module docs: pricing is flat-concurrency, never per-minute).
pub fn plan_for(tier: PlanTier) -> (u32, u32) {
    match tier {
        PlanTier::Starter => (20, 200),
        PlanTier::Pro => (40, 400),
        PlanTier::Team => (80, 800),
        PlanTier::Scale => (160, 1600),
        PlanTier::Max => (320, 3200),
    }
}

/// The per-tier monthly compute ceiling in vCPU·ms (pricing.md §2, RATIFIED 2026-06-16).
///
/// Returns `tier_vcpu_h × MS_PER_VCPU_HOUR`. All ratified ceilings (100–2400 vCPU-h)
/// fit `i64::MAX` with enormous margin (verified by [`compute_meter::fits_ledger`] in
/// the acceptance suite). `0` is the "disabled" sentinel — never `u64::MAX` (per
/// `compute_meter::ceiling_vcpu_ms` contract).
///
/// **Default-off**: nothing calls this yet; the gate activates when the acquire
/// path wires a `ComputeGate` (a later WP). Adding this function changes NO
/// runtime behavior.
pub fn ceiling_for(tier: PlanTier) -> u64 {
    match tier {
        PlanTier::Starter => 100 * MS_PER_VCPU_HOUR,
        PlanTier::Pro => 240 * MS_PER_VCPU_HOUR,
        PlanTier::Team => 600 * MS_PER_VCPU_HOUR,
        PlanTier::Scale => 1200 * MS_PER_VCPU_HOUR,
        PlanTier::Max => 2400 * MS_PER_VCPU_HOUR,
    }
}

/// Org-keyed plan registry — the live cap source of truth CP2 reads.
///
/// Keyed by [`TenantId`] because org = tenant (ADR-0002): the org that signed
/// up for a plan is the same key the cap gate enforces against. Mutating it
/// via [`PlanRegistry::set_plan`] changes admission on the **next** cap check
/// with no restart, rebuild, or cache invalidation (see module docs for why
/// no caching layer exists).
///
/// Fail-closed for unknown tenants: [`PlanRegistry::tenant_plan`] returns a
/// zero plan (`max_concurrency: 0`, `rate_ceiling_per_min: 0`) for any tenant
/// without an entry. A cap of zero admits nothing — `CapGate` rejects before
/// the rate ceiling is even consulted (pinned by `cap_zero_admits_nothing` in
/// `caps.rs`) — so a tenant nobody provisioned can never acquire a slot.
#[derive(Debug, Clone, Default)]
pub struct PlanRegistry {
    tiers: HashMap<TenantId, PlanTier>,
}

impl PlanRegistry {
    /// Empty registry: every tenant is unknown, therefore zero-capped.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (or change) `tenant`'s plan tier. Takes effect on the next
    /// [`PlanRegistry::tenant_plan`] read — no restart.
    pub fn set_plan(&mut self, tenant: TenantId, tier: PlanTier) {
        self.tiers.insert(tenant, tier);
    }

    /// The live [`TenantPlan`] for `tenant`, built fresh from the current
    /// tier map on every call. Unknown tenant → zero plan (fail-closed).
    pub fn tenant_plan(&self, tenant: &TenantId) -> TenantPlan {
        let (max_concurrency, rate_ceiling_per_min) = match self.tiers.get(tenant) {
            Some(&tier) => plan_for(tier),
            // Fail-closed: unprovisioned tenant gets cap 0 / rate 0.
            None => (0, 0),
        };
        TenantPlan {
            tenant: tenant.clone(),
            max_concurrency,
            rate_ceiling_per_min,
            repo_allowlist: Vec::new(),
        }
    }

    /// The monthly compute ceiling in vCPU·ms for `tenant`'s current tier.
    ///
    /// Returns `ceiling_for(tier)` for a provisioned tenant, or `0` (disabled)
    /// for an unknown one. This is consistent with the fail-closed zero-cap
    /// contract: an unknown tenant is already rejected by the concurrency cap
    /// before the compute gate is consulted, so `0` here is never the sole
    /// guard — it is defense-in-depth.
    ///
    /// **Default-off**: nothing calls this yet; the gate activates when the
    /// acquire path wires a `ComputeGate` (a later WP).
    pub fn tenant_ceiling_vcpu_ms(&self, tenant: &TenantId) -> u64 {
        match self.tiers.get(tenant) {
            Some(&tier) => ceiling_for(tier),
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use corelink_runners_contracts::RunnerState;

    use super::*;
    use crate::caps::{CapDecision, CapGate, RateWindow};
    use crate::ledger::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState};

    fn tenant(raw: &str) -> TenantId {
        TenantId::new(raw).unwrap()
    }

    fn held(lease_id: &str, tenant_key: &str) -> LeaseRecord {
        LeaseRecord {
            lease_id: lease_id.to_string(),
            tenant: tenant(tenant_key),
            state: LeaseState::Wire(RunnerState::Held),
            box_ref: format!("box-{lease_id}"),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            deadline_ms: None,
        }
    }

    #[test]
    fn plan_tier_sets_cap_exactly() {
        // Table-driven over ALL tiers: registry output must equal the
        // plan_for table, which must equal the pricing.md §2 ladder.
        let expected: [(PlanTier, u32, u32); 5] = [
            (PlanTier::Starter, 20, 200),
            (PlanTier::Pro, 40, 400),
            (PlanTier::Team, 80, 800),
            (PlanTier::Scale, 160, 1600),
            (PlanTier::Max, 320, 3200),
        ];
        assert_eq!(expected.len(), PlanTier::ALL.len());

        for (tier, cap, rate) in expected {
            assert_eq!(plan_for(tier), (cap, rate), "table mismatch for {tier:?}");

            let t = tenant("acme");
            let mut registry = PlanRegistry::new();
            registry.set_plan(t.clone(), tier);
            let plan = registry.tenant_plan(&t);
            assert_eq!(plan.tenant, t);
            assert_eq!(plan.max_concurrency, cap, "cap mismatch for {tier:?}");
            assert_eq!(
                plan.rate_ceiling_per_min, rate,
                "rate mismatch for {tier:?}"
            );
        }
    }

    #[test]
    fn plan_change_takes_effect_without_restart() {
        // One tenant holds 20 leases; on Starter (cap 20) the next concurrent
        // acquire is rejected — the tier is full.
        let t = tenant("acme");
        let mut ledger = InMemoryLedger::new();
        for i in 0..20 {
            ledger.put(held(&format!("l-{i}"), "acme")).unwrap();
        }

        let mut registry = PlanRegistry::new();
        registry.set_plan(t.clone(), PlanTier::Starter);

        let window = RateWindow::new();
        assert_eq!(
            CapGate.check(&ledger, &registry.tenant_plan(&t), 10_000, &window),
            CapDecision::RejectOverCap
        );

        // Upgrade on the SAME registry — no rebuild of the registry, ledger,
        // gate, or window. The very next check admits (Pro cap 40 > 20 held).
        registry.set_plan(t.clone(), PlanTier::Pro);
        assert_eq!(
            CapGate.check(&ledger, &registry.tenant_plan(&t), 10_000, &window),
            CapDecision::Admit
        );
    }

    #[test]
    fn unknown_tenant_has_zero_cap_fail_closed() {
        // Empty ledger (zero occupancy) AND empty rate window — the ONLY
        // thing rejecting is the zero plan from the unknown tenant.
        let t = tenant("nobody-provisioned-me");
        let registry = PlanRegistry::new();

        let plan = registry.tenant_plan(&t);
        assert_eq!(plan.max_concurrency, 0);
        assert_eq!(plan.rate_ceiling_per_min, 0);

        let ledger = InMemoryLedger::new();
        assert_eq!(
            CapGate.check(&ledger, &plan, 10_000, &RateWindow::new()),
            CapDecision::RejectOverCap
        );
    }

    #[test]
    fn org_is_tenant_doc_pinned() {
        // Source-inclusion pin: the org = tenant keying must stay cited to
        // its deciding ADR in this file.
        let src = include_str!("plans.rs");
        assert!(
            src.contains("ADR-0002"),
            "plans.rs must cite ADR-0002 (org = tenant) for its registry keying"
        );
    }

    #[test]
    fn ceiling_for_ratified_ladder() {
        // Table-driven over ALL tiers: ceiling_for must equal the ratified
        // vCPU-h × MS_PER_VCPU_HOUR (pricing.md §2, RATIFIED 2026-06-16),
        // and every value must fit the i64 ledger column (fits_ledger).
        use crate::compute_meter::{MS_PER_VCPU_HOUR, fits_ledger};

        let expected: [(PlanTier, u64); 5] = [
            (PlanTier::Starter, 100),
            (PlanTier::Pro, 240),
            (PlanTier::Team, 600),
            (PlanTier::Scale, 1200),
            (PlanTier::Max, 2400),
        ];
        assert_eq!(expected.len(), PlanTier::ALL.len());

        for (tier, vcpu_h) in expected {
            let want = vcpu_h * MS_PER_VCPU_HOUR;
            let got = ceiling_for(tier);
            assert_eq!(
                got, want,
                "ceiling_for({tier:?}): expected {want}, got {got}"
            );
            assert!(
                fits_ledger(got),
                "ceiling_for({tier:?}) = {got} overflows i64 ledger"
            );
        }
    }

    #[test]
    fn tenant_ceiling_vcpu_ms_provisioned_and_unknown() {
        // Provisioned tenant: returns the correct ceiling for their tier.
        // Unknown tenant: returns 0 (fail-closed, consistent with zero concurrency cap).
        use crate::compute_meter::MS_PER_VCPU_HOUR;

        let t = tenant("acme");
        let mut registry = PlanRegistry::new();
        registry.set_plan(t.clone(), PlanTier::Pro);

        let want = 240 * MS_PER_VCPU_HOUR;
        assert_eq!(
            registry.tenant_ceiling_vcpu_ms(&t),
            want,
            "provisioned Pro tenant must return 240 vCPU-h ceiling"
        );

        let unknown = tenant("nobody-provisioned-me");
        assert_eq!(
            registry.tenant_ceiling_vcpu_ms(&unknown),
            0,
            "unknown tenant must return 0 (disabled / fail-closed)"
        );
    }
}
