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
use corelink_cloud_engine::{
    CloudflareConfig, CloudflareEngine, NorthflankConfig, NorthflankEngine, ProviderCapacityError,
    RunnerDiskStatus, UreqTransport,
};
use corelink_runner::isolation::{Engine, RunningContainer};
use corelink_runner::lease::{CmdOutput, ContainerSpec};

use crate::exec::LeasedExec;

// ── Capacity-error classification ─────────────────────────────────────────────

/// True iff the anyhow error IS (or wraps) a [`ProviderCapacityError`].
///
/// Uses `anyhow::Error::downcast_ref::<ProviderCapacityError>()` which works
/// for both direct errors (`anyhow::Error::new(ProviderCapacityError{...})`)
/// and context-wrapped errors
/// (`anyhow!("msg").context(ProviderCapacityError{...})`).
///
/// Note: `anyhow::Error::chain()` yields `&dyn std::error::Error` elements
/// that cannot be downcast via `Any::downcast_ref` on stable Rust (the
/// concrete type is anyhow's internal wrapper). Use anyhow's own
/// `downcast_ref` on the `anyhow::Error` root instead.
pub(crate) fn is_capacity_error(e: &anyhow::Error) -> bool {
    e.downcast_ref::<ProviderCapacityError>().is_some()
}

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

    // ── S3 boot-time runner-disk validation ──────────────────────────────────
    // Check ONCE here so a misconfigured runner fabric fails LOUD at boot —
    // an operator sees the warn in startup logs rather than per-acquire 503s
    // after a wasted JIT/CAS mint. See `RunnerDiskStatus::SubFloor` for the
    // decision rationale (warn, not hard-fail: check-only fabrics must boot).
    // The per-spawn `bail!` in `NorthflankEngine::spawn` is the hard backstop.
    match cfg.validate_runner_disk() {
        RunnerDiskStatus::SubFloor { resolved_mb } => {
            let floor = corelink_cloud_engine::RUNNER_EPHEMERAL_STORAGE_FLOOR_MB;
            eprintln!();
            eprintln!(
                "WARNING [S3]: NORTHFLANK RUNNER DISK BELOW FLOOR — runners WILL FAIL AT SPAWN"
            );
            eprintln!("  NORTHFLANK_RUNNER_DEPLOYMENT_PLAN is set but the runner ephemeral disk");
            eprintln!(
                "  resolves to {resolved_mb} MiB — below the {floor} MiB floor a CI build needs."
            );
            eprintln!("  Every runner spawn will fail CLOSED (ENOSPC risk, not a slow run).");
            eprintln!("  Fix: set NORTHFLANK_RUNNER_EPHEMERAL_STORAGE_MB >= {floor}");
            eprintln!("  (within the Northflank disk allowance for your plan).");
            eprintln!();
        }
        // Runner-capable and at/above the floor — nominal path.
        RunnerDiskStatus::Ok => {}
        // CHECK-only fabric — runner floor is irrelevant, no warn needed.
        RunnerDiskStatus::CheckOnly => {}
    }

    let engine = Arc::new(NorthflankEngine::new(UreqTransport::new(), cfg));
    let exec: Arc<dyn LeasedExec> = Arc::new(EngineLeasedExec::new(
        Arc::clone(&engine),
        registry.clone_handle(),
    ));
    let prov: Arc<dyn BoxProvisioner> = Arc::new(NorthflankBoxProvisioner::new(engine, registry));
    Some((exec, prov))
}

// ── CloudflareBoxProvisioner ──────────────────────────────────────────────────

/// Cloud provisioner backed by [`CloudflareEngine`] (ADR-0008: Cloudflare is the
/// DEFAULT compute substrate).
///
/// Mirrors [`NorthflankBoxProvisioner`] EXACTLY — same lease→handle binding
/// discipline, same fail-closed posture:
/// - `provision` calls `engine.spawn(spec)` and binds the returned
///   [`RunningContainer`] into `registry`; a spawn failure propagates `Err`
///   WITHOUT binding (the registry stays empty for that lease, so a later exec
///   fails closed via the empty registry — no box from a failed spawn).
/// - `teardown` resolves the binding, calls `engine.teardown(c)` (delete-first),
///   then `unbind`s. A delete failure propagates `Err` and KEEPS the registry
///   entry so the reaper retries — a failed teardown is never silently dropped.
///   Idempotent: an already-unbound lease returns `Ok(())` without the provider.
/// - `probe` resolves the binding (no binding → [`ProbeStatus::Unbound`]) and
///   maps `engine.is_alive`: `Ok(true)`→`Alive`, `Ok(false)`→`Dead`, `Err`
///   propagates (transient/unreachable is NOT death — fail-safe-alive).
///
/// **Runner-direct + check-host (ADR-0007 / rota A):** a RUNNER container runs
/// the GitHub-Actions agent via its image entrypoint, so there is no post-spawn
/// `exec` step (a runner lease never execs). A CHECK-HOST lease DOES exec —
/// [`cloudflare_backend_from_env`] wires the exec half as an
/// [`EngineLeasedExec`] over the SAME [`CloudflareEngine`], so a check-host box
/// this provisioner binds execs on Cloudflare (`POST /v1/exec`). A plain
/// hermetic check never reaches exec — it fails closed at `spawn` here.
pub struct CloudflareBoxProvisioner<H: corelink_cloud_engine::HttpTransport> {
    engine: Arc<CloudflareEngine<H>>,
    registry: BoxRegistry,
}

impl<H: corelink_cloud_engine::HttpTransport> CloudflareBoxProvisioner<H> {
    /// Construct over an engine and a (shared) registry.
    pub fn new(engine: Arc<CloudflareEngine<H>>, registry: BoxRegistry) -> Self {
        Self { engine, registry }
    }
}

