//! W2' introspect single-flight coalescer — collapse a CONCURRENT burst of
//! same-token introspect calls into ONE upstream round-trip, with ZERO staleness.
//!
//! ## Why single-flight, NOT a TTL cache (decided — do not relitigate)
//!
//! The amplification problem on the introspect seam is **concurrency**: a burst
//! of N acquires for the SAME tenant PAT fires N identical
//! `/internal/v1/auth/introspect` POSTs at once (auth `tenant_of` + plan
//! `plan_of_resolving`), each pinning a blocking-pool thread on the 2-vCPU
//! singleton. Single-flight solves that fully — the FIRST caller for a key runs
//! the one upstream call; the other N-1 AWAIT its published result — **without
//! retaining any result after the in-flight call completes**. A TTL cache would
//! additionally short-circuit *sequential* repeats, but only at a bounded
//! revocation/cap-staleness cost (a revoked PAT or a lowered cap would keep
//! admitting for the TTL window). That security cost is deliberately REJECTED:
//! coalescing carries no staleness because nothing is cached across flights.
//! (Suspend is a separate immediate durable gate, `is_tenant_suspended`,
//! unaffected either way.)
//!
//! ## Zero-staleness by construction
//!
//! The resolved value lives ONLY inside a `tokio::sync::watch` channel reachable
//! through an `Arc<Shared<T>>`. The coalescer map holds a **`Weak`** (non-owning)
//! handle, and a drop-guard removes the slot the instant the flight ends. Once
//! the leader and every follower of one flight drop their `Arc`, the channel and
//! its value are freed — **no result survives a flight**. A subsequent same-token
//! call finds no upgradeable entry and starts a fresh upstream round-trip
//! (proven by the `sequential_same_token_each_hits_upstream` test).
//!
//! ## Permit interaction (composes with W1's `introspect_gate`)
//!
//! The introspect admission gate (`FABRIC_INTROSPECT_MAX_INFLIGHT`) is acquired
//! INSIDE the leader closure — so **only the leader holds a permit** for the
//! upstream round-trip. Followers never run the closure, so they never take a
//! permit: a coalesced same-token burst of N uses **~ONE permit total** (strictly
//! better than W1 alone, which would shed the excess). Distinct tokens do not
//! coalesce, so the gate still bounds concurrent DISTINCT introspects exactly as
//! W1 intended.
//!
//! ## Error + panic propagation (fail-closed, never a hang)
//!
//! The leader publishes a single **cloneable outcome `T`** to every waiter. The
//! call sites fold every failure — an unreachable upstream, a shed, and a
//! panicked blocking task (a `JoinError`) — INTO `T` before publishing, so a
//! coalesced failure fails EVERY waiter closed. If the leader future is dropped
//! or unwinds BEFORE publishing (cancellation / a real panic), the `watch`
//! sender drops, every follower's `wait_for` observes the closed channel, and the
//! coalescer returns the caller-supplied `fail_closed` sentinel — so a follower
//! can never hang and never silently admit.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::watch;

/// The coalescing key: `BLAKE3(token)`. The raw PAT is NEVER stored — only its
/// 32-byte BLAKE3 digest (the same primitive `leases.rs` keys native CAS on).
type Key = [u8; 32];

/// Derive the coalescing key from a token WITHOUT retaining the raw PAT.
fn key_of(token: &str) -> Key {
    *blake3::hash(token.as_bytes()).as_bytes()
}

/// The shared in-flight slot for one key. Holds ONLY a `watch::Receiver` — the
/// value is published by the leader through the paired `Sender` and read by
/// followers via [`Shared::wait`]. Reachable only through an `Arc`; the coalescer
/// map holds a `Weak`, so the value is freed when the flight's last `Arc` drops.
struct Shared<T> {
    rx: watch::Receiver<Option<T>>,
}

impl<T: Clone> Shared<T> {
    /// Await the leader's published outcome. Returns `Some(T)` once the leader
    /// publishes; returns `None` iff the leader vanished (dropped/panicked)
    /// WITHOUT publishing — the `watch` sender dropped and the channel closed —
    /// so the caller substitutes its fail-closed sentinel (never a hang).
    async fn wait(&self) -> Option<T> {
        let mut rx = self.rx.clone();
        // `wait_for` checks the CURRENT value first (so a follower that joins
        // after the leader already published returns immediately, even if the
        // sender has since dropped) and otherwise awaits the next change; a
        // closed channel with no matching value yields `Err`.
        match rx.wait_for(|v| v.is_some()).await {
            Ok(guard) => guard.clone(),
            Err(_) => None,
        }
    }
}

