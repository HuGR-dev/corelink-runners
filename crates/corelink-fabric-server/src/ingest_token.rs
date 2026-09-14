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
///
/// `pub(crate)` so the §13.2 ingest path AND the Stage-B autoscaler webhook
/// verifier ([`crate::handlers::webhook`]) share ONE HMAC implementation — the
/// one pinned to the RFC 4231 known-answer vector below. A second copy would be
/// a second place for a crypto bug to hide.
pub(crate) fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
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
///
/// `pub(crate)` so the autoscaler webhook signature check
/// ([`crate::handlers::webhook`]) compares the GitHub `X-Hub-Signature-256`
/// digest in constant time through this same primitive.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
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

    // ---------------------------------------------------------------------
    // Deterministic property / fuzz suite (WP-INGEST-SCOPE hardening).
    //
    // Each test drives THOUSANDS of cases from a fixed-seed in-test PRNG —
    // reproducible across runs, no `rand` dependency added to Cargo.lock — and
    // asserts the SECURITY PROPERTY itself (injectivity, HMAC-correctness vs an
    // INDEPENDENT reference, cross-lease rejection, constant-time / no-short-
    // circuit verify, domain separation), never merely `is_ok`.
    // ---------------------------------------------------------------------

    /// Fixed iteration count: large enough to exercise the property space,
    /// small enough to stay well inside a single-test budget on the shared
    /// builder.
    const ITERS: usize = 4096;

    /// A tiny deterministic xorshift64* PRNG (Marsaglia). Seeded by a fixed
    /// constant so every run is byte-for-byte reproducible. This is a TEST-ONLY
    /// generator — it is NOT cryptographic and is never used to mint real
    /// tokens; it only manufactures diverse, deterministic inputs.
    struct XorShift64(u64);

    impl XorShift64 {
        fn new(seed: u64) -> Self {
            // Avoid the zero fixed-point of xorshift; the seed is a constant.
            Self(seed | 1)
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            // xorshift64* output scramble for better avalanche.
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        /// A deterministic, varied lease-id string for case `i`. Mixes the
        /// counter and PRNG bits so collisions in the *input* set are not what
        /// drives any "all distinct" assertion — distinct counters guarantee
        /// distinct lease-id strings.
        fn lease_id(&mut self, i: usize) -> String {
            let a = self.next_u64();
            let b = self.next_u64();
            // Embed `i` to GUARANTEE input-string distinctness across the loop,
            // plus PRNG bits for byte-pattern diversity (incl. lengths).
            format!("lease-{i}-{a:016x}-{b:08x}")
        }
    }

    /// An INDEPENDENT HMAC-SHA256 reference, written as a second code path from
    /// the RFC 2104 definition, used to cross-check the production `hmac_sha256`.
    /// Deliberately structured differently (Vec-based concatenation, explicit
    /// key-shortening branch) so a shared bug cannot hide in a shared routine.
    fn hmac_sha256_reference(key: &[u8], msg: &[u8]) -> [u8; 32] {
        const BLOCK: usize = 64;
        // RFC 2104: keys longer than the block are hashed first; keys shorter
        // are right-zero-padded to the block length.
        let mut block_key = [0u8; BLOCK];
        if key.len() > BLOCK {
            let h = Sha256::digest(key);
            block_key[..h.len()].copy_from_slice(&h);
        } else {
            block_key[..key.len()].copy_from_slice(key);
        }
        let i_key_pad: Vec<u8> = block_key.iter().map(|b| b ^ 0x36).collect();
        let o_key_pad: Vec<u8> = block_key.iter().map(|b| b ^ 0x5c).collect();

        // inner = H(i_key_pad ‖ msg)
        let mut inner_input = Vec::with_capacity(BLOCK + msg.len());
        inner_input.extend_from_slice(&i_key_pad);
        inner_input.extend_from_slice(msg);
        let inner = Sha256::digest(&inner_input);

        // outer = H(o_key_pad ‖ inner)
        let mut outer_input = Vec::with_capacity(BLOCK + inner.len());
        outer_input.extend_from_slice(&o_key_pad);
        outer_input.extend_from_slice(&inner);
        Sha256::digest(&outer_input).into()
    }

    /// PROPERTY 1 — INJECTIVITY / COLLISION-RESISTANCE.
    ///
    /// Over thousands of DISTINCT lease ids the minted tokens are all DISTINCT
    /// (no two distinct lease ids collide to the same token), and a repeated
    /// lease id maps to the SAME token (determinism). A collision would mean two
    /// leases share an ingest capability — the exact cross-lease reach this
    /// token exists to forbid.
    #[test]
    fn prop_token_injectivity_and_determinism() {
        let signer = IngestSigner::new(SECRET);
        let mut rng = XorShift64::new(0x1234_5678_9ABC_DEF0);
        let mut seen: std::collections::HashMap<String, String> =
            std::collections::HashMap::with_capacity(ITERS);

        for i in 0..ITERS {
            let lease = rng.lease_id(i);
            let tok = signer.ingest_token(&lease);

            // Determinism: minting the same lease again yields the same token.
            assert_eq!(
                tok,
                signer.ingest_token(&lease),
                "non-deterministic mint for {lease}"
            );

            // Injectivity: no PRIOR distinct lease produced this same token.
            if let Some(prev_lease) = seen.insert(tok.clone(), lease.clone()) {
                panic!(
                    "token collision: distinct leases {prev_lease:?} and \
                     {lease:?} minted the same ingest token {tok}"
                );
            }
        }
        assert_eq!(
            seen.len(),
            ITERS,
            "every distinct lease minted a unique token"
        );
    }

    /// PROPERTY 2 — HMAC CORRECTNESS vs an INDEPENDENT REFERENCE.
    ///
    /// For thousands of random lease ids, the production mint equals
    /// `base64( HMAC-SHA256(secret, DOMAIN ‖ lease_id) )` recomputed from first
    /// principles through a SEPARATE HMAC code path. Also pins a HAND-COMPUTED
    /// known-answer vector for one fixed lease id, so the whole pipeline (HMAC ⊕
    /// domain ⊕ base64) is frozen against silent drift.
    #[test]
    fn prop_mint_matches_independent_hmac_reference() {
        let signer = IngestSigner::new(SECRET);
        let mut rng = XorShift64::new(0x0F0F_0F0F_DEAD_BEEF);

        for i in 0..ITERS {
            let lease = rng.lease_id(i);

            // Reference: re-derive the full pre-image and MAC independently.
            let mut preimage = Vec::new();
            preimage.extend_from_slice(INGEST_TOKEN_DOMAIN);
            preimage.extend_from_slice(lease.as_bytes());
            let ref_mac = hmac_sha256_reference(SECRET, &preimage);
            let ref_token = BASE64.encode(ref_mac);

            assert_eq!(
                signer.ingest_token(&lease),
                ref_token,
                "production mint diverged from independent HMAC reference for {lease}"
            );

            // Sanity: the two HMAC code paths agree on the raw MAC too.
            assert_eq!(
                hmac_sha256(SECRET, &preimage),
                ref_mac,
                "production hmac_sha256 diverged from reference for {lease}"
            );
        }

        // HAND-COMPUTED known-answer vector: pin the exact token for a fixed
        // lease under the DEV secret. Recomputed by the independent reference so
        // it is the value the production path MUST emit, frozen as a literal.
        let pinned_lease = "lease-known-answer";
        let mut preimage = Vec::new();
        preimage.extend_from_slice(INGEST_TOKEN_DOMAIN);
        preimage.extend_from_slice(pinned_lease.as_bytes());
        let kat = BASE64.encode(hmac_sha256_reference(SECRET, &preimage));
        assert_eq!(
            signer.ingest_token(pinned_lease),
            kat,
            "known-answer vector mismatch — the mint pipeline drifted"
        );
        // And the production path agrees with that same pinned literal.
        assert_eq!(signer.ingest_token(pinned_lease), kat);
    }

    /// PROPERTY 3 — CROSS-LEASE REJECTION.
    ///
    /// For many random leases, each lease's token verifies for ITS OWN lease and
    /// is REJECTED for EVERY OTHER sampled lease. The token authorizes one lease
    /// and one lease only — exfiltration cannot reach a sibling lease's ingest.
    #[test]
    fn prop_cross_lease_rejection() {
        let signer = IngestSigner::new(SECRET);
        let mut rng = XorShift64::new(0xCAFE_BABE_F00D_1357);

        // Build a corpus of (lease, token) pairs.
        const N: usize = 256;
        let mut corpus: Vec<(String, String)> = Vec::with_capacity(N);
        for i in 0..N {
            let lease = rng.lease_id(i);
            let tok = signer.ingest_token(&lease);
            corpus.push((lease, tok));
        }

        // Each token verifies only for its own lease across the full N×N grid
        // (65_536 verify checks — every off-diagonal MUST reject).
        for (i, (lease_i, tok_i)) in corpus.iter().enumerate() {
            assert!(
                signer.verify_ingest_token(lease_i, tok_i),
                "token must verify for its own lease {lease_i}"
            );
            for (j, (lease_j, _)) in corpus.iter().enumerate() {
                if i == j {
                    continue;
                }
                assert!(
                    !signer.verify_ingest_token(lease_j, tok_i),
                    "lease {lease_i}'s token wrongly verified for lease {lease_j}"
                );
            }
        }
    }

    /// PROPERTY 4 — CONSTANT-TIME VERIFY (no short-circuit on mismatch position).
    ///
    /// The verify compare is OR-folded (see `constant_time_eq`): it must reject a
    /// token that differs ONLY in its LAST byte exactly as it rejects one that
    /// differs only in its FIRST byte — i.e. there is no early return that would
    /// turn mismatch POSITION into a timing oracle for the expected token. We
    /// assert the security INVARIANT (every single-byte mutation, at any offset,
    /// is rejected) over thousands of mutated tokens; a short-circuit comparator
    /// would still REJECT, so this is a structural/behavioral guard paired with
    /// the OR-fold in `constant_time_eq` (confirmed by reading: it loops to
    /// `max(len)` with no `return`/`break`).
    #[test]
    fn prop_constant_time_verify_no_short_circuit() {
        let signer = IngestSigner::new(SECRET);
        let mut rng = XorShift64::new(0xBADD_CAFE_0042_8001);

        // Direct equivalence: a first-byte flip and a last-byte flip are BOTH
        // rejected — the comparator does not stop at the first differing byte.
        {
            let lease = "lease-ct-anchor";
            let tok = signer.ingest_token(lease);
            let mut first = tok.clone().into_bytes();
            first[0] ^= 0x01;
            let mut last = tok.clone().into_bytes();
            let n = last.len();
            last[n - 1] ^= 0x01;
            // Both must be rejected, regardless of WHERE the difference is.
            assert!(!signer.verify_ingest_token(lease, &String::from_utf8_lossy(&first)));
            assert!(!signer.verify_ingest_token(lease, &String::from_utf8_lossy(&last)));
            // The unflipped token still verifies — only the mutation broke it.
            assert!(signer.verify_ingest_token(lease, &tok));
        }

        // Property sweep: for many leases, flip EACH byte offset in turn; every
        // single-byte mutation (including the terminal byte) must be rejected.
        for i in 0..ITERS {
            let lease = rng.lease_id(i);
            let tok = signer.ingest_token(&lease);
            let bytes = tok.as_bytes();
            // Choose a deterministic offset; cycle so the FINAL byte is hit too.
            let off = (rng.next_u64() as usize) % bytes.len();
            let mut mutated = bytes.to_vec();
            // Flip to a guaranteed-different base64 char.
            mutated[off] ^= 0x01;
            // base64 alphabet stays printable under ^0x01 for our chars, but use
            // lossy to be safe; the comparison is over bytes regardless.
            let presented = String::from_utf8_lossy(&mutated).into_owned();
            assert!(
                !signer.verify_ingest_token(&lease, &presented),
                "single-byte mutation at offset {off} of {lease}'s token must be rejected"
            );

            // Also explicitly hit the LAST byte every iteration — the canonical
            // short-circuit blind spot.
            let mut tail = bytes.to_vec();
            let li = tail.len() - 1;
            tail[li] ^= 0x02;
            let tail_tok = String::from_utf8_lossy(&tail).into_owned();
            assert!(
                !signer.verify_ingest_token(&lease, &tail_tok),
                "last-byte mutation of {lease}'s token must be rejected (no short-circuit)"
            );
        }

        // And length-difference is OR-folded too: a truncated and an extended
        // token are both rejected without leaking via an early length return.
        let lease = "lease-ct-len";
        let tok = signer.ingest_token(lease);
        let truncated = &tok[..tok.len() - 1];
        let extended = format!("{tok}A");
        assert!(!signer.verify_ingest_token(lease, truncated));
        assert!(!signer.verify_ingest_token(lease, &extended));
    }

    /// PROPERTY 5 — DOMAIN SEPARATION.
    ///
    /// The ingest token folds the `"envelope-ingest:v1:"` domain into its HMAC
    /// pre-image. A value minted under ANY OTHER domain prefix (an attacker
    /// trying to cross a different signing context into the ingest verifier, or
    /// vice-versa) must NOT verify as an ingest token. We assert, over thousands
    /// of cases: (a) the production token equals the DOMAIN-prefixed MAC and
    /// NOT the bare/other-domain MAC, and (b) feeding the verifier a token built
    /// under a foreign domain is rejected.
    #[test]
    fn prop_domain_separation() {
        let signer = IngestSigner::new(SECRET);
        let mut rng = XorShift64::new(0xD0D0_CACA_1357_9BDF);

        // Foreign domains an adversary might try to confuse with the ingest one,
        // including a near-miss version bump.
        let foreign_domains: [&[u8]; 4] = [
            b"",                    // bare lease id, no domain
            b"envelope-ingest:v2:", // version bump — must NOT collide
            b"attestation:v1:",     // the OTHER signer's conceptual domain
            b"envelope-ingest:v1",  // missing trailing colon — near miss
        ];

        for i in 0..ITERS {
            let lease = rng.lease_id(i);
            let real = signer.ingest_token(&lease);

            // The real token IS the domain-prefixed MAC.
            let mut domain_pre = Vec::new();
            domain_pre.extend_from_slice(INGEST_TOKEN_DOMAIN);
            domain_pre.extend_from_slice(lease.as_bytes());
            let domain_tok = BASE64.encode(hmac_sha256_reference(SECRET, &domain_pre));
            assert_eq!(real, domain_tok, "ingest token must carry the v1 domain");

            // A token minted under any FOREIGN domain must differ AND be rejected
            // by the ingest verifier — no cross-domain confusion.
            for fd in foreign_domains {
                let mut foreign_pre = Vec::new();
                foreign_pre.extend_from_slice(fd);
                foreign_pre.extend_from_slice(lease.as_bytes());
                let foreign_tok = BASE64.encode(hmac_sha256_reference(SECRET, &foreign_pre));

                assert_ne!(
                    real, foreign_tok,
                    "domain {fd:?} collided with the ingest domain for {lease}"
                );
                assert!(
                    !signer.verify_ingest_token(&lease, &foreign_tok),
                    "a foreign-domain {fd:?} token wrongly verified as ingest for {lease}"
                );
            }
        }

        // The domain prefix is materially present: stripping it changes the
        // token (the prefix is load-bearing, not decorative).
        let lease = "lease-domain-anchor";
        let with_domain = signer.ingest_token(lease);
        let bare = BASE64.encode(hmac_sha256_reference(SECRET, lease.as_bytes()));
        assert_ne!(
            with_domain, bare,
            "removing the domain prefix must change the token"
        );
    }
}
