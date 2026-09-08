// Transplanted from transferred invariants/x4/tests/acceptance_x4.rs @ 69e28e5 (runner-transfer campaign WP-R4, 2026-06-10) — wire-contract seam, no git dep; removed external-consumer side by WP-R4②.
//! WP-X4 acceptance oracle — supply chain: image pinning + integrity +
//! fail-closed. Contract: `docs/plan/wp-contracts/WP-X4.md`.
//!
//! Owned items (one `#[test] item_<n>_<slug>` each):
//!   ① `item_1_image_content_pinned_verified` — a runner image is content
//!      (digest) pinned and integrity-verified against the live box at spawn;
//!      a digest mismatch is detectable. (box-dependent)
//!   ② `item_2_app_deps_pinned_ci` — the App/workspace dependencies are pinned
//!      (committed `Cargo.lock`) and verified by the CI audit gate; a floating
//!      dep is rejected by the gate. (workspace-file inspection)
//!   ③ `item_3_tampered_unpinned_fail_closed` — a tampered AND an unpinned
//!      image both fail CLOSED **before any tenant work**: the verify-before-
//!      spawn guard rejects with no container spawned and no tenant byte
//!      processed. The ordering is load-bearing. (box-dependent)
//!
//! Box-dependence: items ① and ③ drive the live runner box pinned by
//! `CORELINK_RUNNER_HOST` (the suite exports `91.99.11.196`). When the env is set
//! but the box is unreachable they **FAIL** (not skip), per contract.
//!
//! The fail-closed-before-spawn ORDERING (item ③, the load-bearing invariant)
//! is ALSO proven HERMETICALLY in the bare `cargo test --workspace` gate by
//! `item_3_fail_closed_before_spawn_hermetic`, which drives the **production**
//! [`DockerEngine::spawn`] surface against a FAKE box (no network, no env).
//! This closes the brutal-review X4 finding: the ordering proof is no longer a
//! silent no-op when `CORELINK_RUNNER_HOST` is unset — it runs, and FAILS (not
//! skips) if the engine ever issues `docker run` before rejecting a
//! tampered/unpinned image.
//!
//! **Single-source (test-integrity, 2026-06-14):** the oracle exercises the
//! PRODUCTION supply-chain path — `DockerEngine::spawn` → `crate::pin::
//! PinnedImageRef::{parse,verify_on_box}` — not the `x4::pin::VerifiedSpawn`
//! façade. The two were re-transcribed copies that had DRIFTED: production
//! `verify_on_box` classifies pull failures (transient → bounded retry;
//! permanent → fail-closed) while the `x4::pin` copy fails closed on ANY
//! non-ok pull. Proving the façade therefore did not prove what ships. The
//! oracle now drives the surface every real spawn crosses, so a regression in
//! the production gate goes RED here.

use corelink_runner::isolation::{DockerEngine, Engine};
use corelink_runner::lease::{BoxExec, CmdOutput, ContainerSpec, SshBox};
use corelink_runner::namespace::JOB_TMP_ROOT;
use corelink_runner::teardown::teardown;
use corelink_runner::x4::pin::PinnedImage;
use corelink_runners_contracts::{RunnerLease, RunnerState};

/// Tag used only to *resolve* a real content digest from the box; never used as
/// a spawn reference (a tag is unpinned by definition).
const RESOLVE_TAG: &str = "alpine:3.20";

/// A syntactically-valid but content-wrong (tampered) digest: 64 hex zeros.
/// No registry can serve it, so it must fail integrity verification.
const TAMPERED_REF: &str =
    "alpine@sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// Whether the box-dependent acceptance lane is active (env set + non-empty).
/// Mirrors the WP-C2a convention: the suite sets the env; the bare gate does
/// not, so box bodies short-circuit there to keep `cargo test --workspace`
/// green.
fn box_lane_active() -> bool {
    std::env::var("CORELINK_RUNNER_HOST")
        .ok()
        .is_some_and(|h| !h.trim().is_empty())
}

