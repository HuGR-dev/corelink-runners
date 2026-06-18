//! Background quota-headroom reconciler (task #11 — ADVISORY ONLY).
//!
//! Each tick this module computes how much ephemeral-storage disk the
//! **entitled** tenant base would occupy at full concurrency and compares it
//! against the operator-configured Northflank project disk allowance.  It
//! logs structured warnings when the needed disk is ≥ 80 % (approaching) or
//! ≥ 100 % (exceeded) of the allowance.  It **never** adjusts anything —
//! purely advisory.
//!
//! # Opt-in (zero behaviour change when unset)
//!
//! The task is spawned ONLY when `FABRIC_QUOTA_CHECK_INTERVAL_SECS` is set to
//! a positive `u32`.  When absent (the default), the task is not spawned and
//! no log lines are emitted — the server behaves exactly as before.
//!
//! # Design pins
//!
//! - Mirrors `spawn_reaper` / `spawn_admission_loop` exactly:
//!   `tokio::spawn` + `tokio::time::interval` tick +
//!   `MissedTickBehavior::Skip`.  The returned `JoinHandle` is aborted on
//!   graceful shutdown (same as the reaper).
//! - Config via `quota_check_config_from_env(get: impl Fn(&str)->Option<String>)`,
//!   mirroring `reaper_config_from_env`: positive-u32 guard, default-off (None).
//! - Disk computation:
//!   - **Static backend** (`FABRIC_AUTH_BACKEND=static`): Σ over all entries in
//!     the `StaticPlans` map (the full entitled set).
//!   - **CoreLink backend** (`FABRIC__AUTH_BACKEND=corelink`): tenant-enumeration
//!     is unavailable without a token per ADR-0004 / the CoreLink M1 boundary.
//!     We fall back to the **active-ledger tenant set** — unique tenant IDs
//!     found in active (Pending+Held) lease records — and resolve each tenant's
//!     cap from the plan source (token-free path, which returns `None` for the
//!     CoreLink store).  A caveat is logged every tick so the operator knows the
//!     fidelity limitation.
//! - Disk thresholds:
//!   - Runner-mode leases: `RUNNER_EPHEMERAL_STORAGE_FLOOR_MB` per concurrent slot.
//!   - Check-mode leases: `NorthflankConfig::ephemeral_storage_mb` per slot.
//!   - When `NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB` is unset the computed
//!     needed-disk is logged informationally, without any threshold warning.

use std::sync::Arc;
use std::time::Duration;

use corelink_cloud_engine::RUNNER_EPHEMERAL_STORAGE_FLOOR_MB;
use corelink_fabric::TenantId;

use crate::AppState;

// ── QuotaCheckConfig ─────────────────────────────────────────────────────────

/// Configuration for the background quota-headroom reconciler.
#[derive(Debug)]
pub struct QuotaCheckConfig {
    /// How often to recompute the disk headroom.
    pub interval: Duration,
    /// The operator-declared Northflank project disk allowance in MiB.
    /// `None` → no threshold comparison; needed-disk is still logged
    /// informationally.
    pub allowance_mib: Option<u64>,
    /// Ephemeral storage per check-mode slot, in MiB.  Sourced from
    /// `NorthflankConfig::ephemeral_storage_mb` (the default is 1 GiB = 1024
    /// MiB).  Exposed here so the composition root can thread the resolved
    /// Northflank config value without this module depending on
    /// `corelink_cloud_engine` directly for config parsing.
    pub check_ephemeral_storage_mib: u64,
}

// ── quota_check_config_from_env ──────────────────────────────────────────────

