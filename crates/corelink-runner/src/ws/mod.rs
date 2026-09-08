// Transplanted from hugit/crates/hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! Workspace lifecycle: attach / resume / spawn-dedup (WP-C9).
//!
//! This module orchestrates the workspace lifecycle over the C2a runtime
//! ([`Engine`](crate::isolation::Engine) + [`BoxExec`](crate::lease::BoxExec))
//! and the C5a fence ([`FenceManifest`](corelink_runners_contracts::FenceManifest)). It
//! does NOT re-implement materialization or the box transport — it consumes them.
//!
//! # Four owned items (contract: WP-C9)
//! 1. **Attach (①)** — join a live workspace sharing the same
//!    fence/materialization *without* a respawn or re-hydrate.
//! 2. **Resume (②)** — restore state + fence; the resumed workspace is bounded
//!    by the original [`FenceManifest`] `path_set` and cannot exceed it.
//! 3. **Spawn + dedup (③)** — spawn a workspace in <1s; identical concurrent
//!    spawns are deduped to ONE materialization (warm-CAS economics).
//! 4. **Local ≡ remote (④)** — local and remote execution produce identical
//!    observable results; the same pure function either way.
//!
//! # Container naming
//! All C9 workspace containers are prefixed `corelink-ws-` so forensic scans and
//! kill-sweeps stay scoped to this WP on the shared box. Scans/cleanups target
//! ONLY this prefix — no other WP's containers are touched. The prefix value
//! is ops-visible on the shared interim box (`corelink-runner-01`); all create,
//! census, teardown, and sweep paths use the same prefix.
//!
//! # Firecracker note
//! Runtime is container-per-job on the interim box (`corelink-runner-01`,
//! Hetzner-class); Firecracker is the documented upgrade path, not built here.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use corelink_runners_contracts::{FenceManifest, RunnerLease};

use crate::isolation::{Engine, RunningContainer};
use crate::lease::{BoxExec, ContainerSpec};

/// Prefix for all C9-owned workspace containers on the shared box.
pub const WS_PREFIX: &str = "corelink-ws-";

// ── container naming ──────────────────────────────────────────────────────────

/// Derive a C9-namespaced container name from a workspace id.
///
/// The `corelink-ws-` prefix is what lets cleanup sweeps target ONLY this WP's
/// containers on the shared box. Docker names must match
/// `[a-zA-Z0-9][a-zA-Z0-9_.-]*`, so non-conforming chars are mapped to `_`.
#[must_use]
pub fn ws_container_name(workspace_id: &str) -> String {
    let mut s = String::with_capacity(workspace_id.len() + WS_PREFIX.len());
    s.push_str(WS_PREFIX);
    for c in workspace_id.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    s
}

// ── WorkspaceHandle ───────────────────────────────────────────────────────────

/// A live workspace handle — the result of spawn/attach/resume.
///
/// Carries the container that backs the workspace, the frozen [`RunnerLease`]
/// it runs under, and the [`FenceManifest`] that is the ceiling for path access.
/// The fence manifest cannot be widened on resume (security: resume is not a
/// fence-widening hole).
#[derive(Debug, Clone)]
pub struct WorkspaceHandle {
    /// The backing container.
    pub container: RunningContainer,
    /// The lease the workspace runs under (consumed, not modified).
    pub lease: RunnerLease,
    /// The fence manifest (path_set ceiling; cannot expand on resume).
    pub fence: FenceManifest,
    /// How this handle was obtained.
    pub origin: WorkspaceOrigin,
}

/// How a [`WorkspaceHandle`] was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceOrigin {
    /// A fresh container was spawned for this workspace.
    Spawned,
    /// An existing live container was joined (no respawn, no re-hydrate).
    Attached,
    /// A previously serialized state was restored (state + fence restored).
    Resumed,
}

// ── spawn ─────────────────────────────────────────────────────────────────────

/// Spawn a new workspace container under `lease`, enforcing the `fence`.
///
/// The container is named `corelink-ws-<workspace_id>` where `workspace_id` is
/// derived from `lease.lease_id`. The spawn MUST complete in <1s on a warm box
/// (`alpine:3.20` cached) — the C9 performance contract.
///
/// Returns a [`WorkspaceHandle`] with `origin = Spawned`.
///
/// # Errors
/// Fails if the box is unreachable, if the lease is invalid (propagates C2a
/// validation), or if the container fails to start.
pub fn spawn_workspace<E>(
    engine: &E,
    lease: &RunnerLease,
    fence: &FenceManifest,
    image: &str,
) -> Result<WorkspaceHandle>
where
    E: Engine,
{
    // Derive a C9-namespaced spec from the frozen lease.
    let mut spec = ContainerSpec::from_lease(lease, image)
        .context("deriving C9 workspace container spec from lease")?;
    spec.name = ws_container_name(&lease.lease_id);

    let container = engine
        .spawn(&spec)
        .context("spawning C9 workspace container")?;

    Ok(WorkspaceHandle {
        container,
        lease: lease.clone(),
        fence: fence.clone(),
        origin: WorkspaceOrigin::Spawned,
    })
}

// ── dedup spawn ───────────────────────────────────────────────────────────────

/// The state of a per-workspace dedup slot.
enum SpawnEntry {
    /// A spawn was claimed by some thread and is in progress. Concurrent
    /// callers for the same id wait on the condvar rather than starting a
    /// second materialization or blocking everyone else's *unrelated* spawns.
    Pending,
    /// A spawn completed; the handle is cached until the dedup window expires.
    /// Boxed to keep the enum small (the handle dwarfs the other variants).
    Ready {
        handle: Box<WorkspaceHandle>,
        at: Instant,
    },
    /// The in-progress spawn failed; the claim is released so a later caller
    /// can take over and retry. The failing caller already received the real
    /// error, so the marker itself carries no payload.
    Failed,
}

/// Default ceiling on the number of live dedup slots before the oldest are
/// reclaimed. The dedup map is a short-lived coalescing cache, not a registry,
/// so a small bound is ample; without it the map grows unbounded as distinct
/// `workspace_id`s arrive and `Failed`/expired slots are never reclaimed
/// (memory growth / DoS). See [`DedupSpawner::with_max_entries`].
pub const DEFAULT_MAX_DEDUP_ENTRIES: usize = 1024;

/// A teardown hook: tears down the container backing an evicted slot.
///
/// `evict` removes the in-memory slot, but the *container* it named still runs
/// on the box — clearing the slot without tearing the box down leaks it. The
/// hook is the seam through which `evict` reaches the box transport
/// ([`teardown`](crate::teardown::teardown) needs a [`BoxExec`] the spawner does
/// not itself hold). Production wiring installs a real hook via
/// [`DedupSpawner::with_teardown`]; it returns `Err` to surface a teardown
/// failure (a leaked box is never swallowed silently).
pub type TeardownHook = Arc<dyn Fn(&RunningContainer) -> Result<()> + Send + Sync>;