/// Removes the map slot when a flight ends — on normal completion, cancellation,
/// or a leader panic — but ONLY if the slot still points at THIS flight's
/// `Shared` (a later leader for the same key may have replaced a stale `Weak`).
/// This keeps the map from accumulating dead `Weak`s and guarantees a same-token
/// call arriving after completion starts a FRESH flight (zero staleness).
struct SlotGuard<'a, T> {
    map: &'a Mutex<HashMap<Key, Weak<Shared<T>>>>,
    key: Key,
    shared: Weak<Shared<T>>,
}

impl<T> Drop for SlotGuard<'_, T> {
    fn drop(&mut self) {
        let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(current) = map.get(&self.key)
            && current.ptr_eq(&self.shared)
        {
            map.remove(&self.key);
        }
    }
}

/// An async single-flight coalescer keyed by `BLAKE3(token)`, generic over the
/// cloneable outcome `T`. One instance per offload site (auth + plan use separate
/// instances, so their distinct `T`s never collide and the two sequential legs of
/// one acquire correctly do NOT coalesce with each other).
///
/// The coalescing happens on the TOKIO side, BEFORE `spawn_blocking`: concurrent
/// tasks for the same key share ONE in-flight blocking call rather than each
/// entering the blocking pool.
pub(crate) struct SingleFlight<T> {
    map: Mutex<HashMap<Key, Weak<Shared<T>>>>,
}

impl<T: Clone> SingleFlight<T> {
    /// Construct an empty coalescer.
    pub(crate) fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Coalesce concurrent same-`token` calls into ONE `leader` execution.
    ///
    /// The first caller for `BLAKE3(token)` becomes the **leader**: it runs
    /// `leader` (which acquires the introspect permit and offloads the blocking
    /// introspect), then publishes the cloneable outcome to all waiters. Callers
    /// that arrive for the same key while the leader is in flight become
    /// **followers**: they await the published outcome WITHOUT running `leader`
    /// and WITHOUT taking a permit. Different tokens run independently.
    ///
    /// `fail_closed` is returned to any follower whose leader is dropped or
    /// unwinds before publishing — so a coalesced failure ALWAYS fails closed,
    /// never hangs, never admits.
    pub(crate) async fn run<F, Fut>(&self, token: &str, fail_closed: T, leader: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let key = key_of(token);

        // Decide leader-vs-follower atomically under the map lock. The std
        // `Mutex` guard is dropped at the end of THIS block — never held across
        // an `.await` (so the returned future stays `Send`).
        let leader_state = {
            let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
            match map.get(&key).and_then(Weak::upgrade) {
                // A live flight already exists for this key → follow it.
                Some(existing) => Err(existing),
                // No live flight → become the leader. Insert a `Weak`; the leader
                // holds the owning `Arc` (`shared`) so followers can upgrade it
                // for the duration of the upstream call.
                None => {
                    let (tx, rx) = watch::channel(None);
                    let shared = Arc::new(Shared { rx });
                    map.insert(key, Arc::downgrade(&shared));
                    Ok((shared, tx))
                }
            }
        };

        match leader_state {
            // ── Follower: await the leader's published outcome (no permit). ──
            Err(existing) => match existing.wait().await {
                Some(v) => v,
                // Leader vanished without publishing → fail closed.
                None => fail_closed,
            },
            // ── Leader: run the one upstream call, publish, remove the slot. ──
            Ok((shared, tx)) => {
                // The guard removes the slot on ANY exit (completion / cancel /
                // panic). On leader cancellation or a real unwind, `tx` also drops
                // → the channel closes → every follower's `wait` yields `None` →
                // fail-closed. No follower can hang.
                let _guard = SlotGuard {
                    map: &self.map,
                    key,
                    shared: Arc::downgrade(&shared),
                };
                let out = leader().await;
                // Publish to all waiters. `send` errors only if there are no
                // receivers left, which is fine — the leader still returns `out`.
                let _ = tx.send(Some(out.clone()));
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::SingleFlight;

    /// (a) N concurrent calls for the SAME token collapse to EXACTLY ONE leader
    /// execution; all N receive the same correct outcome.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn same_token_burst_runs_leader_exactly_once() {
        const N: usize = 32;
        let sf: Arc<SingleFlight<u64>> = Arc::new(SingleFlight::new());
        let calls = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for _ in 0..N {
            let sf = Arc::clone(&sf);
            let calls = Arc::clone(&calls);
            tasks.push(tokio::spawn(async move {
                sf.run("pat-acme", 0, move || async move {
                    // Count the upstream call; hold long enough that every
                    // follower is attached before the leader publishes.
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    777_u64
                })
                .await
            }));
        }
        for t in tasks {
            assert_eq!(
                t.await.unwrap(),
                777,
                "every waiter gets the leader's result"
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the same-token burst made EXACTLY ONE upstream call"
        );
    }

    /// (b) N concurrent DIFFERENT tokens each run their own leader — no false
    /// coalescing across distinct keys.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn distinct_tokens_run_independently() {
        const N: usize = 16;
        let sf: Arc<SingleFlight<u64>> = Arc::new(SingleFlight::new());
        let calls = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for i in 0..N {
            let sf = Arc::clone(&sf);
            let calls = Arc::clone(&calls);
            tasks.push(tokio::spawn(async move {
                let token = format!("pat-{i}");
                sf.run(&token, 0, move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    i as u64
                })
                .await
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            N,
            "distinct tokens must NOT coalesce (one upstream call each)"
        );
    }

    /// (c) The single in-flight leader ERRORS → ALL waiters get the fail-closed
    /// outcome the leader folded into `T`; none hangs, none silently admits.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn coalesced_error_fails_all_waiters_closed() {
        const N: usize = 24;
        // `T = Result<u64, ()>`; the leader returns `Err` (the unreachable-fold).
        let sf: Arc<SingleFlight<Result<u64, ()>>> = Arc::new(SingleFlight::new());
        let calls = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for _ in 0..N {
            let sf = Arc::clone(&sf);
            let calls = Arc::clone(&calls);
            tasks.push(tokio::spawn(async move {
                sf.run("pat-acme", Err(()), move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    Err(()) // upstream unreachable → fail-closed outcome
                })
                .await
            }));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap(), Err(()), "every waiter fails closed");
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "one upstream attempt, shared"
        );
    }

    /// (d) The leader PANICS (mid-flight, a real unwind) → every follower fails
    /// closed via the `fail_closed` sentinel; no deadlock, no hang.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn leader_panic_fails_all_waiters_closed() {
        const N: usize = 16;
        let sf: Arc<SingleFlight<&'static str>> = Arc::new(SingleFlight::new());

        // The leader is spawned first and PANICS after a beat, while followers
        // attach. Its panic unwinds the leader future → the watch sender drops →
        // followers observe the closed channel → fail-closed sentinel.
        let leader_sf = Arc::clone(&sf);
        let leader = tokio::spawn(async move {
            leader_sf
                .run("pat-acme", "fail-closed", || async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    panic!("leader blew up mid-flight");
                    #[allow(unreachable_code)]
                    "unreachable"
                })
                .await
        });

