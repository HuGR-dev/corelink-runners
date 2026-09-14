//! Lease-id → shard routing (multi-instance fabricd, option 3).
//!
//! The control plane runs `N` container instances (`fabricd-shard-0..N-1`). A
//! lease's whole lifecycle (`acquire → §13-ingest → close`) MUST stay on the
//! instance that acquired it — the §13 `CaptureHook` is a live in-process object
//! (the JobClose ack state machine) that cannot move mid-lease. So every request
//! for lease `L` routes to `shard_of(L, N)`, and the acquiring instance mints a
//! lease-id whose shard is its OWN (rejection sampling) so the frozen
//! `lease-<uuid-v4>` wire shape is UNCHANGED.
//!
//! ## FROZEN CROSS-LANGUAGE CONTRACT
//!
//! The proxy Worker (`deploy/cloudflare-fabricd/src/shard.ts`) routes by the
//! SAME function. `shard_of` MUST be byte-for-byte identical on both sides or a
//! lease routes to the wrong instance and its close fails (hook-not-found). The
//! tripwire is `conformance/lease_shard.json` — committed byte-identical, tested
//! on both sides (`shard::tests` here + the Worker's vitest). Any divergence in
//! the hash, the modulo, or the byte encoding breaks a golden test on one side.
//!
//! The hash is **FNV-1a (32-bit)** over the UTF-8 bytes of the lease-id: trivial
//! to reproduce identically in Rust and TS, deterministic, well-distributed for
//! this use. It is NOT a security primitive (shard distribution only) — no crypto
//! strength is needed or claimed.
//!
//! ## N = 1 is inert
//!
//! With `num_shards == 1`, `shard_of` always returns 0 and the mint accepts the
//! first uuid — byte-identical to today's singleton. The routing change ships
//! OFF and is flipped by raising `N` in `wrangler.jsonc`.

/// FNV-1a 32-bit offset basis (`0x811c9dc5`).
const FNV_OFFSET_BASIS: u32 = 2_166_136_261;
/// FNV-1a 32-bit prime (`0x01000193`).
const FNV_PRIME: u32 = 16_777_619;