impl<H: corelink_cloud_engine::HttpTransport + Send + Sync> BoxProvisioner
    for CloudflareBoxProvisioner<H>
{
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()> {
        // OFF-BOX / plain-hermetic lease → admit NO-BOX (bind nothing, no spawn).
        // A lease that is hermetic (`!allow_egress`) AND carries no `TOOLCHAIN_DIGEST`
        // has no exec substrate on the CF backend — e.g. hugit's off-box §13 A-path,
        // which hosts the lease + attestation but NEVER execs (it submits §13 off-box).
        // CloudflareEngine::spawn fail-closes for such a spec (runner-only floor); on
        // this CF-only backend that would 503 the acquire and BLOCK the single-flight
        // control-plane singleton on a box the caller never uses (the 2026-07-07
        // acquire-storm incident). Admit no-box instead: a later exec fails closed via
        // the empty registry (503, honest), so nothing runs unattested. A RUNNER lease
        // (`allow_egress`) or a CHECK-HOST lease (`TOOLCHAIN_DIGEST`) still provisions.
        // Scoped to CloudflareBoxProvisioner — a Hybrid deployment routes plain checks
        // to the Northflank sub (rota B), which is unaffected.
        if !spec.allow_egress && !is_check_host_spec(spec) {
            return Ok(());
        }
        // Fail-closed: if spawn errors, nothing is bound (mirrors Northflank).
        let container = self.engine.spawn(spec)?;
        self.registry.bind(lease_id, container);
        Ok(())
    }

    fn teardown(&self, lease_id: &str) -> Result<()> {
        // Idempotent: if the lease is already unbound, skip the engine call.
        if let Some(c) = self.registry.resolve(lease_id) {
            // Delete-first, then unbind. If delete fails we propagate Err and
            // intentionally do NOT call unbind — the registry entry is kept so
            // a future reaper can retry teardown on the orphaned handle.
            self.engine.teardown(&c)?;
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
        // dead/terminal status; ambiguous/5xx → Err (propagated, NOT death).
        match self.engine.is_alive(&c) {
            Ok(true) => Ok(ProbeStatus::Alive),
            Ok(false) => Ok(ProbeStatus::Dead),
            Err(e) => Err(e),
        }
    }
}

// ── cloudflare_backend_from_env ───────────────────────────────────────────────

/// Build the Cloudflare backend from the process environment — or return `None`
/// if the required `CLOUDFLARE_*` vars are absent (DEFAULT-OFF, fail-closed).
///
/// ADR-0008: Cloudflare is the DEFAULT compute substrate. When
/// [`CloudflareConfig::from_env`] yields a config (both
/// `CLOUDFLARE_SPAWN_WORKER_URL` and `CLOUDFLARE_SPAWN_AUTH_TOKEN` present),
/// this returns BOTH halves of the backend, over ONE engine + the SHARED
/// `registry`:
/// - exec → [`EngineLeasedExec`] over [`CloudflareEngine`] (**rota A**): a
///   RUNNER lease is runner-direct (its container runs the Actions agent at
///   spawn) and NEVER calls exec; a CHECK-HOST lease (hermetic + carrying
///   `TOOLCHAIN_DIGEST`) execs ON Cloudflare via
///   [`CloudflareEngine::exec_captured`](corelink_cloud_engine::CloudflareEngine)
///   (`POST /v1/exec`) — R2-co-located, the moat win. A PLAIN hermetic check
///   (no `TOOLCHAIN_DIGEST`) fails closed at SPAWN inside
///   [`CloudflareBoxProvisioner`] (`CloudflareEngine` does not serve it), so
///   exec is never reached for it — no fabricated result.
/// - provisioner → [`CloudflareBoxProvisioner`] over the SHARED `registry`.
///
/// When this returns `None`, the composition root MUST keep both the
/// [`NoBoxExec`](crate::exec::NoBoxExec) and [`NoBoxProvisioner`] defaults.
pub fn cloudflare_backend_from_env(
    registry: BoxRegistry,
) -> Option<(Arc<dyn LeasedExec>, Arc<dyn BoxProvisioner>)> {
    let cfg = CloudflareConfig::from_env()?;
    let engine = Arc::new(CloudflareEngine::new(UreqTransport::new(), cfg));
    // Rota A: the exec half is a CF-native EngineLeasedExec over the SAME engine
    // and a SHARED registry handle, so a check-host box the provisioner binds is
    // the box exec resolves. Runner leases never call it; check-host leases exec
    // on Cloudflare (the moat). Fail-closed for an unbound lease (empty registry).
    let exec: Arc<dyn LeasedExec> = Arc::new(EngineLeasedExec::new(
        Arc::clone(&engine),
        registry.clone_handle(),
    ));
    let prov: Arc<dyn BoxProvisioner> = Arc::new(CloudflareBoxProvisioner::new(engine, registry));
    Some((exec, prov))
}

// ── HybridBoxProvisioner (rota A/B) ───────────────────────────────────────────

/// Which sub-backend provisioned a given lease — the routing key the hybrid
/// remembers so `teardown`/`probe` reach the SAME engine that `spawn`ed the box.
///
/// [`RunningContainer`] carries only a name (no provider tag), so the hybrid
/// cannot infer the owning engine from the registry binding alone — it records
/// the route at `provision` time and replays it on teardown/probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HybridRoute {
    /// Runner lease (`spec.allow_egress == true`) → the runner sub-provisioner.
    Runner,
    /// Check-exec lease (`spec.allow_egress == false`) → the check sub-provisioner.
    Check,
    /// CHECK-HOST lease (CF-native check-host, C1/C6): `spec.allow_egress == false`
    /// AND `spec.env` carries the `TOOLCHAIN_DIGEST` discriminator → the SAME CF
    /// (`runner`) sub-provisioner, which W4's `CloudflareEngine` spawns in
    /// check-mode (it reads `TOOLCHAIN_DIGEST` from the spec env). A check-host box
    /// is hermetic (no egress) but lives on the Cloudflare moat substrate, not
    /// Northflank. Teardown/probe replay against the CF sub-provisioner.
    CheckHost,
}

/// The C6 env discriminator: a check-host spec is a hermetic (`!allow_egress`)
/// spec whose env carries `TOOLCHAIN_DIGEST` (injected at acquire, leases.rs).
/// This is the SAME marker the exec-time assert keys on — server-internal, never
/// the wire `net_policy` string (the C2 invariant). Plain-hermetic checks carry
/// no `TOOLCHAIN_DIGEST`, so this is `false` for them (→ Northflank, rota B).
const TOOLCHAIN_DIGEST_ENV: &str = "TOOLCHAIN_DIGEST";

fn is_check_host_spec(spec: &ContainerSpec) -> bool {
    !spec.allow_egress && spec.env.iter().any(|(k, _)| k == TOOLCHAIN_DIGEST_ENV)
}

