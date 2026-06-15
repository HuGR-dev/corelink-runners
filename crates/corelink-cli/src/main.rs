//! `corelink` — the client/ops CLI for a CoreLink Runners fabric.
//!
//! Three commands today:
//!
//! - `smoke`  — automated live-deploy verification (health · attestation key ·
//!   fail-closed gates; `--full` also acquires→cancels a real box).
//! - `verify` — verify a fabric `result_binding_sig_v2` against the published
//!   key: the customer-trust primitive ("should I trust this verdict?").
//!   Pure-offline crypto; no network unless fetching the key via `--pubkey-url`.
//! - `run`    — full job lifecycle: acquire → exec → verify attestation → close.
//!   The customer adoption primitive.
//!
//! Auth/config via env: `CORELINK_URL`, `CORELINK_PAT`.

use std::io::Read as _;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use corelink_cli::{binding, client, run, smoke};
use corelink_fabric_api::dto::AttestationKeyResponse;

const HELP: &str = "\
corelink — client/ops CLI for a CoreLink Runners fabric

USAGE:
  corelink run    --url <fabric-url> --check '<shell cmd>' [--check-id <id>]
                  [--image <ref@sha256:...>] [--json] [--no-verify]
  corelink smoke  [--url <fabric-url>] [--full] [--image <ref>]
  corelink verify [--pubkey <b64> | --pubkey-url <fabric-url>] [--input <file>]

COMMANDS:
  run      Full job lifecycle: acquire → exec → verify attestation → close.
           The customer adoption primitive. Exits 0 on success, 1 if the job
           itself failed (but the run was clean), 2 on protocol/auth/verify
           errors or an unpinned image (fail-closed before any box contact).
  smoke    Verify a live deployment end-to-end. Default checks are side-effect-
           free (health, attestation key, fail-closed unpinned→400 / bad-PAT→401).
           --full also does a real acquire→cancel (provisions + tears down a box).
  verify   Verify a result_binding_sig_v2 over a CheckResult against the fabric's
           published ed25519 key. Reads a CloseResponse/ExecResponse JSON from
           --input or stdin (uses its `check_result`/`result` + the sig). Exit 0
           = authentic, 1 = NOT verified.

ENV:
  CORELINK_URL   fabric base URL (fallback for --url / --pubkey-url)
  CORELINK_PAT   tenant PAT (required by `smoke` and `run`)

EXAMPLES:
  CORELINK_PAT=... corelink run --url https://fabric.example --check 'cargo test'
  CORELINK_PAT=... corelink smoke --url https://fabric.example
  corelink verify --pubkey-url https://fabric.example --input close-response.json
";

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("corelink: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// Dispatch to a subcommand and return its exit code.
/// `smoke` and `verify` return `bool` (true = 0, false = 1).
/// `run` returns an explicit `i32` exit code (0, 1, or 2).
fn dispatch() -> Result<u8> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str).unwrap_or("help") {
        "run" => {
            let code = cmd_run(&args)?;
            Ok(code.clamp(0, 255) as u8)
        }
        "smoke" => {
            let ok = cmd_smoke(&args)?;
            Ok(if ok { 0 } else { 1 })
        }
        "verify" => {
            let ok = cmd_verify(&args)?;
            Ok(if ok { 0 } else { 1 })
        }
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(0)
        }
        other => {
            eprintln!("unknown command: {other:?}\n");
            print!("{HELP}");
            Ok(1)
        }
    }
}

fn cmd_run(args: &[String]) -> Result<i32> {
    run::cmd_run(args)
}

fn cmd_smoke(args: &[String]) -> Result<bool> {
    let base = flag(args, "--url")
        .or_else(|| std::env::var("CORELINK_URL").ok())
        .context("`smoke` needs --url <fabric-url> (or CORELINK_URL)")?;
    let pat = std::env::var("CORELINK_PAT")
        .context("`smoke` needs the CORELINK_PAT env var (the tenant PAT)")?;
    let full = has_flag(args, "--full");
    let image = flag(args, "--image").unwrap_or_else(|| smoke::PINNED_IMAGE.to_string());
    smoke::run(&base, &pat, full, &image)
}

fn cmd_verify(args: &[String]) -> Result<bool> {
    // Resolve the fabric public key: explicit --pubkey, or fetch it from a fabric.
    let pubkey = if let Some(pk) = flag(args, "--pubkey") {
        pk
    } else if let Some(base) =
        flag(args, "--pubkey-url").or_else(|| std::env::var("CORELINK_URL").ok())
    {
        // The key endpoint is behind the tenant-auth layer, so fetching it needs
        // a PAT. (Use --pubkey to verify fully offline with no PAT.)
        let pat = std::env::var("CORELINK_PAT").context(
            "fetching the key via --pubkey-url needs CORELINK_PAT (the key endpoint is \
             authenticated); or pass the key directly with --pubkey <b64> to verify offline",
        )?;
        fetch_key(&base, &pat)?
    } else {
        bail!("`verify` needs --pubkey <b64> or --pubkey-url <fabric-url> (or CORELINK_URL)");
    };

    // Read the response JSON from --input or stdin.
    let raw = match flag(args, "--input") {
        Some(path) => std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?,
        None => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("reading stdin")?;
            s
        }
    };

    let out = binding::verify_response_json(&raw, &pubkey)?;
    if out.verified {
        // Honest scope (audit F2): this checks the v2 OUTCOME binding (verdict +
        // outputs) against the fabric key — NOT the separate provenance chain
        // (tree/def/runner), which carries its own signature this command does
        // not surface.
        println!(
            "✓ result_binding_sig_v2 VERIFIED — the outcome (exit {} + {} artifact(s)) is \
             authentic to this fabric key (checks the v2 outcome binding, not the separate \
             provenance chain)",
            out.exit, out.artifacts
        );
    } else {
        println!(
            "✗ result_binding_sig_v2 FAILED to verify — do NOT trust this verdict \
             (forged, tampered, or wrong key)"
        );
    }
    Ok(out.verified)
}

/// Fetch the published ed25519 pubkey (std-base64) from `GET /v1/attestation/key`
/// (Bearer-PAT authenticated — the key endpoint sits behind the tenant-auth layer).
fn fetch_key(base: &str, pat: &str) -> Result<String> {
    let c = client::Client::new(base, pat);
    let r = c.get("/v1/attestation/key", true)?;
    if r.status != 200 {
        bail!("GET {base}/v1/attestation/key → {} (want 200)", r.status);
    }
    Ok(r.json::<AttestationKeyResponse>()?.ed25519_pubkey_b64)
}

/// `--flag value` → `Some(value)`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// `--flag` present?
fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}
