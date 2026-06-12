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
//! (RunnerLease/RunnerState/FenceManifest/MaterializedEntry);
//! IntentMetrics/TokenCounts/ToolCount @ 443ff1b (context_envelope,
//! schema 1.2.0 — see `intent_metrics`).

pub mod fence_manifest;
pub mod intent_metrics;
pub mod runner_lease;

pub use fence_manifest::{FenceManifest, MaterializedEntry};
pub use intent_metrics::{CONTEXT_ENVELOPE_SCHEMA_VERSION, IntentMetrics, TokenCounts, ToolCount};
pub use runner_lease::{RunnerLease, RunnerState};

#[cfg(test)]
mod golden_tests {
    use super::*;
    use std::path::Path;

    /// Path of a conformance vector in the workspace-root `conformance/`
    /// directory. Works whether tests run from crate root or workspace root.
    fn vector_path(name: &str) -> std::path::PathBuf {
        // Walk up from the manifest dir until we find `conformance/`.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        // crate is at <workspace>/crates/corelink-runners-contracts → two levels up
        let workspace = manifest
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace root
            .expect("workspace root not found");
        workspace.join("conformance").join(name)
    }

    /// Load a conformance vector as a UTF-8 string.
    fn load_vector(name: &str) -> String {
        std::fs::read_to_string(vector_path(name))
            .unwrap_or_else(|e| panic!("cannot read conformance vector {name}: {e}"))
    }

    /// Load a conformance vector's raw bytes.
    fn load_vector_bytes(name: &str) -> Vec<u8> {
        std::fs::read(vector_path(name))
            .unwrap_or_else(|e| panic!("cannot read conformance vector {name}: {e}"))
    }

    /// Verify a conformance vector's raw bytes against a recorded lowercase
    /// hex SHA-256 digest.
    fn verify_vector(bytes: &[u8], expected_hex: &str) -> bool {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(bytes);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        hex == expected_hex
    }

    /// Parse `manifest.sha256` lines into `(hex digest, filename)` pairs.
    /// Format: `<hex sha256>  <filename>` per line.
    fn parse_manifest(content: &str) -> Vec<(String, String)> {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| {
                let mut parts = line.split_whitespace();
                let hex = parts
                    .next()
                    .unwrap_or_else(|| panic!("manifest.sha256 line missing digest: {line}"));
                let name = parts
                    .next()
                    .unwrap_or_else(|| panic!("manifest.sha256 line missing filename: {line}"));
                (hex.to_string(), name.to_string())
            })
            .collect()
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

    // ── manifest.sha256 integrity checks ──────────────────────────────────

    #[test]
    fn conformance_vectors_hash_verified() {
        let entries = parse_manifest(&load_vector("manifest.sha256"));
        assert!(!entries.is_empty(), "manifest.sha256 lists no vectors");
        for (hex, name) in &entries {
            let bytes = load_vector_bytes(name);
            assert!(
                verify_vector(&bytes, hex),
                "conformance vector {name} does not match its recorded SHA-256 digest"
            );
        }
    }

    #[test]
    fn conformance_manifest_membership_pinned() {
        let listed: std::collections::BTreeSet<String> =
            parse_manifest(&load_vector("manifest.sha256"))
                .into_iter()
                .map(|(_, name)| name)
                .collect();
        let expected: std::collections::BTreeSet<String> =
            ["RunnerLease.json", "FenceManifest.json"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(
            listed, expected,
            "manifest.sha256 membership drifted from the pinned vector set"
        );
    }

    #[test]
    fn conformance_hash_verifier_rejects_tamper() {
        let entries = parse_manifest(&load_vector("manifest.sha256"));
        let (hex, _) = entries
            .iter()
            .find(|(_, name)| name == "RunnerLease.json")
            .expect("manifest.sha256 must list RunnerLease.json");
        let mut bytes = load_vector_bytes("RunnerLease.json");
        assert!(
            verify_vector(&bytes, hex),
            "pre-tamper sanity: RunnerLease.json must verify"
        );
        bytes[0] ^= 0x01;
        assert!(
            !verify_vector(&bytes, hex),
            "tampered RunnerLease.json bytes must fail verification"
        );
    }
}