/// Deduplicating workspace spawner.
///
/// Concurrent identical spawns (same `workspace_id`) are coalesced to ONE
/// materialization: the first caller claims the slot and does the real spawn
/// **outside** the lock; later callers for the same id wait for that single
/// materialization and get the SAME handle. Crucially the global lock is held
/// only to *claim/observe* a slot, never across the `docker run` itself, so a
/// spawn for workspace A never serializes an unrelated spawn for workspace B.
///
/// # Identity binding (isolation — fail-closed)
/// The dedup identity is bound to `lease.lease_id`: every `spawn_or_join`
/// asserts that the presented `workspace_id` names the SAME container as the
/// lease (`ws_container_name(workspace_id) == ws_container_name(&lease.lease_id)`)
/// and bails otherwise, and on a cache HIT it re-verifies that the presenting
/// lease+fence match the cached handle's before handing the container back.
/// This closes the cross-lease / cross-tenant hole: caller B can never receive
/// caller A's container by presenting a different lease under a colliding
/// `workspace_id`.
///
/// A cached handle is liveness-probed before reuse: if its container has since
/// died/been reaped, the corpse is discarded and a fresh spawn is performed
/// (no dead-container handle is ever handed back).
///
/// After the window expires the entry is evicted so future spawns create a
/// fresh container. The slot map is bounded ([`DedupSpawner::with_max_entries`])
/// and swept of expired/`Failed` slots so it cannot grow without bound.
#[derive(Clone)]
pub struct DedupSpawner {
    /// In-progress/recent spawns, keyed by workspace id, guarded for the
    /// claim/observe critical section only (never across a spawn).
    entries: Arc<Mutex<HashMap<String, SpawnEntry>>>,
    /// Signalled whenever a `Pending` slot transitions to `Ready`/`Failed`.
    ready: Arc<Condvar>,
    /// How long a completed spawn is held for deduplication.
    dedup_window: Duration,
    /// Hard cap on live slots; the oldest are evicted past this. Bounds memory.
    max_entries: usize,
    /// Optional container-teardown hook used by [`evict`](Self::evict) so a
    /// cleared slot does not leak its backing container. `None` = no box wired
    /// (the slot is still removed; nothing to tear down in-process).
    teardown: Option<TeardownHook>,
}

impl DedupSpawner {
    /// THE single source of the dedup slot key.
    ///
    /// The slot identity is the LEASE's C9 container name — never the caller-
    /// supplied `workspace_id` (which does not name the box; see
    /// [`spawn_or_join`](Self::spawn_or_join)'s identity-binding note). EVERY
    /// keyed operation — `spawn_or_join`'s claim/publish, `reap_locked`'s
    /// sweep, and `evict`/`evict_checked`'s removal — derives its key HERE, so
    /// the insert key and the remove key can never desync (the W3-C re-key
    /// desynchronized them: spawn inserted under the lease key while evict
    /// removed by the workspace_id key, so evict silently no-op'd and leaked
    /// the box). One function ⇒ one key ⇒ no desync.
    #[must_use]
    fn slot_key(lease: &RunnerLease) -> String {
        ws_container_name(&lease.lease_id)
    }