/// A [`BoxProvisioner`] that routes each lease to one of two sub-backends by the
/// lease's KIND, decided at `provision` from `spec.allow_egress`:
///
/// - **runner** lease (`allow_egress == true`) → `runner` sub-provisioner
///   (production: [`CloudflareBoxProvisioner`] — the moat substrate, co-located
///   with R2 for in-network cache hydration);
/// - **check-host** lease (`allow_egress == false` AND `spec.env` carries
///   `TOOLCHAIN_DIGEST`, [`is_check_host_spec`]) → the SAME `runner` (Cloudflare)
///   sub-provisioner, spawned in check-mode — **rota A**: check-exec on the moat;
/// - **plain hermetic check** (`allow_egress == false`, NO `TOOLCHAIN_DIGEST`) →
///   `check` sub-provisioner (production: [`NorthflankBoxProvisioner`] —
///   `CloudflareEngine` does not serve a plain check, ADR-0008/#198).
///
/// **Rota A (native CF check-exec) is now LIVE**, not deferred: a check-host box
/// both provisions AND execs on Cloudflare — the exec half is the paired
/// [`HybridLeasedExec`] ([`with_paired_exec`](HybridBoxProvisioner::with_paired_exec)),
/// which dispatches a check-host lease's exec to the CF engine. A plain hermetic
/// check remains on Northflank (**rota B**) until it too carries a toolchain
/// digest. DEFAULT-OFF: absent `toolchain_digest` at acquire, no `TOOLCHAIN_DIGEST`
/// is injected, so every check is a plain check (byte-identical to rota B).
///
/// `allow_egress` is the red-team-blessed discriminator: a runner lease is built
/// only through `ContainerSpec::from_runner_lease` (egress granted), a check
/// lease through `ContainerSpec::from_lease` (no_network, fail-closed). Egress is
/// never inferred from the wire `net_policy` string (the C2 invariant), so the
/// routing fork is exactly the lease-kind fork — it cannot be spoofed.
///
/// **Routing memory & fail-closed posture:** both sub-provisioners bind into the
/// SAME shared [`BoxRegistry`], so the wired exec ([`EngineLeasedExec`] over the
/// check engine — only a check lease ever execs) resolves the right box. The
/// `routes` map records lease→backend at provision; `teardown` replays it and
/// removes the entry ONLY on success (a failed teardown keeps the route so the
/// reaper retries against the correct engine — mirrors the registry-keep
/// discipline). `teardown`/`probe` of a lease the hybrid never provisioned are
/// idempotent (`Ok(())` / `Unbound`).
pub struct HybridBoxProvisioner {
    runner: Arc<dyn BoxProvisioner>,
    check: Arc<dyn BoxProvisioner>,
    /// Shared with the wired [`HybridLeasedExec`] (rota A): the provisioner
    /// RECORDS the route at `provision`; the exec READS it to dispatch a
    /// check-host lease's exec to the CF engine and a plain-check lease's to
    /// Northflank. Shared behind `Arc` so both halves see the same table over
    /// the one shared [`BoxRegistry`]. See [`routes_handle`].
    ///
    /// [`routes_handle`]: HybridBoxProvisioner::routes_handle
    routes: Arc<Mutex<HashMap<String, HybridRoute>>>,
}

impl HybridBoxProvisioner {
    /// Construct over the runner sub-provisioner (Cloudflare in prod) and the
    /// check sub-provisioner (Northflank in prod). Both MUST be built over the
    /// same shared [`BoxRegistry`] as the wired exec.
    pub fn new(runner: Arc<dyn BoxProvisioner>, check: Arc<dyn BoxProvisioner>) -> Self {
        Self {
            runner,
            check,
            routes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// A cloned handle to the shared route table, for building the matching
    /// [`HybridLeasedExec`] (rota A). The exec reads the same routes this
    /// provisioner records, so a check-host lease execs on the SAME engine that
    /// spawned its box. Call AFTER `new`, BEFORE wrapping `self` in an `Arc`.
    ///
    /// Internal: the returned type names the private [`HybridRoute`]. External
    /// callers use [`with_paired_exec`](HybridBoxProvisioner::with_paired_exec),
    /// which returns type-erased handles.
    pub(crate) fn routes_handle(&self) -> Arc<Mutex<HashMap<String, HybridRoute>>> {
        Arc::clone(&self.routes)
    }

    /// Build the hybrid provisioner AND its matching [`HybridLeasedExec`] over
    /// ONE shared route table (rota A) — the BLESSED way to wire the pair. The
    /// provisioner routes `runner`/`check-host` → the runner sub (Cloudflare) and
    /// `plain-check` → the check sub (Northflank); the returned exec dispatches
    /// each lease's `exec_captured_for` to the engine that provisioned it
    /// (`check_host_exec` for a check-host lease, `check_exec` for a plain check).
    /// Callers never handle the internal route type — the pair is returned
    /// type-erased and MUST be wired together (same shared registry upstream).
    pub fn with_paired_exec(
        runner: Arc<dyn BoxProvisioner>,
        check: Arc<dyn BoxProvisioner>,
        check_host_exec: Arc<dyn LeasedExec>,
        check_exec: Arc<dyn LeasedExec>,
    ) -> (Arc<dyn BoxProvisioner>, Arc<dyn LeasedExec>) {
        let prov = Self::new(runner, check);
        let routes = prov.routes_handle();
        let exec: Arc<dyn LeasedExec> =
            Arc::new(HybridLeasedExec::new(check_host_exec, check_exec, routes));
        (Arc::new(prov) as Arc<dyn BoxProvisioner>, exec)
    }

    /// Look up the recorded route for a lease (poison-safe).
    fn route_of(&self, lease_id: &str) -> Option<HybridRoute> {
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(lease_id)
            .copied()
    }

    fn record_route(&self, lease_id: &str, route: HybridRoute) {
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string(), route);
    }

    fn forget_route(&self, lease_id: &str) {
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id);
    }
}

impl BoxProvisioner for HybridBoxProvisioner {
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()> {
        // The lease-kind fork: egress ⇒ runner (Cloudflare); no egress ⇒ check.
        // The no-egress branch FORKS AGAIN (C1/C6): a CHECK-HOST spec (env carries
        // `TOOLCHAIN_DIGEST`) routes to the SAME CF (`runner`) sub-provisioner —
        // W4's CloudflareEngine spawns it in check-mode off the digest in env — so
        // a hermetic check-host box lives on the Cloudflare moat; a PLAIN-hermetic
        // check (no `TOOLCHAIN_DIGEST`) stays on Northflank (rota B). DEFAULT-OFF:
        // leases.rs only injects `TOOLCHAIN_DIGEST` when an acquire carried
        // `toolchain_digest`, so absent that field this branch is byte-identical to
        // rota B (→ Check → Northflank). Record the route BEFORE delegating so
        // teardown can always reach the intended engine — even if the spawn fails
        // (its teardown is idempotent against the empty registry). Fail-closed: a
        // sub-provisioner error propagates unchanged (nothing bound on a failed
        // spawn).
        let (route, sub): (HybridRoute, &Arc<dyn BoxProvisioner>) = if spec.allow_egress {
            (HybridRoute::Runner, &self.runner)
        } else if is_check_host_spec(spec) {
            (HybridRoute::CheckHost, &self.runner)
        } else {
            (HybridRoute::Check, &self.check)
        };
        self.record_route(lease_id, route);
        sub.provision(lease_id, spec)
    }

