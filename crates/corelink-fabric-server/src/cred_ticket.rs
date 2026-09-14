//! Track-C C2c — the single-use, lease-bound **credential ticket** that carries
//! the per-job CAS PAT OUT of the untrusted container env.
//!
//! The per-job CAS PAT used to be injected into the runner container env
//! (`CLW_TOKEN`, `crate::runner_inject`), where untrusted code could scrape it.
//! C2c delivers it env-0: the fabric injects a ticket instead
//! (`CLW_CRED_TICKET`), holds the minted PAT server-side
//! (`AppState::stash_cred`), and `clw` REDEEMS the ticket ONCE at the trusted
//! boot — BEFORE any untrusted `clw run` — at `POST
//! /v1/leases/{id}/cas-cred` (`crate::handlers::cas_cred`) to fetch the PAT. A
//! second redemption (e.g. by untrusted code after boot) is `410/gone` because
//! the server-side stash was already taken (single-use latch).
//!
//! ## The ticket
//! ```text
//! CLW_CRED_TICKET = base64_standard( HMAC-SHA256( cred_secret, DOMAIN ‖ lease_id ) )
//! ```
//! Stateless to verify (recompute from `lease_id`), lease-bound (the id is in
//! the pre-image, so lease A's ticket never verifies for lease B), and
//! constant-time compared. The ticket is NOT itself secret — its safety is
//! single-use + redeem-before-untrusted, NOT env secrecy (clw coordinator,
//! 2026-07-02): once redeemed the stash is gone, so a ticket read by untrusted
//! code afterwards buys nothing.
//!
//! The HMAC key is a DEDICATED per-fabric secret — SEPARATE from the ingest
//! secret ([`crate::ingest_token::IngestSigner`]) AND the ed25519 attestation
//! key: domain separation is by construction (distinct key bytes + a distinct
//! [`CRED_TICKET_DOMAIN`] prefix), so no cross-use collision is possible. Reuses
//! the ONE pinned `hmac_sha256` + `constant_time_eq` (no second crypto copy).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::ingest_token::{constant_time_eq, hmac_sha256};

/// Domain-separation prefix folded into every cred-ticket pre-image, so a
/// cred ticket can never be confused with an ingest token (different domain)
/// even if the two secrets were ever mis-wired to the same bytes.
const CRED_TICKET_DOMAIN: &[u8] = b"corelink/cred-ticket/v1:";

/// The dedicated per-fabric credential-ticket secret: the HMAC-SHA256 key that
/// mints + verifies every lease-bound cred ticket. Held opaque (no `Debug`
/// derive) so the secret can never appear in `{:?}` output.
#[derive(Clone)]
pub struct CredTicketSigner {
    key: Vec<u8>,
}

impl std::fmt::Debug for CredTicketSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CredTicketSigner(***REDACTED***)")
    }
}

impl CredTicketSigner {
    /// Construct from raw secret key bytes (HMAC accepts any key size).
    #[must_use]
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self { key: secret.into() }
    }

    /// Mint the lease-bound cred ticket for `lease_id`: standard-base64
    /// HMAC-SHA256 over `DOMAIN ‖ lease_id`. Deterministic — no per-lease state
    /// is needed to recompute it at redemption.
    #[must_use]
    pub fn ticket(&self, lease_id: &str) -> String {
        let mut msg = Vec::with_capacity(CRED_TICKET_DOMAIN.len() + lease_id.len());
        msg.extend_from_slice(CRED_TICKET_DOMAIN);
        msg.extend_from_slice(lease_id.as_bytes());
        BASE64.encode(hmac_sha256(&self.key, &msg))
    }

    /// Verify a `presented` ticket against the expected ticket for `lease_id`,
    /// in constant time. `true` iff `presented` is EXACTLY the ticket this
    /// secret mints for THIS lease.
    #[must_use]
    pub fn verify(&self, lease_id: &str, presented: &str) -> bool {
        let expected = self.ticket(lease_id);
        constant_time_eq(expected.as_bytes(), presented.as_bytes())
    }
}