/// FNV-1a 32-bit hash over `bytes`. Wrapping multiply (mod 2^32) — matches the
/// TS `>>> 0` / `Math.imul` reference in `shard.ts`.
#[must_use]
pub fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut hash = FNV_OFFSET_BASIS;
    for &b in bytes {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// The shard a `lease_id` routes to under `num_shards` instances.
///
/// `num_shards == 0` is treated as 1 (never divide by zero; a misconfigured
/// `N = 0` degrades to the single-shard singleton rather than panicking).
#[must_use]
pub fn shard_of(lease_id: &str, num_shards: u32) -> u32 {
    let n = num_shards.max(1);
    fnv1a_32(lease_id.as_bytes()) % n
}

/// Mint a lease-id that routes to `target_shard` under `num_shards`, by
/// rejection-sampling `gen()` (expected ~`num_shards` tries). `gen` produces the
/// frozen `lease-<uuid-v4>` shape; this only SELECTS among freshly-minted ids, so
/// the wire shape is unchanged. `num_shards <= 1` accepts the first id (inert).
///
/// `max_tries` bounds the loop (defense against a broken `gen` that never varies)
/// — on exhaustion it returns the last id (fail-open to *a* valid lease-id;
/// mis-routing is caught by the reaper/close honesty path, never a hang).
pub fn mint_lease_id_for_shard(
    target_shard: u32,
    num_shards: u32,
    max_tries: u32,
    mut mint: impl FnMut() -> String,
) -> String {
    let n = num_shards.max(1);
    let target = target_shard % n;
    let mut last = mint();
    if n == 1 || shard_of(&last, n) == target {
        return last;
    }
    for _ in 1..max_tries.max(1) {
        let id = mint();
        if shard_of(&id, n) == target {
            return id;
        }
        last = id;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// One conformance case: `lease_id` under `num_shards` → `shard`.
    #[derive(Debug, Deserialize)]
    struct ShardCase {
        lease_id: String,
        num_shards: u32,
        shard: u32,
    }

    #[derive(Debug, Deserialize)]
    struct ShardVector {
        /// A few raw `fnv1a_32(lease_id)` values, so the Worker can also pin the
        /// hash itself (not just the modulo result).
        hashes: Vec<HashCase>,
        cases: Vec<ShardCase>,
    }

    #[derive(Debug, Deserialize)]
    struct HashCase {
        lease_id: String,
        fnv1a_32: u32,
    }

    fn vector() -> ShardVector {
        let raw = include_str!("../../../conformance/lease_shard.json");
        serde_json::from_str(raw).expect("lease_shard.json parses")
    }

    #[test]
    fn conformance_hashes_match() {
        for h in vector().hashes {
            assert_eq!(
                fnv1a_32(h.lease_id.as_bytes()),
                h.fnv1a_32,
                "fnv1a_32 mismatch for {:?} — the TS/Rust hash MUST agree",
                h.lease_id
            );
        }
    }

    #[test]
    fn conformance_shards_match() {
        for c in vector().cases {
            assert_eq!(
                shard_of(&c.lease_id, c.num_shards),
                c.shard,
                "shard_of({:?}, {}) mismatch — TS/Rust routing MUST agree",
                c.lease_id,
                c.num_shards
            );
        }
    }

    #[test]
    fn n1_is_inert_always_shard_zero() {
        for id in [
            "lease-abc",
            "lease-00000000-0000-4000-8000-000000000000",
            "x",
        ] {
            assert_eq!(shard_of(id, 1), 0, "N=1 must always map to shard 0");
        }
    }

    #[test]
    fn n0_degrades_to_single_shard_no_panic() {
        assert_eq!(shard_of("lease-anything", 0), 0);
    }

    #[test]
    fn mint_selects_target_shard() {
        // A deterministic generator cycling ids whose shards differ under N=3,
        // proving rejection sampling picks the target.
        let ids = [
            "lease-a".to_string(),
            "lease-b".to_string(),
            "lease-c".to_string(),
            "lease-d".to_string(),
        ];
        for target in 0..3u32 {
            let mut i = 0usize;
            let picked = mint_lease_id_for_shard(target, 3, 64, || {
                let id = ids[i % ids.len()].clone();
                i += 1;
                id
            });
            assert_eq!(
                shard_of(&picked, 3),
                target,
                "mint must return an id on the target shard"
            );
        }
    }

    #[test]
    fn mint_n1_accepts_first() {
        let mut calls = 0;
        let id = mint_lease_id_for_shard(0, 1, 64, || {
            calls += 1;
            "lease-first".to_string()
        });
        assert_eq!(id, "lease-first");
        assert_eq!(calls, 1, "N=1 must accept the first id (no rejection loop)");
    }

    #[test]
    fn mint_exhaustion_fails_open_to_last_id_never_hangs() {
        // A `gen` that NEVER produces the target shard: it always returns the
        // same id whose shard is fixed. With `max_tries` bounded, the loop must
        // terminate and fail-open to *a* valid lease-id (the last minted), never
        // hang and never panic. This is the defense against a broken generator.
        let fixed = "lease-fixed".to_string();
        let fixed_shard = shard_of(&fixed, 4);
        // Pick a target that is deliberately NOT the fixed id's shard.
        let target = (fixed_shard + 1) % 4;
        let mut calls = 0;
        let id = mint_lease_id_for_shard(target, 4, 8, || {
            calls += 1;
            fixed.clone()
        });
        assert_eq!(id, "lease-fixed", "exhaustion returns the last minted id");
        assert_eq!(
            calls, 8,
            "the loop is bounded by max_tries (1 initial + 7 more)"
        );
        // The returned id is a real lease-id even though it mis-routes; the
        // reaper/close honesty path catches the mis-route, never a hang.
        assert_ne!(shard_of(&id, 4), target);
    }

    #[test]
    fn mint_max_tries_zero_is_clamped_to_one_attempt() {
        // `max_tries.max(1)` — a caller passing 0 still mints exactly once.
        let mut calls = 0;
        let id = mint_lease_id_for_shard(2, 4, 0, || {
            calls += 1;
            "lease-only".to_string()
        });
        assert_eq!(id, "lease-only");
        assert_eq!(calls, 1, "max_tries=0 clamps to a single mint, no hang");
    }

    #[test]
    fn mint_target_shard_is_normalized_modulo_n() {
        // `target_shard % n` — an out-of-range target normalizes into [0, n).
        // target 5 under N=3 is shard 2; the minted id must route to shard 2.
        let ids = [
            "lease-a".to_string(),
            "lease-b".to_string(),
            "lease-c".to_string(),
            "lease-d".to_string(),
            "lease-e".to_string(),
            "lease-f".to_string(),
        ];
        let mut i = 0usize;
        let picked = mint_lease_id_for_shard(5, 3, 64, || {
            let id = ids[i % ids.len()].clone();
            i += 1;
            id
        });
        assert_eq!(
            shard_of(&picked, 3),
            5 % 3,
            "an out-of-range target normalizes to target % n"
        );
    }
}
