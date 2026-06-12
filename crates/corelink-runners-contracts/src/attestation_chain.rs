// Transcribed from hugit-contracts @ 7736d02 (frozen WP-00) — wire-contract seam, no git dep.

//! AttestationChain — full provenance attestation chain (decomposition §1,
//! item 14 (+); whitepaper §9).
//!
//! Transcribed from hugit-contracts @ 7736d02 (frozen WP-00) per the
//! wire-contract rule — no git dep.

use serde::{Deserialize, Serialize};

/// Full provenance attestation chain (decomposition §1, item 14 (+);
/// whitepaper §9).
///
/// Each field is a content-addressed ref or signature blob representing one
/// link in the provenance chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationChain {
    /// Content-addressed ref to the workspace tree link of the chain.
    pub tree: String,

    /// Content-addressed ref to the check-definition link of the chain.
    pub def: String,

    /// Content-addressed ref to the runner link of the chain.
    pub runner: String,

    /// Content-addressed ref to the model link of the chain.
    pub model: String,

    /// Ordered chain of principals (agent ids / user ids) in the provenance
    /// chain.
    pub principal: Vec<String>,

    /// Detached signature over the canonical pre-image below (base64-encoded
    /// Ed25519 signature).
    ///
    /// # FROZEN signature pre-image (BYTE-EXACT, single-sourced)
    ///
    /// On the hugit side built only by
    /// `hugit_refstore::attestation_sig_preimage`; transcribed here for the
    /// fabric side.
    ///
    /// ```text
    /// preimage = LP(tree) ‖ LP(def) ‖ LP(runner) ‖ LP(model) ‖ VEC(principal)
    /// where LP(s)  = u32_be(byte_len(s)) ‖ utf8_bytes(s)
    /// and   VEC(v) = u32_be(elem_count(v)) ‖ LP(v[0]) ‖ LP(v[1]) ‖ …
    /// ```
    ///
    /// Fields in struct order; the result is the raw ed25519 message —
    /// signed/verified directly, no extra hashing.
    pub sig: String,
}