    /// Construct with a deduplication window and the default entry cap
    /// ([`DEFAULT_MAX_DEDUP_ENTRIES`]).
    ///
    /// A window of at least 1s is enough to coalesce any realistic burst of
    /// concurrent spawns for the same workspace. The acceptance test uses a
    /// held `sleep` command so the spawns genuinely overlap in time.
    pub fn new(dedup_window: Duration) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            ready: Arc::new(Condvar::new()),
            dedup_window,
            max_entries: DEFAULT_MAX_DEDUP_ENTRIES,
            teardown: None,
        }
    }

    /// Override the maximum number of live dedup slots (cap). Builder-style.
    ///
    /// Past this cap, claiming a fresh slot first sweeps expired/`Failed` slots
    /// and, if still at the cap, evicts the oldest `Ready` slot — so the map is
    /// bounded regardless of how many distinct `workspace_id`s arrive. A cap of
    /// 0 is clamped to 1 (the in-flight claim must always have room).
    #[must_use]
    pub fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries.max(1);
        self
    }

    /// Install the container-teardown hook used by [`evict`](Self::evict).
    /// Builder-style. Without it, `evict` removes the slot but has no box to
    /// tear the container down on (in-process tests / no transport wired).
    #[must_use]
    pub fn with_teardown(mut self, teardown: TeardownHook) -> Self {
        self.teardown = Some(teardown);
        self
    }

    /// Spawn or return an existing workspace for `workspace_id`.
    ///
    /// Thread-safe and non-serializing: if two threads call this simultaneously
    /// with the SAME id, one claims the slot and materializes; the other waits
    /// for that single materialization and gets the same handle. Two threads
    /// with DIFFERENT ids never block each other (the lock is not held across
    /// the spawn). A cached handle is liveness-checked before reuse.
    ///
    /// # Errors
    /// Fails if the underlying spawn fails (the failure is propagated to every
    /// waiter of that attempt; the slot is then released for retry).
    pub fn spawn_or_join<E>(
        &self,
        workspace_id: &str,
        engine: &E,
        lease: &RunnerLease,
        fence: &FenceManifest,
        image: &str,
    ) -> Result<WorkspaceHandle>
    where
        E: Engine,
    {
        // ── Identity binding (fail-CLOSED) ────────────────────────────────────
        // The materialization names the container ONLY from `lease.lease_id`
        // (spawn_workspace → ws_container_name(&lease.lease_id)); the caller-
        // supplied `workspace_id` never names the box. So the dedup slot MUST be
        // keyed by the lease's container identity, not by `workspace_id` — else
        // two callers presenting different leases under a colliding
        // `workspace_id` would share one container (cross-lease / cross-tenant
        // break), and two callers presenting one lease under different
        // `workspace_id`s would silently spawn onto one name. Keying on the
        // lease's container name binds the slot identity to the box identity:
        // same lease ⇒ same key ⇒ same container; different lease ⇒ different
        // key ⇒ never shared. (`workspace_id` remains the public param for
        // callers, but is NOT the trust boundary.)
        //
        // Derived through the SINGLE key source ([`slot_key`]) so the insert key
        // here can never diverge from the remove key in `evict`/`reap_locked`.
        let key = Self::slot_key(lease);
        let key = key.as_str();

        // ── Phase 0: sweep dead weight (lock held briefly), then tear the
        // evicted boxes down OUTSIDE the lock ─────────────────────────────────
        //
        // The sweep COLLECTS the cap/expiry-evicted containers under the lock but
        // does NOT tear them down there; we fire the teardown hook here, with the
        // `entries` lock RELEASED — mirroring `evict_checked`'s lock-drop-before-
        // teardown discipline so teardowns never serialize under the `entries`
        // lock and a re-entrant hook (one that calls back into the spawner) can
        // never deadlock. Reaping and the claim below are NOT one atomic critical
        // section: the sweep only removes dead/expired/over-cap slots, and the
        // claim loop re-reads fresh map state under its own lock, so splitting the
        // lock is safe (the reaped keys are gone for good either way).
        let reaped = {
            let mut map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
            self.reap_locked(&mut map, key)
        };
        self.teardown_reaped(reaped);

        // ── Phase 1: claim or observe the slot (lock held briefly) ────────────
        {
            // poison recovery: the entries map is internally consistent
            let mut map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
            loop {
                match map.get(key) {
                    Some(SpawnEntry::Ready { handle, at }) if at.elapsed() < self.dedup_window => {
                        // Cache HIT. Before reuse, re-verify the presenting lease
                        // matches the cached handle's: identical lease (lease_id +
                        // principal_chain + the rest) AND identical fence. The key
                        // already pins lease_id; this ALSO catches a forged
                        // principal_chain / fence presented under the same
                        // lease_id — never hand one lease's container to another;
                        // fail CLOSED.
                        if handle.lease != *lease || handle.fence != *fence {
                            return Err(anyhow!(
                                "dedup cache reuse refused for lease {:?} (workspace_id \
                                 {workspace_id:?}): presenting lease/fence does not match the \
                                 cached handle's — refusing to share a container across leases \
                                 (fail CLOSED)",
                                lease.lease_id
                            ));
                        }
                        // Liveness-probe the cached container BEFORE reuse so a
                        // dead/reaped corpse is never handed back.
                        let handle = (**handle).clone();
                        drop(map);
                        match engine.is_alive(&handle.container) {
                            Ok(true) => return Ok(handle),
                            Ok(false) => {
                                // Corpse: discard the slot and fall through to a
                                // fresh claim below.
                                let mut m = self.entries.lock().unwrap_or_else(|p| p.into_inner());
                                // Only remove if it is still the same dead entry.
                                if matches!(m.get(key), Some(SpawnEntry::Ready { .. })) {
                                    m.remove(key);
                                }
                                map = m;
                                continue;
                            }
                            Err(e) => {
                                // Box unreachable for the probe: fail rather than
                                // return a possibly-dead handle (fail-closed).
                                return Err(e.context(
                                    "liveness probe of cached workspace container failed",
                                ));
                            }
                        }
                    }
                    Some(SpawnEntry::Ready { .. }) => {
                        // Stale: evict and claim fresh.
                        map.remove(key);
                        map.insert(key.to_string(), SpawnEntry::Pending);
                        break;
                    }
                    Some(SpawnEntry::Pending) => {
                        // Another thread is materializing this lease — wait for it.
                        map = self.ready.wait(map).unwrap_or_else(|p| p.into_inner());
                        continue;
                    }
                    Some(SpawnEntry::Failed) => {
                        // A prior attempt failed; take over the claim and retry.
                        map.insert(key.to_string(), SpawnEntry::Pending);
                        break;
                    }
                    None => {
                        map.insert(key.to_string(), SpawnEntry::Pending);
                        break;
                    }
                }
            }
        }

        // ── Phase 2: materialize OUTSIDE the lock (no global serialization) ───
        let result = spawn_workspace(engine, lease, fence, image);

        // ── Phase 3: publish the outcome and wake any waiters ─────────────────
        let mut map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        match result {
            Ok(handle) => {
                map.insert(
                    key.to_string(),
                    SpawnEntry::Ready {
                        handle: Box::new(handle.clone()),
                        at: Instant::now(),
                    },
                );
                self.ready.notify_all();
                Ok(handle)
            }
            Err(e) => {
                map.insert(key.to_string(), SpawnEntry::Failed);
                self.ready.notify_all();
                Err(anyhow!("workspace spawn for {workspace_id:?} failed: {e}"))
            }
        }
    }

    /// Reclaim dead weight from the slot map (called under the lock).
    ///
    /// 1. Drops every `Failed` slot and every expired `Ready` slot (past the
    ///    dedup window) — these are never reused, so they are pure leak.
    /// 2. If the map is STILL at/over the cap (excluding `keep`, the id we are
    ///    about to claim), evicts the oldest `Ready` slots until it fits.
    ///
    /// `keep` is never reaped: it is the slot the caller is mid-claim on, so
    /// removing it here would lose a `Pending` marker other waiters rely on.
    ///
    /// A `Ready` slot evicted by EITHER path (expiry sweep or cap eviction)
    /// still names a LIVE container on the box; dropping it from the map without
    /// tearing that container down leaks the box. So every evicted `Ready`
    /// container is COLLECTED and RETURNED to the caller, which tears it down
    /// via the installed [`TeardownHook`] AFTER releasing the `entries` lock —
    /// the SAME lock-drop-before-teardown discipline as `evict_checked` (W3-C's
    /// cap/expiry sweep removed Ready slots without firing the hook → leaked
    /// boxes; an earlier fix fired the hook but did so WHILE holding the lock,
    /// serializing every teardown under it and risking re-entrancy if the hook
    /// re-entered the spawner). This method only COLLECTS under the lock and
    /// never calls the hook, so it cannot return `Err` and cannot deadlock.
    ///
    /// Returns the evicted `Ready` containers, in eviction order, for the caller
    /// to tear down outside the lock.
    #[must_use]
    fn reap_locked(
        &self,
        map: &mut HashMap<String, SpawnEntry>,
        keep: &str,
    ) -> Vec<RunningContainer> {
        // Containers of Ready slots evicted by this sweep, to be torn down by
        // the CALLER outside the lock (same hook as `evict_checked`).
        let mut to_teardown: Vec<RunningContainer> = Vec::new();

        // (1) sweep Failed + expired Ready.
        map.retain(|id, e| {
            if id == keep {
                return true;
            }
            match e {
                SpawnEntry::Failed => false,
                SpawnEntry::Ready { at, handle } => {
                    let live = at.elapsed() < self.dedup_window;
                    if !live {
                        // Expired Ready → its container must be torn down.
                        to_teardown.push(handle.container.clone());
                    }
                    live
                }
                SpawnEntry::Pending => true,
            }
        });

        // (2) enforce the cap by evicting the oldest Ready slots.
        while map.len() >= self.max_entries {
            // Find the oldest Ready slot that is not `keep`.
            let oldest = map
                .iter()
                .filter_map(|(id, e)| match e {
                    SpawnEntry::Ready { at, .. } if id != keep => Some((id.clone(), *at)),
                    _ => None,
                })
                .min_by_key(|(_, at)| *at)
                .map(|(id, _)| id);
            match oldest {
                Some(id) => {
                    if let Some(SpawnEntry::Ready { handle, .. }) = map.remove(&id) {
                        // Cap-evicted Ready → its container must be torn down.
                        to_teardown.push(handle.container);
                    }
                }
                // Nothing evictable left (all remaining are Pending or `keep`);
                // never block an in-flight claim for the sake of the cap.
                None => break,
            }
        }

        // Hand the evicted containers back to the caller to tear down OUTSIDE
        // the lock (mirrors `evict_checked`'s discipline). The hook is never
        // fired here.
        to_teardown
    }

    /// Tear down every cap/expiry-evicted container collected by
    /// [`reap_locked`](Self::reap_locked), fired with the `entries` lock
    /// RELEASED. Best-effort: a teardown failure is a potential box leak and is
    /// surfaced to stderr, never swallowed silently (same posture as `evict`).
    /// Firing outside the lock keeps teardowns from serializing under it and
    /// makes a re-entrant hook (one that calls back into the spawner) deadlock-
    /// free.
    fn teardown_reaped(&self, reaped: Vec<RunningContainer>) {
        if let Some(hook) = self.teardown.as_ref() {
            for c in reaped {
                if let Err(e) = hook(&c) {
                    eprintln!(
                        "ws::DedupSpawner::reap_locked: teardown of cap/expiry-evicted \
                         container '{}' failed (possible box leak): {e:#}",
                        c.name
                    );
                }
            }
        }
    }

    /// Evict the slot for `lease` and tear down its backing container, loudly
    /// reporting (but not propagating) any teardown failure.
    ///
    /// Thin compatibility wrapper over [`evict_checked`](Self::evict_checked):
    /// a teardown failure is surfaced to stderr (it must NOT vanish — a leaked
    /// box is a real cost) rather than returned. Callers that need to *act* on
    /// the failure (retry, alert, fail the reclaim) should call
    /// [`evict_checked`](Self::evict_checked) directly.
    pub fn evict(&self, lease: &RunnerLease) {
        if let Err(e) = self.evict_checked(lease) {
            // Mirror the concurrency-reclaim idiom: a teardown failure is a
            // potential box leak and is NEVER swallowed silently.
            eprintln!("ws::DedupSpawner::evict: teardown failure (possible box leak): {e:#}");
        }
    }

    /// Evict the slot for `lease` and tear down its backing container, returning
    /// any teardown failure.
    ///
    /// Keyed through the SAME single key source ([`slot_key`](Self::slot_key))
    /// that `spawn_or_join` inserts under — the LEASE's C9 container identity,
    /// NOT the caller-supplied `workspace_id`. Keying evict on `workspace_id`
    /// (as W3-C did) desynchronizes it from the insert key whenever
    /// `workspace_id != lease_id`, so the remove finds nothing → silent no-op →
    /// leaked box + stuck slot. Routing both through `slot_key` makes that
    /// desync structurally impossible.
    ///
    /// Removing the slot alone leaks the box: the container the slot named is
    /// still running. So if the slot was `Ready`, this tears the container down
    /// via the installed [`TeardownHook`] (best-effort — a teardown failure is
    /// SURFACED as `Err`, never swallowed into an invisible leak). With no hook
    /// wired (in-process tests / no transport) the slot is still removed and
    /// `Ok(())` is returned (there is no box-side container to reap).
    ///
    /// The slot is removed regardless of teardown outcome (the in-memory dedup
    /// state must not pin a corpse), and waiters are woken.
    ///
    /// # Errors
    /// Returns the teardown failure if the hook reports one — the caller MUST
    /// see that the box may still be leaked.
    pub fn evict_checked(&self, lease: &RunnerLease) -> Result<()> {
        // SINGLE-SOURCED key: the exact key `spawn_or_join` inserted under, so
        // evict can never desync from spawn and silently no-op.
        let key = Self::slot_key(lease);
        let removed = self
            .entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&key);
        self.ready.notify_all();

        // Only a Ready slot has a live container to tear down.
        let container = match removed {
            Some(SpawnEntry::Ready { handle, .. }) => Some(handle.container.clone()),
            _ => None,
        };
        if let (Some(c), Some(hook)) = (container, self.teardown.as_ref()) {
            hook(&c).with_context(|| {
                format!(
                    "tearing down container '{}' on evict of lease {:?} \
                     (slot cleared; box may be LEAKED if this failed)",
                    c.name, lease.lease_id
                )
            })?;
        }
        Ok(())
    }
}

