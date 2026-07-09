// Lease-id → shard routing (multi-instance fabricd, option 3).
//
// FROZEN CROSS-LANGUAGE CONTRACT with the Rust `crate::shard`
// (crates/corelink-fabric-server/src/shard.rs). `shardOf` MUST be byte-for-byte
// identical to `shard::shard_of` or a lease routes to the wrong container
// instance and its close fails (§13 hook-not-found). The tripwire is
// conformance/lease_shard.json — tested on BOTH sides (Rust `shard::tests` +
// the check in this file). Any divergence in the hash, the modulo, or the byte
// encoding breaks a golden test on one side.
//
// Hash = FNV-1a (32-bit) over the UTF-8 bytes of the lease-id. NOT a security
// primitive — shard distribution only.
//
// N = 1 is inert: shardOf always returns 0 → byte-identical to the singleton.

const FNV_OFFSET_BASIS = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

/** FNV-1a 32-bit over the UTF-8 bytes of `s`. Returns an unsigned 32-bit int. */
export function fnv1a32(s: string): number {
  let h = FNV_OFFSET_BASIS | 0; // 32-bit
  const bytes = new TextEncoder().encode(s);
  for (const b of bytes) {
    h ^= b;
    h = Math.imul(h, FNV_PRIME); // 32-bit wrapping multiply (== Rust wrapping_mul)
  }
  return h >>> 0; // to unsigned 32-bit (== Rust u32)
}

/**
 * The shard a `leaseId` routes to under `numShards` instances.
 * `numShards <= 0` degrades to a single shard (never divide by zero).
 */
export function shardOf(leaseId: string, numShards: number): number {
  const n = Math.max(1, Math.trunc(numShards) || 1);
  return fnv1a32(leaseId) % n;
}
