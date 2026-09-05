//! Small, backend-neutral predicates for pending cleanup fencing.
//!
//! Keeping the age/state predicate here prevents the cleanup contract from
//! growing the ledger implementation's already-large state-machine file.

/// A pending row is newly eligible only when it is strictly older than the
/// configured bound. Existing claims are handled by each backend's claim set.
pub(crate) fn is_stale_pending(is_pending: bool, created_at_ms: u64, cutoff_ms: u64) -> bool {
    is_pending && created_at_ms < cutoff_ms
}