    fn teardown(&self, lease_id: &str) -> Result<()> {
        // Idempotent: a lease the hybrid never provisioned has no route → nothing
        // to tear down (mirrors NoBoxProvisioner / an already-unbound lease).
        let Some(route) = self.route_of(lease_id) else {
            return Ok(());
        };
        let sub = match route {
            // CheckHost rides the CF (`runner`) sub-provisioner — the same engine
            // that spawned it (check-mode).
            HybridRoute::Runner | HybridRoute::CheckHost => &self.runner,
            HybridRoute::Check => &self.check,
        };
        // On success, drop the route. On failure, KEEP it so the reaper retries
        // teardown against the SAME engine (a failed teardown is never silently
        // dropped — mirrors the registry-keep-on-failure discipline).
        sub.teardown(lease_id)?;
        self.forget_route(lease_id);
        Ok(())
    }

    fn probe(&self, lease_id: &str) -> Result<ProbeStatus> {
        // No recorded route ⇒ the hybrid holds no box for this lease.
        let Some(route) = self.route_of(lease_id) else {
            return Ok(ProbeStatus::Unbound);
        };
        match route {
            HybridRoute::Runner | HybridRoute::CheckHost => self.runner.probe(lease_id),
            HybridRoute::Check => self.check.probe(lease_id),
        }
    }

    fn binds_boxes(&self) -> bool {
        // A real cloud backend on both sides binds boxes — a runner lease admits.
        true
    }
}

// ── HybridLeasedExec (rota A) ─────────────────────────────────────────────────

/// The exec counterpart to [`HybridBoxProvisioner`]: routes a lease's
/// `exec_captured_for` to the SAME engine that provisioned its box, using the
/// route the provisioner recorded (shared table, [`routes_handle`]).
///
/// Rota B wired a single Northflank exec for every check — correct only while
/// every check ran on Northflank. Rota A splits provisioning by lease kind (a
/// check-host box runs on Cloudflare, the moat), so the exec MUST split the same
/// way — else a check-host box (spawned on CF) would be exec'd against
/// Northflank (a handle mismatch, fail-closed at best, wrong at worst). This
/// type restores the invariant *exec-engine == spawn-engine*, per lease:
/// - [`HybridRoute::CheckHost`] → `check_host` exec (CF: `POST /v1/exec`, moat);
/// - [`HybridRoute::Check`]     → `check` exec (Northflank, rota B — unchanged);
/// - [`HybridRoute::Runner`]    → **fail closed**: a runner lease is
///   runner-direct and must never exec (reaching here is a wiring bug);
/// - no recorded route          → **fail closed**: exec before/without provision
///   (or an unknown lease) never fabricates a result.
///
/// Both sub-execs resolve their box from the ONE shared [`BoxRegistry`]; this
/// type only decides WHICH engine, never touches the registry itself.
///
/// [`routes_handle`]: HybridBoxProvisioner::routes_handle
pub struct HybridLeasedExec {
    /// Cloudflare exec (`EngineLeasedExec<CloudflareEngine>` in prod) — serves
    /// check-host leases on the moat.
    check_host: Arc<dyn LeasedExec>,
    /// Northflank exec (`EngineLeasedExec<NorthflankEngine>` in prod) — serves
    /// plain hermetic checks (rota B).
    check: Arc<dyn LeasedExec>,
    /// The SAME route table [`HybridBoxProvisioner`] records into.
    routes: Arc<Mutex<HashMap<String, HybridRoute>>>,
}

impl HybridLeasedExec {
    /// Construct over the check-host exec (Cloudflare), the plain-check exec
    /// (Northflank), and the shared route table obtained from
    /// [`HybridBoxProvisioner::routes_handle`]. All three MUST come from the
    /// same hybrid wiring (one shared registry + one shared route table).
    ///
    /// Internal: takes the private [`HybridRoute`] table. External callers use
    /// [`HybridBoxProvisioner::with_paired_exec`], which wires the pair and
    /// returns type-erased handles.
    pub(crate) fn new(
        check_host: Arc<dyn LeasedExec>,
        check: Arc<dyn LeasedExec>,
        routes: Arc<Mutex<HashMap<String, HybridRoute>>>,
    ) -> Self {
        Self {
            check_host,
            check,
            routes,
        }
    }

    /// Look up the recorded route for a lease (poison-safe, mirrors the
    /// provisioner's accessor).
    fn route_of(&self, lease_id: &str) -> Option<HybridRoute> {
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(lease_id)
            .copied()
    }
}

impl LeasedExec for HybridLeasedExec {
    fn exec_captured_for(&self, lease_id: &str, argv: &[&str]) -> Result<CmdOutput> {
        match self.route_of(lease_id) {
            // Check-host → the SAME CF engine that spawned the box (moat exec).
            Some(HybridRoute::CheckHost) => self.check_host.exec_captured_for(lease_id, argv),
            // Plain hermetic check → Northflank (rota B, unchanged).
            Some(HybridRoute::Check) => self.check.exec_captured_for(lease_id, argv),
            // A runner lease is runner-direct — it must NEVER exec. Reaching here
            // means a runner lease was routed to the exec path: a wiring bug.
            // Fail closed rather than dispatch a runner box to a check engine.
            Some(HybridRoute::Runner) => bail!(
                "lease {lease_id} is a RUNNER lease (runner-direct) and must never exec: \
                 failing closed"
            ),
            // No route recorded: exec without a preceding provision, or an
            // unknown lease. Never guess an engine — fail closed (mirrors the
            // empty-registry posture in EngineLeasedExec).
            None => bail!(
                "no hybrid route recorded for lease {lease_id} (exec before provision or \
                 unknown lease): failing closed"
            ),
        }
    }
}

// ── backend selection (ADR-0008 + rota A/B) ───────────────────────────────────

/// Which compute substrate the composition root selected.
///
/// Selection order:
/// - **both** Cloudflare + Northflank env present → [`Hybrid`](SelectedBackend::Hybrid):
///   runner leases → Cloudflare (the moat), check-exec leases → Northflank (rota B).
/// - **only Cloudflare** → [`Cloudflare`](SelectedBackend::Cloudflare): runner-only;
///   a check lease fails closed at spawn (`CloudflareEngine` v0 is runner-only, #198).
/// - **only Northflank** → [`Northflank`](SelectedBackend::Northflank): both kinds
///   on Northflank.
/// - **neither** → [`Off`](SelectedBackend::Off): NoBox defaults (default-off,
///   fail-closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedBackend {
    /// Both env present → runner→Cloudflare, check-exec→Northflank (rota B).
    Hybrid,
    /// Cloudflare env present (no Northflank) → Cloudflare backend (runner-only).
    Cloudflare,
    /// Northflank env present (no Cloudflare) → Northflank backend (both kinds).
    Northflank,
    /// Neither present → NoBox defaults (default-off, fail-closed).
    Off,
}

