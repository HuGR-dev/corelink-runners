//! Verification of the short-lived, attempt-bound external compute grant.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use corelink_fabric::compute_budget::{ExternalComputeReservation, ExternalWorkloadKind};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const MAX_TOKEN_BYTES: usize = 8 * 1024;
const MAX_TTL_MS: u64 = 90_000;
const MAX_WALL_MS: u64 = 28_800_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GrantPayload {
    pub v: u8,
    pub key_id: String,
    pub tenant_id: String,
    pub workload_kind: ExternalWorkloadKind,
    pub workload_id: String,
    pub reservation_id: String,
    pub period_key: u32,
    pub ceiling_vcpu_ms: String,
    pub vcpu_count: u32,
    pub maximum_wall_ms: u64,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VerifiedGrant {
    pub payload: GrantPayload,
    pub grant_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrantError {
    Malformed,
    InvalidSignature,
    Expired,
}

pub(crate) struct GrantVerifier {
    public_keys: HashMap<String, Vec<u8>>,
}

impl GrantVerifier {
    pub(crate) fn new(public_keys: HashMap<String, Vec<u8>>) -> Self {
        Self { public_keys }
    }

    pub(crate) fn verify(
        &self,
        token: &str,
        now_ms: u64,
        allow_expired: bool,
    ) -> Result<VerifiedGrant, GrantError> {
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(GrantError::Malformed);
        }
        let mut parts = token.split('.');
        let payload_part = parts.next().ok_or(GrantError::Malformed)?;
        let signature_part = parts.next().ok_or(GrantError::Malformed)?;
        if parts.next().is_some() || payload_part.is_empty() || signature_part.is_empty() {
            return Err(GrantError::Malformed);
        }
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_part)
            .map_err(|_| GrantError::Malformed)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature_part)
            .map_err(|_| GrantError::Malformed)?;
        if signature.len() != 64 {
            return Err(GrantError::Malformed);
        }
        let payload: GrantPayload =
            serde_json::from_slice(&payload_bytes).map_err(|_| GrantError::Malformed)?;
        validate_payload(&payload, now_ms, allow_expired)?;
        let key = self
            .public_keys
            .get(&payload.key_id)
            .filter(|key| key.len() == 32)
            .ok_or(GrantError::InvalidSignature)?;
        UnparsedPublicKey::new(&ED25519, key)
            .verify(&payload_bytes, &signature)
            .map_err(|_| GrantError::InvalidSignature)?;

        let mut digest = Sha256::new();
        digest.update(token.as_bytes());
        Ok(VerifiedGrant {
            payload,
            grant_digest: hex_lower(&digest.finalize()),
        })
    }
}

impl VerifiedGrant {
    pub(crate) fn reservation(&self) -> ExternalComputeReservation {
        ExternalComputeReservation {
            reservation_id: self.payload.reservation_id.clone(),
            tenant_id: self.payload.tenant_id.clone(),
            workload_kind: self.payload.workload_kind.clone(),
            workload_id: self.payload.workload_id.clone(),
            period_key: self.payload.period_key,
            ceiling_vcpu_ms: self
                .payload
                .ceiling_vcpu_ms
                .parse()
                .expect("validated decimal ceiling"),
            vcpu_count: self.payload.vcpu_count,
            maximum_wall_ms: self.payload.maximum_wall_ms,
            grant_expires_at_ms: self.payload.expires_at_ms,
            grant_digest: self.grant_digest.clone(),
        }
    }
}

fn validate_payload(
    payload: &GrantPayload,
    now_ms: u64,
    allow_expired: bool,
) -> Result<(), GrantError> {
    if payload.v != 1
        || payload.key_id.is_empty()
        || payload.key_id.len() > 64
        || !payload.key_id.is_ascii()
        || !payload
            .key_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        || Uuid::parse_str(&payload.tenant_id)
            .map(|id| id.is_nil())
            .unwrap_or(true)
        || Uuid::parse_str(&payload.reservation_id)
            .map(|id| id.is_nil())
            .unwrap_or(true)
        || payload.workload_id.is_empty()
        || payload.workload_id.len() > 256
        || !payload.workload_id.is_ascii()
        || !payload
            .workload_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b":_./-".contains(&c))
        || !valid_period(payload.period_key)
        || !valid_decimal(&payload.ceiling_vcpu_ms)
        || payload
            .ceiling_vcpu_ms
            .parse::<u64>()
            .ok()
            .filter(|v| *v > 0 && *v <= i64::MAX as u64)
            .is_none()
        || !(1..=16).contains(&payload.vcpu_count)
        || !(1..=MAX_WALL_MS).contains(&payload.maximum_wall_ms)
        || payload.issued_at_ms > now_ms
        || payload.expires_at_ms <= payload.issued_at_ms
        || payload.expires_at_ms - payload.issued_at_ms > MAX_TTL_MS
        || (!allow_expired && payload.expires_at_ms <= now_ms)
        || !wall_stays_in_period(
            payload.period_key,
            payload.expires_at_ms,
            payload.maximum_wall_ms,
        )
    {
        return Err(if !allow_expired && payload.expires_at_ms <= now_ms {
            GrantError::Expired
        } else {
            GrantError::Malformed
        });
    }
    Ok(())
}