        // Give the leader time to insert its slot, then attach followers.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut followers = Vec::new();
        for _ in 0..N {
            let sf = Arc::clone(&sf);
            followers.push(tokio::spawn(async move {
                sf.run("pat-acme", "fail-closed", || async {
                    // A follower must NEVER run this; if coalescing failed and it
                    // became a leader it would still fail closed, but assert the
                    // followers got the sentinel below.
                    "leaked-leader"
                })
                .await
            }));
        }

        // The leader task itself panics (its JoinHandle is Err) — expected.
        assert!(
            leader.await.is_err(),
            "the leader task panicked as designed"
        );
        for f in followers {
            assert_eq!(
                f.await.unwrap(),
                "fail-closed",
                "every follower failed closed after the leader panic (no hang)"
            );
        }
    }

    /// (e) SEQUENTIAL (non-overlapping) same-token calls each hit upstream —
    /// proving ZERO cross-time caching (nothing is retained after a flight ends).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn sequential_same_token_each_hits_upstream() {
        const N: usize = 5;
        let sf: SingleFlight<u64> = SingleFlight::new();
        let calls = Arc::new(AtomicUsize::new(0));

        for _ in 0..N {
            let calls = Arc::clone(&calls);
            let out = sf
                .run("pat-acme", 0, move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    1_u64
                })
                .await;
            assert_eq!(out, 1);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            N,
            "each non-overlapping same-token call makes its OWN upstream call \
             (no result retained across flights)"
        );
        // The map slot is removed after each flight → no unbounded growth.
        assert!(
            sf.map.lock().unwrap().is_empty(),
            "the coalescer retains no slot after the last flight (zero staleness)"
        );
    }
}