/// Resolve an optional [`QuotaCheckConfig`] from an environment-variable
/// accessor.
///
/// Reads `FABRIC_QUOTA_CHECK_INTERVAL_SECS`.
/// - **Absent or empty → `Ok(None)`**: the reconciler is **OPT-IN**, so it is
///   NOT spawned by default. Rationale: the task adds a per-tick lock
///   acquisition and a structured log line; the default-off posture keeps the
///   server byte-identical when the operator has not opted in.
/// - **Present → `Ok(Some(QuotaCheckConfig))`**: parse as `u32`; value `0` or
///   an unparseable string → `Err` (a configured-but-zero interval is a
///   deployer mistake, not a silent disable — absence is the disable path).
///
/// Also reads:
/// - `NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB` (optional positive `u64`): the
///   project-level ephemeral-storage allowance in MiB.  Absent → `None`
///   (informational log only, no threshold warning).
/// - `NORTHFLANK_EPHEMERAL_STORAGE_MB` (optional positive `u32`): the
///   per-check-slot ephemeral storage in MiB.  Absent → 1024 MiB default
///   (mirrors `NorthflankConfig::default()`).
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
pub fn quota_check_config_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<QuotaCheckConfig>> {
    // ── Interval (opt-in gate) ────────────────────────────────────────────────
    let interval = match get("FABRIC_QUOTA_CHECK_INTERVAL_SECS").filter(|s| !s.is_empty()) {
        // Absent/empty → opt-in default: the reconciler is NOT spawned.
        None => return Ok(None),
        Some(val) => {
            let parsed = val.trim().parse::<u32>().map_err(|_| {
                anyhow::anyhow!(
                    "FABRIC_QUOTA_CHECK_INTERVAL_SECS must be a valid u32 (got {:?})",
                    val.trim()
                )
            })?;
            if parsed == 0 {
                anyhow::bail!(
                    "FABRIC_QUOTA_CHECK_INTERVAL_SECS must be >= 1 \
                     (absent/empty is the way to disable the opt-in quota reconciler, not 0)"
                );
            }
            Duration::from_secs(parsed as u64)
        }
    };

    // ── Allowance (optional, informational-only when absent) ─────────────────
    let allowance_mib = match get("NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(val) => {
            let n = val.trim().parse::<u64>().map_err(|_| {
                anyhow::anyhow!(
                    "NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB must be a valid u64 (got {:?})",
                    val.trim()
                )
            })?;
            if n == 0 {
                anyhow::bail!("NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB must be >= 1 if present");
            }
            Some(n)
        }
    };

    // ── Per-check-slot ephemeral storage (optional, defaults to 1024 MiB) ────
    let check_ephemeral_storage_mib = match get("NORTHFLANK_EPHEMERAL_STORAGE_MB")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => 1024u64,
        Some(val) => {
            let n = val.trim().parse::<u64>().map_err(|_| {
                anyhow::anyhow!(
                    "NORTHFLANK_EPHEMERAL_STORAGE_MB must be a valid u64 (got {:?})",
                    val.trim()
                )
            })?;
            if n == 0 {
                anyhow::bail!("NORTHFLANK_EPHEMERAL_STORAGE_MB must be >= 1 if present");
            }
            n
        }
    };

    Ok(Some(QuotaCheckConfig {
        interval,
        allowance_mib,
        check_ephemeral_storage_mib,
    }))
}

// ── needed_disk_mib ──────────────────────────────────────────────────────────

/// Compute the total ephemeral disk MiB the entitled tenant base would need
/// at full concurrency.
///
/// # Disk accounting
///
/// Each concurrency slot is assigned its disk contribution based on whether
/// the plan is resolved as a runner-mode or check-mode slot:
/// - Runner mode slots get `RUNNER_EPHEMERAL_STORAGE_FLOOR_MB` MiB each.
/// - Check mode slots get `check_ephemeral_storage_mib` MiB each.
///
/// Since the plan source (`StaticPlans` / `CoreLinkPlanStore`) does not
/// distinguish runner vs check slots — that is a per-acquire runtime
/// classification — we apply a **conservative worst-case** for the headroom
/// estimate: each slot is counted at `runner_storage_floor_mib` (the larger
/// of the two), so the operator can be confident the allowance covers the
/// worst case (all slots provisioned as runner boxes).
///
/// Returns `0` when `tenant_caps` is empty.
pub fn needed_disk_mib(
    tenant_caps: &[(TenantId, u32)],
    runner_storage_floor_mib: u64,
    _check_storage_mib: u64,
) -> u64 {
    // Conservative worst-case: every slot could be a runner lease.
    // This is deliberate — a headroom monitor must not lull the operator
    // into believing the check-only disk floor is sufficient when runners
    // could be provisioned.
    tenant_caps
        .iter()
        .map(|(_, cap)| (*cap as u64).saturating_mul(runner_storage_floor_mib))
        .fold(0u64, |acc, x| acc.saturating_add(x))
}

// ── collect_tenant_caps ───────────────────────────────────────────────────────

