//! WP-INGEST-SCOPE — the per-lease, write-only, ingest-scoped capability token.
//!
//! ## Why this exists (the P0 it closes)
//!
//! The §13.2 turn-feed (WP-TURNFEED) injects an ingest credential into the
//! UNTRUSTED box env ([`crate::envelope_inject`]) so the in-box agent loop can
//! POST its own transcript to the lease's ingest endpoint. The box runs
//! untrusted customer/agent code (contract §4) WITH internet egress (ADR-0003).
//! The original wiring injected the acquiring tenant's RAW, tenant-wide, long-
//! lived Bearer PAT — so a job could read `$CORELINK_ENVELOPE_INGEST_CREDENTIAL`,
//! exfiltrate the PAT, and take over the ENTIRE tenant API (spin billable leases,
//! exec/cancel/close other leases, queue/trigger) — surviving the lease. That
//! violated contract §5 ("secrets never on the box; credential-scan env=0") and
//! broke the ADR-0003 bound (secrets are never on the box — the justification
//! for open egress).
//!
//! The fix: inject a token that authorizes ONLY trajectory-ingest for THAT ONE
//! lease. If exfiltrated, an attacker can only POST trajectory to that (soon-
//! dead) lease's own ingest endpoint — harmless: no tenant takeover, no other
//! capability, no cross-lease reach.
//!
//! ## §5 framing (the scoped-capability exception)
//!
//! §5's `env=0` credential-scan protects TENANT / PLATFORM secrets. The tenant
//! PAT — a tenant-wide, long-lived secret — is **no longer injected** and stays
//! off the box. The scoped ingest token is NOT such a secret: it is a write-only,
//! lease-scoped, ingest-only CAPABILITY the box legitimately needs to stream its
//! OWN trajectory. It is an explicit, documented, scoped exception to env=0 —
//! its worst-case disclosure is bounded to one dying lease's ingest endpoint.
//! The fully-§5-pure channel (a broker / unix-socket with NO credential in env
//! at all) is the FC-era hardening (follow-up); the scoped token removes the P0
//! (tenant takeover) NOW.
//!
//! ## Token formula (deterministic, no per-lease storage)
//!
//! ```text
//! ingest_token = base64_standard( HMAC-SHA256(ingest_secret, DOMAIN ‖ lease_id) )
//! where DOMAIN = "envelope-ingest:v1:"   (ASCII, domain separation)
//! ```
//!
//! The fabric recomputes + verifies it from `lease_id` alone — no per-lease
//! state. The HMAC key ([`IngestSigner`]) is a DEDICATED ingest secret, NEVER
//! the ed25519 attestation signing key: the two key materials are
//! domain-separated by construction (different keys, different algorithms), so
//! an ingest token can never be confused with or forged from an attestation
//! signature and vice versa.
//!
//! HMAC-SHA256 is implemented over the workspace-pinned `sha2` (the same hash
//! dep the fence/memo-key paths already use) — NO new crate enters `Cargo.lock`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use sha2::{Digest, Sha256};

/// Domain-separation prefix folded into every ingest-token pre-image, so the
/// HMAC input can never collide with any other use of the ingest secret.
const INGEST_TOKEN_DOMAIN: &[u8] = b"envelope-ingest:v1:";

/// The dedicated per-fabric ingest secret: the HMAC-SHA256 key that mints +
/// verifies every lease-scoped ingest token.
///
/// This is a SEPARATE key material from the ed25519 attestation
/// [`FabricSigner`](corelink_runner::attest::FabricSigner): domain separation
/// is by construction (distinct key bytes, distinct algorithm), so the two
/// domains can never be confused. The composition root wires a per-region
/// secret in production; tests/local use the deterministic DEV seed.
#[derive(Clone)]
pub struct IngestSigner {
    /// The raw HMAC key bytes. Held opaque — `Debug` is intentionally NOT
    /// derived so the secret can never appear in `{:?}` output.
    key: Vec<u8>,
}

impl std::fmt::Debug for IngestSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IngestSigner(***REDACTED***)")
    }
}