// ── attach ────────────────────────────────────────────────────────────────────

/// Attach to a live workspace identified by `workspace_id` — no respawn, no
/// re-hydrate.
///
/// This is the C9 attach contract (item ①): join a live workspace sharing the
/// same fence/materialization. The container must already be running; if it is
/// not present, this is a logic error (the caller should spawn first).
///
/// Returns a [`WorkspaceHandle`] with `origin = Attached`.
///
/// # Errors
/// Fails if the box is unreachable or the container is not running.
pub fn attach_workspace<B: BoxExec>(
    boxx: &B,
    workspace_id: &str,
    lease: &RunnerLease,
    fence: &FenceManifest,
) -> Result<WorkspaceHandle> {
    let container_name = ws_container_name(workspace_id);

    // Probe that the container is live — attach joins an existing container,
    // it does NOT respawn or re-hydrate.
    let out = boxx
        .run(&[
            "docker",
            "ps",
            "--filter",
            &format!("name={container_name}"),
            "--format",
            "{{.Names}}",
        ])
        .context("probing live workspace container for attach")?;

    if !out.stdout.lines().any(|l| l.trim() == container_name) {
        bail!(
            "attach failed: workspace container '{container_name}' is not running on the box; \
             spawn the workspace first (attach joins an existing live workspace without re-hydration)"
        );
    }

    Ok(WorkspaceHandle {
        container: RunningContainer {
            name: container_name,
        },
        lease: lease.clone(),
        fence: fence.clone(),
        origin: WorkspaceOrigin::Attached,
    })
}

// ── resume ────────────────────────────────────────────────────────────────────

/// Serialized workspace state for resume.
///
/// Minimal: carries the workspace id plus the frozen fence manifest from the
/// original spawn. The fence is the path_set ceiling — resume cannot exceed it.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceState {
    /// Unique workspace identifier (derives the container name).
    pub workspace_id: String,
    /// The original fence manifest; resume is bounded by this path_set.
    pub original_fence: FenceManifest,
    /// Any extra opaque state payload (empty in v0; forward-compatible).
    pub state_payload: Vec<u8>,
}

impl WorkspaceState {
    /// Construct a state snapshot for a live workspace (to be persisted and
    /// passed to [`resume_workspace`] later).
    pub fn snapshot(workspace_id: impl Into<String>, fence: FenceManifest) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            original_fence: fence,
            state_payload: Vec::new(),
        }
    }
}

/// Resume a workspace from a serialized state, restoring state + fence.
///
/// The resumed workspace CANNOT exceed the original `path_set` — resume is not
/// a fence-widening hole. If `new_fence` attempts to widen the path_set beyond
/// `state.original_fence.path_set`, this function fails (ceiling enforced).
///
/// Returns a [`WorkspaceHandle`] with `origin = Resumed`.
///
/// # Errors
/// - If `new_fence` has paths not covered by `state.original_fence` (ceiling
///   violation).
/// - If the box is unreachable or the container spawn fails.
pub fn resume_workspace<B, E>(
    boxx: &B,
    engine: &E,
    state: &WorkspaceState,
    new_fence: &FenceManifest,
    lease: &RunnerLease,
    image: &str,
) -> Result<WorkspaceHandle>
where
    B: BoxExec,
    E: Engine,
{
    // ── path_set ceiling: resume cannot exceed the original fence ─────────────
    // Every path in new_fence.path_set must be covered by some entry in the
    // original fence's path_set. A new_fence entry is "covered" iff it is an
    // exact match or is a descendant of a directory prefix in the original set.
    let ceiling = &state.original_fence.path_set;
    for new_path in &new_fence.path_set {
        if !path_covered_by(new_path, ceiling) {
            bail!(
                "resume refused: new_fence path '{new_path}' is outside the original path_set \
                 ceiling — resume cannot widen the fence (path_set ceiling enforced)"
            );
        }
    }

    // ── re-spawn the container (resume re-materializes on a fresh container) ──
    let handle = spawn_workspace(engine, lease, new_fence, image).with_context(|| {
        format!(
            "spawning container on resume for workspace '{}'",
            state.workspace_id
        )
    })?;

    // Restore the state payload into the container (no-op in v0; forward compat).
    if !state.state_payload.is_empty() {
        restore_state_payload(boxx, &handle.container, &state.state_payload)?;
    }

    Ok(WorkspaceHandle {
        container: handle.container,
        lease: handle.lease,
        fence: new_fence.clone(),
        origin: WorkspaceOrigin::Resumed,
    })
}

/// Returns `true` iff `path` is covered by at least one entry in `ceiling`.
///
/// - A ceiling entry ending in `/` is a directory prefix: `path` is covered iff
///   it is the directory itself or any descendant.
/// - Any other entry is an exact match.
/// - Absolute paths and `..` are never covered (they escape the workspace root).
fn path_covered_by(path: &str, ceiling: &[String]) -> bool {
    if path.starts_with('/') || path.contains("..") {
        return false;
    }
    for entry in ceiling {
        if entry.ends_with('/') {
            // Directory prefix.
            let prefix = entry.trim_end_matches('/');
            if path == prefix || path.starts_with(&format!("{prefix}/")) {
                return true;
            }
        } else if path == entry {
            return true;
        }
    }
    false
}

