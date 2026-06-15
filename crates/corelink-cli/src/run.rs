//! `corelink run` — the customer adoption primitive.
//!
//! Executes a full job lifecycle on the fabric:
//!   acquire → exec → verify attestation → close (best-effort)
//!
//! Design axioms (CONTRACT-RUN v1):
//! - An UNPINNED image (`name:tag`, no `@sha256:`) is rejected with exit 2
//!   BEFORE any box contact (fail-closed; mirrors the X4 supply-chain floor).
//! - On ANY error after a successful acquire the lease is best-effort
//!   cancelled so NO lease ever leaks.
//! - Exit code semantics: 0 = exec ran AND (verified OR --no-verify) AND check
//!   exit == 0; 1 = exec ran, attestation verified (or --no-verify), check
//!   exit != 0; 2 = attestation failed / wire/protocol/auth error / unpinned
//!   image / lease could not be acquired.
//! - `--json`: EXACTLY one JSON object on stdout; step lines go to stderr.
//! - `--no-verify`: skip the attestation step (exit 2 on verify failures only
//!   occurs when verification is requested AND fails).

use anyhow::{Context, Result, bail};
use corelink_fabric_api::dto::{
    AcquireRequest, AcquireResponse, AttestationKeyResponse, ExecResponse,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{binding, client::Client, smoke::PINNED_IMAGE};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Returns `true` iff `image` is sha256-pinned (contains `@sha256:`).
/// An unpinned image must be rejected fail-closed BEFORE any box contact.
fn is_pinned(image: &str) -> bool {
    image.contains("@sha256:")
}

/// Build the acquire request body (mirrors smoke.rs).
fn acquire_body(image: &str, expiry_ms: u64) -> String {
    serde_json::to_string(&AcquireRequest {
        image_digest: image.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms,
    })
    .expect("AcquireRequest serializes")
}

/// Build the exec request body for a shell check.
/// `command` is the shell fragment the customer supplied via `--check`;
/// `check_id` is the mnemonic. The `def_digest` is a REAL SHA-256 content
/// address over the canonical check definition — stable + honest (not a
/// provenance claim about materialized inputs, but a true digest of the def).
fn exec_body(command: &str, check_id: &str) -> String {
    let toolchain_ref = "cli-run-v1";
    // def_digest = sha256 over the canonical check-def string. A real content
    // hash: identical (command, check_id, toolchain) → identical digest, so the
    // one-shot runner is deterministically addressable. The CLI has no workspace
    // Merkle root, so tree_hash is the zero axis (a command runner, not a
    // memoized check pipeline).
    let def_digest = sha256_hex(&format!(
        "cli-run-v1\0{check_id}\0{command}\0{toolchain_ref}"
    ));
    serde_json::to_string(&serde_json::json!({
        "check_def": {
            "def_digest": def_digest,
            "command": command,
            "inputs": [],
            "toolchain_ref": toolchain_ref,
            "env_manifest": format!("sha256:{}", "0".repeat(64)),
            "glob_set": [],
        },
        "tree_hash": "0".repeat(64),
    }))
    .expect("ExecRequest serializes")
}

/// Real SHA-256, lowercase hex. Used for the `def_digest` content address.
fn sha256_hex(s: &str) -> String {
    let digest = Sha256::digest(s.as_bytes());
    digest.iter().fold(String::with_capacity(64), |mut acc, b| {
        acc.push_str(&format!("{b:02x}"));
        acc
    })
}

// ── step printers ─────────────────────────────────────────────────────────────

/// Print a progress line. In `--json` mode send to stderr; otherwise stdout.
fn step(json_mode: bool, line: &str) {
    if json_mode {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

// ── the command ───────────────────────────────────────────────────────────────

/// Outcome of a completed `corelink run`.
struct RunOutcome {
    lease_id: String,
    exec_exit: i32,
    verified: bool,
    check_id: String,
    /// Raw `ExecResponse` body (for artifact/ref extraction in --json mode).
    exec_body: String,
}

/// Entry point called from `main.rs`. Returns the process exit code (0, 1, 2).
pub fn cmd_run(args: &[String]) -> Result<i32> {
    // ── arg parsing ───────────────────────────────────────────────────────────
    let base = flag(args, "--url")
        .or_else(|| std::env::var("CORELINK_URL").ok())
        .context("`run` needs --url <fabric-url> (or CORELINK_URL)")?;
    let pat = std::env::var("CORELINK_PAT")
        .context("`run` needs the CORELINK_PAT env var (the tenant PAT)")?;
    let check_cmd = flag(args, "--check").context("`run` needs --check '<shell command>'")?;
    let check_id = flag(args, "--check-id").unwrap_or_else(|| "run".to_string());
    let image = flag(args, "--image").unwrap_or_else(|| PINNED_IMAGE.to_string());
    let json_mode = has_flag(args, "--json");
    let no_verify = has_flag(args, "--no-verify");

    // ── fail-closed: unpinned image → exit 2 BEFORE any box contact ──────────
    if !is_pinned(&image) {
        eprintln!(
            "error: image {image:?} is not sha256-pinned (must contain `@sha256:`) — \
             refusing to acquire a lease with an unpinned image (X4 supply-chain floor)"
        );
        return Ok(2);
    }
    let c = Client::new(&base, &pat);

    // ── step 1: acquire ───────────────────────────────────────────────────────
    let acq_resp = c
        .post_json("/v1/leases", &acquire_body(&image, 120_000), Some(&pat))
        .context("acquire: transport error")?;
    if acq_resp.status != 200 {
        eprintln!(
            "error: acquire → {} (want 200); body: {}",
            acq_resp.status, acq_resp.body
        );
        return Ok(2);
    }
    let acq: AcquireResponse = acq_resp
        .json()
        .context("acquire: response is not the expected shape")?;
    let lease_id = acq.lease.lease_id.clone();
    step(json_mode, &format!("acquire ✓  lease={lease_id}"));

    // From here: any error → best-effort cancel so no lease leaks.
    match run_after_acquire(
        &c, &pat, &lease_id, &check_cmd, &check_id, no_verify, json_mode,
    ) {
        Ok(outcome) => {
            // ── emit output ───────────────────────────────────────────────────
            if json_mode {
                print_json_output(&outcome, &c, &pat)?;
            } else {
                let verified_str = if no_verify {
                    "(--no-verify, skipped)"
                } else if outcome.verified {
                    "✓"
                } else {
                    "✗ FAILED"
                };
                println!("verify {verified_str}");
                let verdict = if outcome.exec_exit == 0 {
                    "PASS"
                } else {
                    "FAIL"
                };
                println!(
                    "\n{verdict}  lease={lease_id}  check_id={}  exit={}  verified={}",
                    outcome.check_id,
                    outcome.exec_exit,
                    if no_verify {
                        "skipped"
                    } else if outcome.verified {
                        "true"
                    } else {
                        "false"
                    }
                );
            }

            // Decide exit code.
            if !no_verify && !outcome.verified {
                Ok(2)
            } else if outcome.exec_exit != 0 {
                Ok(1)
            } else {
                Ok(0)
            }
        }
        Err(e) => {
            // Best-effort cancel.
            best_effort_cancel(&c, &pat, &lease_id, json_mode);
            eprintln!("error: {e:#}");
            Ok(2)
        }
    }
}

/// All steps after a successful acquire. On error the caller best-effort
/// cancels.
fn run_after_acquire(
    c: &Client,
    pat: &str,
    lease_id: &str,
    check_cmd: &str,
    check_id: &str,
    no_verify: bool,
    json_mode: bool,
) -> Result<RunOutcome> {
    // ── step 2: exec ──────────────────────────────────────────────────────────
    let exec_path = format!("/v1/leases/{lease_id}/exec");
    let exec_resp = c
        .post_json(&exec_path, &exec_body(check_cmd, check_id), Some(pat))
        .context("exec: transport error")?;
    if exec_resp.status != 200 {
        bail!(
            "exec → {} (want 200); body: {}",
            exec_resp.status,
            exec_resp.body
        );
    }
    let exec: ExecResponse = exec_resp
        .json()
        .context("exec: response is not the expected shape")?;
    let exec_exit = exec.result.exit;
    step(json_mode, &format!("exec  ✓  exit={exec_exit}"));

    // ── step 3: verify ────────────────────────────────────────────────────────
    // `verified` means CRYPTOGRAPHICALLY VERIFIED — it is `false` when skipped
    // (`--no-verify`) so the --json field never overclaims. The exit-code logic
    // in `cmd_run` guards on `no_verify` separately, so a skip is still exit 0.
    let verified = if no_verify {
        step(
            json_mode,
            "verify    (--no-verify, skipped — emitted as verified=false)",
        );
        false // not verified: verification did not happen
    } else {
        // Fetch the fabric's published key (Bearer-PAT authenticated).
        let key_resp = c
            .get("/v1/attestation/key", true)
            .context("verify: GET /v1/attestation/key transport error")?;
        if key_resp.status != 200 {
            bail!(
                "verify: GET /v1/attestation/key → {} (want 200)",
                key_resp.status
            );
        }
        let key: AttestationKeyResponse = key_resp
            .json()
            .context("verify: key response is not the expected shape")?;
        let pubkey = key.ed25519_pubkey_b64;

        let outcome = binding::verify_response_json(&exec_resp.body, &pubkey)
            .context("verify: malformed sig or key")?;
        if outcome.verified {
            step(json_mode, "verify ✓  result_binding_sig_v2 authentic");
        } else {
            step(
                json_mode,
                "verify ✗  result_binding_sig_v2 FAILED — do NOT trust this verdict",
            );
        }
        outcome.verified
    };

    // ── step 4: close (best-effort) ───────────────────────────────────────────
    // close drives the §13 ack-window; we fire-and-forget (best-effort, short
    // timeout acceptable — contract says "best-effort close").
    let close_path = format!("/v1/leases/{lease_id}/close");
    let close_body = serde_json::json!({
        "status": if exec_exit == 0 { "succeeded" } else { "failed" },
        "check_result": null,
    })
    .to_string();
    match c.post_json(&close_path, &close_body, Some(pat)) {
        Ok(r) if r.status == 200 => step(json_mode, "close  ✓"),
        Ok(r) => step(
            json_mode,
            &format!("close  (best-effort; status {})", r.status),
        ),
        Err(e) => step(json_mode, &format!("close  (best-effort; error: {e})")),
    }

    Ok(RunOutcome {
        lease_id: lease_id.to_string(),
        exec_exit,
        verified,
        check_id: check_id.to_string(),
        exec_body: exec_resp.body,
    })
}

/// Best-effort cancel — fire and forget; never surfaced as an error.
fn best_effort_cancel(c: &Client, pat: &str, lease_id: &str, json_mode: bool) {
    let path = format!("/v1/leases/{lease_id}/cancel");
    match c.post_json(&path, "", Some(pat)) {
        Ok(r) if r.status == 200 => step(
            json_mode,
            &format!("cancel ✓  (best-effort; lease={lease_id})"),
        ),
        Ok(r) => step(
            json_mode,
            &format!(
                "cancel (best-effort; status {}; lease={lease_id})",
                r.status
            ),
        ),
        Err(e) => step(
            json_mode,
            &format!("cancel (best-effort; error: {e}; lease={lease_id})"),
        ),
    }
}

/// Emit the single JSON object required by CONTRACT-RUN v1 `--json` mode.
fn print_json_output(outcome: &RunOutcome, _c: &Client, _pat: &str) -> Result<()> {
    // Parse the exec response body for artifacts/refs.
    let exec_val: serde_json::Value =
        serde_json::from_str(&outcome.exec_body).unwrap_or(serde_json::Value::Null);
    let result = exec_val
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let artifacts: Vec<serde_json::Value> = result
        .get("artifacts")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            json!({
                "path": a.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                "digest": a.get("digest").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    let stdout_ref = result
        .get("stdout_ref")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let stderr_ref = result
        .get("stderr_ref")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    println!(
        "{}",
        serde_json::to_string(&json!({
            "lease_id": outcome.lease_id,
            "exit": outcome.exec_exit,
            "verified": outcome.verified,
            "check_id": outcome.check_id,
            "artifacts": artifacts,
            "stdout_ref": stdout_ref,
            "stderr_ref": stderr_ref,
        }))
        .expect("JSON output serializes")
    );
    Ok(())
}

// ── arg helpers (local copies — main.rs has the same, but they're `fn`, not
//    `pub fn`, so we can't call them from lib) ──────────────────────────────

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}
