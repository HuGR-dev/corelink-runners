//! Focused fail-closed checks for held-lease provider teardown.

use std::sync::Arc;

use corelink_fabric_server::cloud_exec::HybridBoxProvisioner;
use corelink_fabric_server::{BoxProvisioner, NoBoxProvisioner};

#[test]
fn no_box_requires_positive_per_lease_provision_evidence() {
    let provisioner = NoBoxProvisioner::default();
    assert!(provisioner.teardown("restart-unknown-no-box").is_err());

    provisioner
        .provision("known-no-box", &fixture_spec())
        .unwrap();
    assert!(provisioner.teardown("known-no-box").is_ok());
    provisioner.forget_pending_cleanup("known-no-box");
    assert!(provisioner.teardown("known-no-box").is_err());
}

#[test]
fn hybrid_unknown_route_is_not_confirmed() {
    let runner: Arc<dyn BoxProvisioner> = Arc::new(NoBoxProvisioner::default());
    let check: Arc<dyn BoxProvisioner> = Arc::new(NoBoxProvisioner::default());
    let hybrid = HybridBoxProvisioner::new(runner, check);
    assert!(hybrid.teardown("restart-unknown-route").is_err());
}

fn fixture_spec() -> corelink_runner::lease::ContainerSpec {
    corelink_runner::lease::ContainerSpec {
        name: "held-confirmation-fixture".to_owned(),
        image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
            .to_owned(),
        tmp_root: "/tmp/fixture".to_owned(),
        no_network: true,
        allow_egress: false,
        run_on_create: false,
        path_set: Vec::new(),
        env: Vec::new(),
    }
}