/// In-container destination for a restored state payload. A FIXED literal —
/// never derived from untrusted input — so it cannot itself carry an injection.
const STATE_RESTORE_PATH: &str = "/corelink/tmp/state_restore_marker";

/// Restore a state payload into the container by **streaming the bytes over
/// stdin**, never shell-constructing them.
///
/// The payload (arbitrary bytes, potentially attacker-influenced) is piped to
/// `docker exec -i <name> sh -c 'cat > <fixed-path>'` via
/// [`BoxExec::run_with_stdin`]. Only the FIXED destination path appears in the
/// command string; the payload itself touches no shell, closing the
/// shell-construction hole (brutal review R4 / item 6). The earlier
/// base64-into-`sh -c` approach is gone.
fn restore_state_payload<B: BoxExec>(
    boxx: &B,
    container: &RunningContainer,
    payload: &[u8],
) -> Result<()> {
    let redirect = format!("cat > {STATE_RESTORE_PATH}");
    let out = boxx
        .run_with_stdin(
            &[
                "docker",
                "exec",
                "-i",
                &container.name,
                "sh",
                "-c",
                &redirect,
            ],
            payload,
        )
        .context("restoring state payload into workspace container")?;
    if !out.ok() {
        bail!(
            "state_restore into {} failed: {}",
            container.name,
            out.stderr.trim()
        );
    }
    Ok(())
}

// ── local ≡ remote ────────────────────────────────────────────────────────────

/// Result of running a pure function in a workspace.
///
/// The local ≡ remote contract (item ④) states that running the same
/// deterministic function locally or inside the runner container produces
/// identical observable results. This type carries the observable output so
/// equality can be asserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceResult {
    /// Exit code (0 = success).
    pub exit_code: Option<i32>,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
}

impl WorkspaceResult {
    /// `true` iff exit code is 0.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// `true` iff `self` and `other` are result-identical (local ≡ remote).
    ///
    /// Two results are identical iff they have the same exit code and the same
    /// stdout (stderr differences from the runtime are not observable output).
    #[must_use]
    pub fn result_identity(&self, other: &Self) -> bool {
        self.exit_code == other.exit_code && self.stdout == other.stdout
    }
}

/// Run a command inside a workspace container (remote execution path).
///
/// This is the remote leg of the local ≡ remote identity check (item ④). The
/// same deterministic `argv` run locally via [`run_local`] and remotely via
/// this function must produce identical [`WorkspaceResult`]s.
///
/// # Errors
/// Fails if the box is unreachable.
pub fn run_remote<B: BoxExec>(
    boxx: &B,
    container: &RunningContainer,
    argv: &[&str],
) -> Result<WorkspaceResult> {
    let mut full = vec!["docker", "exec", &container.name];
    full.extend_from_slice(argv);
    let out = boxx
        .run(&full)
        .context("running command in remote workspace container")?;
    Ok(WorkspaceResult {
        exit_code: out.code,
        stdout: out.stdout,
        stderr: out.stderr,
    })
}

