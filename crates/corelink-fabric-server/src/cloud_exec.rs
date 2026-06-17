//! WP-CF-WIRE — wires the managed-sandbox [`Engine`] into the fabric-server
//! exec path, config-gated and DEFAULT-OFF.
//!
//! Two pieces live here:
//!
//! 1. [`BoxRegistry`] — the lease→live-container seam. The spawn lifecycle
//!    calls [`BoxRegistry::bind`] at spawn time to record the
//!    [`RunningContainer`] for a lease. An unbound lease fails closed (see
//!    [`EngineLeasedExec`]). State is honest: nothing is silently fabricated
//!    when no container is bound.
//!
//! 2. [`EngineLeasedExec`] — bridges the [`Engine`] trait (container-scoped)
//!    to the [`LeasedExec`] port (lease-scoped) by resolving the lease id via
//!    the registry before calling the engine. An unbound lease is
//!    fail-closed (`bail!`); the engine is NEVER called for an unbound lease.
//!
//! 3. [`BoxProvisioner`] — the spawn/teardown lifecycle seam. `provision`
//!    spawns the box and binds it into the registry; `teardown` deletes it and
//!    unbinds on the NORMAL close path. The default is [`NoBoxProvisioner`]
//!    (no-op, default-off); [`NorthflankBoxProvisioner`] is the cloud impl.
//!
//! ## Leak posture (honest)
//!
//! Teardown is wired to **both** the normal close path and the background
//! reaper (WP-D / CF-REAP). A lease that is acquired but never closed (client
//! crash / orphan) is reclaimed by `reap_once` (expired deadline) or
//! `surface_crashes` (box probed Dead), each of which calls
//! [`AppState::teardown_lease`] → [`NorthflankBoxProvisioner::teardown`] →
//! `registry.unbind()` **after** the provider job is deleted, mirroring the
//! normal close path.  No orphaned registry entry accumulates indefinitely.
//!
//! The **cost** is bounded: a job created with `runOnCreate:false` never
//! runs until `exec` triggers it (free, scale-to-zero); a run that did start
//! is killed by Northflank `activeDeadlineSeconds`. No unbounded compute cost.
//!
//! The double-unbind case (close path AND reaper both fire for the same lease)
//! is harmless: `unbind` is idempotent (a missing key is a silent no-op), and
//! the ledger's terminal-state CAS ensures only one path wins the transition
//! — the other sees a non-`Held` state and does not call teardown.
//!
//! 4. [`cloud_executor_from_env`] — builds only the exec side (legacy; prefer
//!    [`cloud_backend_from_env`] which wires both exec + provisioner over a
//!    SHARED registry, which is the crux of the spawn→exec lifecycle).
//!
//! 5. [`cloud_backend_from_env`] — the **complete production entry**: reads
//!    `NORTHFLANK_*` env vars, builds ONE engine + ONE shared registry, and
//!    returns BOTH the exec and the provisioner wired over them. Either both
//!    are wired or neither is (default-off, fail-closed). Used by
//!    [`AppState::with_cloud_backend_from_env`].
//!
//! ## Constructor discipline
//!
//! [`cloud_backend_from_env`] is the **BLESSED constructor** for the full
//! lifecycle (provision + exec). [`cloud_executor_from_env`] is kept for
//! exec-only composition; both enforce the both-credentials-required check via
//! [`NorthflankConfig::from_env`]. Hand-built constructors bypass the
//! credential check — use the composition seam unless you need direct
//! construction (e.g. tests).
//!
//! ## Dependency note
//!
//! `corelink-cloud-engine` (and its `ureq` HTTP transport) is linked into the
//! always-compiled server binary intentionally: it IS the production execution
//! backend. Feature-gating it behind an off-by-default cargo feature is a
//! possible future optimization, but it is NOT required for the fail-closed
//! guarantee — the default-off property is enforced by the composition seam
//! (absent env vars → `None` → defaults), not by conditional compilation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use corelink_cloud_engine::{NorthflankConfig, NorthflankEngine, UreqTransport};
use corelink_runner::isolation::{Engine, RunningContainer};
use corelink_runner::lease::{CmdOutput, ContainerSpec};

use crate::exec::LeasedExec;

// ── BoxRegistry ───────────────────────────────────────────────────────────────

