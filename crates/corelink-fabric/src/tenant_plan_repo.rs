//! Tenant-plan persistence seam (M1 WAVE-0 frozen anchor).
//!
//! The **root contract** for the M1 multi-tenant control plane: the cap source
//! of truth ([`TenantPlan`](crate::tenant::TenantPlan), org = tenant per
//! ADR-0002) must outlive any single instance. [`TenantPlanRepository`] is the
//! seam the WAVE-1/WAVE-2 work-packages build against — the durable Postgres
//! impl ([`PgTenantPlanRepository`], WP-PERSIST) lands later against the frozen
//! `tenant_plans` DDL declared in [`crate::pg_ledger`].
//!
//! ## Sync trait over an (eventually) async backend
//!
//! Mirrors the crate's existing persistence seams ([`LeaseLedger`] /
//! [`BillingSink`]): the trait is **SYNC** (`&self`, `anyhow::Result`). The
//! Postgres impl bridges to the async client with
//! [`tokio::task::block_in_place`] + `block_on` exactly like
//! [`PgLedger`](crate::pg_ledger::PgLedger). Keeping the trait sync means the
//! caps callers (CP2 admission) thread through the same `std::sync::Mutex`
//! guards as the ledger — no async colouring across the control plane.
//!
//! [`LeaseLedger`]: crate::ledger::LeaseLedger
//! [`BillingSink`]: crate::billing_sink::BillingSink

use crate::tenant::{TenantId, TenantPlan};

/// Durable persistence for per-tenant plan caps — the M1 cap source of truth.
///
/// `load_all` hydrates the in-memory cap registry at boot; `upsert` persists a
/// plan change (tenant lifecycle API / billing-tier sync). Fail-closed by the
/// crate convention: any backend error is an `Err`, never a silently empty load
/// (an empty `load_all` must mean "no plans", never "the DB was unreachable").
pub trait TenantPlanRepository {
    /// Load every persisted tenant plan. The hydration path for the in-memory
    /// cap registry at boot. An empty `Vec` means "no plans persisted", NOT a
    /// backend failure (which is an `Err`).
    fn load_all(&self) -> anyhow::Result<Vec<(TenantId, TenantPlan)>>;

    /// Persist (insert-or-replace) one tenant's plan. Keyed on the tenant; a
    /// repeat upsert overwrites the prior plan for that tenant.
    fn upsert(&self, tenant: &TenantId, plan: &TenantPlan) -> anyhow::Result<()>;
}

/// In-memory [`TenantPlanRepository`] test double — an `RwLock` map.
///
/// Lets the WAVE-1/WAVE-2 cap-registry + tenant-lifecycle logic be exercised
/// WITHOUT a live Postgres (mirrors [`InMemoryLedger`](crate::ledger::InMemoryLedger)
/// / [`MemBillingSink`](crate::billing_sink::MemBillingSink)). Not a production
/// store: the map dies with the process and is per-instance (not cross-instance
/// cap-safe) — the durable [`PgTenantPlanRepository`] (WP-PERSIST) is that.
#[derive(Debug, Default)]
pub struct InMemTenantPlanRepo {
    plans: std::sync::RwLock<std::collections::HashMap<TenantId, TenantPlan>>,
}

impl InMemTenantPlanRepo {
    /// A fresh, empty repository.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct tenant plans currently held.
    pub fn len(&self) -> usize {
        self.plans.read().expect("InMemTenantPlanRepo rwlock").len()
    }

    /// Whether the repository holds no plans.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl TenantPlanRepository for InMemTenantPlanRepo {
    fn load_all(&self) -> anyhow::Result<Vec<(TenantId, TenantPlan)>> {
        let plans = self.plans.read().expect("InMemTenantPlanRepo rwlock");
        Ok(plans.iter().map(|(t, p)| (t.clone(), p.clone())).collect())
    }

    fn upsert(&self, tenant: &TenantId, plan: &TenantPlan) -> anyhow::Result<()> {
        let mut plans = self.plans.write().expect("InMemTenantPlanRepo rwlock");
        plans.insert(tenant.clone(), plan.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(s: &str) -> TenantId {
        TenantId::new(s).expect("valid tenant id")
    }

    fn plan(t: &TenantId, max_concurrency: u32, rate_ceiling_per_min: u32) -> TenantPlan {
        TenantPlan {
            tenant: t.clone(),
            max_concurrency,
            rate_ceiling_per_min,
        }
    }

    /// The root-contract round-trip: an upserted plan is loaded back verbatim,
    /// and a repeat upsert for the same tenant overwrites (never duplicates).
    #[test]
    fn upsert_then_load_round_trips() {
        let repo = InMemTenantPlanRepo::new();
        assert!(repo.is_empty(), "fresh repo holds no plans");

        let acme = tenant("acme");
        let beta = tenant("beta");
        let acme_plan = plan(&acme, 8, 60);
        let beta_plan = plan(&beta, 2, 10);

        repo.upsert(&acme, &acme_plan).expect("upsert acme");
        repo.upsert(&beta, &beta_plan).expect("upsert beta");
        assert_eq!(repo.len(), 2, "two distinct tenants");

        let loaded = repo.load_all().expect("load_all");
        assert_eq!(loaded.len(), 2, "load returns every persisted plan");
        let by_tenant: std::collections::HashMap<TenantId, TenantPlan> =
            loaded.into_iter().collect();
        assert_eq!(by_tenant.get(&acme), Some(&acme_plan), "acme round-trips");
        assert_eq!(by_tenant.get(&beta), Some(&beta_plan), "beta round-trips");

        // Repeat upsert overwrites the same tenant (no duplicate row).
        let acme_plan_v2 = plan(&acme, 16, 120);
        repo.upsert(&acme, &acme_plan_v2).expect("upsert acme v2");
        assert_eq!(repo.len(), 2, "still two tenants — overwrite, not insert");
        let reloaded: std::collections::HashMap<TenantId, TenantPlan> =
            repo.load_all().expect("reload").into_iter().collect();
        assert_eq!(
            reloaded.get(&acme),
            Some(&acme_plan_v2),
            "the overwriting plan is the one loaded back"
        );
    }
}