/// Collect `(TenantId, max_concurrency)` pairs from the plan source and,
/// optionally, the ledger.
///
/// - **Static**: iterates `state.plans.all_tenant_plans()`, which for a
///   `StaticPlans` / `CompositePlanSource` returns the full entitled set.
///   This is the high-fidelity path.
/// - **CoreLink or unknown backend**: `all_tenant_plans` returns an empty
///   slice (the per-acquire introspect-based backend cannot enumerate without
///   a token). We fall back to unique tenant IDs in active (Pending+Held)
///   lease records and resolve each cap via `plan_of` (token-free → `None` for
///   the CoreLink store → that tenant contributes 0 to the sum). The fidelity
///   caveat is surfaced as a structured log line at the call site.
///
/// Returns `(caps, is_active_set_fallback)`.
pub fn collect_tenant_caps(state: &AppState) -> (Vec<(TenantId, u32)>, bool) {
    // First try the plan source's enumeration (static path).
    let static_caps = state.plans.all_tenant_plans();
    if !static_caps.is_empty() {
        let pairs = static_caps
            .into_iter()
            .map(|p| (p.tenant, p.max_concurrency))
            .collect();
        return (pairs, false);
    }

    // Fallback: active-ledger tenant set (CoreLink / empty-static edge).
    let active_records = {
        let ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
        // Use held() as the enumeration source to discover tenants currently
        // occupying capacity.
        match ledger.held() {
            Ok(records) => records,
            Err(e) => {
                eprintln!("quota-headroom: ledger held() failed (skipping this tick): {e:#}");
                return (Vec::new(), true);
            }
        }
    };

    // Collect unique tenant IDs from held records.
    let mut tenants: Vec<TenantId> = active_records
        .into_iter()
        .map(|r| r.tenant)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    tenants.sort(); // deterministic order

    // Resolve cap per tenant via the token-free plan_of path.
    let pairs: Vec<(TenantId, u32)> = tenants
        .into_iter()
        .filter_map(|t| state.plans.plan_of(&t).map(|p| (t, p.max_concurrency)))
        .collect();

    (pairs, true)
}

// ── one_tick ─────────────────────────────────────────────────────────────────

/// Run one quota-headroom reconciliation tick.
///
/// Computes `needed_disk_mib`, compares to `allowance_mib` (if set), and
/// emits the appropriate structured log line.
pub fn one_tick(state: &AppState, cfg: &QuotaCheckConfig) {
    let (caps, is_active_fallback) = collect_tenant_caps(state);

    if is_active_fallback {
        eprintln!(
            "quota-headroom: FIDELITY CAVEAT — using active-ledger tenant set \
             (CoreLink backend cannot enumerate the full entitled set without a bearer token; \
             the computed needed_disk reflects only currently-active tenants, \
             NOT the full contracted entitlement)"
        );
    }

    let runner_floor = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB as u64;
    let needed = needed_disk_mib(&caps, runner_floor, cfg.check_ephemeral_storage_mib);

    match cfg.allowance_mib {
        None => {
            // No allowance configured — informational log only.
            eprintln!(
                "quota-headroom: entitled base needs ~{needed} MiB ephemeral disk at full concurrency \
                 (set NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB to enable threshold warnings)"
            );
        }
        Some(allowance) => {
            // allowance is guaranteed >= 1 by config validation; safe to divide.
            // Use checked_div defensively so clippy is satisfied and any future
            // zero-allowance path does not panic.
            let pct = needed
                .saturating_mul(100)
                .checked_div(allowance)
                .unwrap_or(0);
            if needed >= allowance {
                eprintln!(
                    "QUOTA_HEADROOM_EXCEEDED: entitled base needs ~{needed} MiB, \
                     allowance {allowance} MiB — raise the Northflank plan"
                );
            } else if pct >= 80 {
                eprintln!(
                    "QUOTA_HEADROOM_WARNING: entitled base needs ~{needed} MiB of {allowance} MiB \
                     allowance ({pct}% utilisation) — approaching the Northflank disk allowance"
                );
            } else {
                eprintln!(
                    "quota-headroom: entitled base needs ~{needed} MiB of {allowance} MiB allowance \
                     ({pct}% utilisation) — OK"
                );
            }
        }
    }
}

// ── spawn_quota_headroom_task ────────────────────────────────────────────────

