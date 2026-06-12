//! WP-API3 — the exec mechanism: run a `CheckDef` inside the leased
//! box/VM and build the frozen `CheckResult` (contract §3, the transport
//! replacement). Engine-agnostic: everything here speaks through the
//! [`LeasedExec`] port, so the Docker engine today and Firecracker tomorrow
//! slot in behind the same seam without touching the result path.
//!
//! Fail-closed law (contract §3 + §2): a result is NEVER fabricated. If the
//! execution is refused, the transport dies, or the process is killed by a
//! signal (`CmdOutput.code == None`), [`run_check`] returns `Err` — there is
//! no partial or synthetic `CheckResult` on any failure path.
//!
//! M1 honesty notes (deliberate, documented scope):
//! - `stdout_ref`/`stderr_ref` are `sha256:<hex>` content refs computed over
//!   the captured bytes. The DIGEST is the content address already; the CAS
//!   write that makes it dereferenceable lands with FC3.
//! - `artifacts` is the empty vec: artifact capture is FC-domain and arrives
//!   explicitly, never silently.
//! - `toolchain_digest = CheckDef.toolchain_ref` — the M1 equivalence: the
//!   def's toolchain ref IS the third memo axis until a resolver maps refs
//!   to content digests.

use anyhow::{Context, Result, bail};
use corelink_runner::lease::CmdOutput;
use corelink_runners_contracts::{CheckDef, CheckResult};
use sha2::{Digest, Sha256};

/// The execution port: "run `argv` inside the box/VM serving `lease_id`,
/// capturing output". This is `isolation.rs::Engine::exec_captured` (Engine
/// v2) lifted to lease scope — the composition root resolves `lease_id` to
/// its live `RunningContainer` and drives the real engine; tests inject a
/// [`FakeLeasedExec`]. The result path never names an engine.
pub trait LeasedExec: Send + Sync {
    /// Run `argv` inside the container/VM leased as `lease_id`, returning
    /// the captured output (exit code + stdout/stderr bytes).
    ///
    /// # Errors
    /// Fails if the lease has no live box attached or the transport refuses;
    /// callers treat any error as fail-closed (no result is fabricated).
    fn exec_captured_for(&self, lease_id: &str, argv: &[&str]) -> Result<CmdOutput>;
}

/// The fail-closed default [`LeasedExec`]: every exec is refused with an
/// explicit error. Wired by the convenience constructors (`app`,
/// `app_with_registry`) so a composition that never attached a real engine
/// can never silently "succeed" — the refusal surfaces as 503, never as a
/// fabricated result.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoBoxExec;

impl LeasedExec for NoBoxExec {
    fn exec_captured_for(&self, lease_id: &str, _argv: &[&str]) -> Result<CmdOutput> {
        bail!("no execution backend attached for lease {lease_id}: failing closed")
    }
}

/// Scripted [`LeasedExec`] test double: replies with a fixed [`CmdOutput`]
/// and records every invocation (lease id + argv), so tests can assert both
/// the bytes-to-digest path and — crucially — that refused/expired paths
/// performed ZERO executions.
#[derive(Debug)]
pub struct FakeLeasedExec {
    /// The scripted reply (`None` exit code models a signal-killed process).
    reply: CmdOutput,
    /// Every call: `(lease_id, argv)` in order.
    calls: std::sync::Mutex<Vec<(String, Vec<String>)>>,
}

