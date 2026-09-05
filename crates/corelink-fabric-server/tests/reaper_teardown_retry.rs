//! Offline checks for the confirmed-cleanup seam.
//!
//! The ledger race/claim and provider-engine HTTP tests live with their
//! respective implementations; these tests pin the server-side fail-safe
//! mapping independently of a cloud account.

use corelink_fabric_server::{BoxProvisioner, CleanupTeardown, NoBoxProvisioner};

#[test]
fn no_box_is_the_only_explicit_positive_confirmation_without_a_handle() {
    assert_eq!(
        NoBoxProvisioner.teardown_pending("missing"),
        CleanupTeardown::ConfirmedDestroyed
    );
}

struct UnspecifiedProvisioner;

impl BoxProvisioner for UnspecifiedProvisioner {
    fn provision(
        &self,
        _lease_id: &str,
        _spec: &corelink_runner::lease::ContainerSpec,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn teardown(&self, _lease_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn default_cleanup_is_unconfirmed_not_absence_proof() {
    assert_eq!(
        UnspecifiedProvisioner.teardown_pending("after-restart"),
        CleanupTeardown::Unconfirmed
    );
}