/// The per-job CAS credential held server-side between acquire (mint + stash)
/// and the single ticket redemption. env-0: the PAT (`token`) never rides the
/// container env — it is handed out ONCE via `POST /v1/leases/{id}/cas-cred`.
#[derive(Clone)]
pub struct StashedCred {
    /// The per-job CAS PAT plaintext (redacting `Debug` below keeps it out of logs).
    pub token: String,
    /// The CAS/AC base URL the runner's `clw` targets (`CLW_ENDPOINT`).
    pub endpoint: String,
    /// The tenant the per-job PAT is scoped to (`CLW_TENANT`).
    pub tenant: String,
}

impl std::fmt::Debug for StashedCred {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StashedCred")
            .field("token", &"***REDACTED***")
            .field("endpoint", &self.endpoint)
            .field("tenant", &self.tenant)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest_token::IngestSigner;

    #[test]
    fn mint_then_verify_round_trips() {
        let s = CredTicketSigner::new(*b"cred-ticket-dev-secret-32-bytes!");
        let t = s.ticket("lease-abc");
        assert!(s.verify("lease-abc", &t), "the minted ticket must verify");
    }

    #[test]
    fn a_ticket_is_lease_bound() {
        let s = CredTicketSigner::new(*b"cred-ticket-dev-secret-32-bytes!");
        let t = s.ticket("lease-A");
        assert!(
            !s.verify("lease-B", &t),
            "lease A's ticket must NOT verify for lease B"
        );
    }

    #[test]
    fn a_tampered_ticket_fails() {
        let s = CredTicketSigner::new(*b"cred-ticket-dev-secret-32-bytes!");
        assert!(!s.verify("lease-abc", "not-a-real-ticket"));
        assert!(!s.verify("lease-abc", ""));
    }

    #[test]
    fn signer_debug_redacts_the_secret_key() {
        // The secret-hygiene contract: a `CredTicketSigner` must NEVER print its
        // key bytes in `{:?}` (a logged AppState would leak the fabric secret).
        let s = CredTicketSigner::new(*b"cred-ticket-dev-secret-32-bytes!");
        let dbg = format!("{s:?}");
        assert_eq!(dbg, "CredTicketSigner(***REDACTED***)");
        assert!(
            !dbg.contains("cred-ticket-dev-secret"),
            "the secret key must never appear in Debug output"
        );
    }

    #[test]
    fn stashed_cred_debug_redacts_the_pat_but_shows_routing() {
        // The stashed PAT plaintext must be redacted; the non-secret routing
        // fields (endpoint, tenant) stay visible for ops debugging.
        let c = StashedCred {
            token: "pat-super-secret-value".to_string(),
            endpoint: "https://cas.example".to_string(),
            tenant: "acme".to_string(),
        };
        let dbg = format!("{c:?}");
        assert!(
            !dbg.contains("pat-super-secret-value"),
            "the PAT plaintext must never appear in Debug output"
        );
        assert!(dbg.contains("***REDACTED***"), "token field is redacted");
        assert!(dbg.contains("cas.example"), "endpoint stays visible");
        assert!(dbg.contains("acme"), "tenant stays visible");
    }

    #[test]
    fn domain_separation_from_ingest_token() {
        // The SAME key bytes wired into both signers must produce DIFFERENT
        // tokens for the same lease — the domain prefix guarantees an ingest
        // token can never be redeemed as a cred ticket (or vice-versa).
        let key = *b"same-bytes-wired-into-both-3232!";
        let cred = CredTicketSigner::new(key);
        let ingest = IngestSigner::new(key);
        let lease = "lease-xyz";
        assert_ne!(
            cred.ticket(lease),
            ingest.ingest_token(lease),
            "domain separation: cred ticket != ingest token even under a shared key"
        );
        assert!(
            !cred.verify(lease, &ingest.ingest_token(lease)),
            "an ingest token must NOT verify as a cred ticket"
        );
    }
}