/// The lease→live-container binding table.
///
/// The spawn lifecycle (a SEPARATE, future work-package) calls [`bind`] at
/// spawn time to record the [`RunningContainer`] for a lease. Today the
/// registry starts empty — an unbound lease in [`EngineLeasedExec`] fails
/// closed with an explicit error; the engine is NEVER called.
///
/// The inner [`Arc`] is cheap to clone; all clones share the same table.
/// A poisoned mutex is recovered via `.unwrap_or_else(|p| p.into_inner())`
/// (the codebase idiom from `app.rs`).
///
/// [`bind`]: BoxRegistry::bind
#[derive(Clone)]
pub struct BoxRegistry(Arc<Mutex<HashMap<String, RunningContainer>>>);

impl BoxRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }

    /// Bind `container` as the live box for `lease_id`. Called by the spawn
    /// lifecycle path at spawn time (future work-package).
    ///
    /// **Re-binding semantics (last-bind-wins):** re-binding the same
    /// `lease_id` OVERWRITES the previous entry. A re-spawned container
    /// replaces a stale handle, matching the dedup/recovery semantics of the
    /// spawn lifecycle — there is no "already bound" error; the latest bind
    /// always wins.
    pub fn bind(&self, lease_id: &str, container: RunningContainer) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string(), container);
    }

    /// Resolve the live container bound to `lease_id`, if any.
    ///
    /// Returns `None` when no container has been bound — the caller
    /// ([ `EngineLeasedExec`]) must fail closed on `None`.
    pub fn resolve(&self, lease_id: &str) -> Option<RunningContainer> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(lease_id)
            .cloned()
    }

    /// Remove the entry for `lease_id`, if any.
    ///
    /// Called by the teardown path ([`NorthflankBoxProvisioner::teardown`])
    /// after the provider job is deleted, so a stale handle cannot be resolved
    /// after teardown. Idempotent: a missing entry is silently ignored.
    /// Poison-safe (mirrors [`bind`]/[`resolve`]).
    ///
    /// [`bind`]: BoxRegistry::bind
    pub fn unbind(&self, lease_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id);
    }

    /// Clone the inner [`Arc`] so multiple owners share the same registry
    /// (e.g. the composition root and the spawn-lifecycle path).
    pub fn clone_handle(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl Default for BoxRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── EngineLeasedExec ──────────────────────────────────────────────────────────

/// Bridges the container-scoped [`Engine`] seam to the lease-scoped
/// [`LeasedExec`] port.
///
/// At exec time:
/// 1. The registry is consulted for a live container bound to `lease_id`.
/// 2. If none is bound → `bail!` (fail-closed; the engine is NOT called).
/// 3. If bound → `engine.exec_captured(&container, argv)` is called;
///    any error propagates as-is (fail-closed; no fabricated output).
pub struct EngineLeasedExec<E: Engine + Send + Sync> {
    engine: Arc<E>,
    registry: BoxRegistry,
}

impl<E: Engine + Send + Sync> EngineLeasedExec<E> {
    /// Construct over an engine and a registry.
    pub fn new(engine: Arc<E>, registry: BoxRegistry) -> Self {
        Self { engine, registry }
    }
}

impl<E: Engine + Send + Sync> LeasedExec for EngineLeasedExec<E> {
    fn exec_captured_for(&self, lease_id: &str, argv: &[&str]) -> Result<CmdOutput> {
        let Some(container) = self.registry.resolve(lease_id) else {
            bail!("no box bound for lease {lease_id}: failing closed")
        };
        self.engine.exec_captured(&container, argv)
    }
}

// ── cloud_executor_from_env ───────────────────────────────────────────────────

/// Build a [`LeasedExec`] backed by [`NorthflankEngine`] from the process
/// environment, or return `None` if the required `NORTHFLANK_*` vars are
/// absent.
///
/// When this returns `None`, the composition root MUST keep the [`NoBoxExec`]
/// default (default-off, fail-closed) — there is no partial wiring.
///
/// Required env vars: `NORTHFLANK_API_TOKEN`, `NORTHFLANK_PROJECT_ID`.
/// Optional: `NORTHFLANK_BASE_URL`, `NORTHFLANK_DEPLOYMENT_PLAN`.
pub fn cloud_executor_from_env(registry: BoxRegistry) -> Option<Arc<dyn LeasedExec>> {
    let cfg = NorthflankConfig::from_env()?;
    let engine = Arc::new(NorthflankEngine::new(UreqTransport::new(), cfg));
    Some(Arc::new(EngineLeasedExec::new(engine, registry)) as Arc<dyn LeasedExec>)
}

// ── BoxProvisioner ────────────────────────────────────────────────────────────

/// The spawn/teardown lifecycle seam.
///
/// `provision` spawns the box for a lease and binds the resulting
/// [`RunningContainer`] into the [`BoxRegistry`] so the exec path
/// (`EngineLeasedExec`) can resolve it.  `teardown` deletes the provider job
/// and unbinds the entry on **both** the normal close path and the background
/// reaper (WP-D / CF-REAP: `reap_once` for Expired leases, `surface_crashes`
/// for Crashed leases — both call [`AppState::teardown_lease`] which delegates
/// here after teardown succeeds).
///
/// Both operations are **fail-closed** in the cloud impl:
/// - a `provision` failure leaves the registry EMPTY for that lease (nothing
///   is bound on error), so a subsequent exec fails closed via the empty
///   registry — no box is ever handed out from a failed spawn.
/// - a `teardown` failure propagates `Err`; the caller (close handler or
///   reaper) treats it best-effort. On delete failure the registry entry is
///   intentionally KEPT so the next sweep retries — a failed teardown is never
///   silently discarded (the reaper logs and retries on the next tick).
///
/// **Orphan / crash posture:** teardown is reachable via the normal close
/// path AND the background reaper (WP-D).  A lease acquired but never closed
/// is reclaimed by the reaper on its next sweep, which calls teardown and
/// therefore unbind — preventing unbounded registry growth.
///
/// The default implementation is [`NoBoxProvisioner`] (no-op, DEFAULT-OFF):
/// `provision` returns `Ok(())` without binding anything, so an exec on such
/// a lease still fails closed via the empty registry; `teardown` is a no-op.
pub trait BoxProvisioner: Send + Sync {
    /// Spawn a container for `lease_id` / `spec` and bind it into the
    /// registry.  Returns `Err` on any spawn failure (fail-closed; nothing is
    /// bound on error).
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()>;

    /// Delete the container for `lease_id` from the provider and unbind it
    /// from the registry.  Idempotent: an already-unbound lease returns
    /// `Ok(())` without calling the provider.
    fn teardown(&self, lease_id: &str) -> Result<()>;

    /// Liveness of the box bound to `lease_id`. FAIL-SAFE: only `Ok(Dead)`
    /// authorizes reclamation; `Alive`/`Unbound`/`Err` all leave the lease alone.
    ///
    /// Resolves the lease's binding from the registry exactly as
    /// [`teardown`](BoxProvisioner::teardown) does (no binding →
    /// [`ProbeStatus::Unbound`]); a present binding is probed via
    /// [`Engine::is_alive`], whose `Ok(true)`→`Alive`, `Ok(false)`→`Dead`, and
    /// whose `Err` (transient/unreachable — NOT death) propagates unchanged.
    ///
    /// The default is the FAIL-SAFE [`ProbeStatus::Unbound`] — an implementor
    /// that holds no liveness signal reports "nothing to reclaim", so the crash
    /// sweep never touches its leases. The two production impls
    /// ([`NoBoxProvisioner`], [`NorthflankBoxProvisioner`]) override it
    /// explicitly per the contract.
    fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }

    /// Whether this provisioner actually binds a box (i.e. a real cloud backend
    /// is wired). The default is `true`; the no-op [`NoBoxProvisioner`] overrides
    /// it to `false`.
    ///
    /// The acquire path uses this to reject a RUNNER lease *at admission*
    /// (before reserving a slot) when no box backend is configured: a runner box
    /// that never binds would otherwise admit, return `Held`, and fail LATE —
    /// the ephemeral GitHub runner never comes up and the job hangs. A CHECK
    /// lease is unaffected (it still fails closed at exec via the empty
    /// registry).
    fn binds_boxes(&self) -> bool {
        true
    }
}

