//! Wire-contract types for the hugit ⇄ CoreLink Runners seam.
//!
//! # Wire-contract rule
//!
//! Types in this crate are **transcriptions** of the frozen hugit-contracts
//! types, NOT dependencies on that crate. The seam between hugit and
//! corelink-runners is the **wire contract**: types declared independently on
//! each side and proven equivalent by **shared JSON conformance vectors
//! committed byte-identical in both repos**.
//!
//! - No `git` dependencies in either direction (both `deny.toml`: crates.io
//!   only).
//! - The frozen list (R0, 2026-06-10): `RunnerLease`, `RunnerState`,
//!   `FenceManifest` — the runner's whole hugit-contracts surface. Plus the
//!   closure type `MaterializedEntry` referenced by `FenceManifest`.
//! - Conformance vectors live in `conformance/` at the workspace root;
//!   `conformance/manifest.sha256` ties both repos to the same byte-exact
//!   digests. Drift on either side breaks golden tests immediately.
//!
//! Source repo for the originals:
//! hugit-contracts @ 7c2f1e64bc1ba46d4941dc3e5b4a6247c21b0ec0

pub mod fence_manifest;
pub mod runner_lease;

pub use fence_manifest::{FenceManifest, MaterializedEntry};
pub use runner_lease::{RunnerLease, RunnerState};

#[cfg(test)]
mod golden_tests {
    use super::*;
    use std::path::Path;

    /// Load a conformance vector from the workspace-root `conformance/`
    /// directory. Works whether tests run from crate root or workspace root.
    fn load_vector(name: &str) -> String {
        // Walk up from the manifest dir until we find `conformance/`.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        // crate is at <workspace>/crates/corelink-runners-contracts → two levels up
        let workspace = manifest
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace root
            .expect("workspace root not found");
        let path = workspace.join("conformance").join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read conformance vector {name}: {e}"))
    }

    // ── RunnerLease golden round-trip ─────────────────────────────────────

    #[test]
    fn runner_lease_golden_deserialize() {
        let raw = load_vector("RunnerLease.json");
        let _lease: RunnerLease =
            serde_json::from_str(&raw).expect("RunnerLease golden must deserialize");
    }

    #[test]
    fn runner_lease_golden_round_trip_byte_exact() {
        let raw = load_vector("RunnerLease.json");
        let lease: RunnerLease =
            serde_json::from_str(&raw).expect("RunnerLease golden must deserialize");
        let re_serialized =
            serde_json::to_string_pretty(&lease).expect("RunnerLease must re-serialize");
        assert_eq!(
            raw.trim_end(),
            re_serialized.trim_end(),
            "RunnerLease round-trip is not byte-exact"
        );
    }

    // ── FenceManifest golden round-trip ───────────────────────────────────

    #[test]
    fn fence_manifest_golden_deserialize() {
        let raw = load_vector("FenceManifest.json");
        let _manifest: FenceManifest =
            serde_json::from_str(&raw).expect("FenceManifest golden must deserialize");
    }

    #[test]
    fn fence_manifest_golden_round_trip_byte_exact() {
        let raw = load_vector("FenceManifest.json");
        let manifest: FenceManifest =
            serde_json::from_str(&raw).expect("FenceManifest golden must deserialize");
        let re_serialized =
            serde_json::to_string_pretty(&manifest).expect("FenceManifest must re-serialize");
        assert_eq!(
            raw.trim_end(),
            re_serialized.trim_end(),
            "FenceManifest round-trip is not byte-exact"
        );
    }

    // ── manifest.sha256 integrity check ──────────────────────────────────

    #[test]
    fn manifest_sha256_exists_and_covers_both_vectors() {
        let content = load_vector("manifest.sha256");
        assert!(
            content.contains("RunnerLease.json"),
            "manifest.sha256 must list RunnerLease.json"
        );
        assert!(
            content.contains("FenceManifest.json"),
            "manifest.sha256 must list FenceManifest.json"
        );
    }
}
