//! Confirmed cleanup of stale `Pending` leases.
//!
//! Claims are durable ledger fencing, while provider teardown is deliberately
//! outside ledger locks.  A claim is retained until a provider gives an
//! authoritative confirmation; unknown process-local state is never treated
//! as proof that a cloud object is absent.

use std::time::Duration;

/// Result of the cleanup-specific teardown seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupTeardown {
    /// The provider returned its existing successful 2xx/404 result for a
    /// known handle (or the explicit no-box backend was used).
    ConfirmedDestroyed,
    /// The provider or blocking task failed; retain the claim for retry.
    Retryable,
    /// Destruction could not be authoritatively established. Retain claim.
    Unconfirmed,
}

/// Default stale-Pending bound. This remains the existing configuration and
/// is not a default-off switch: only rows older than this bound qualify.
pub const DEFAULT_PENDING_MAX_AGE: Duration = Duration::from_secs(300);

/// Resolve the existing stale-Pending setting.
pub fn pending_max_age_from_env(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Duration> {
    match get("FABRIC_PENDING_MAX_AGE_SECS").filter(|s| !s.is_empty()) {
        None => Ok(DEFAULT_PENDING_MAX_AGE),
        Some(val) => {
            let parsed = val.trim().parse::<u32>().map_err(|_| {
                anyhow::anyhow!(
                    "FABRIC_PENDING_MAX_AGE_SECS must be a valid u32 (got {:?})",
                    val.trim()
                )
            })?;
            anyhow::ensure!(
                parsed > 0,
                "FABRIC_PENDING_MAX_AGE_SECS must be >= 1 (0 would reap a Pending mid-provision)"
            );
            Ok(Duration::from_secs(parsed as u64))
        }
    }
}

/// Claim, teardown, and conditionally finish stale Pending rows.
pub async fn sweep_stale_pending(state: &crate::AppState, max_age: Duration) -> usize {
    let now = state.clock.now_ms();
    let claimed = match state
        .ledger
        .claim_stale_pending_cleanup(now, max_age.as_millis() as u64)
    {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("pending-cleanup: claim failed; will retry next tick: {e:#}");
            return 0;
        }
    };

    let mut finished = 0;
    for row in claimed {
        let result = state.teardown_pending_lease(&row.lease_id).await;
        if result != CleanupTeardown::ConfirmedDestroyed {
            eprintln!(
                "pending-cleanup: lease={} tenant={} not confirmed ({result:?}); retaining claim, cap, and compute reservation",
                row.lease_id, row.tenant
            );
            continue;
        }

        let removed = match state.ledger.finish_pending_cleanup(&row.lease_id) {
            Ok(done) => done,
            Err(e) => {
                eprintln!(
                    "pending-cleanup: finish failed for lease={}: {e:#}",
                    row.lease_id
                );
                false
            }
        };
        if removed {
            // Side effects happen only after the conditional finish won.
            state.revoke_pat_for(&row.lease_id).await;
            state.forget_lease(&row.lease_id);
            finished += 1;
        }
    }
    finished
}