// ── ProbeStatus ───────────────────────────────────────────────────────────────

/// The liveness verdict for the box bound to a lease, returned by
/// [`BoxProvisioner::probe`].
///
/// **Fail-safe semantics:** only [`ProbeStatus::Dead`] authorizes reclamation.
/// [`ProbeStatus::Alive`], [`ProbeStatus::Unbound`], and any `Err` from `probe`
/// all leave the lease alone — the crash sweep ([`crate::reaper::surface_crashes`])
/// acts ONLY on an authoritative `Ok(Dead)`. This mirrors the underlying
/// [`Engine::is_alive`] guarantee, which itself only reports `Ok(false)` for an
/// authoritative dead/terminal status (ambiguous/5xx → `Ok(true)` = alive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    /// The box is live on the provider (or `is_alive` could not authoritatively
    /// prove it dead — fail-safe-alive).
    Alive,
    /// The box is authoritatively gone (provider reports a terminal/dead
    /// status). This is the ONLY status that authorizes crash reclamation.
    Dead,
    /// No box is bound to the lease in the registry — nothing to probe.
    Unbound,
}

// ── NoBoxProvisioner ──────────────────────────────────────────────────────────

/// The DEFAULT no-op provisioner (DEFAULT-OFF).
///
/// `provision` returns `Ok(())` without binding anything into the registry, so
/// a subsequent exec on the lease still fails closed via the empty registry
/// (the same failure mode as today — acquire behaviour is unchanged under this
/// provisioner).  `teardown` is also a no-op.
///
/// This is the value wired by [`AppState::new`]; switching to a cloud backend
/// requires calling [`AppState::with_cloud_backend_from_env`].
pub struct NoBoxProvisioner;