// ─────────────────────────────────────────────────────────────────────────────
// HERMETIC fake box — prove the fail-closed-before-spawn ORDERING in the BARE
// gate, driving the PRODUCTION `DockerEngine::spawn` surface (single-sourced).
//
// This carries NO network and NO env dependency, so the load-bearing item ③
// ordering invariant ("a tampered/unpinned image fails CLOSED with no `docker
// run` issued") executes inside `cargo test --workspace`. It records every argv
// so the oracle can prove a `docker run` (the tenant-work threshold) was NEVER
// reached for a rejected image, and DID follow the verify (`docker pull`) for a
// good pin. The fake serves exactly one good content digest; any other digest is
// refused with a PERMANENT registry signal so production `verify_on_box` fails
// closed at once (mirroring the content-addressed store).
// ─────────────────────────────────────────────────────────────────────────────

/// The 64-hex digest of the "good" hermetic pin the fake box will serve.
const FAKE_GOOD_DIGEST: &str = "d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// A genuinely content-pinned reference whose digest the fake box resolves.
const FAKE_GOOD_REF: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// A fake `BoxExec` that emulates a registry-backed docker daemon WITHOUT any
/// network: it serves exactly one good content digest. A `docker pull` of any
/// other reference is "refused" with a PERMANENT signal (`manifest … not
/// found`), so a tampered digest fails the production integrity check CLOSED —
/// mirroring the content-addressed-store integrity property the real box has.
/// Every argv is recorded so ordering ("`docker run` never precedes a passed
/// verify, and never follows a failed one") is provable.
#[derive(Clone)]
struct FakeBox {
    /// The only digest this fake daemon can serve.
    good_digest: String,
    /// Every `run` argv joined by space, in order — the ordering audit trail.
    calls: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

impl FakeBox {
    fn new(good_digest: &str) -> Self {
        Self {
            good_digest: good_digest.to_string(),
            calls: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        }
    }

