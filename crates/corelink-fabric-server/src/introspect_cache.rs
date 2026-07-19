//! Short-TTL, success-only introspect cache — blunt SEQUENTIAL same-token bursts.
//!
//! ## The gap it closes (the 2026-07-19 incident)
//!
//! The W2' single-flight coalescer collapses a CONCURRENT burst of same-token auths
//! into one introspect, but it retains NOTHING after the flight — so a SEQUENTIAL
//! burst (a tenant's CI firing job after job, or a validation sweep) still fires one
//! introspect round-trip PER request. A few hundred such requests brown out
//! corelink-server's `/internal/v1/auth/introspect`, the breaker trips OPEN, and the
//! fabric fail-closes ALL auth until the endpoint recovers. This cache remembers a
//! SUCCESSFUL token→tenant resolution for a short TTL, so a repeated PAT is served
//! WITHOUT an introspect — the dominant real-world load pattern (same tenant, many
//! jobs) stops hitting the upstream at all.
//!
//! ## Security — the fail-closed law is preserved
//!
//! - **SUCCESS-ONLY.** Only a 200-valid resolution (`Ok(Some((tenant, _)))`) is cached.
//!   A failure / unknown-token 401 / `Unreachable` is NEVER cached — an unanswerable
//!   or negative auth question is always re-asked, so a MISS during a brownout still
//!   fail-closes exactly as before.
//! - **SHORT TTL bounds revocation staleness.** A PAT revoked at corelink-server keeps
//!   authenticating here for at most `ttl`. This is the one trade-off — bounded, and
//!   the same pattern CoreLink Cache uses (interop §2).
//! - **DEFAULT-OFF.** `ttl == 0` ⇒ the cache is disabled: `get` always misses and `put`
//!   is a no-op, byte-identical to the pre-cache path. Arming (a non-zero TTL) is an
//!   owner decision (`FABRIC_INTROSPECT_CACHE_TTL_MS`).
//! - **KEYED BY BLAKE3(token).** The raw PAT never sits in the map (same discipline as
//!   the coalescer); the cached introspect body is kept out of `Debug`.
//! - **BOUNDED.** At most `max_entries`; a full map sweeps expired entries and, if still
//!   full, simply declines to cache (fail-safe: fall back to a real introspect) — it can
//!   never grow unbounded.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use corelink_fabric::TenantId;

/// Default max entries when armed. From `FABRIC_INTROSPECT_CACHE_MAX`.
pub const DEFAULT_INTROSPECT_CACHE_MAX: usize = 4096;

/// Injectable monotonic clock (tests drive TTL expiry without real sleeps).
type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

struct Entry {
    tenant: TenantId,
    /// The captured introspect 200 body (W4), so a cache hit ALSO serves the plan
    /// leg without a round-trip. `None` for backends that capture nothing.
    body: Option<Arc<str>>,
    expires_at: Instant,
}

/// A bounded, TTL, success-only token→tenant cache shared across requests.
pub struct IntrospectCache {
    /// `0` ⇒ disabled (default-off). Non-zero ⇒ armed with this TTL.
    ttl: Duration,
    max_entries: usize,
    map: Mutex<HashMap<[u8; 32], Entry>>,
    clock: Clock,
}

impl std::fmt::Debug for IntrospectCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render entries (they carry tenant identity + introspect bodies).
        f.debug_struct("IntrospectCache")
            .field("armed", &self.is_armed())
            .field("ttl_ms", &self.ttl.as_millis())
            .field("max_entries", &self.max_entries)
            .finish()
    }
}

fn key_of(token: &str) -> [u8; 32] {
    *blake3::hash(token.as_bytes()).as_bytes()
}