impl BoxProvisioner for NoBoxProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        Ok(())
    }

    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }

    fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
        // It never holds boxes — nothing to reclaim.
        Ok(ProbeStatus::Unbound)
    }

    fn binds_boxes(&self) -> bool {
        // The no-op default-off backend never binds a box. A RUNNER lease must
        // be rejected at admit when this is the wired provisioner.
        false
    }
}

// ── NorthflankBoxProvisioner ─────────────────────────────────────────────────

/// Cloud provisioner backed by [`NorthflankEngine`].
///
/// `provision` calls `engine.spawn(spec)` and binds the returned
/// [`RunningContainer`] into `registry`; `teardown` calls
/// `engine.delete_job(c)` and unbinds the entry.
///
/// Both operations are fail-closed:
/// - a spawn failure propagates `Err` without binding (registry stays empty).
/// - a delete-job failure propagates `Err`; the caller (close handler) treats
///   teardown failures as best-effort.
///
/// `NorthflankBoxProvisioner` and `EngineLeasedExec` share the SAME
/// `BoxRegistry` instance (built via `BoxRegistry::clone_handle`); provision
/// binds, exec resolves — the shared registry is the crux of the lifecycle.
pub struct NorthflankBoxProvisioner<H: corelink_cloud_engine::HttpTransport> {
    engine: Arc<NorthflankEngine<H>>,
    registry: BoxRegistry,
}

impl<H: corelink_cloud_engine::HttpTransport> NorthflankBoxProvisioner<H> {
    /// Construct over an engine and a (shared) registry.
    pub fn new(engine: Arc<NorthflankEngine<H>>, registry: BoxRegistry) -> Self {
        Self { engine, registry }
    }
}

impl<H: corelink_cloud_engine::HttpTransport + Send + Sync> BoxProvisioner
    for NorthflankBoxProvisioner<H>
{
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()> {
        // Fail-closed: if spawn errors, nothing is bound.
        let container = self.engine.spawn(spec)?;
        self.registry.bind(lease_id, container);
        Ok(())
    }

    fn teardown(&self, lease_id: &str) -> Result<()> {
        // Idempotent: if the lease is already unbound, skip the engine call.
        if let Some(c) = self.registry.resolve(lease_id) {
            // Delete-first, then unbind. If delete fails we propagate Err and
            // intentionally do NOT call unbind — the registry entry is kept so
            // a future reaper (CF-REAP) can retry teardown on the orphaned handle.
            self.engine.delete_job(&c)?;
            self.registry.unbind(lease_id);
        }
        Ok(())
    }

    fn probe(&self, lease_id: &str) -> Result<ProbeStatus> {
        // Resolve the binding EXACTLY as teardown does: no binding → Unbound.
        let Some(c) = self.registry.resolve(lease_id) else {
            return Ok(ProbeStatus::Unbound);
        };
        // FAIL-SAFE: is_alive only returns Ok(false) for an authoritative
        // dead/terminal status; ambiguous/5xx → Ok(true) (alive). A transient
        // or unreachable provider surfaces as Err and is propagated — it is NOT
        // death (the crash sweep treats Err as leave-Held).
        match self.engine.is_alive(&c) {
            Ok(true) => Ok(ProbeStatus::Alive),
            Ok(false) => Ok(ProbeStatus::Dead),
            Err(e) => Err(e),
        }
    }
}