    fn ok(stdout: &str) -> CmdOutput {
        CmdOutput {
            code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn fail(stderr: &str) -> CmdOutput {
        CmdOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    /// Whether `reference` carries the one digest this fake can serve.
    fn serves(&self, reference: &str) -> bool {
        reference.contains(&format!("sha256:{}", self.good_digest))
    }

    /// The recorded argv audit trail (space-joined).
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// `true` iff any recorded command actually started a container (`docker
    /// run`) — the threshold past which tenant work could begin.
    fn ran_a_container(&self) -> bool {
        self.calls().iter().any(|c| c.starts_with("docker run"))
    }
}

impl BoxExec for FakeBox {
    fn run(&self, argv: &[&str]) -> anyhow::Result<CmdOutput> {
        self.calls.borrow_mut().push(argv.join(" "));
        match argv {
            // `docker pull <ref>` — succeeds ONLY for the servable good digest;
            // a tampered/unresolvable digest is refused with a PERMANENT signal
            // so production `verify_on_box` fails CLOSED at once (no retry).
            ["docker", "pull", reference] => {
                if self.serves(reference) {
                    Ok(Self::ok("Status: Image is up to date"))
                } else {
                    Ok(Self::fail(&format!(
                        "manifest for {reference} not found: content-addressed \
                         store cannot serve a tampered/unknown digest"
                    )))
                }
            }
            // `docker image inspect <ref> --format {{RepoDigests}}` — echoes the
            // resolved RepoDigest so the integrity check confirms the pin matches.
            ["docker", "image", "inspect", reference, "--format", _] => {
                if self.serves(reference) {
                    Ok(Self::ok(&format!("alpine@sha256:{}\n", self.good_digest)))
                } else {
                    Ok(Self::ok(""))
                }
            }
            // `docker run …` — the real engine's spawn surface. Reaching here is
            // the ordering threshold; succeed quietly so a GOOD pin spawns.
            ["docker", "run", ..] => Ok(Self::ok("")),
            _ => Ok(Self::ok("")),
        }
    }
}

/// Build a ContainerSpec by STRUCT LITERAL (bypassing `from_lease`, which now
/// correctly REJECTS an unpinned image at the supply-chain floor). This lets the
/// oracle hand the PRODUCTION engine a deliberately unpinned/tampered spec and
/// prove the engine ITSELF (`DockerEngine::spawn`) rejects it fail-closed before
/// any `docker run` — the point of item ③, on the surface every spawn crosses.
fn spec_literal(name: &str, image: &str) -> ContainerSpec {
    ContainerSpec {
        name: name.to_string(),
        image: image.to_string(),
        tmp_root: JOB_TMP_ROOT.to_string(),
        no_network: true,
        allow_egress: false,
        run_on_create: false,
        path_set: vec!["src/".to_string()],
        env: vec![],
    }
}

// ── ③ (hermetic) tampered/unpinned image → fail CLOSED BEFORE `docker run` ───
// This runs in the BARE `cargo test --workspace` gate (no env, no network). It
// is the load-bearing ordering proof, driven through the PRODUCTION
// `DockerEngine::spawn` → `crate::pin::PinnedImageRef` path (single-sourced):
// a regression in the shipped supply-chain gate goes RED here.
#[test]
fn item_3_fail_closed_before_spawn_hermetic() {
    // ── attack 1: UNPINNED image (floating tag) via STRUCT LITERAL ───────────
    // (from_lease rejects this at the floor; we bypass it to prove the engine
    // is ALSO the floor — a directly-built unpinned spec never reaches run.)
    let boxx = FakeBox::new(FAKE_GOOD_DIGEST);
    let engine = DockerEngine::new(boxx.clone());
    let spec_unpinned = spec_literal("corelink-job-hermetic-unpinned", RESOLVE_TAG);
    let r1 = engine.spawn(&spec_unpinned);
    assert!(
        r1.is_err(),
        "unpinned image must be REJECTED by the production engine, got: {r1:?}"
    );
    assert!(
        !boxx.ran_a_container(),
        "ORDERING VIOLATION: `docker run` was issued for an UNPINNED image — \
         tenant work began before verification (fail-OPEN); calls: {:?}",
        boxx.calls()
    );

    // ── attack 2: TAMPERED image (valid-form digest, content-wrong) ──────────
    let boxx = FakeBox::new(FAKE_GOOD_DIGEST);
    let engine = DockerEngine::new(boxx.clone());
    let spec_tampered = spec_literal("corelink-job-hermetic-tampered", TAMPERED_REF);
    let r2 = engine.spawn(&spec_tampered);
    assert!(
        r2.is_err(),
        "tampered image must be REJECTED by the production engine, got: {r2:?}"
    );
    assert!(
        boxx.calls().iter().any(|c| c.starts_with("docker pull")),
        "the production gate must attempt a content-addressed pull (verify) \
         for a syntactically-pinned digest; calls: {:?}",
        boxx.calls()
    );
    assert!(
        !boxx.ran_a_container(),
        "ORDERING VIOLATION: `docker run` was issued for a TAMPERED image — \
         integrity verification did not gate the spawn (fail-OPEN); calls: {:?}",
        boxx.calls()
    );

    // ── positive control: a genuine content pin DOES spawn (the gate is not
    //    vacuously rejecting everything), and the verify (`docker pull`)
    //    PRECEDES the single `docker run` — ordering is real, not a stub. ─────
    let boxx = FakeBox::new(FAKE_GOOD_DIGEST);
    let engine = DockerEngine::new(boxx.clone());
    let spec_ok = spec_literal("corelink-job-hermetic-ok", FAKE_GOOD_REF);
    engine
        .spawn(&spec_ok)
        .expect("a genuine, box-verified content pin MUST spawn");
    let calls = boxx.calls();
    let pull_idx = calls.iter().position(|c| c.starts_with("docker pull"));
    let run_idx = calls.iter().position(|c| c.starts_with("docker run"));
    assert!(
        pull_idx.is_some() && run_idx.is_some(),
        "a good pin must be pulled (verified) AND run; calls: {calls:?}"
    );
    assert!(
        pull_idx < run_idx,
        "verify (`docker pull`) MUST precede `docker run`; calls: {calls:?}"
    );
}

/// Track-C C2: the production `DockerEngine::spawn` applies the untrusted-
/// container hardening flags — a hostile job cannot escalate privileges,
/// fork-bomb, or OOM the box. Asserted on the REAL `docker run` argv the
/// production surface emits (single-sourced, not a stub).
#[test]
fn item_4_spawn_applies_untrusted_hardening_flags() {
    let boxx = FakeBox::new(FAKE_GOOD_DIGEST);
    let engine = DockerEngine::new(boxx.clone());
    let spec_ok = spec_literal("corelink-job-hardening", FAKE_GOOD_REF);
    engine.spawn(&spec_ok).expect("a good pin must spawn");

    let run = boxx
        .calls()
        .into_iter()
        .find(|c| c.starts_with("docker run"))
        .expect("spawn must issue a docker run");

    for needle in [
        "--cap-drop ALL",                   // no Linux capabilities
        "--security-opt no-new-privileges", // no setuid/setcap escalation
        "--pids-limit 4096",                // fork-bomb bound
        "--memory 12g",                     // OOM ceiling
        "--memory-swap 12g",                // == memory ⇒ no swap escape
        "--network none",                   // (pre-existing) isolated netns
    ] {
        assert!(
            run.contains(needle),
            "docker run must carry the untrusted-hardening flag `{needle}`; got: {run}"
        );
    }
}

/// Connect to the live box; FAIL (panic) if unreachable, per contract. Only
/// called inside the active box lane.
fn live_box() -> SshBox {
    let boxx = SshBox::from_env().expect("CORELINK_RUNNER_HOST must be set inside the box lane");
    let ping = boxx
        .run(&["docker", "version", "--format", "{{.Server.Version}}"])
        .expect("ssh to runner box failed to spawn");
    assert!(
        ping.ok() && !ping.stdout.trim().is_empty(),
        "runner box {} unreachable or docker down (code={:?} stderr={:?}); \
         box-dependent acceptance must FAIL, not skip",
        boxx.target,
        ping.code,
        ping.stderr.trim(),
    );
    boxx
}

/// Resolve a real `repo@sha256:…` content pin from the box by pulling a tag and
/// reading its `RepoDigests`. This is how a pinned reference is obtained from a
/// floating tag in practice; the result is a genuine, servable content digest.
fn resolve_pin(boxx: &SshBox) -> PinnedImage {
    let pull = boxx
        .run(&["docker", "pull", RESOLVE_TAG])
        .expect("docker pull failed to spawn");
    assert!(
        pull.ok(),
        "docker pull {RESOLVE_TAG} failed: {}",
        pull.stderr.trim()
    );
    let inspect = boxx
        .run(&[
            "docker",
            "image",
            "inspect",
            RESOLVE_TAG,
            "--format",
            "{{range .RepoDigests}}{{.}}\n{{end}}",
        ])
        .expect("docker inspect failed to spawn");
    assert!(
        inspect.ok(),
        "docker inspect failed: {}",
        inspect.stderr.trim()
    );
    let reference = inspect
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| l.contains("@sha256:"))
        .unwrap_or_else(|| {
            panic!(
                "no RepoDigest resolved for {RESOLVE_TAG}: {:?}",
                inspect.stdout
            )
        })
        .to_string();
    PinnedImage::parse(&reference)
        .unwrap_or_else(|e| panic!("box-resolved reference {reference:?} is not a valid pin: {e}"))
}

/// A fresh, unique, C2a-conformant lease for the given image reference.
fn fresh_lease(slug: &str) -> RunnerLease {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    RunnerLease {
        lease_id: format!("x4-{slug}-{nonce}"),
        principal_chain: vec!["agent:acceptance".to_string()],
        path_set: vec!["src/".to_string()],
        expiry: u64::MAX,
        net_policy: "none".to_string(),
        tmp_root: JOB_TMP_ROOT.to_string(),
        state: RunnerState::Held,
    }
}

// ── ① image content-pinned + integrity-verified at spawn ─────────────────────
#[test]
fn item_1_image_content_pinned_verified() {
    if !box_lane_active() {
        return;
    }
    let boxx = live_box();

    // A genuine content digest resolved from the box is pinned-parseable.
    let pinned = resolve_pin(&boxx);
    assert!(
        PinnedImage::is_pinned(pinned.reference()),
        "resolved reference {} must be content-pinned",
        pinned.reference()
    );
    assert_eq!(
        pinned.digest_hex().len(),
        64,
        "sha256 digest is 64 hex chars"
    );

    // A floating tag is NOT content-pinned — pinning is digest, not tag.
    assert!(
        !PinnedImage::is_pinned(RESOLVE_TAG),
        "a floating tag ({RESOLVE_TAG}) must not count as content-pinned"
    );

    // Integrity verification against the live box passes for the true pin …
    pinned
        .verify_on_box(&boxx)
        .expect("a box-resolved content pin must integrity-verify at spawn");

    // … and a digest MISMATCH is detectable (the integrity check fails CLOSED).
    let tampered = PinnedImage::parse(TAMPERED_REF).expect("tampered ref is syntactically pinned");
    assert!(
        tampered.verify_on_box(&boxx).is_err(),
        "a tampered (content-wrong) digest must fail integrity verification"
    );
}

// ── ② App deps pinned + verified in CI ───────────────────────────────────────
#[test]
fn item_2_app_deps_pinned_ci() {
    // Locate the repo root from this crate's manifest dir.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent() // crates/
        .and_then(std::path::Path::parent) // repo root
        .expect("repo root above crates/corelink-runner")
        .to_path_buf();

    // (a) Dependencies are PINNED: a committed root Cargo.lock exists and pins
    //     concrete versions (`version = "…"` entries under [[package]]).
    let lock = repo_root.join("Cargo.lock");
    let lock_body = std::fs::read_to_string(&lock).unwrap_or_else(|e| {
        panic!(
            "committed Cargo.lock must pin deps ({}): {e}",
            lock.display()
        )
    });
    assert!(
        lock_body.contains("[[package]]") && lock_body.contains("version = \""),
        "Cargo.lock must pin concrete dependency versions"
    );

    // The runner crate's own deps must appear pinned in the lockfile.
    let app_pinned = lock_body
        .split("[[package]]")
        .any(|p| p.contains("name = \"corelink-runner\"") && p.contains("version = \""));
    assert!(
        app_pinned,
        "the runner crate (corelink-runner) must be pinned in Cargo.lock"
    );

    // (b) Verified in CI: the existing audit gate runs `cargo audit` over the
    //     pinned graph. X4 rides this gate, it does not add a new one.
    let ci = std::fs::read_to_string(repo_root.join(".github/workflows/ci.yml"))
        .expect("CI workflow must exist");
    assert!(
        ci.contains("cargo audit"),
        "CI must verify the pinned dependency graph via `cargo audit`"
    );
    assert!(
        ci.contains("cargo test --workspace"),
        "CI must build/test the pinned workspace"
    );

    // (c) An unpinned/floating dep is REJECTED: a Cargo.toml requirement with no
    //     concrete version (e.g. `dep = \"*\"`) is the floating case the gate
    //     refuses. Assert the App manifest carries no wildcard requirement.
    let app_toml = std::fs::read_to_string(repo_root.join("crates/corelink-runner/Cargo.toml"))
        .expect("corelink-runner Cargo.toml must exist");
    for line in app_toml.lines() {
        let l = line.trim();
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        assert!(
            !l.contains("\"*\"") && !l.ends_with("= \"*\""),
            "runner dependency must not be a floating wildcard: {l:?}"
        );
    }
}

// ── ③ tampered/unpinned image → fail CLOSED before any tenant work ───────────
// Load-bearing: verify-before-spawn ORDERING. A post-hoc detection is a FAIL.
// Single-sourced (test-integrity, 2026-06-14): drives the PRODUCTION
// `DockerEngine::spawn` surface — the path every real spawn crosses — NOT the
// `x4::pin::VerifiedSpawn` façade. The engine rejection (`Err`) and the
// box-side "no container by that name" check together prove fail-closed.
#[test]
fn item_3_tampered_unpinned_fail_closed() {
    if !box_lane_active() {
        return;
    }
    let boxx = live_box();
    let engine = DockerEngine::new(boxx.clone());

    // ── attack 1: UNPINNED image (floating tag). ─────────────────────────────
    // `ContainerSpec::from_lease` REJECTS an unpinned image at the supply-chain
    // floor, so an unpinned spec can no longer be derived through it. We build
    // the unpinned spec via STRUCT LITERAL to hand the PRODUCTION engine a
    // deliberately unpinned input and prove the ENGINE ITSELF rejects it
    // fail-closed before any `docker run`.
    let lease_unpinned = fresh_lease("unpinned");
    let spec_unpinned = spec_literal(
        &format!("corelink-job-{}", lease_unpinned.lease_id),
        RESOLVE_TAG,
    );
    let r1 = engine.spawn(&spec_unpinned);
    assert!(
        r1.is_err(),
        "unpinned image must be REJECTED by the production engine, got: {r1:?}"
    );
    // Prove fail-closed ordering: NO container by that name exists on the box.
    assert_no_container(&boxx, &spec_unpinned.name, "unpinned");

    // ── attack 2: TAMPERED image (valid-form digest, content-wrong). ─────────
    let lease_tampered = fresh_lease("tampered");
    let spec_tampered =
        ContainerSpec::from_lease(&lease_tampered, TAMPERED_REF).expect("derive spec (tampered)");
    let r2 = engine.spawn(&spec_tampered);
    assert!(
        r2.is_err(),
        "tampered image must be REJECTED by the production engine, got: {r2:?}"
    );
    assert_no_container(&boxx, &spec_tampered.name, "tampered");

    // ── positive control: a genuine content pin DOES spawn (proving the gate
    //    is not vacuously rejecting everything), then is torn down clean. ─────
    let pinned = resolve_pin(&boxx);
    let lease_ok = fresh_lease("pinned-ok");
    let spec_ok =
        ContainerSpec::from_lease(&lease_ok, pinned.reference()).expect("derive spec (pinned)");
    let container = engine
        .spawn(&spec_ok)
        .expect("a genuine content pin must spawn through the production engine");
    // It really ran: tear it down and forensic-clean the box.
    let report = teardown(&boxx, &container).expect("teardown");
    assert!(
        report.is_clean(),
        "forensic re-scan found residue after teardown: {report:#?}"
    );
}

/// Assert no container with `name` exists on the box — proof that a rejected
/// spawn processed **no tenant work** (fail-closed BEFORE spawn).
fn assert_no_container(boxx: &SshBox, name: &str, label: &str) {
    let ps = boxx
        .run(&[
            "docker",
            "ps",
            "-a",
            "--filter",
            &format!("name=^{name}$"),
            "--format",
            "{{.Names}}",
        ])
        .expect("docker ps failed to spawn");
    assert!(
        ps.stdout.trim().is_empty(),
        "{label} image must leave NO container (fail-closed before tenant work); \
         found: {:?}",
        ps.stdout.trim()
    );
}
