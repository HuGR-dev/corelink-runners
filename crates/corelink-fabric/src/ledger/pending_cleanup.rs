//! Atomic stale-Pending cleanup operations for the ledger backends.

use super::{FileInner, InMemoryInner, JournalLine, LeaseRecord, LeaseState};

fn is_stale_pending(record: &LeaseRecord, cutoff_ms: u64) -> bool {
    matches!(record.state, LeaseState::Pending) && record.created_at_ms < cutoff_ms
}

fn candidates(inner: &InMemoryInner, now_ms: u64, max_age_ms: u64) -> Vec<LeaseRecord> {
    let cutoff = now_ms.saturating_sub(max_age_ms);
    let mut rows: Vec<_> = inner
        .records
        .values()
        .filter(|record| {
            matches!(record.state, LeaseState::Pending)
                && (is_stale_pending(record, cutoff)
                    || inner.pending_cleanup_claims.contains(&record.lease_id))
        })
        .cloned()
        .collect();
    rows.sort_by(|a, b| a.lease_id.cmp(&b.lease_id));
    rows
}

impl InMemoryInner {
    pub(super) fn claim_stale_pending_cleanup(
        &mut self,
        now_ms: u64,
        max_age_ms: u64,
    ) -> anyhow::Result<Vec<LeaseRecord>> {
        let rows = candidates(self, now_ms, max_age_ms);
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
        let rows = candidates(&self.index, now_ms, max_age_ms);
        for row in &rows {
            if !prior.contains(&row.lease_id) {
                self.append_line(&JournalLine::PendingCleanupClaim {
                    lease_id: row.lease_id.clone(),
                })?;
            }
            // Publish the in-memory fence only after the fsynced claim line.
            self.index
                .pending_cleanup_claims
                .insert(row.lease_id.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::path::PathBuf;

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "corelink-cleanup-append-failure-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn record() -> LeaseRecord {
        LeaseRecord {
            lease_id: "append-failure".into(),
            tenant: crate::tenant::TenantId::new("append-failure").unwrap(),
            state: LeaseState::Pending,
            box_ref: "box".into(),
            created_at_ms: 1,
            updated_at_ms: 1,
            deadline_ms: None,
            billing_acquired_at_ms: None,
        }
    }

    #[test]
    fn failed_append_does_not_publish_unpersisted_claim() {
        let path = path();
        let ledger = crate::ledger::FileLedger::open(&path).unwrap();
        ledger.put(record()).unwrap();
        let directory = File::open(std::env::temp_dir()).unwrap();
        {
            let mut inner = ledger.lock().unwrap();
            inner.file = directory;
            assert!(inner.claim_stale_pending_cleanup(100, 10).is_err());
            assert!(
                !inner
                    .index
                    .pending_cleanup_claims
                    .contains("append-failure")
            );
            inner.file = OpenOptions::new().append(true).open(&path).unwrap();
        }
        assert_eq!(
            ledger.claim_stale_pending_cleanup(100, 10).unwrap().len(),
            1
        );
        drop(ledger);
        let reopened = crate::ledger::FileLedger::open(&path).unwrap();
        assert_eq!(
            reopened.claim_stale_pending_cleanup(100, 10).unwrap().len(),
            1
        );
        let _ = std::fs::remove_file(path);
    }
}
