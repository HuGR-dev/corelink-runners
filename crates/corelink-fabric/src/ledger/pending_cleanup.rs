//! Atomic stale-Pending cleanup operations for the ledger backends.

use super::{FileInner, InMemoryInner, JournalLine, LeaseRecord, LeaseState};

fn is_stale_pending(record: &LeaseRecord, cutoff_ms: u64) -> bool {
    matches!(record.state, LeaseState::Pending) && record.created_at_ms < cutoff_ms
}

impl InMemoryInner {
    pub(super) fn claim_stale_pending_cleanup(
        &mut self,
        now_ms: u64,
        max_age_ms: u64,
    ) -> anyhow::Result<Vec<LeaseRecord>> {
        let cutoff = now_ms.saturating_sub(max_age_ms);
        let mut rows: Vec<_> = self
            .records
            .values()
            .filter(|record| {
                is_stale_pending(record, cutoff)
                    || self.pending_cleanup_claims.contains(&record.lease_id)
            })
            .cloned()
            .collect();
        rows.sort_by(|a, b| a.lease_id.cmp(&b.lease_id));
        for row in &rows {
            self.pending_cleanup_claims.insert(row.lease_id.clone());
        }
        Ok(rows)
    }

    pub(super) fn finish_pending_cleanup(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        if !self.pending_cleanup_claims.contains(lease_id)
            || !matches!(
                self.records.get(lease_id).map(|record| &record.state),
                Some(LeaseState::Pending)
            )
        {
            return Ok(false);
        }
        self.pending_cleanup_claims.remove(lease_id);
        self.checkpoints.remove(lease_id);
        self.reservations.remove(lease_id);
        Ok(self.records.remove(lease_id).is_some())
    }
}

impl FileInner {
    pub(super) fn claim_stale_pending_cleanup(
        &mut self,
        now_ms: u64,
        max_age_ms: u64,
    ) -> anyhow::Result<Vec<LeaseRecord>> {
        let prior = self.index.pending_cleanup_claims.clone();
        let rows = self.index.claim_stale_pending_cleanup(now_ms, max_age_ms)?;
        for row in &rows {
            if !prior.contains(&row.lease_id) {
                self.append_line(&JournalLine::PendingCleanupClaim {
                    lease_id: row.lease_id.clone(),
                })?;
            }
        }
        Ok(rows)
    }

    pub(super) fn finish_pending_cleanup(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        if !self.index.pending_cleanup_claims.contains(lease_id)
            || !matches!(
                self.index.records.get(lease_id).map(|record| &record.state),
                Some(LeaseState::Pending)
            )
        {
            return Ok(false);
        }
        self.append_line(&JournalLine::Tombstone {
            lease_id: lease_id.to_string(),
        })?;
        self.index.records.remove(lease_id);
        self.index.pending_cleanup_claims.remove(lease_id);
        self.index.checkpoints.remove(lease_id);
        self.index.reservations.remove(lease_id);
        Ok(true)
    }
}