// ── cloud_backend_from_env ────────────────────────────────────────────────────

/// Build BOTH the exec backend AND the provisioner from the process
/// environment, sharing ONE engine and ONE registry — or return `None` if the
/// required `NORTHFLANK_*` vars are absent.
///
/// The shared registry is the crux: [`NorthflankBoxProvisioner::provision`]
/// binds into it at acquire, and [`EngineLeasedExec`] resolves from it at
/// exec — they are the same map, so provision → exec forms a coherent
/// lifecycle.
///
/// When this returns `None`, the composition root MUST keep both the
/// [`NoBoxExec`] and [`NoBoxProvisioner`] defaults (default-off, fail-closed).
///
/// Required env vars: `NORTHFLANK_API_TOKEN`, `NORTHFLANK_PROJECT_ID`.
/// Optional: `NORTHFLANK_BASE_URL`, `NORTHFLANK_TEAM_ID`,
///   `NORTHFLANK_DEPLOYMENT_PLAN`.
///
/// [`NoBoxExec`]: crate::exec::NoBoxExec
pub fn cloud_backend_from_env(
    registry: BoxRegistry,
) -> Option<(Arc<dyn LeasedExec>, Arc<dyn BoxProvisioner>)> {
    let cfg = NorthflankConfig::from_env()?;
    let engine = Arc::new(NorthflankEngine::new(UreqTransport::new(), cfg));
    let exec: Arc<dyn LeasedExec> = Arc::new(EngineLeasedExec::new(
        Arc::clone(&engine),
        registry.clone_handle(),
    ));
    let prov: Arc<dyn BoxProvisioner> = Arc::new(NorthflankBoxProvisioner::new(engine, registry));
    Some((exec, prov))
}

// ── boot diagnostic ─────────────────────────────────────────────────────────

/// What the cloud-backend wiring will ACTUALLY resolve to — for an honest boot
/// log.
///
/// The trap this exists to kill: [`cloud_backend_from_env`] (via
/// [`NorthflankConfig::from_env`]) requires BOTH `NORTHFLANK_API_TOKEN` and
/// `NORTHFLANK_PROJECT_ID`. A naive "is the token set?" boot check reports
/// "Northflank" while the backend silently falls back to NoBox when the project
/// id is missing/empty — every `exec` then 503s "no execution backend attached"
/// with no clue why. This status mirrors the wiring's REAL condition so the
/// diagnostic can never lie, and names the missing var on a partial config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudBackendStatus {
    /// Both required vars present (non-empty) → the Northflank backend is wired.
    Wired,
    /// Exactly one required var present → cloud is OFF (NoBox), but the operator
    /// almost certainly INTENDED cloud. Name the missing var loudly.
    PartialConfig {
        /// The required var that IS set.
        present: &'static str,
        /// The required var that is missing/empty (the fix).
        missing: &'static str,
    },
    /// No required `NORTHFLANK_*` vars → cloud deliberately off (fail-closed).
    Off,
}

/// Resolve the [`CloudBackendStatus`] from an env accessor, using the SAME two
/// required vars as [`cloud_backend_from_env`] / [`NorthflankConfig::from_env`].
///
/// A present-but-empty value counts as missing — matching `from_env`'s
/// `filter(|s| !s.is_empty())`, so this status can never disagree with the
/// actual wiring.
pub fn cloud_backend_status(get: impl Fn(&str) -> Option<String>) -> CloudBackendStatus {
    let has = |k: &str| get(k).is_some_and(|s| !s.is_empty());
    match (has("NORTHFLANK_API_TOKEN"), has("NORTHFLANK_PROJECT_ID")) {
        (true, true) => CloudBackendStatus::Wired,
        (true, false) => CloudBackendStatus::PartialConfig {
            present: "NORTHFLANK_API_TOKEN",
            missing: "NORTHFLANK_PROJECT_ID",
        },
        (false, true) => CloudBackendStatus::PartialConfig {
            present: "NORTHFLANK_PROJECT_ID",
            missing: "NORTHFLANK_API_TOKEN",
        },
        (false, false) => CloudBackendStatus::Off,
    }
}

