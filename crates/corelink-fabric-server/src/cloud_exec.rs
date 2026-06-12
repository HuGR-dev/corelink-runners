//! WP-CF-WIRE — wires the managed-sandbox [`Engine`] into the fabric-server
//! exec path, config-gated and DEFAULT-OFF.
//!
//! Two pieces live here:
//!
//! 1. [`BoxRegistry`] — the lease→live-container seam. The spawn lifecycle (a
//!    SEPARATE, future work-package) binds a [`RunningContainer`] into this
//!    registry at spawn time. Today the registry starts empty — an unbound
//!    lease fails closed (see [`EngineLeasedExec`]). State is honest: nothing
//!    is silently fabricated when no container is bound.
//!
//! 2. [`EngineLeasedExec`] — bridges the [`Engine`] trait (container-scoped)
//!    to the [`LeasedExec`] port (lease-scoped) by resolving the lease id via
//!    the registry before calling the engine. An unbound lease is
//!    fail-closed (`bail!`); the engine is NEVER called for an unbound lease.
//!
//! 3. [`cloud_executor_from_env`] — the **BLESSED constructor** (see below).
//!    Reads `NORTHFLANK_*` env vars and returns a wired `Arc<dyn LeasedExec>`,
//!    or `None` if the required vars are absent (caller keeps [`NoBoxExec`]).
//!
//! ## Constructor discipline
//!
//! [`cloud_executor_from_env`] is the **BLESSED constructor**: it enforces the
//! both-credentials-required check via [`NorthflankConfig::from_env`] and is
//! consumed by the trusted composition seam
//! (`AppState::with_cloud_executor` / `AppState::with_cloud_executor_from_env`).
//! [`EngineLeasedExec::new`] remains public for composition and tests, but
//! hand-built executors bypass the credential check — use the composition seam
//! unless you specifically need direct construction.
//!
//! ## Dependency note
//!
//! `corelink-cloud-engine` (and its `ureq` HTTP transport) is linked into the
//! always-compiled server binary intentionally: it IS the production execution
//! backend. Feature-gating it behind an off-by-default cargo feature is a
//! possible future optimization, but it is NOT required for the fail-closed
//! guarantee — the default-off property is enforced by the composition seam
//! (absent env vars → `None` → `NoBoxExec`), not by conditional compilation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use corelink_cloud_engine::{NorthflankConfig, NorthflankEngine, UreqTransport};
use corelink_runner::isolation::{Engine, RunningContainer};
use corelink_runner::lease::CmdOutput;

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

// ── Static Send+Sync gate ──────────────────────────────────────────────────────

// The concrete production executor MUST satisfy `Arc<dyn LeasedExec>` (Send+Sync)
// even though no binary wires it yet — this static gate fails the build if a
// future change makes the Northflank stack non-thread-safe.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<
        EngineLeasedExec<
            corelink_cloud_engine::NorthflankEngine<corelink_cloud_engine::UreqTransport>,
        >,
    >();
};
