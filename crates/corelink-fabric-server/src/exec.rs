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

/// The FROZEN deterministic output emitted by [`MockLeasedExec`] on every
/// exec.
///
/// **FROZEN — DO NOT CHANGE.** External consumers pin the SHA-256 of this
/// string (`sha256:a30dd181a99e2acecd791e826
/// 347f30104e7e7db30fd14035d0287affb51d254`) in their test fixtures. Any edit
/// to this constant silently breaks those pinned digests.
pub const MOCK_STDOUT: &str = "corelink-fabricd mock-exec: deterministic stub output\n";

/// A named mock [`LeasedExec`] for offline adapter development.
///
/// Every call to [`exec_captured_for`] returns a deterministic
/// `Ok(CmdOutput { code: Some(0), stdout: MOCK_STDOUT, stderr: "" })`
/// regardless of `lease_id` or `argv`. This is NOT a real execution — it
/// is a pure offline stub that allows external consumers to
/// drive the REAL binary + REAL HTTP API + REAL signed attestation without a
/// cloud provider.
///
/// **Prod-safety:** wired only when `FABRIC_MOCK_EXEC=1`, which requires
/// `FABRIC_DEV_UNSAFE=1` (loopback-only bind, dev signing key, forged-but-
/// detectable attestations) and no real `FABRIC_SIGNING_KEY` or
/// `NORTHFLANK_*` vars. See `config_from_env` interlock.
///
/// [`exec_captured_for`]: LeasedExec::exec_captured_for
pub struct MockLeasedExec;

impl LeasedExec for MockLeasedExec {
    fn exec_captured_for(&self, _lease_id: &str, _argv: &[&str]) -> Result<CmdOutput> {
        Ok(CmdOutput {
            code: Some(0),
            stdout: MOCK_STDOUT.to_string(),
            stderr: String::new(),
        })
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
/// the external consumer side as the external refstore's compute_memo_key implementation):
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

    // ----------------------------------------------------------------------
    // Property hardening (deterministic in-test PRNG — NO new dependency).
    //
    // A fixed-seed xorshift64* generator drives thousands of randomized axis
    // triples so the suite is fully reproducible. The test asserts the SECURITY
    // property (formula equivalence against an INDEPENDENT reference, plus
    // injectivity of distinct triples → distinct keys), not just `is_ok`.
    // ----------------------------------------------------------------------

    /// Deterministic xorshift64* PRNG, fixed-seed (reproducible, no dep).
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u32) -> u32 {
            if n == 0 {
                0
            } else {
                (self.next_u64() % u64::from(n)) as u32
            }
        }

        /// A short token from a tiny alphabet, INCLUDING `""` — the boundary
        /// case that makes length-prefix framing load-bearing for injectivity.
        fn token(&mut self) -> String {
            const ALPHABET: &[u8] = b"ab0:/-";
            let len = self.below(6); // 0..=5; "" sampled often
            (0..len)
                .map(|_| ALPHABET[self.below(ALPHABET.len() as u32) as usize] as char)
                .collect()
        }
    }

    /// An INDEPENDENT reference memo-key, re-derived from first principles via
    /// a DIFFERENT code path than `compute_memo_key`: build the full LP
    /// pre-image as one `Vec<u8>` (rather than streaming into the hasher), hash
    /// it once, and hex-encode with a different formatter. If this agrees with
    /// the production formula over thousands of cases, the production formula
    /// is the documented `lower_hex(SHA-256(LP(tree)‖LP(def)‖LP(toolchain)))`.
    fn reference_memo_key(tree: &str, def: &str, toolchain: &str) -> String {
        fn lp_into(out: &mut Vec<u8>, s: &str) {
            let len = u32::try_from(s.len()).expect("axis exceeds u32::MAX");
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        let mut pre = Vec::new();
        lp_into(&mut pre, tree);
        lp_into(&mut pre, def);
        lp_into(&mut pre, toolchain);
        let digest = Sha256::digest(&pre);
        // Different hex path: a lookup table, not `write!("{:02x}")`.
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(64);
        for b in digest {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0x0f) as usize] as char);
        }
        s
    }

    /// PROPERTY 3 — memo_key formula equivalence + injectivity, randomized.
    ///
    /// Over thousands of random axis triples:
    ///  - the production `compute_memo_key` equals the INDEPENDENT
    ///    `reference_memo_key` (re-derived via a different code path) — proving
    ///    the production formula is exactly
    ///    `lower_hex(SHA-256(LP(tree)‖LP(def)‖LP(toolchain)))`;
    ///  - the output is always 64 lowercase-hex chars;
    ///  - distinct triples yield distinct keys (collision-resistance modulo
    ///    SHA-256) — no LP boundary-shift collision (`("ab","","cd")` vs
    ///    `("a","b","cd")`) ever slips through.
    #[test]
    fn prop_memo_key_equiv_and_injective() {
        const ITERS: u32 = 10_000;
        let mut rng = Rng::new(0x5EED_1234_ABCD_0F0F);

        // The textbook boundary-shift witness must already differ.
        assert_ne!(
            compute_memo_key("ab", "", "cd"),
            compute_memo_key("a", "b", "cd"),
            "LP boundary-shift must change the key"
        );

        use std::collections::HashMap;
        let mut seen: HashMap<String, (String, String, String)> = HashMap::new();

        for _ in 0..ITERS {
            let tree = rng.token();
            let def = rng.token();
            let tc = rng.token();

            let key = compute_memo_key(&tree, &def, &tc);

            // Equivalence: production == independent reference.
            assert_eq!(
                key,
                reference_memo_key(&tree, &def, &tc),
                "compute_memo_key diverged from the first-principles reference \
                 for ({tree:?},{def:?},{tc:?})"
            );
            // Shape: 64 lowercase-hex chars.
            assert_eq!(key.len(), 64, "memo_key must be 64 hex chars");
            assert!(
                key.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "memo_key must be lowercase hex"
            );

            // Injectivity: distinct triples → distinct keys.
            let triple = (tree, def, tc);
            match seen.get(&key) {
                Some(prev) if *prev != triple => {
                    panic!(
                        "MEMO_KEY COLLISION on distinct triples {prev:?} and \
                         {triple:?} — LP framing failed to separate the axes"
                    );
                }
                _ => {
                    seen.entry(key).or_insert(triple);
                }
            }
        }
        // Reaching here = no collision across ITERS iterations (the panic in the
        // match arm is the memo_key injectivity assertion).
    }
}
