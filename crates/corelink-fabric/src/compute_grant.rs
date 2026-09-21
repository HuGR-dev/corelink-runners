//! Compute-grant-only identifier validation.
//!
//! This intentionally does not alter [`crate::TenantId`], whose broader
//! lifecycle domain accepts established tenant keys. Compute grants carry the
//! server's cross-runtime UUID identity contract instead.

/// Return whether `value` is the raw canonical tenant UUID accepted in a
/// compute grant: lowercase, hyphenated RFC 4122 UUID v4 or v7.
pub fn is_canonical_tenant_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes.get(index) == Some(&b'-'))
        && bytes
            .get(14)
            .is_some_and(|byte| matches!(byte, b'4' | b'7'))
        && bytes
            .get(19)
            .is_some_and(|byte| matches!(byte, b'8' | b'9' | b'a' | b'b'))
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || byte.is_ascii_digit()
                || (b'a'..=b'f').contains(byte)
        })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::is_canonical_tenant_id;

    #[derive(Deserialize)]
    struct Vectors {
        tenant_id: TenantIdVectors,
    }

    #[derive(Deserialize)]
    struct TenantIdVectors {
        accept: Vec<String>,
        reject: Vec<String>,
    }

    #[test]
    fn accepts_and_rejects_every_contract_vector() {
        let vectors: Vectors = serde_json::from_str(include_str!(
            "../../../docs/contracts/compute-grant-identifiers-v1.json"
        ))
        .unwrap();
        assert!(
            vectors
                .tenant_id
                .accept
                .iter()
                .all(|value| is_canonical_tenant_id(value))
        );
        assert!(
            vectors
                .tenant_id
                .reject
                .iter()
                .all(|value| !is_canonical_tenant_id(value))
        );
    }
}