/// Pure selection oracle (both → Hybrid; CF → Cloudflare; NF → Northflank; else
/// off), factored out of the composition root so the order is unit-testable
/// without mutating the process environment. `cf_present` / `nf_present` are the
/// `*_backend_from_env(...).is_some()` results.
#[must_use]
pub fn select_backend(cf_present: bool, nf_present: bool) -> SelectedBackend {
    match (cf_present, nf_present) {
        // Rota B: both wired ⇒ split by lease kind (runner→CF, check→NF).
        (true, true) => SelectedBackend::Hybrid,
        // Cloudflare is the DEFAULT runner substrate (ADR-0008); runner-only.
        (true, false) => SelectedBackend::Cloudflare,
        (false, true) => SelectedBackend::Northflank,
        (false, false) => SelectedBackend::Off,
    }
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
    assert_send_sync::<CloudflareBoxProvisioner<corelink_cloud_engine::UreqTransport>>();
    // Rota A: the hybrid provisioner + its exec counterpart are wired into the
    // production composition root — both MUST be thread-safe.
    assert_send_sync::<HybridBoxProvisioner>();
    assert_send_sync::<HybridLeasedExec>();
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

    // ── ADR-0008 + rota B backend selection order ─────────────────────────────

    #[test]
    fn select_backend_hybrid_when_both_present() {
        // Rota B: both substrates wired ⇒ split by lease kind (runner→Cloudflare,
        // check-exec→Northflank). This is the killer-box path.
        assert_eq!(select_backend(true, true), SelectedBackend::Hybrid);
    }

    #[test]
    fn select_backend_cloudflare_when_only_cf_present() {
        // Cloudflare alone ⇒ runner-only substrate (a check fails closed at spawn).
        assert_eq!(select_backend(true, false), SelectedBackend::Cloudflare);
    }

    #[test]
    fn select_backend_northflank_when_only_nf_present() {
        // No Cloudflare env, Northflank present → Northflank (both lease kinds).
        assert_eq!(select_backend(false, true), SelectedBackend::Northflank);
    }

    #[test]
    fn select_backend_off_when_neither_present() {
        // Neither env present ⇒ NoBox defaults (DEFAULT-OFF, fail-closed).
        assert_eq!(select_backend(false, false), SelectedBackend::Off);
    }

    // ── HybridBoxProvisioner (rota B) routing ──────────────────────────────────

    /// A fake sub-provisioner that records every call and returns a configurable
    /// result, so the hybrid's ROUTING can be asserted without any engine/network.
    struct SpyProvisioner {
        tag: &'static str,
        calls: Arc<Mutex<Vec<String>>>,
        teardown_ok: bool,
        probe: ProbeStatus,
    }

    impl SpyProvisioner {
        fn new(tag: &'static str, calls: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                tag,
                calls,
                teardown_ok: true,
                probe: ProbeStatus::Alive,
            }
        }
        fn log(&self, op: &str, lease_id: &str) {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(format!("{}:{}:{}", self.tag, op, lease_id));
        }
    }

    impl BoxProvisioner for SpyProvisioner {
        fn provision(&self, lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
            self.log("provision", lease_id);
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.log("teardown", lease_id);
            if self.teardown_ok {
                Ok(())
            } else {
                bail!("spy {} teardown forced failure", self.tag)
            }
        }
        fn probe(&self, lease_id: &str) -> Result<ProbeStatus> {
            self.log("probe", lease_id);
            Ok(self.probe)
        }
    }

    /// Build a runner spec (`allow_egress = true`, the runner-acquire fork) and a
    /// check spec (`allow_egress = false`, `no_network = true`, the hermetic fork)
    /// — the exact lease-kind fork the hybrid routes on.
    fn runner_and_check_specs() -> (ContainerSpec, ContainerSpec) {
        let runner = ContainerSpec {
            name: "runner-box".to_string(),
            image: "alpine@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: false,
            allow_egress: true,
            run_on_create: true,
            path_set: vec![],
            env: vec![],
        };
        let check = ContainerSpec {
            name: "check-box".to_string(),
            image: "alpine@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: true,
            allow_egress: false,
            run_on_create: false,
            path_set: vec![],
            env: vec![],
        };
        (runner, check)
    }

    #[test]
    fn hybrid_routes_runner_to_runner_sub_and_check_to_check_sub() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let check_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("CHECK", Arc::clone(&calls)));
        let hybrid = HybridBoxProvisioner::new(runner_sub, check_sub);
        let (runner_spec, check_spec) = runner_and_check_specs();

        hybrid.provision("lease-r", &runner_spec).unwrap();
        hybrid.provision("lease-c", &check_spec).unwrap();

        let log = calls.lock().unwrap().clone();
        // Runner lease → RUNNER sub (Cloudflare in prod); check lease → CHECK sub
        // (Northflank in prod). The egress fork IS the routing fork.
        assert!(log.contains(&"RUNNER:provision:lease-r".to_string()));
        assert!(log.contains(&"CHECK:provision:lease-c".to_string()));
        assert!(!log.contains(&"CHECK:provision:lease-r".to_string()));
        assert!(!log.contains(&"RUNNER:provision:lease-c".to_string()));
    }

    #[test]
    fn hybrid_routes_check_host_spec_to_the_cf_runner_sub() {
        // C1/C6: a hermetic spec whose env carries TOOLCHAIN_DIGEST is a CHECK-HOST
        // spec → the CF (RUNNER) sub-provisioner (the moat substrate, W4 check-mode),
        // NOT Northflank. A plain-hermetic check (no TOOLCHAIN_DIGEST) still routes
        // to the CHECK (Northflank) sub — default-off, byte-identical to rota B.
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let check_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("CHECK", Arc::clone(&calls)));
        let hybrid = HybridBoxProvisioner::new(runner_sub, check_sub);
        let (_runner_spec, mut check_host_spec) = runner_and_check_specs();
        // Inject the C6 discriminator exactly as leases.rs does at acquire.
        check_host_spec
            .env
            .push(("TOOLCHAIN_DIGEST".to_string(), "sha256:tool".to_string()));

        hybrid.provision("lease-ch", &check_host_spec).unwrap();
        // teardown/probe must replay to the CF (RUNNER) sub, not CHECK.
        hybrid.probe("lease-ch").unwrap();
        hybrid.teardown("lease-ch").unwrap();

        let log = calls.lock().unwrap().clone();
        assert!(log.contains(&"RUNNER:provision:lease-ch".to_string()));
        assert!(log.contains(&"RUNNER:probe:lease-ch".to_string()));
        assert!(log.contains(&"RUNNER:teardown:lease-ch".to_string()));
        assert!(!log.contains(&"CHECK:provision:lease-ch".to_string()));
    }

    #[test]
    fn hybrid_routes_plain_hermetic_check_to_northflank_default_off() {
        // The default-off guarantee: a hermetic check with NO TOOLCHAIN_DIGEST in
        // env is a plain-hermetic check → the CHECK (Northflank) sub — byte-identical
        // to rota B. This is the spec produced when an acquire omits `toolchain_digest`.
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let check_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("CHECK", Arc::clone(&calls)));
        let hybrid = HybridBoxProvisioner::new(runner_sub, check_sub);
        let (_runner_spec, check_spec) = runner_and_check_specs();

        hybrid.provision("lease-c", &check_spec).unwrap();

        let log = calls.lock().unwrap().clone();
        assert!(log.contains(&"CHECK:provision:lease-c".to_string()));
        assert!(!log.contains(&"RUNNER:provision:lease-c".to_string()));
    }

    #[test]
    fn hybrid_teardown_and_probe_replay_the_provision_route() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let check_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("CHECK", Arc::clone(&calls)));
        let hybrid = HybridBoxProvisioner::new(runner_sub, check_sub);
        let (runner_spec, check_spec) = runner_and_check_specs();

        hybrid.provision("lease-r", &runner_spec).unwrap();
        hybrid.provision("lease-c", &check_spec).unwrap();

        // teardown/probe carry no spec — they must replay the recorded route.
        hybrid.probe("lease-c").unwrap();
        hybrid.teardown("lease-c").unwrap();
        hybrid.probe("lease-r").unwrap();
        hybrid.teardown("lease-r").unwrap();

        let log = calls.lock().unwrap().clone();
        assert!(log.contains(&"CHECK:probe:lease-c".to_string()));
        assert!(log.contains(&"CHECK:teardown:lease-c".to_string()));
        assert!(log.contains(&"RUNNER:probe:lease-r".to_string()));
        assert!(log.contains(&"RUNNER:teardown:lease-r".to_string()));
        // No cross-routing of teardown/probe.
        assert!(!log.contains(&"RUNNER:teardown:lease-c".to_string()));
        assert!(!log.contains(&"CHECK:teardown:lease-r".to_string()));
    }

    #[test]
    fn hybrid_teardown_and_probe_unknown_lease_are_idempotent() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let check_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("CHECK", Arc::clone(&calls)));
        let hybrid = HybridBoxProvisioner::new(runner_sub, check_sub);

        // A lease the hybrid never provisioned: teardown is Ok(()), probe is
        // Unbound, and NEITHER sub-provisioner is contacted (no route recorded).
        assert!(hybrid.teardown("never-provisioned").is_ok());
        assert_eq!(
            hybrid.probe("never-provisioned").unwrap(),
            ProbeStatus::Unbound
        );
        assert!(
            calls.lock().unwrap().is_empty(),
            "no sub-provisioner should be contacted for an unknown lease"
        );
    }

    #[test]
    fn hybrid_keeps_route_when_teardown_fails_so_reaper_retries() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        // The check sub fails teardown → the route MUST be kept so the reaper
        // retries against the SAME engine (mirrors registry-keep-on-failure).
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let mut failing = SpyProvisioner::new("CHECK", Arc::clone(&calls));
        failing.teardown_ok = false;
        let check_sub: Arc<dyn BoxProvisioner> = Arc::new(failing);
        let hybrid = HybridBoxProvisioner::new(runner_sub, check_sub);
        let (_runner_spec, check_spec) = runner_and_check_specs();

        hybrid.provision("lease-c", &check_spec).unwrap();
        // First teardown fails → route kept.
        assert!(hybrid.teardown("lease-c").is_err());
        // Second teardown still routes to CHECK (route was NOT forgotten).
        assert!(hybrid.teardown("lease-c").is_err());

        let teardown_calls = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.as_str() == "CHECK:teardown:lease-c")
            .count();
        assert_eq!(
            teardown_calls, 2,
            "a failed teardown must keep the route so the reaper retries"
        );
    }

    #[test]
    fn hybrid_binds_boxes_so_a_runner_lease_admits() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let check_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("CHECK", Arc::clone(&calls)));
        let hybrid = HybridBoxProvisioner::new(runner_sub, check_sub);
        assert!(
            hybrid.binds_boxes(),
            "the hybrid binds boxes — a runner lease must not be rejected at admit"
        );
    }

    // ── HybridLeasedExec (rota A) routing ─────────────────────────────────────

    /// A fake exec that tags its output so a test can assert WHICH sub-exec ran,
    /// without any engine/registry/network. Records the lease ids it was asked to
    /// exec for.
    struct SpyLeasedExec {
        tag: &'static str,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl SpyLeasedExec {
        fn new(tag: &'static str, calls: Arc<Mutex<Vec<String>>>) -> Self {
            Self { tag, calls }
        }
    }

    impl LeasedExec for SpyLeasedExec {
        fn exec_captured_for(&self, lease_id: &str, _argv: &[&str]) -> Result<CmdOutput> {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(format!("{}:{}", self.tag, lease_id));
            Ok(CmdOutput {
                code: Some(0),
                stdout: self.tag.to_string(),
                stderr: String::new(),
            })
        }
    }

    /// Build a `HybridBoxProvisioner` + a matching `HybridLeasedExec` over the
    /// SAME route table (as the composition root does), plus spy sub-provisioners
    /// and spy sub-execs sharing one call log. Returns them for a routing assert.
    fn hybrid_prov_and_exec(
        calls: Arc<Mutex<Vec<String>>>,
    ) -> (HybridBoxProvisioner, HybridLeasedExec) {
        let runner_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("RUNNER", Arc::clone(&calls)));
        let check_sub: Arc<dyn BoxProvisioner> =
            Arc::new(SpyProvisioner::new("CHECK", Arc::clone(&calls)));
        let prov = HybridBoxProvisioner::new(runner_sub, check_sub);
        let routes = prov.routes_handle();
        // check_host exec = the CF branch (tag CF); check exec = the NF branch.
        let cf_exec: Arc<dyn LeasedExec> = Arc::new(SpyLeasedExec::new("CF", Arc::clone(&calls)));
        let nf_exec: Arc<dyn LeasedExec> = Arc::new(SpyLeasedExec::new("NF", Arc::clone(&calls)));
        let exec = HybridLeasedExec::new(cf_exec, nf_exec, routes);
        (prov, exec)
    }

    #[test]
    fn hybrid_exec_routes_check_host_to_cf_and_plain_check_to_nf() {
        // The crux of rota A: a check-host lease (provisioned on CF) execs on the
        // CF engine; a plain hermetic check (provisioned on NF) execs on NF. The
        // exec follows the SAME route the provisioner recorded — exec-engine ==
        // spawn-engine, per lease.
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (prov, exec) = hybrid_prov_and_exec(Arc::clone(&calls));
        let (_runner_spec, mut check_host_spec) = runner_and_check_specs();
        let (_r2, plain_check_spec) = runner_and_check_specs();
        check_host_spec
            .env
            .push(("TOOLCHAIN_DIGEST".to_string(), "sha256:tool".to_string()));

        // Provision records the routes the exec reads.
        prov.provision("lease-ch", &check_host_spec).unwrap();
        prov.provision("lease-pc", &plain_check_spec).unwrap();

        let out_ch = exec
            .exec_captured_for("lease-ch", &["sh", "-lc", "true"])
            .unwrap();
        let out_pc = exec
            .exec_captured_for("lease-pc", &["sh", "-lc", "true"])
            .unwrap();

        // The tag in stdout proves which engine served each lease.
        assert_eq!(
            out_ch.stdout, "CF",
            "check-host lease must exec on Cloudflare"
        );
        assert_eq!(
            out_pc.stdout, "NF",
            "plain check lease must exec on Northflank"
        );
        let log = calls.lock().unwrap().clone();
        assert!(log.contains(&"CF:lease-ch".to_string()));
        assert!(log.contains(&"NF:lease-pc".to_string()));
        assert!(!log.contains(&"NF:lease-ch".to_string()));
        assert!(!log.contains(&"CF:lease-pc".to_string()));
    }

    #[test]
    fn hybrid_exec_runner_lease_fails_closed() {
        // A runner lease is runner-direct — it must never exec. If one reaches the
        // exec path, fail closed rather than dispatch a runner box to a check engine.
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (prov, exec) = hybrid_prov_and_exec(Arc::clone(&calls));
        let (runner_spec, _check_spec) = runner_and_check_specs();
        prov.provision("lease-r", &runner_spec).unwrap();

        let err = exec
            .exec_captured_for("lease-r", &["sh", "-lc", "true"])
            .unwrap_err();
        assert!(
            err.to_string().contains("RUNNER"),
            "a runner lease exec must fail closed, got: {err}"
        );
        // Neither sub-exec was called.
        let log = calls.lock().unwrap().clone();
        assert!(
            !log.iter()
                .any(|c| c.starts_with("CF:") || c.starts_with("NF:"))
        );
    }

    #[test]
    fn hybrid_exec_unknown_lease_fails_closed() {
        // No recorded route (exec before provision, or an unknown lease) ⇒ never
        // guess an engine — fail closed (no fabricated result).
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (_prov, exec) = hybrid_prov_and_exec(Arc::clone(&calls));

        let err = exec
            .exec_captured_for("never-provisioned", &["sh", "-lc", "true"])
            .unwrap_err();
        assert!(
            err.to_string().contains("no hybrid route recorded"),
            "an unrouted lease exec must fail closed, got: {err}"
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    // ── CloudflareBoxProvisioner over a fake transport ────────────────────────

    use corelink_cloud_engine::{HttpRequest, HttpResponse, HttpTransport};
    use std::sync::Mutex as StdMutex;

    /// A fake transport: returns a canned status/body and records the last
    /// request. Mirrors the `RecordingTransport` pattern from
    /// `corelink-cloud-engine`'s cloudflare tests — zero network dependency.
    struct FakeTransport {
        status: u16,
        body: String,
        last: StdMutex<Option<HttpRequest>>,
    }

    impl FakeTransport {
        fn new(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.to_string(),
                last: StdMutex::new(None),
            }
        }
    }

    impl HttpTransport for FakeTransport {
        fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
            *self.last.lock().unwrap_or_else(|p| p.into_inner()) = Some(req.clone());
            Ok(HttpResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    fn cf_cfg() -> CloudflareConfig {
        CloudflareConfig::new("https://spawn.example.dev", "tok")
    }

    /// A RUNNER spec (`allow_egress == true`, runner-direct) with a pinned image.
    fn runner_spec() -> ContainerSpec {
        ContainerSpec {
            name: "runner-job".to_string(),
            image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: false,
            allow_egress: true,
            run_on_create: true,
            path_set: vec![],
            env: vec![],
        }
    }

    fn cf_provisioner(
        status: u16,
        body: &str,
        registry: BoxRegistry,
    ) -> CloudflareBoxProvisioner<FakeTransport> {
        let engine = Arc::new(CloudflareEngine::new(
            FakeTransport::new(status, body),
            cf_cfg(),
        ));
        CloudflareBoxProvisioner::new(engine, registry)
    }

    #[test]
    fn cloudflare_provision_binds_handle_then_probe_alive_then_teardown_unbinds() {
        let registry = BoxRegistry::new();
        // spawn returns a handle (200), is_alive 200 = Alive, teardown 200 = Ok.
        let prov = cf_provisioner(200, r#"{"handle":"cf-1"}"#, registry.clone_handle());

        // provision binds the returned container into the SHARED registry.
        prov.provision("lease-A", &runner_spec())
            .expect("provision");
        assert_eq!(
            registry.resolve("lease-A").map(|c| c.name),
            Some("cf-1".to_string()),
            "provision must bind the spawned handle under the lease id"
        );

        // probe resolves the binding and maps is_alive → Alive.
        assert_eq!(prov.probe("lease-A").unwrap(), ProbeStatus::Alive);

        // teardown deletes then unbinds.
        prov.teardown("lease-A").expect("teardown");
        assert!(
            registry.resolve("lease-A").is_none(),
            "teardown must unbind the lease"
        );
    }

    #[test]
    fn cloudflare_provision_fails_closed_binds_nothing() {
        // spawn returns 500 → spawn errors → NOTHING is bound (fail-closed).
        let registry = BoxRegistry::new();
        let prov = cf_provisioner(500, "boom", registry.clone_handle());
        assert!(
            prov.provision("lease-B", &runner_spec()).is_err(),
            "a non-2xx spawn must propagate Err"
        );
        assert!(
            registry.resolve("lease-B").is_none(),
            "a failed provision must leave the registry empty for the lease"
        );
    }

    #[test]
    fn cloudflare_provision_off_box_hermetic_admits_no_box() {
        // The 2026-07-07 acquire-storm fix: a plain-hermetic (off-box) spec —
        // `!allow_egress` and NO `TOOLCHAIN_DIGEST` — admits NO-BOX: provision
        // returns Ok, binds nothing, and NEVER calls the engine/spawn-Worker (so
        // it can't 503 or block the singleton on a box the caller never uses, e.g.
        // hugit's off-box §13 A-path). The fake transport 500s if reached — proving
        // spawn is skipped.
        let registry = BoxRegistry::new();
        let prov = cf_provisioner(500, "boom", registry.clone_handle());
        let hermetic = ContainerSpec {
            name: "offbox-a-path".to_string(),
            image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: true,
            allow_egress: false,
            run_on_create: false,
            path_set: vec![],
            env: vec![], // no TOOLCHAIN_DIGEST → NOT a check-host lease
        };
        assert!(
            prov.provision("lease-offbox", &hermetic).is_ok(),
            "an off-box hermetic lease must admit no-box (Ok), never spawn/503"
        );
        assert!(
            registry.resolve("lease-offbox").is_none(),
            "no box is bound for an off-box lease"
        );
    }

    #[test]
    fn cloudflare_provision_check_host_still_spawns() {
        // A check-host spec (hermetic + TOOLCHAIN_DIGEST) is NOT off-box — it MUST
        // still spawn (the no-box short-circuit must not swallow it).
        let registry = BoxRegistry::new();
        let prov = cf_provisioner(200, r#"{"handle":"cf-ch"}"#, registry.clone_handle());
        let check_host = ContainerSpec {
            name: "check-host".to_string(),
            image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: true,
            allow_egress: false,
            run_on_create: false,
            path_set: vec![],
            env: vec![("TOOLCHAIN_DIGEST".to_string(), "sha256:tool".to_string())],
        };
        prov.provision("lease-ch", &check_host)
            .expect("check-host provisions");
        assert_eq!(
            registry.resolve("lease-ch").map(|c| c.name),
            Some("cf-ch".to_string()),
            "a check-host lease must still spawn + bind (not no-box)"
        );
    }

    #[test]
    fn cloudflare_teardown_is_idempotent_when_unbound() {
        // No binding → teardown returns Ok without calling the provider (the
        // fake transport would 500, but it is never reached).
        let registry = BoxRegistry::new();
        let prov = cf_provisioner(500, "boom", registry.clone_handle());
        assert!(
            prov.teardown("never-bound").is_ok(),
            "teardown of an unbound lease is a no-op Ok (idempotent)"
        );
    }

    #[test]
    fn cloudflare_teardown_failure_keeps_binding_for_retry() {
        // Bind via a 200 provisioner, then a teardown that 500s must propagate
        // Err and KEEP the binding so the reaper can retry.
        let registry = BoxRegistry::new();
        cf_provisioner(200, r#"{"handle":"cf-2"}"#, registry.clone_handle())
            .provision("lease-C", &runner_spec())
            .expect("provision");

        let failing = cf_provisioner(500, "boom", registry.clone_handle());
        assert!(
            failing.teardown("lease-C").is_err(),
            "a failing delete must propagate Err"
        );
        assert!(
            registry.resolve("lease-C").is_some(),
            "a failed teardown must KEEP the binding for a future retry"
        );
    }

    #[test]
    fn cloudflare_probe_unbound_when_no_binding() {
        let registry = BoxRegistry::new();
        let prov = cf_provisioner(200, "", registry.clone_handle());
        assert_eq!(
            prov.probe("never-bound").unwrap(),
            ProbeStatus::Unbound,
            "no binding → Unbound (nothing to reclaim)"
        );
    }

    #[test]
    fn cloudflare_probe_dead_on_404_and_err_on_indeterminate() {
        // 404 → authoritatively Dead.
        let registry = BoxRegistry::new();
        cf_provisioner(200, r#"{"handle":"cf-3"}"#, registry.clone_handle())
            .provision("lease-D", &runner_spec())
            .expect("provision");
        let dead = cf_provisioner(404, "", registry.clone_handle());
        assert_eq!(dead.probe("lease-D").unwrap(), ProbeStatus::Dead);

        // 503 → indeterminate → is_alive Err propagates (NOT death; fail-safe).
        let reg2 = BoxRegistry::new();
        cf_provisioner(200, r#"{"handle":"cf-4"}"#, reg2.clone_handle())
            .provision("lease-E", &runner_spec())
            .expect("provision");
        let indet = cf_provisioner(503, "", reg2.clone_handle());
        assert!(
            indet.probe("lease-E").is_err(),
            "an indeterminate is_alive must propagate Err, never report Dead"
        );
    }

    #[test]
    fn cloudflare_backend_exec_half_is_cf_native_rota_a() {
        // Rota A: the exec half wired alongside the Cloudflare provisioner is a
        // CF-native EngineLeasedExec over CloudflareEngine — NOT NoBoxExec. It
        // (a) fails closed for an UNBOUND lease (empty registry, engine never
        // dialed), and (b) for a BOUND check-host box, dispatches to the CF
        // engine's `/v1/exec` and returns the captured output. Build the pair
        // exactly as `cloudflare_backend_from_env` does (env-free).
        let registry = BoxRegistry::new();
        let engine = Arc::new(CloudflareEngine::new(
            FakeTransport::new(200, r#"{"exit_code":0,"stdout":"cf-native","stderr":""}"#),
            cf_cfg(),
        ));
        let exec: Arc<dyn LeasedExec> = Arc::new(EngineLeasedExec::new(
            Arc::clone(&engine),
            registry.clone_handle(),
        ));

        // (a) UNBOUND lease → fail closed (the engine is never called).
        assert!(
            exec.exec_captured_for("unbound", &["true"]).is_err(),
            "an unbound lease must fail closed via the empty registry"
        );

        // (b) BOUND check-host box → exec dispatches to CF /v1/exec (moat), and
        // returns the real captured output — a NoBox half could never do this.
        registry.bind(
            "lease-ch",
            RunningContainer {
                name: "cf-1".to_string(),
            },
        );
        let out = exec
            .exec_captured_for("lease-ch", &["sh", "-lc", "echo hi"])
            .expect("bound check-host exec must succeed on the CF engine");
        assert_eq!(out.code, Some(0));
        assert_eq!(
            out.stdout, "cf-native",
            "the CF-native exec half must relay the /v1/exec captured output"
        );
    }
}