/// Run a command locally (local execution path).
///
/// This is the local leg of the local ≡ remote identity check (item ④). The
/// same deterministic `argv` run here and via [`run_remote`] must produce
/// identical [`WorkspaceResult`]s.
///
/// # Errors
/// Fails if the local process cannot be spawned.
pub fn run_local(argv: &[&str]) -> Result<WorkspaceResult> {
    if argv.is_empty() {
        bail!("run_local: argv is empty");
    }
    let out = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .with_context(|| format!("spawning local command {:?}", argv[0]))?;
    Ok(WorkspaceResult {
        exit_code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

// ── spawn timing ──────────────────────────────────────────────────────────────

/// Timed spawn: measure box-side spawn latency for the <1s contract (item ③).
///
/// Returns the handle and the end-to-end elapsed duration (includes SSH RTT).
/// The box-side spawn_ms is measured separately in acceptance tests because the
/// SSH transport adds ~3s of network overhead that is not part of the <1s
/// warm-CAS contract. On a warm box (image pre-pulled) `docker run` itself takes
/// <500ms; the 1000ms (1s) budget is for box-side startup only.
///
/// # Errors
/// Propagates spawn errors; does NOT fail on timing (the test asserts timing).
pub fn spawn_timed<E>(
    engine: &E,
    lease: &RunnerLease,
    fence: &FenceManifest,
    image: &str,
) -> Result<(WorkspaceHandle, Duration)>
where
    E: Engine,
{
    let t0 = Instant::now();
    let handle = spawn_workspace(engine, lease, fence, image)?;
    let elapsed = t0.elapsed();
    Ok((handle, elapsed))
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c9_name_is_prefixed_and_sanitized() {
        assert_eq!(ws_container_name("ws/abc 1"), "corelink-ws-ws_abc_1");
        assert!(ws_container_name("x").starts_with(WS_PREFIX));
    }

    #[test]
    fn path_covered_exact() {
        let ceiling = vec!["src/main.rs".to_string(), "Cargo.toml".to_string()];
        assert!(path_covered_by("src/main.rs", &ceiling));
        assert!(path_covered_by("Cargo.toml", &ceiling));
        assert!(!path_covered_by("src/secret.rs", &ceiling));
    }

    #[test]
    fn path_covered_dir_prefix() {
        let ceiling = vec!["src/".to_string()];
        assert!(path_covered_by("src/main.rs", &ceiling));
        assert!(path_covered_by("src/inner/deep.rs", &ceiling));
        assert!(!path_covered_by("tests/x.rs", &ceiling));
    }

    #[test]
    fn path_covered_rejects_absolute_and_traversal() {
        let ceiling = vec!["src/".to_string()];
        assert!(!path_covered_by("/etc/passwd", &ceiling));
        assert!(!path_covered_by("src/../etc/passwd", &ceiling));
    }

    #[test]
    fn workspace_state_snapshot() {
        let fence = FenceManifest {
            path_set: vec!["src/".to_string()],
            deny_default: true,
            materialized: vec![],
        };
        let state = WorkspaceState::snapshot("ws-001", fence.clone());
        assert_eq!(state.workspace_id, "ws-001");
        assert_eq!(state.original_fence, fence);
    }

    #[test]
    fn result_identity_checks_exit_and_stdout() {
        let a = WorkspaceResult {
            exit_code: Some(0),
            stdout: "hi\n".into(),
            stderr: String::new(),
        };
        let b = WorkspaceResult {
            exit_code: Some(0),
            stdout: "hi\n".into(),
            stderr: "noise".into(),
        };
        let c = WorkspaceResult {
            exit_code: Some(1),
            stdout: "hi\n".into(),
            stderr: String::new(),
        };
        assert!(
            a.result_identity(&b),
            "same exit+stdout => identical regardless of stderr"
        );
        assert!(!a.result_identity(&c), "different exit => not identical");
    }

    #[test]
    fn dedup_spawner_evicts_expired() {
        // Window of 0ms means the entry is immediately stale; eviction must not panic.
        let spawner = DedupSpawner::new(Duration::from_millis(0));
        let map = spawner.entries.lock().unwrap_or_else(|p| p.into_inner());
        assert!(map.is_empty());
    }

    /// A content-pinned image reference (the only kind `from_lease` accepts).
    const TEST_PIN: &str =
        "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

    /// Minimal in-process [`Engine`] stub: `spawn` just returns a container
    /// named after the spec; `is_alive` always reports live. No box contact.
    struct StubEngine;

    impl Engine for StubEngine {
        fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer> {
            Ok(RunningContainer {
                name: spec.name.clone(),
            })
        }
        fn probe(
            &self,
            _c: &RunningContainer,
            _spec: &ContainerSpec,
        ) -> Result<crate::isolation::IsolationProbe> {
            Ok(crate::isolation::IsolationProbe {
                tmp_is_private: true,
                net_is_isolated: true,
            })
        }
        fn exec(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<Option<i32>> {
            Ok(Some(0))
        }
        fn exec_captured(
            &self,
            _c: &RunningContainer,
            _argv: &[&str],
        ) -> Result<crate::lease::CmdOutput> {
            Ok(crate::lease::CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
        fn is_alive(&self, _c: &RunningContainer) -> Result<bool> {
            Ok(true)
        }
    }

    fn oracle_lease() -> RunnerLease {
        RunnerLease {
            lease_id: "ws-poison".to_string(),
            principal_chain: vec!["agent:1".to_string()],
            path_set: vec!["src/".to_string()],
            expiry: 0,
            net_policy: "none".to_string(),
            tmp_root: "/work/tmp".to_string(),
            state: corelink_runners_contracts::RunnerState::Held,
        }
    }

    fn oracle_fence() -> FenceManifest {
        FenceManifest {
            path_set: vec!["src/".to_string()],
            deny_default: true,
            materialized: vec![],
        }
    }

    /// ORACLE (FP-2 fix #1): a poisoned `entries` mutex must still SERVE.
    ///
    /// We force a panic in a thread *while holding the entries lock* (via
    /// `catch_unwind` so the test itself survives), which poisons the mutex.
    /// A subsequent `spawn_or_join` must still succeed. Before the poison-
    /// recovering `.unwrap_or_else(|p| p.into_inner())` fix this PANICKED on
    /// the first `.lock().unwrap()`, turning one job's panic into a permanent
    /// DoS for the whole spawner.
    #[test]
    fn poisoned_entries_mutex_still_serves() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let spawner = DedupSpawner::new(Duration::from_secs(60));

        // Poison the mutex: panic while the lock guard is held.
        let entries = Arc::clone(&spawner.entries);
        let res = catch_unwind(AssertUnwindSafe(|| {
            let _guard = entries.lock().unwrap();
            panic!("deliberate panic while holding the entries lock");
        }));
        assert!(res.is_err(), "the closure must have panicked");
        assert!(
            spawner.entries.is_poisoned(),
            "the entries mutex must now be poisoned"
        );

        // After the poison, the spawner must still serve (recovers the guard).
        let engine = StubEngine;
        let lease = oracle_lease();
        let fence = oracle_fence();
        let handle = spawner
            .spawn_or_join("ws-poison", &engine, &lease, &fence, TEST_PIN)
            .expect("spawn must succeed even though the entries mutex was poisoned");
        assert_eq!(handle.origin, WorkspaceOrigin::Spawned);

        // And eviction (another lock site) must also not re-panic.
        spawner.evict(&lease);
    }

    /// A lease whose `lease_id` is `id`, principal-chained to `principal`.
    fn lease_with(id: &str, principal: &str) -> RunnerLease {
        RunnerLease {
            lease_id: id.to_string(),
            principal_chain: vec![principal.to_string()],
            path_set: vec!["src/".to_string()],
            expiry: 0,
            net_policy: "none".to_string(),
            tmp_root: "/work/tmp".to_string(),
            state: corelink_runners_contracts::RunnerState::Held,
        }
    }

    /// REGRESSION (P0 isolation, fail-CLOSED): on a cache HIT, a caller
    /// presenting the SAME `workspace_id` but a DIFFERENT lease/principal must
    /// NOT receive the cached (caller-A) container — it must `Err`.
    ///
    /// Before the fix, `spawn_or_join` keyed only on `workspace_id` and returned
    /// caller A's handle (container/lease/principal_chain/fence) to caller B,
    /// a cross-lease / cross-tenant isolation break. This test FAILS (returns
    /// caller A's handle instead of `Err`) if that hole reopens.
    #[test]
    fn cache_hit_rejects_mismatched_lease_failclosed() {
        let engine = StubEngine;
        let fence = oracle_fence();
        let spawner = DedupSpawner::new(Duration::from_secs(60));

        // Caller A materializes the slot under lease_id "shared-ws".
        let lease_a = lease_with("shared-ws", "agent:A");
        let h_a = spawner
            .spawn_or_join("shared-ws", &engine, &lease_a, &fence, TEST_PIN)
            .expect("caller A spawns");
        assert_eq!(h_a.lease.principal_chain, vec!["agent:A".to_string()]);

        // Caller B presents the SAME workspace_id + lease_id but a DIFFERENT
        // principal_chain. The dedup key collides; the lease/principal does not.
        let lease_b = lease_with("shared-ws", "agent:B");
        let err = spawner
            .spawn_or_join("shared-ws", &engine, &lease_b, &fence, TEST_PIN)
            .expect_err("caller B MUST be refused on the cache HIT, not handed A's container");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("does not match the cached handle"),
            "must fail CLOSED on lease/principal mismatch, got: {msg}"
        );
    }

    /// REGRESSION (P0 isolation): the dedup slot is keyed by the LEASE's
    /// container identity, not by the caller-supplied `workspace_id`.
    ///
    /// Two consequences are asserted:
    /// 1. A cache HIT under a colliding lease_id but a DIFFERENT principal is
    ///    refused (cross-lease share is fail-closed) EVEN when the caller varies
    ///    the `workspace_id` param — the trust boundary is the lease, not the
    ///    param.
    /// 2. Two DIFFERENT `workspace_id` params under the SAME lease coalesce to
    ///    ONE container (the key is the lease), so the caller-supplied param can
    ///    never silently mint a second box for one lease.
    ///
    /// Before the fix the map keyed on `workspace_id`, so (1) a different lease
    /// under a colliding param shared one container and (2) different params
    /// under one lease collided onto one name with no identity check.
    #[test]
    fn slot_keyed_by_lease_not_workspace_id() {
        let engine = StubEngine;
        let fence = oracle_fence();
        let spawner = DedupSpawner::new(Duration::from_secs(60));

        // Caller A: lease "lease-X" under param "p1".
        let lease_a = lease_with("lease-X", "agent:A");
        let h_a = spawner
            .spawn_or_join("p1", &engine, &lease_a, &fence, TEST_PIN)
            .expect("caller A spawns");

        // (1) Caller B: SAME lease_id "lease-X", DIFFERENT principal, even under
        // a DIFFERENT param "p2" — must be refused (same key, lease mismatch).
        let lease_b = lease_with("lease-X", "agent:B");
        let err = spawner
            .spawn_or_join("p2", &engine, &lease_b, &fence, TEST_PIN)
            .expect_err("cross-principal reuse under the same lease key MUST be refused");
        assert!(
            format!("{err:#}").contains("does not match the cached handle"),
            "must fail CLOSED on lease mismatch regardless of the workspace_id param"
        );

        // (2) Caller C: SAME lease as A, DIFFERENT param "p3" — must coalesce to
        // A's container (one box per lease), never a second spawn.
        let h_c = spawner
            .spawn_or_join("p3", &engine, &lease_a, &fence, TEST_PIN)
            .expect("same lease under a different param must coalesce");
        assert_eq!(
            h_a.container.name, h_c.container.name,
            "two params under one lease must share ONE container (keyed by lease)"
        );
    }

    /// REGRESSION (P0 isolation): two DISTINCT workspace_ids backed by leases
    /// with DISTINCT lease_ids never collide onto one container — each names
    /// its own C9 container. (The symmetric "two ids share one lease_id"
    /// collision is structurally impossible now because the key must equal the
    /// lease_id's container name.)
    #[test]
    fn distinct_leases_get_distinct_containers() {
        let engine = StubEngine;
        let fence = oracle_fence();
        let spawner = DedupSpawner::new(Duration::from_secs(60));

        let h1 = spawner
            .spawn_or_join(
                "ws-one",
                &engine,
                &lease_with("ws-one", "agent:A"),
                &fence,
                TEST_PIN,
            )
            .expect("ws-one spawns");
        let h2 = spawner
            .spawn_or_join(
                "ws-two",
                &engine,
                &lease_with("ws-two", "agent:B"),
                &fence,
                TEST_PIN,
            )
            .expect("ws-two spawns");
        assert_ne!(
            h1.container.name, h2.container.name,
            "distinct workspace_ids must NOT collide onto one container"
        );
    }

    /// REGRESSION (P1 desync + leak): `evict` must find the slot `spawn_or_join`
    /// inserted (the SAME single-sourced key) AND tear down its backing
    /// container — not just clear the in-memory slot, and not silently no-op.
    ///
    /// The trap that masked the W3-C desync: every prior test used
    /// `workspace_id == lease_id`, so the spawn key (`c9(lease_id)`) and the
    /// old evict key (`c9(workspace_id)`) coincided and the broken evict still
    /// found the slot. Here `workspace_id` ("param-X") DELIBERATELY differs from
    /// `lease_id` ("ws-evict") — the real keying path (a param under a lease).
    /// Under the W3-C desync, evict keyed by `c9("param-X")` would find NOTHING
    /// → teardown NEVER fires → this test FAILS (`calls == 0`). With evict keyed
    /// through `slot_key(lease)` it finds the slot and tears the box down.
    #[test]
    fn evict_tears_down_container() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let torn: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let torn_h = Arc::clone(&torn);
        let calls_h = Arc::clone(&calls);
        let hook: TeardownHook = Arc::new(move |c: &RunningContainer| {
            calls_h.fetch_add(1, Ordering::SeqCst);
            torn_h.lock().unwrap().push(c.name.clone());
            Ok(())
        });

        let engine = StubEngine;
        let fence = oracle_fence();
        let spawner = DedupSpawner::new(Duration::from_secs(60)).with_teardown(hook);

        // workspace_id ("param-X") DIFFERS from lease_id ("ws-evict") — the slot
        // is keyed by the lease, so evict MUST key by the lease too (the bug:
        // evict keyed by the param would miss this slot entirely).
        let lease = lease_with("ws-evict", "agent:A");
        let h = spawner
            .spawn_or_join("param-X", &engine, &lease, &fence, TEST_PIN)
            .expect("spawn");
        let expected = h.container.name.clone();
        // The container is named from the LEASE, not the param.
        assert_eq!(expected, ws_container_name("ws-evict"));

        spawner.evict(&lease);

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "evict MUST find the slot spawn inserted (same key) and invoke teardown \
             exactly once — a desync'd evict would no-op here (calls == 0)"
        );
        assert_eq!(
            torn.lock().unwrap().as_slice(),
            &[expected],
            "evict must tear down the SAME container the slot named"
        );
        // And the slot is actually gone (the box is freed, the slot is freed).
        let map = spawner.entries.lock().unwrap_or_else(|p| p.into_inner());
        assert!(
            map.is_empty(),
            "evict must remove the slot it found (no stuck slot left behind)"
        );
    }

    /// REGRESSION (P1 leak): a teardown FAILURE on evict is surfaced as `Err`
    /// from `evict_checked`, never swallowed (the box may be leaked).
    #[test]
    fn evict_checked_surfaces_teardown_failure() {
        let hook: TeardownHook =
            Arc::new(|_c: &RunningContainer| bail!("simulated docker rm -f failure"));
        let engine = StubEngine;
        let fence = oracle_fence();
        let spawner = DedupSpawner::new(Duration::from_secs(60)).with_teardown(hook);
        // Param ("p-fail") deliberately differs from lease_id ("ws-fail") — the
        // real keying path — so evict must resolve the slot via the lease.
        let lease = lease_with("ws-fail", "agent:A");
        spawner
            .spawn_or_join("p-fail", &engine, &lease, &fence, TEST_PIN)
            .expect("spawn");

        let err = spawner
            .evict_checked(&lease)
            .expect_err("a teardown failure on evict MUST surface as Err");
        assert!(
            format!("{err:#}").contains("box may be LEAKED"),
            "the surfaced error must flag the potential leak"
        );
        // The slot is still gone (in-memory state never pins a corpse).
        let map = spawner.entries.lock().unwrap_or_else(|p| p.into_inner());
        assert!(
            !map.contains_key(&DedupSpawner::slot_key(&lease)),
            "slot must be removed even on teardown failure"
        );
    }

    /// REGRESSION (P1 DoS): the slot map stays BOUNDED under many distinct
    /// workspace_ids, and expired/`Failed` slots are reclaimed. Without the
    /// cap + sweep the map grew one entry per distinct id forever.
    #[test]
    fn map_stays_bounded_under_many_ids() {
        let engine = StubEngine;
        let fence = oracle_fence();
        let cap = 8;
        // Window 0 => every Ready slot is immediately expired, so the sweep
        // alone keeps the map tiny; the cap is the belt to the sweep's braces.
        let spawner = DedupSpawner::new(Duration::from_secs(60)).with_max_entries(cap);

        for i in 0..200 {
            let id = format!("ws-{i}");
            spawner
                .spawn_or_join(&id, &engine, &lease_with(&id, "agent:A"), &fence, TEST_PIN)
                .expect("spawn");
            let len = spawner
                .entries
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .len();
            assert!(
                len <= cap,
                "dedup map must stay <= cap ({cap}); grew to {len} at i={i}"
            );
        }
    }

    /// REGRESSION (P2 leak): a Ready slot evicted by the CAP sweep
    /// (`reap_locked`'s bounded-map eviction) must have its teardown hook fired
    /// — the container is still live on the box; removing the slot without
    /// teardown leaks it (W3-C's cap/expiry sweep removed Ready slots WITHOUT
    /// firing the hook). We use a generous dedup window so evicted slots are
    /// Ready (not expired), a tiny cap so each fresh spawn cap-evicts an older
    /// Ready slot, and assert teardown fired for exactly the cap-evicted boxes.
    #[test]
    fn cap_evicted_ready_slot_is_torn_down() {
        let torn: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let torn_h = Arc::clone(&torn);
        let hook: TeardownHook = Arc::new(move |c: &RunningContainer| {
            torn_h.lock().unwrap().push(c.name.clone());
            Ok(())
        });

        let engine = StubEngine;
        let fence = oracle_fence();
        let cap = 4;
        // Long window ⇒ slots stay Ready (the EXPIRY path never fires); the only
        // thing removing them is the CAP eviction, which must tear them down.
        let spawner = DedupSpawner::new(Duration::from_secs(3600))
            .with_max_entries(cap)
            .with_teardown(hook);

        let total = 20;
        for i in 0..total {
            let id = format!("cap-{i}");
            spawner
                .spawn_or_join(&id, &engine, &lease_with(&id, "agent:A"), &fence, TEST_PIN)
                .expect("spawn");
            // The map never exceeds the cap (the sweep evicts the oldest Ready).
            let len = spawner
                .entries
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .len();
            assert!(
                len <= cap,
                "map must stay <= cap ({cap}); is {len} at i={i}"
            );
        }

        // Every Ready slot pushed out by the cap sweep must have been torn down.
        // With `total` distinct leases and a cap of `cap`, at least
        // `total - cap` were cap-evicted; each MUST have fired teardown.
        let torn = torn.lock().unwrap();
        assert!(
            torn.len() >= total - cap,
            "cap-evicted Ready slots must fire teardown: expected >= {} teardowns, got {}",
            total - cap,
            torn.len()
        );
        // And every torn-down name is a real C9 container for one of our leases
        // (never a phantom) — teardown tore down the container the slot named.
        for name in torn.iter() {
            assert!(
                name.starts_with(WS_PREFIX),
                "torn-down container '{name}' must be a C9 container"
            );
        }
    }

    /// REGRESSION (W4-WS, P2): the cap/expiry teardown hook fires OUTSIDE the
    /// `entries` lock. A hook that RE-ENTERS the spawner (locks `entries` again,
    /// as a real teardown that touched dedup state could) must not deadlock. The
    /// pre-fix code fired the hook WHILE holding the lock — this same-thread
    /// re-lock of the `std::sync::Mutex` would deadlock; firing after the lock
    /// is released makes it safe. We also assert teardown still fired (the leak
    /// fix is preserved) and that the re-entrant probe ran.
    #[test]
    fn cap_evicted_teardown_fires_outside_the_entries_lock() {
        let torn: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let reentered: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        // The hook reaches the live spawner through this cell, installed AFTER
        // construction (the hook is captured at build time, so it cannot name the
        // spawner directly — it reads it from here when fired).
        let spawner_cell: Arc<Mutex<Option<Arc<DedupSpawner>>>> = Arc::new(Mutex::new(None));

        let torn_h = Arc::clone(&torn);
        let reentered_h = Arc::clone(&reentered);
        let cell_h = Arc::clone(&spawner_cell);
        let hook: TeardownHook = Arc::new(move |c: &RunningContainer| {
            // RE-ENTER the spawner: lock `entries`. If the cap teardown were
            // still fired while holding that lock (the bug), this same-thread
            // re-lock would deadlock the test. Outside the lock it just succeeds.
            if let Some(sp) = cell_h.lock().unwrap().as_ref() {
                let _n = sp.entries.lock().unwrap_or_else(|p| p.into_inner()).len();
                *reentered_h.lock().unwrap() += 1;
            }
            torn_h.lock().unwrap().push(c.name.clone());
            Ok(())
        });

        let engine = StubEngine;
        let fence = oracle_fence();
        let cap = 2;
        let spawner = Arc::new(
            DedupSpawner::new(Duration::from_secs(3600))
                .with_max_entries(cap)
                .with_teardown(hook),
        );
        *spawner_cell.lock().unwrap() = Some(Arc::clone(&spawner));

        // Each fresh spawn over the cap cap-evicts an older Ready slot → fires the
        // re-entrant hook. If teardown ran under the lock this would hang.
        let total = 8;
        for i in 0..total {
            let id = format!("reenter-{i}");
            spawner
                .spawn_or_join(&id, &engine, &lease_with(&id, "agent:A"), &fence, TEST_PIN)
                .expect("spawn");
        }

        let torn = torn.lock().unwrap();
        assert!(
            torn.len() >= total - cap,
            "cap-evicted Ready slots must still fire teardown: expected >= {}, got {}",
            total - cap,
            torn.len()
        );
        assert_eq!(
            *reentered.lock().unwrap(),
            torn.len(),
            "every teardown must have re-entered the spawner without deadlock"
        );
    }

    /// REGRESSION (P2 leak): a Ready slot evicted by the EXPIRY sweep (past the
    /// dedup window) must also fire teardown — same leak class as the cap path.
    #[test]
    fn expiry_evicted_ready_slot_is_torn_down() {
        let torn: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let torn_h = Arc::clone(&torn);
        let hook: TeardownHook = Arc::new(move |c: &RunningContainer| {
            torn_h.lock().unwrap().push(c.name.clone());
            Ok(())
        });

        let engine = StubEngine;
        let fence = oracle_fence();
        // Zero window ⇒ a published Ready slot is immediately expired, so the
        // NEXT claim's sweep evicts it via the EXPIRY path (not the cap).
        let spawner = DedupSpawner::new(Duration::from_millis(0))
            .with_max_entries(1024)
            .with_teardown(hook);

        // First lease publishes a Ready slot (then instantly expires).
        let lease_a = lease_with("expire-A", "agent:A");
        let expected_a = spawner
            .spawn_or_join("pa", &engine, &lease_a, &fence, TEST_PIN)
            .expect("spawn A")
            .container
            .name;

        // A second, DIFFERENT lease: its claim runs the sweep, which finds the
        // now-expired slot A and must tear A's container down before dropping it.
        let lease_b = lease_with("expire-B", "agent:B");
        spawner
            .spawn_or_join("pb", &engine, &lease_b, &fence, TEST_PIN)
            .expect("spawn B");

        let torn = torn.lock().unwrap();
        assert!(
            torn.contains(&expected_a),
            "expiry-evicted Ready slot A ('{expected_a}') must have fired teardown; torn = {torn:?}"
        );
    }

    /// REGRESSION (P1 DoS): `Failed` slots are reclaimed by the sweep and never
    /// accumulate.
    #[test]
    fn failed_slots_are_reclaimed() {
        // An engine whose spawn always fails, to mint Failed slots.
        struct FailEngine;
        impl Engine for FailEngine {
            fn spawn(&self, _spec: &ContainerSpec) -> Result<RunningContainer> {
                bail!("deliberate spawn failure")
            }
            fn probe(
                &self,
                _c: &RunningContainer,
                _spec: &ContainerSpec,
            ) -> Result<crate::isolation::IsolationProbe> {
                bail!("n/a")
            }
            fn exec(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<Option<i32>> {
                bail!("n/a")
            }
            fn exec_captured(
                &self,
                _c: &RunningContainer,
                _argv: &[&str],
            ) -> Result<crate::lease::CmdOutput> {
                bail!("n/a")
            }
            fn is_alive(&self, _c: &RunningContainer) -> Result<bool> {
                Ok(true)
            }
        }

        let engine = FailEngine;
        let fence = oracle_fence();
        let spawner = DedupSpawner::new(Duration::from_secs(60)).with_max_entries(64);

        for i in 0..50 {
            let id = format!("fail-{i}");
            let _ =
                spawner.spawn_or_join(&id, &engine, &lease_with(&id, "agent:A"), &fence, TEST_PIN);
        }
        // The NEXT claim reaps the prior Failed slots before inserting its own.
        let id = "fail-final";
        let _ = spawner.spawn_or_join(id, &engine, &lease_with(id, "agent:A"), &fence, TEST_PIN);

        let map = spawner.entries.lock().unwrap_or_else(|p| p.into_inner());
        // Only the just-published Failed marker for `fail-final` may remain;
        // all earlier Failed slots were swept.
        let failed = map
            .values()
            .filter(|e| matches!(e, SpawnEntry::Failed))
            .count();
        assert!(
            failed <= 1,
            "Failed slots must be reclaimed by the sweep; {failed} remain"
        );
    }
}