impl IntrospectCache {
    /// Build from explicit config. `ttl == 0` ⇒ disabled (default-off).
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self::with_clock(ttl, max_entries, Arc::new(Instant::now))
    }

    /// Build from env: `FABRIC_INTROSPECT_CACHE_TTL_MS` (default 0 = OFF) +
    /// `FABRIC_INTROSPECT_CACHE_MAX` (default 4096).
    pub fn from_env() -> Self {
        let ttl_ms = std::env::var("FABRIC_INTROSPECT_CACHE_TTL_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let max = std::env::var("FABRIC_INTROSPECT_CACHE_MAX")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_INTROSPECT_CACHE_MAX);
        Self::new(Duration::from_millis(ttl_ms), max)
    }

    pub(crate) fn with_clock(ttl: Duration, max_entries: usize, clock: Clock) -> Self {
        Self {
            ttl,
            max_entries: max_entries.max(1),
            map: Mutex::new(HashMap::new()),
            clock,
        }
    }

    /// Armed ⇔ a non-zero TTL.
    pub fn is_armed(&self) -> bool {
        !self.ttl.is_zero()
    }

    /// Look up a token. Returns `Some((tenant, body))` on a live (unexpired) hit,
    /// `None` on a miss / expiry / when disabled. An expired entry is evicted.
    pub fn get(&self, token: &str) -> Option<(TenantId, Option<Arc<str>>)> {
        if !self.is_armed() {
            return None;
        }
        let now = (self.clock)();
        let key = key_of(token);
        let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
        match map.get(&key) {
            Some(e) if e.expires_at > now => Some((e.tenant.clone(), e.body.clone())),
            Some(_) => {
                map.remove(&key); // expired — evict, force a fresh introspect
                None
            }
            None => None,
        }
    }

    /// Cache a SUCCESSFUL resolution. No-op when disabled. Never call this for a
    /// failure/401/Unreachable — only a 200-valid resolution belongs here.
    pub fn put(&self, token: &str, tenant: TenantId, body: Option<Arc<str>>) {
        if !self.is_armed() {
            return;
        }
        let now = (self.clock)();
        let expires_at = now + self.ttl;
        let key = key_of(token);
        let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= self.max_entries && !map.contains_key(&key) {
            // At capacity: reclaim expired entries first.
            map.retain(|_, e| e.expires_at > now);
            if map.len() >= self.max_entries {
                // Still full of LIVE entries — decline to cache (fall back to a real
                // introspect). Never grow past the bound.
                return;
            }
        }
        map.insert(
            key,
            Entry {
                tenant,
                body,
                expires_at,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tenant(s: &str) -> TenantId {
        TenantId::new(s.to_string()).expect("valid tenant id")
    }

    // A clock the test advances by hand.
    fn manual_clock() -> (Clock, Arc<AtomicU64>) {
        let base = Instant::now();
        let nanos = Arc::new(AtomicU64::new(0));
        let n2 = Arc::clone(&nanos);
        let clock: Clock = Arc::new(move || base + Duration::from_nanos(n2.load(Ordering::SeqCst)));
        (clock, nanos)
    }

    #[test]
    fn disabled_by_default_ttl_zero_never_caches() {
        let c = IntrospectCache::new(Duration::ZERO, 16);
        assert!(!c.is_armed());
        c.put("tok", tenant("t1"), None);
        assert!(c.get("tok").is_none(), "disabled cache must always miss");
    }

    #[test]
    fn armed_hit_then_expiry_miss() {
        let (clock, nanos) = manual_clock();
        let c = IntrospectCache::with_clock(Duration::from_millis(30), 16, clock);
        c.put("tok", tenant("t1"), Some(Arc::from("body")));
        let hit = c.get("tok").expect("live hit");
        assert_eq!(hit.0, tenant("t1"));
        assert_eq!(hit.1.as_deref(), Some("body"));
        // advance past the TTL → expired → miss (and evicted)
        nanos.store(
            Duration::from_millis(31).as_nanos() as u64,
            Ordering::SeqCst,
        );
        assert!(c.get("tok").is_none(), "expired entry must miss");
    }

    #[test]
    fn distinct_tokens_do_not_collide() {
        let c = IntrospectCache::new(Duration::from_secs(60), 16);
        c.put("a", tenant("ta"), None);
        c.put("b", tenant("tb"), None);
        assert_eq!(c.get("a").unwrap().0, tenant("ta"));
        assert_eq!(c.get("b").unwrap().0, tenant("tb"));
        assert!(c.get("c").is_none());
    }

    #[test]
    fn bounded_never_grows_past_max() {
        let c = IntrospectCache::new(Duration::from_secs(600), 2);
        c.put("a", tenant("t"), None);
        c.put("b", tenant("t"), None);
        c.put("c", tenant("t"), None); // at cap, all live → declined
        let map = c.map.lock().unwrap();
        assert!(map.len() <= 2, "must never exceed max_entries");
    }
}