impl IngestSigner {
    /// Construct from raw secret key bytes (any length — HMAC accepts any key
    /// size, normalizing internally).
    #[must_use]
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self { key: secret.into() }
    }

    /// Mint the lease-scoped ingest token for `lease_id`: the standard-base64
    /// HMAC-SHA256 over `DOMAIN ‖ lease_id`. Deterministic — the same
    /// `(secret, lease_id)` always yields the same token, so the fabric needs
    /// no per-lease state to recompute it.
    #[must_use]
    pub fn ingest_token(&self, lease_id: &str) -> String {
        let mut msg = Vec::with_capacity(INGEST_TOKEN_DOMAIN.len() + lease_id.len());
        msg.extend_from_slice(INGEST_TOKEN_DOMAIN);
        msg.extend_from_slice(lease_id.as_bytes());
        BASE64.encode(hmac_sha256(&self.key, &msg))
    }

    /// Verify a `presented` ingest token against the expected token for
    /// `lease_id`, in constant time (no early exit on the first mismatching
    /// byte). Returns `true` iff the presented token is EXACTLY the one this
    /// secret mints for THIS lease.
    ///
    /// Cross-lease isolation is intrinsic: the expected token folds `lease_id`
    /// into the HMAC pre-image, so lease A's token never verifies for lease B.
    #[must_use]
    pub fn verify_ingest_token(&self, lease_id: &str, presented: &str) -> bool {
        let expected = self.ingest_token(lease_id);
        constant_time_eq(expected.as_bytes(), presented.as_bytes())
    }
}

/// HMAC-SHA256(key, msg) per RFC 2104, built on the workspace `sha2` (no new
/// crate). Block size for SHA-256 is 64 bytes; a key longer than the block is
/// first hashed, a shorter key is zero-padded.
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    // Normalize the key to one block.
    let mut k0 = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        k0[..digest.len()].copy_from_slice(&digest);
    } else {
        k0[..key.len()].copy_from_slice(key);
    }
    // ipad / opad.
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k0[i];
        opad[i] ^= k0[i];
    }
    // inner = H(ipad ‖ msg); outer = H(opad ‖ inner).
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let inner = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner);
    outer.finalize().into()
}

/// Constant-time byte-slice equality: no early exit on the first mismatch and
/// the length difference is OR-folded in, so neither value nor length leaks via
/// timing. (Mirrors the hook's `credential_matches` posture.)
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"corelink-runners-DEV-ingest-key!";

    /// HMAC-SHA256 matches the RFC 4231 Test Case 2 known-answer vector
    /// (key="Jefe", data="what do ya want for nothing?"), proving the
    /// hand-rolled construction is a correct HMAC — not a bespoke MAC.
    #[test]
    fn hmac_sha256_rfc4231_tc2_known_vector() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// The token is deterministic: same secret + lease ⇒ same token (the
    /// no-per-lease-state recompute property the verify path depends on).
    #[test]
    fn token_is_deterministic() {
        let signer = IngestSigner::new(SECRET);
        assert_eq!(
            signer.ingest_token("lease-abc"),
            signer.ingest_token("lease-abc")
        );
    }

    /// A token verifies for ITS lease, and the wrong/forged token does not.
    #[test]
    fn verify_accepts_own_token_rejects_wrong() {
        let signer = IngestSigner::new(SECRET);
        let tok = signer.ingest_token("lease-abc");
        assert!(signer.verify_ingest_token("lease-abc", &tok));
        assert!(!signer.verify_ingest_token("lease-abc", "forged"));
        assert!(!signer.verify_ingest_token("lease-abc", ""));
    }

    /// CROSS-LEASE ISOLATION: lease A's token never verifies for lease B (the
    /// lease id is bound into the HMAC pre-image).
    #[test]
    fn cross_lease_token_is_rejected() {
        let signer = IngestSigner::new(SECRET);
        let tok_a = signer.ingest_token("lease-A");
        assert!(signer.verify_ingest_token("lease-A", &tok_a));
        assert!(
            !signer.verify_ingest_token("lease-B", &tok_a),
            "lease A's token must NOT authorize ingest to lease B"
        );
    }

    /// A token minted under a DIFFERENT secret never verifies — the secret is
    /// load-bearing, not just the lease id.
    #[test]
    fn token_under_other_secret_is_rejected() {
        let a = IngestSigner::new(b"secret-A".to_vec());
        let b = IngestSigner::new(b"secret-B".to_vec());
        let tok = a.ingest_token("lease-1");
        assert!(!b.verify_ingest_token("lease-1", &tok));
    }

    /// The token is NOT the secret and NOT trivially derivable: it is a base64
    /// SHA-256-width (32-byte → 44-char padded) MAC, never the raw key.
    #[test]
    fn token_is_a_mac_not_the_secret() {
        let signer = IngestSigner::new(SECRET);
        let tok = signer.ingest_token("lease-abc");
        assert_eq!(tok.len(), 44, "standard base64 of 32 bytes is 44 chars");
        assert_ne!(tok.as_bytes(), SECRET);
    }
}