impl FakeLeasedExec {
    /// A fake that replies to every exec with `reply`, verbatim.
    pub fn replying(reply: CmdOutput) -> Self {
        Self {
            reply,
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// The recorded invocations: `(lease_id, argv)` in call order.
    pub fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl LeasedExec for FakeLeasedExec {
    fn exec_captured_for(&self, lease_id: &str, argv: &[&str]) -> Result<CmdOutput> {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).push((
            lease_id.to_string(),
            argv.iter().map(|s| s.to_string()).collect(),
        ));
        Ok(self.reply.clone())
    }
}

/// Lowercase-hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// `sha256:<hex>` content ref of `bytes` — the content address itself; the
/// CAS write that makes it dereferenceable is FC3 domain (module docs).
fn content_ref(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

/// The FROZEN memo-key formula, transcribed byte-exactly from
/// `corelink-runners-contracts/src/check_result.rs` (single-sourced on the
/// hugit side as `hugit_refstore::compute_memo_key`):
///
/// ```text
/// memo_key = lower_hex( SHA-256( LP(tree_hash) ‖ LP(def_digest) ‖ LP(toolchain_digest) ) )
/// where LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)
/// ```
///
/// Axes in struct field order; output 64-char lowercase hex. The
/// length-prefix framing makes the concatenation injective — `("ab","","cd")`
/// can never collide with `("a","b","cd")`. Pinned by
/// `memo_key_formula_known_vector` (hand-computed expected hex).
pub fn compute_memo_key(tree_hash: &str, def_digest: &str, toolchain_digest: &str) -> String {
    let mut hasher = Sha256::new();
    for axis in [tree_hash, def_digest, toolchain_digest] {
        let len: u32 = axis
            .len()
            .try_into()
            .expect("memo axis exceeds u32::MAX bytes");
        hasher.update(len.to_be_bytes());
        hasher.update(axis.as_bytes());
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Execute `def` inside the lease's box via the [`LeasedExec`] port and
/// build the frozen `CheckResult`.
///
/// - `argv = ["sh", "-lc", def.command]` — the command is the def's own
///   shell string; the shim's quoting discipline applies upstream (the def
///   author owns the command's internal quoting, exactly as in the
///   Actions-YAML shim).
/// - `clock` is the caller-supplied clock seam (epoch ms): read once before
///   and once after the execution — `duration_ms` is the difference,
///   `produced_at` the closing read. Deterministic under test.
/// - Exit comes from the captured [`CmdOutput`]; a `None` exit (signal-
///   killed) or any transport error is `Err` — fail-closed, no result is
///   EVER fabricated (module docs).
///
/// # Errors
/// Fails if the port refuses the execution or the process did not exit
/// (signal-killed). Never returns a partial `CheckResult`.
pub fn run_check(
    executor: &dyn LeasedExec,
    lease_id: &str,
    def: &CheckDef,
    tree_hash: &str,
    clock: &dyn Fn() -> u64,
    runner_ref: &str,
) -> Result<CheckResult> {
    let started_at = clock();
    let output = executor
        .exec_captured_for(lease_id, &["sh", "-lc", &def.command])
        .with_context(|| format!("exec refused for lease {lease_id}: failing closed"))?;
    let produced_at = clock();

    // Fail-closed: a signal-killed process has NO exit code — there is no
    // honest CheckResult to build, and none is fabricated.
    let Some(exit) = output.code else {
        bail!(
            "check on lease {lease_id} was killed by a signal (no exit code): \
             refusing to fabricate a CheckResult"
        );
    };

    Ok(CheckResult {
        memo_key: compute_memo_key(tree_hash, &def.def_digest, &def.toolchain_ref),
        tree_hash: tree_hash.to_string(),
        def_digest: def.def_digest.clone(),
        // M1 equivalence (module docs): the def's toolchain ref IS the
        // third memo axis until a resolver maps refs to content digests.
        toolchain_digest: def.toolchain_ref.clone(),
        exit,
        // Artifact capture is FC-domain — explicit when it lands, never
        // silently faked here (module docs).
        artifacts: Vec::new(),
        stdout_ref: content_ref(output.stdout.as_bytes()),
        stderr_ref: content_ref(output.stderr.as_bytes()),
        duration_ms: produced_at.saturating_sub(started_at),
        runner_ref: runner_ref.to_string(),
        produced_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The FROZEN formula against a hand-computed vector:
    /// `LP("ab") ‖ LP("") ‖ LP("cd")` =
    /// `00000002 6162 00000000 00000002 6364`, and
    /// `sha256` of those 16 bytes is the constant below
    /// (`printf '\x00\x00\x00\x02ab\x00\x00\x00\x00\x00\x00\x00\x02cd' | shasum -a 256`).
    #[test]
    fn memo_key_formula_known_vector() {
        assert_eq!(
            compute_memo_key("ab", "", "cd"),
            "09eb0a232caeae9031bf4f9475efcf3f8d37f2beabeb85efaa00e6a5948d7370"
        );
    }

    /// The length-prefix framing is injective: shifting a byte across an
    /// axis boundary changes the key.
    #[test]
    fn memo_key_framing_is_injective_across_axes() {
        assert_ne!(
            compute_memo_key("ab", "", "cd"),
            compute_memo_key("a", "b", "cd")
        );
        assert_ne!(
            compute_memo_key("ab", "", "cd"),
            compute_memo_key("ab", "c", "d")
        );
    }

    /// `NoBoxExec` refuses every exec — the fail-closed default can never
    /// fabricate a result.
    #[test]
    fn no_box_exec_always_refuses() {
        assert!(
            NoBoxExec
                .exec_captured_for("lease-x", &["sh", "-lc", "true"])
                .is_err()
        );
    }
}
