//! Cloudflare Access service-token headers for outbound calls to
//! corelink-server's `/internal/v1/*` control plane (Inc-3 lockdown, 2026-08-19).
//!
//! corelink-server put `/internal/v1/*` behind a Cloudflare Access self-hosted
//! app (Service-Auth): a request that reaches the public edge without a valid
//! service token is rejected **403 before the Worker**. This fabric is an
//! EXTERNAL caller (it hits `corelink-api.humangr.com/internal/v1/{runner/*,
//! auth/introspect,billing/usage}`), so every such request must now also carry
//! the `CF-Access-Client-Id` / `CF-Access-Client-Secret` pair for the
//! `corelink-internal-runners` service token — IN ADDITION to the existing
//! `x-corelink-internal-auth` app-layer gate (defense-in-depth: network token
//! AND app secret).
//!
//! **Backwards-compatible by design:** the headers are emitted ONLY when BOTH
//! env vars are set and non-empty. Until the operator binds them (and until the
//! CF Access app is created), this is a no-op — the fabric behaves exactly as
//! before. This lets the fabric deploy FIRST (carrying the headers harmlessly),
//! then the CF Access app be created as the cutover, with no strand window.

/// Env var holding the CF Access service-token **Client Id**.
const CF_ACCESS_CLIENT_ID_ENV: &str = "CORELINK_CF_ACCESS_CLIENT_ID";
/// Env var holding the CF Access service-token **Client Secret**.
const CF_ACCESS_CLIENT_SECRET_ENV: &str = "CORELINK_CF_ACCESS_CLIENT_SECRET";

/// The CF Access service-token headers to attach to every outbound
/// `/internal/v1/*` request, or an empty vec when unconfigured (no-op).
///
/// Returns the two `(name, value)` header pairs iff BOTH env vars are set and
/// non-empty; otherwise an empty vec so the caller adds nothing (the pre-Inc-3
/// behaviour, and the fail-open path if the operator has not yet bound the
/// token — CF Access itself is the enforcing layer, not this helper).
#[must_use]
pub fn cf_access_headers() -> Vec<(&'static str, String)> {
    headers_from(
        std::env::var(CF_ACCESS_CLIENT_ID_ENV).ok(),
        std::env::var(CF_ACCESS_CLIENT_SECRET_ENV).ok(),
    )
}

/// Pure core of [`cf_access_headers`] — the two header pairs iff both inputs are
/// present and non-empty, else empty. Split out so the policy is unit-testable
/// without mutating the process environment (unsafe in edition 2024).
#[must_use]
fn headers_from(id: Option<String>, secret: Option<String>) -> Vec<(&'static str, String)> {
    match (id, secret) {
        (Some(id), Some(secret)) if !id.is_empty() && !secret.is_empty() => vec![
            ("CF-Access-Client-Id", id),
            ("CF-Access-Client-Secret", secret),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_present_only_when_both_values_set() {
        assert!(headers_from(None, None).is_empty(), "unset ⇒ no headers");
        assert!(
            headers_from(Some("id.access".to_owned()), None).is_empty(),
            "id-only ⇒ no headers (both required)"
        );
        assert!(
            headers_from(None, Some("shhh".to_owned())).is_empty(),
            "secret-only ⇒ no headers"
        );
        assert!(
            headers_from(Some(String::new()), Some("shhh".to_owned())).is_empty(),
            "empty id ⇒ no headers"
        );

        let h = headers_from(Some("id.access".to_owned()), Some("shhh".to_owned()));
        assert_eq!(h.len(), 2);
        assert_eq!(h[0], ("CF-Access-Client-Id", "id.access".to_owned()));
        assert_eq!(h[1], ("CF-Access-Client-Secret", "shhh".to_owned()));
    }
}