// ── Static Send+Sync gate ──────────────────────────────────────────────────────

// The concrete production executor AND provisioner MUST satisfy Send+Sync
// even though no binary wires them yet — these static gates fail the build if
// a future change makes the Northflank stack non-thread-safe.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<
        EngineLeasedExec<
            corelink_cloud_engine::NorthflankEngine<corelink_cloud_engine::UreqTransport>,
        >,
    >();
    assert_send_sync::<NorthflankBoxProvisioner<corelink_cloud_engine::UreqTransport>>();
};

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: a map-backed env accessor for the status tests. Owns its data so
    /// the returned closure borrows nothing (no lifetime threading).
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| {
            owned
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn cloud_status_wired_when_both_required_present() {
        let s = cloud_backend_status(env(&[
            ("NORTHFLANK_API_TOKEN", "tok"),
            ("NORTHFLANK_PROJECT_ID", "corelink-runners"),
        ]));
        assert_eq!(s, CloudBackendStatus::Wired);
    }

    #[test]
    fn cloud_status_partial_names_the_missing_project_id() {
        // The exact production trap: token set, project id absent → NoBox,
        // and the diagnostic must name PROJECT_ID as the fix (not claim "Northflank").
        let s = cloud_backend_status(env(&[("NORTHFLANK_API_TOKEN", "tok")]));
        assert_eq!(
            s,
            CloudBackendStatus::PartialConfig {
                present: "NORTHFLANK_API_TOKEN",
                missing: "NORTHFLANK_PROJECT_ID",
            }
        );
    }

    #[test]
    fn cloud_status_partial_names_the_missing_token() {
        let s = cloud_backend_status(env(&[("NORTHFLANK_PROJECT_ID", "corelink-runners")]));
        assert_eq!(
            s,
            CloudBackendStatus::PartialConfig {
                present: "NORTHFLANK_PROJECT_ID",
                missing: "NORTHFLANK_API_TOKEN",
            }
        );
    }

    #[test]
    fn cloud_status_empty_value_counts_as_missing() {
        // Mirrors `NorthflankConfig::from_env`'s `filter(|s| !s.is_empty())`:
        // a present-but-empty PROJECT_ID is NOT wired — the status must agree
        // with the wiring, else the log lies again.
        let s = cloud_backend_status(env(&[
            ("NORTHFLANK_API_TOKEN", "tok"),
            ("NORTHFLANK_PROJECT_ID", ""),
        ]));
        assert_eq!(
            s,
            CloudBackendStatus::PartialConfig {
                present: "NORTHFLANK_API_TOKEN",
                missing: "NORTHFLANK_PROJECT_ID",
            }
        );
    }

    #[test]
    fn cloud_status_off_when_neither_present() {
        assert_eq!(cloud_backend_status(env(&[])), CloudBackendStatus::Off);
    }

    /// `NoBoxProvisioner` never holds boxes, so `probe` is always `Unbound`
    /// (FAIL-SAFE: an `Unbound` lease is never reclaimed by the crash sweep).
    #[test]
    fn no_box_provisioner_probe_is_unbound() {
        let prov = NoBoxProvisioner;
        assert_eq!(
            prov.probe("any-lease").unwrap(),
            ProbeStatus::Unbound,
            "NoBoxProvisioner::probe must always be Unbound"
        );
    }

    #[test]
    fn no_box_provisioner_does_not_bind_boxes() {
        // The no-op provisioner reports it binds nothing — the signal the acquire
        // path uses to reject a runner lease at admit (S2 cold-start guard).
        assert!(
            !NoBoxProvisioner.binds_boxes(),
            "NoBoxProvisioner must report binds_boxes() == false"
        );
    }
}
