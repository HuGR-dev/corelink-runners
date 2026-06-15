//! `corelink smoke` — automated live-deploy verification.
//!
//! Replaces the manual `docs/deploy/post-redeploy-smoke-checklist.md` curl
//! dance with one command. The DEFAULT checks are cheap and side-effect-free
//! (they hit health, the attestation key, and the two fail-closed gates —
//! nothing provisions a box). `--full` additionally does a real acquire→cancel,
//! which provisions and tears down a real box on the cloud backend.

use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use corelink_fabric_api::dto::{AcquireRequest, AcquireResponse, AttestationKeyResponse};

use crate::client::Client;

/// A known-good content-pinned reference (`name@sha256:<64hex>`) — used for the
/// bad-PAT gate (auth fails before the image is ever pulled) and as the
/// `--full` acquire default.
pub const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
/// A deliberately UNPINNED reference (a tag, no digest) — must be rejected 400.
const UNPINNED_IMAGE: &str = "alpine:latest";

struct Tally {
    pass: usize,
    fail: usize,
}

impl Tally {
    fn new() -> Self {
        Self { pass: 0, fail: 0 }
    }
    fn check(&mut self, name: &str, ok: bool, detail: impl AsRef<str>) {
        println!(
            "  {} {name}: {}",
            if ok { "✓" } else { "✗" },
            detail.as_ref()
        );
        if ok {
            self.pass += 1;
        } else {
            self.fail += 1;
        }
    }
}

fn acquire_body(image: &str) -> String {
    serde_json::to_string(&AcquireRequest {
        image_digest: image.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    })
    .expect("AcquireRequest serializes")
}

/// Run the smoke against `base` with tenant `pat`. Returns `Ok(true)` iff every
/// check passed. `full` enables the provisioning acquire→cancel; `image` is the
/// pinned ref used for the `--full` acquire.
pub fn run(base: &str, pat: &str, full: bool, image: &str) -> Result<bool> {
    let c = Client::new(base, pat);
    let mut t = Tally::new();
    println!("corelink smoke → {base}\n");

    // 1. Liveness (no auth).
    match c.get("/v1/health", false) {
        Ok(r) => t.check(
            "health",
            r.status == 200,
            format!("GET /v1/health → {}", r.status),
        ),
        Err(e) => t.check("health", false, format!("transport error: {e}")),
    }

    // 2. Published attestation key (Bearer PAT — the key endpoint is behind the
    //    tenant-auth layer; any authenticated tenant gets the per-region key).
    //    Must be a 32-byte ed25519 pubkey.
    match c.get("/v1/attestation/key", true) {
        Ok(r) if r.status == 200 => match r.json::<AttestationKeyResponse>() {
            Ok(k) => {
                let ok = B64
                    .decode(k.ed25519_pubkey_b64.trim())
                    .map(|b| b.len() == 32)
                    .unwrap_or(false);
                t.check(
                    "attestation-key",
                    ok,
                    format!("ed25519 pubkey {}", k.ed25519_pubkey_b64),
                );
            }
            Err(e) => t.check("attestation-key", false, format!("bad shape: {e}")),
        },
        Ok(r) => t.check(
            "attestation-key",
            false,
            format!("GET /v1/attestation/key → {}", r.status),
        ),
        Err(e) => t.check("attestation-key", false, format!("transport error: {e}")),
    }

    // 3. Fail-closed: an UNPINNED image (valid PAT) is rejected 400 before any
    //    box contact (X4 supply-chain floor).
    match c.post_json("/v1/leases", &acquire_body(UNPINNED_IMAGE), Some(pat)) {
        Ok(r) => t.check(
            "fail-closed: unpinned→400",
            r.status == 400,
            format!("unpinned image → {} (want 400)", r.status),
        ),
        Err(e) => t.check(
            "fail-closed: unpinned→400",
            false,
            format!("transport error: {e}"),
        ),
    }

    // 4. Fail-closed: a bad PAT (well-formed pinned image) is rejected 401 —
    //    auth fails before the body matters.
    match c.post_json(
        "/v1/leases",
        &acquire_body(PINNED_IMAGE),
        Some("definitely-not-a-registered-pat"),
    ) {
        Ok(r) => t.check(
            "fail-closed: bad-PAT→401",
            r.status == 401,
            format!("bad PAT → {} (want 401)", r.status),
        ),
        Err(e) => t.check(
            "fail-closed: bad-PAT→401",
            false,
            format!("transport error: {e}"),
        ),
    }

    // 5. (--full) Real acquire→cancel — provisions and tears down a real box.
    if full {
        match c.post_json("/v1/leases", &acquire_body(image), Some(pat)) {
            Ok(r) if r.status == 200 => match r.json::<AcquireResponse>() {
                Ok(a) => {
                    let lease_id = a.lease.lease_id.clone();
                    t.check(
                        "acquire",
                        true,
                        format!("lease {lease_id} (state {:?})", a.lease.state),
                    );
                    let path = format!("/v1/leases/{lease_id}/cancel");
                    match c.post_json(&path, "", Some(pat)) {
                        Ok(rc) => t.check(
                            "cancel",
                            rc.status == 200,
                            format!("cancel → {} (want 200)", rc.status),
                        ),
                        Err(e) => t.check("cancel", false, format!("transport error: {e}")),
                    }
                }
                Err(e) => t.check("acquire", false, format!("bad shape: {e}")),
            },
            Ok(r) => t.check(
                "acquire",
                false,
                format!("acquire → {} (want 200); body: {}", r.status, r.body),
            ),
            Err(e) => t.check("acquire", false, format!("transport error: {e}")),
        }
    } else {
        println!("  · acquire→cancel skipped (pass --full to provision a real box)");
    }

    println!("\n{} passed, {} failed", t.pass, t.fail);
    Ok(t.fail == 0)
}
