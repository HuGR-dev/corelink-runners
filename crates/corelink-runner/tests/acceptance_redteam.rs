// Transplanted from hugit/crates/hugit-fence/tests/acceptance_c5b.rs (item ⑤ + its box-lane helpers) @ 69e28e5 (runner-transfer campaign WP-R4, 2026-06-10); removed hugit-side by WP-R4②.
//! WP-C5b item ⑤ acceptance oracle — the active escape red-team, in its new
//! home WITH the fence enforcement it drives.
//!
//! `item_5_escape_redteam_all_attacks_contained` — the active escape red-team
//! (traversal / symlink / out-of-fence / fork-bomb / disk-fill /
//! fence-materialized-escape) is run against live containers and **every**
//! vector is contained; none can reach another lease or starve the box; box
//! residue is **0**. The fence-materialized-escape vector is the one where
//! the fence itself (classify + sparse materialize), not the Docker
//! namespace, is the control — it would escape under a no-op classifier.
//!
//! Relocated, not weakened (WP-R4): the harness moved together with
//! `materialize`/`enforce` so this assertion keeps red-teaming the REAL
//! classifier in-process. Its hermetic load-bearing twin (FakeFsBox, bare
//! gate, no box) rides inside `src/redteam.rs` unchanged. The broker items
//! ②③④⑥ stay hugit-side with the broker.
//!
//! **Box-dependent**: drives the live runner box pinned by
//! `CORELINK_RUNNER_HOST` (env name preserved exactly across the transfer).
//! When the box is unreachable it **FAILS** (not skip) — per contract. It
//! skips only when `CORELINK_RUNNER_HOST` is unset (the bare cargo gate lane).
//!
//! Box-sharing: everything here is namespaced with the prefix `corelink-c5b-`;
//! spawn/probe/teardown touch only that prefix.

use corelink_runner::lease::{BoxExec, SshBox};
use corelink_runner::redteam::{AttackVector, ContainerLimits, RedTeamHarness, RedTeamOutcome};

const IMAGE: &str = "alpine:3.20";

/// Whether the box-dependent acceptance lane is active.
fn box_lane_active() -> bool {
    std::env::var("CORELINK_RUNNER_HOST")
        .ok()
        .is_some_and(|h| !h.trim().is_empty())
}

/// Connect to the live box; FAIL (panic) if it is unreachable, per contract.
fn live_box() -> SshBox {
    let boxx = SshBox::from_env().expect("CORELINK_RUNNER_HOST must be set inside the box lane");
    let ping = boxx
        .run(&["docker", "version", "--format", "{{.Server.Version}}"])
        .expect("ssh to runner box failed to spawn");
    assert!(
        ping.ok() && !ping.stdout.trim().is_empty(),
        "runner box {} unreachable or docker down (code={:?} stderr={:?}); \
         box-dependent acceptance must FAIL, not skip",
        boxx.target,
        ping.code,
        ping.stderr.trim(),
    );
    boxx
}

/// Ensure the job image is present on the box.
fn ensure_image(boxx: &SshBox) {
    let pull = boxx
        .run(&["docker", "pull", IMAGE])
        .expect("docker pull failed to spawn");
    assert!(
        pull.ok(),
        "docker pull {IMAGE} failed: {}",
        pull.stderr.trim()
    );
}

// ── ⑤ active escape red-team: all five vectors contained, residue 0 ──────────
#[test]
fn item_5_escape_redteam_all_attacks_contained() {
    if !box_lane_active() {
        return;
    }
    let boxx = live_box();
    ensure_image(&boxx);

    let mut harness = RedTeamHarness::new(&boxx, IMAGE, ContainerLimits::default());

    // Genuinely attempt every escape against live containers.
    let result = (|| {
        let reports = harness.run_all().map_err(|e| format!("run_all: {e}"))?;
        // Every one of the six vectors must be contained.
        let covered: Vec<AttackVector> = reports.iter().map(|r| r.vector).collect();
        for v in AttackVector::all() {
            if !covered.contains(&v) {
                return Err(format!("attack vector {} was not exercised", v.slug()));
            }
        }
        // The fence-as-the-control vector MUST be exercised: it materializes a
        // real FenceManifest and reads an out-of-fence path in the SAME
        // container. This is the only vector that would ESCAPE if classify()
        // were a no-op constant Inside, so its presence + containment is the
        // honest proof the fence (not just the Docker namespace) holds.
        if !covered.contains(&AttackVector::FenceMaterializedEscape) {
            return Err("fence_materialized_escape vector must be exercised".to_string());
        }
        for r in &reports {
            if r.outcome != RedTeamOutcome::Contained {
                return Err(format!(
                    "ESCAPE: vector {} was NOT contained — {}",
                    r.vector.slug(),
                    r.evidence
                ));
            }
        }
        Ok(())
    })();

    // Always tear down + verify residue, regardless of outcome (prefix-scoped).
    let residue = harness.teardown_all().expect("teardown must reach the box");
    result.expect("escape red-team: all vectors contained");
    assert!(
        residue.is_zero(),
        "box must have ZERO corelink-c5b-* residue; remaining: {:?}",
        residue.remaining
    );
}
