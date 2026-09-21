//! Raw Ed25519 verification for compute-grant payloads.

use ring::signature::{ED25519, UnparsedPublicKey};

use crate::compute_grant::GrantError;

/// Verify the exact decoded payload bytes before callers evaluate its fields.
pub(crate) fn verify_raw_payload(
    public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), GrantError> {
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(payload, signature)
        .map_err(|_| GrantError::InvalidSignature)
}