fn valid_decimal(value: &str) -> bool {
    value.len() <= 19
        && !value.is_empty()
        && (value == "0"
            || (value.as_bytes()[0] != b'0' && value.bytes().all(|c| c.is_ascii_digit())))
}

fn valid_period(period: u32) -> bool {
    let month = period % 100;
    (197001..=999912).contains(&period) && (1..=12).contains(&month)
}

fn wall_stays_in_period(period: u32, expires_ms: u64, wall_ms: u64) -> bool {
    let year = (period / 100) as i64;
    let month = (period % 100) as i64;
    let Some(start_days) = days_from_civil(year, month, 1) else {
        return false;
    };
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let Some(next_days) = days_from_civil(next_year, next_month, 1) else {
        return false;
    };
    let start = (start_days as u64).saturating_mul(86_400_000);
    let end = (next_days as u64).saturating_mul(86_400_000);
    expires_ms >= start && expires_ms.checked_add(wall_ms).is_some_and(|v| v <= end)
}

// Howard Hinnant's civil-date conversion, with a fixed Unix epoch.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use serde_json::json;

    fn signed_payload(
        extra: Option<(&str, serde_json::Value)>,
    ) -> (String, HashMap<String, Vec<u8>>) {
        let now = now_for_test();
        let period = 202609;
        let mut payload = json!({
            "v": 1, "key_id": "issuer-1", "tenant_id": "11111111-1111-4111-8111-111111111111",
            "workload_kind": "devenv", "workload_id": "job/1", "reservation_id": "22222222-2222-4222-8222-222222222222",
            "period_key": period, "ceiling_vcpu_ms": "1000", "vcpu_count": 1,
            "maximum_wall_ms": 1000, "issued_at_ms": now - 1000, "expires_at_ms": now + 5000
        });
        if let Some((key, value)) = extra {
            payload.as_object_mut().unwrap().insert(key.into(), value);
        }
        sign_bytes(&serde_json::to_vec(&payload).unwrap())
    }

    fn sign_bytes(bytes: &[u8]) -> (String, HashMap<String, Vec<u8>>) {
        let keypair = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(keypair.as_ref()).unwrap();
        let token = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(bytes),
            URL_SAFE_NO_PAD.encode(pair.sign(bytes).as_ref())
        );
        let mut keys = HashMap::new();
        keys.insert("issuer-1".into(), pair.public_key().as_ref().to_vec());
        (token, keys)
    }

    #[test]
    fn verifies_signature_and_digest() {
        let (token, keys) = signed_payload(None);
        let grant = GrantVerifier::new(keys)
            .verify(&token, now_for_test(), false)
            .unwrap();
        assert_eq!(
            grant.grant_digest,
            hex_lower(&Sha256::digest(token.as_bytes()))
        );
    }

    #[test]
    fn rejects_tamper_duplicate_and_unknown_fields() {
        let (token, keys) = signed_payload(None);
        let mut tampered = token.into_bytes();
        tampered[4] = if tampered[4] == b'A' { b'B' } else { b'A' };
        assert!(matches!(
            GrantVerifier::new(keys.clone()).verify(
                std::str::from_utf8(&tampered).unwrap(),
                now_for_test(),
                false
            ),
            Err(GrantError::InvalidSignature | GrantError::Malformed)
        ));
        let (unknown, _) = signed_payload(Some(("unknown", json!(true))));
        assert!(matches!(
            GrantVerifier::new(keys).verify(&unknown, now_for_test(), false),
            Err(GrantError::Malformed)
        ));
        let (base, _) = signed_payload(None);
        let payload = URL_SAFE_NO_PAD
            .decode(base.split('.').next().unwrap())
            .unwrap();
        let mut duplicate = br#"{"v":1,"#.to_vec();
        duplicate.extend_from_slice(&payload[1..]);
        let (duplicate, keys) = sign_bytes(&duplicate);
        assert!(matches!(
            GrantVerifier::new(keys).verify(&duplicate, now_for_test(), false),
            Err(GrantError::Malformed)
        ));
    }

    #[test]
    fn expired_is_rejected_for_admission_but_cleanup_can_authenticate() {
        let (token, keys) = signed_payload(None);
        let verifier = GrantVerifier::new(keys);
        assert!(matches!(
            verifier.verify(&token, u64::MAX, false),
            Err(GrantError::Expired)
        ));
        assert!(verifier.verify(&token, u64::MAX, true).is_ok());
    }

    fn now_for_test() -> u64 {
        1_788_652_800_000
    }
}

#[cfg(test)]
#[path = "compute_grant_cross_language.rs"]
mod cross_language;
