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

/// What the acquisition path knows about provider contact when abandoning its
/// reserved Pending admission. This evidence is deliberately call-site
/// explicit: an absent registry entry is never used to guess that no box exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingRollbackPhase {
    /// Provisioning was not invoked for this acquisition, so no provider I/O is
    /// necessary before conditionally finishing the durable Pending claim.
    BeforeProvision,
    /// Provisioning was invoked and may have partially spawned a box; only an
    /// authoritative cleanup teardown permits the conditional finish.
    AfterProvision,
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
            let _ = e;
            eprintln!("pending-cleanup: claim failed; will retry next tick");
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
                let _ = e;
                eprintln!("pending-cleanup: finish failed for lease={}", row.lease_id);
                false
            }
        };
        if removed {
            // Side effects happen only after the conditional finish won.
            state.forget_pending_cleanup(&row.lease_id);
            state.revoke_pat_for(&row.lease_id).await;
            state.forget_lease(&row.lease_id);
            finished += 1;
        }
    }
    finished
}

/// Abandon one named acquisition's reserved Pending lease through the same
/// claim/confirm/finish fence as the stale sweep.
///
/// An existing cleanup claim is deliberately reused, so concurrent retries may
/// perform at-least-once teardown of the same Pending handle. A `Pending → Held`
/// winner is never torn down by this rollback. PAT revoke and caller-specific
/// retry policy intentionally remain with the caller.
pub async fn rollback_pending_admission(
    state: &crate::AppState,
    lease_id: &str,
    phase: PendingRollbackPhase,
) -> bool {
    let now = state.clock.now_ms();
    let claimed = match state.ledger.claim_pending_cleanup(lease_id, now) {
        Ok(Some(row)) => row,
        Ok(None) => return false,
        Err(e) => {
            let _ = e;
            eprintln!("pending-cleanup: rollback claim failed for lease={lease_id}");
            return false;
        }
    };

    if phase == PendingRollbackPhase::AfterProvision
        && state.teardown_pending_lease(&claimed.lease_id).await
            != CleanupTeardown::ConfirmedDestroyed
    {
        eprintln!(
            "pending-cleanup: rollback lease={} not confirmed after provision; retaining claim, cap, and compute reservation",
            claimed.lease_id
        );
        return false;
    }

    let finished = match state.ledger.finish_pending_cleanup(&claimed.lease_id) {
        Ok(done) => done,
        Err(e) => {
            let _ = e;
            eprintln!("pending-cleanup: rollback finish failed for lease={lease_id}");
            false
        }
    };
    if finished {
        state.forget_pending_cleanup(&claimed.lease_id);
    }
    finished
}