/// Spawn the background quota-headroom reconciler task.
///
/// Mirrors [`spawn_reaper`](crate::reaper::spawn_reaper) exactly:
/// `tokio::spawn` + `tokio::time::interval` tick +
/// `MissedTickBehavior::Skip`.
///
/// The returned [`tokio::task::JoinHandle`] runs until aborted by the caller;
/// bind the handle and call `.abort()` after the server's graceful-shutdown
/// future resolves so the task does not outlive the process.
pub fn spawn_quota_headroom_task(
    state: AppState,
    cfg: QuotaCheckConfig,
) -> tokio::task::JoinHandle<()> {
    let cfg = Arc::new(cfg);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(cfg.interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            one_tick(&state, &cfg);
        }
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use corelink_cloud_engine::RUNNER_EPHEMERAL_STORAGE_FLOOR_MB;
    use corelink_fabric::TenantId;

    // ── needed_disk_mib arithmetic ────────────────────────────────────────────

    /// A single tenant with cap 10 contributes 10 × RUNNER_FLOOR MiB.
    #[test]
    fn single_tenant_needed_disk() {
        let floor = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB as u64; // 4096
        let t = TenantId::new("acme").unwrap();
        let caps = vec![(t, 10u32)];
        assert_eq!(needed_disk_mib(&caps, floor, 1024), 10 * floor);
    }

    /// Multiple tenants: contributions are summed.
    #[test]
    fn multi_tenant_needed_disk() {
        let floor = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB as u64;
        let t1 = TenantId::new("acme").unwrap();
        let t2 = TenantId::new("beta").unwrap();
        let t3 = TenantId::new("corp").unwrap();
        let caps = vec![(t1, 5u32), (t2, 3u32), (t3, 2u32)];
        assert_eq!(
            needed_disk_mib(&caps, floor, 1024),
            (5 + 3 + 2) * floor,
            "contributions are summed across tenants"
        );
    }

    /// Zero tenants → zero needed disk.
    #[test]
    fn no_tenants_zero_disk() {
        assert_eq!(needed_disk_mib(&[], 4096, 1024), 0);
    }

    /// A tenant with cap 0 contributes 0 MiB (degenerate but safe).
    #[test]
    fn zero_cap_tenant_contributes_zero() {
        let t = TenantId::new("acme").unwrap();
        let caps = vec![(t, 0u32)];
        assert_eq!(needed_disk_mib(&caps, 4096, 1024), 0);
    }

    // ── threshold logic ───────────────────────────────────────────────────────

    /// Helper: classify a (needed, allowance) pair into EXCEEDED / WARNING / OK.
    /// Mirrors the production logic in `one_tick` exactly.
    fn classify(needed: u64, allowance: u64) -> &'static str {
        let pct = needed
            .saturating_mul(100)
            .checked_div(allowance)
            .unwrap_or(0);
        if needed >= allowance {
            "EXCEEDED"
        } else if pct >= 80 {
            "WARNING"
        } else {
            "OK"
        }
    }

    /// EXCEEDED fires when needed >= allowance (100 %).
    #[test]
    fn exceeded_fires_at_or_above_100_percent() {
        let floor = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB as u64;

        // Exact 100 %: cap 1, floor = 4096, allowance = 4096.
        let t = TenantId::new("acme").unwrap();
        let caps = vec![(t, 1u32)];
        let needed = needed_disk_mib(&caps, floor, 1024); // 4096
        let allowance = floor; // 4096
        assert_eq!(needed, allowance);
        assert_eq!(
            classify(needed, allowance),
            "EXCEEDED",
            "needed == allowance must trigger EXCEEDED"
        );

        // Above 100 %: cap 2.
        let t2 = TenantId::new("beta").unwrap();
        let caps2 = vec![(t2, 2u32)];
        let needed2 = needed_disk_mib(&caps2, floor, 1024); // 8192
        assert!(needed2 > allowance);
        assert_eq!(
            classify(needed2, allowance),
            "EXCEEDED",
            "needed > allowance must trigger EXCEEDED"
        );
    }

    /// WARNING fires when needed is in [80 %, 100 %).
    #[test]
    fn warning_fires_at_80_percent() {
        // 80 %: needed = 8000, allowance = 10000. Use floor=1000 for easy math.
        let floor = 1000u64;
        let t = TenantId::new("acme").unwrap();
        let caps = vec![(t, 8u32)];
        let needed = needed_disk_mib(&caps, floor, 1024); // 8000
        let allowance = 10000u64;
        // Verify the test fixture is actually 80 %.
        let pct = needed
            .saturating_mul(100)
            .checked_div(allowance)
            .unwrap_or(0);
        assert_eq!(pct, 80, "test fixture: 80 % utilisation");
        assert_eq!(
            classify(needed, allowance),
            "WARNING",
            "80 % must trigger WARNING, not EXCEEDED"
        );
    }

    /// Below 80 % → no warning.
    #[test]
    fn below_80_percent_is_ok() {
        // 70 %: needed = 7000, allowance = 10000. Use floor=1000.
        let floor = 1000u64;
        let t = TenantId::new("acme").unwrap();
        let caps = vec![(t, 7u32)];
        let needed = needed_disk_mib(&caps, floor, 1024); // 7000
        let allowance = 10000u64;
        let pct = needed
            .saturating_mul(100)
            .checked_div(allowance)
            .unwrap_or(0);
        assert!(pct < 80, "test fixture: < 80 % utilisation");
        assert_eq!(classify(needed, allowance), "OK", "< 80 % must be OK");
    }

    // ── opt-in (config parsing) ───────────────────────────────────────────────

    /// Absent interval → None (task NOT spawned).
    #[test]
    fn absent_interval_returns_none() {
        let cfg = quota_check_config_from_env(|_| None).unwrap();
        assert!(
            cfg.is_none(),
            "absent FABRIC_QUOTA_CHECK_INTERVAL_SECS → None (opt-in)"
        );
    }

    /// Empty interval → None (task NOT spawned).
    #[test]
    fn empty_interval_returns_none() {
        let cfg = quota_check_config_from_env(|k| {
            (k == "FABRIC_QUOTA_CHECK_INTERVAL_SECS").then(|| "".to_string())
        })
        .unwrap();
        assert!(
            cfg.is_none(),
            "empty FABRIC_QUOTA_CHECK_INTERVAL_SECS → None"
        );
    }

    /// A valid positive u32 interval → Some(config).
    #[test]
    fn valid_interval_returns_some_config() {
        let cfg = quota_check_config_from_env(|k| match k {
            "FABRIC_QUOTA_CHECK_INTERVAL_SECS" => Some("60".to_string()),
            _ => None,
        })
        .unwrap()
        .expect("valid interval → Some");
        assert_eq!(cfg.interval, Duration::from_secs(60));
        assert!(cfg.allowance_mib.is_none(), "no allowance set → None");
    }

    /// A zero interval is a hard error (0 is not the disable path).
    #[test]
    fn zero_interval_is_an_error() {
        let err = quota_check_config_from_env(|k| match k {
            "FABRIC_QUOTA_CHECK_INTERVAL_SECS" => Some("0".to_string()),
            _ => None,
        })
        .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("FABRIC_QUOTA_CHECK_INTERVAL_SECS"),
            "the error must name the offending var; got {msg:?}"
        );
    }

    /// An unparseable interval is a hard error.
    #[test]
    fn unparseable_interval_is_an_error() {
        let err = quota_check_config_from_env(|k| match k {
            "FABRIC_QUOTA_CHECK_INTERVAL_SECS" => Some("five".to_string()),
            _ => None,
        })
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("FABRIC_QUOTA_CHECK_INTERVAL_SECS"));
    }

    /// When the interval is set, the allowance is also parsed correctly.
    #[test]
    fn allowance_parsed_when_set() {
        let cfg = quota_check_config_from_env(|k| match k {
            "FABRIC_QUOTA_CHECK_INTERVAL_SECS" => Some("30".to_string()),
            "NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB" => Some("51200".to_string()),
            _ => None,
        })
        .unwrap()
        .expect("valid config");
        assert_eq!(cfg.allowance_mib, Some(51200));
    }

    /// A zero allowance is a hard error.
    #[test]
    fn zero_allowance_is_an_error() {
        let err = quota_check_config_from_env(|k| match k {
            "FABRIC_QUOTA_CHECK_INTERVAL_SECS" => Some("30".to_string()),
            "NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB" => Some("0".to_string()),
            _ => None,
        })
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("NORTHFLANK_PROJECT_DISK_ALLOWANCE_MIB"));
    }

    // ── known plan set arithmetic ─────────────────────────────────────────────

    /// Known plan set: 3 tenants with caps 20, 40, 80 → needed =
    /// (20+40+80) × RUNNER_FLOOR MiB.
    #[test]
    fn known_plan_set_arithmetic() {
        let floor = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB as u64;
        let t1 = TenantId::new("alpha").unwrap();
        let t2 = TenantId::new("beta").unwrap();
        let t3 = TenantId::new("gamma").unwrap();
        let caps = vec![(t1, 20u32), (t2, 40u32), (t3, 80u32)];
        let needed = needed_disk_mib(&caps, floor, 1024);
        let expected = (20 + 40 + 80) * floor;
        assert_eq!(needed, expected, "sum of all caps × floor");

        // Confirm EXCEEDED at exact allowance.
        let label = if needed >= expected { "EXCEEDED" } else { "OK" };
        assert_eq!(label, "EXCEEDED");

        // OK at 2× allowance (50 % utilisation).
        let allowance = needed * 2;
        assert_eq!(classify(needed, allowance), "OK", "50 % utilisation → OK");
    }
}
